//! Operation requests as data.
//!
//! An [`OpRequest`] says *what* the user (or a program acting for the user) wants done,
//! with nothing about *how*. [`Engine::plan_request`] turns it into a [`Plan`], and that
//! plan goes through exactly the same confirmation window, the same executor and the
//! same journal as one built by hand in the interface. Nothing here can touch the
//! filesystem: producing a plan only reads.
//!
//! The JSON form is described by `schema/request.schema.json` in the repository.

use std::path::PathBuf;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::engine::Engine;
use super::plan::{ConflictPolicy, Plan};
use super::planner::TransferOptions;
use super::rename::Pattern;
use super::scan::{Scan, ScanControl};
use super::undo::UndoPlan;
use crate::journal::Journal;
use crate::{Error, Result, pathcodec};

/// A request to do something to files. All paths must be absolute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum OpRequest {
    /// Copy items into a folder. Symlinks are copied as symlinks.
    Copy {
        #[serde(with = "pathcodec::paths")]
        #[schemars(with = "Vec<String>")]
        sources: Vec<PathBuf>,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        destination: PathBuf,
        /// What to do when a name already exists in the destination.
        #[serde(default)]
        conflict: ConflictPolicy,
        /// Re-read and compare every copy.
        #[serde(default)]
        verify: bool,
    },
    /// Move items into a folder (a rename on the same filesystem, copy + verify + remove across filesystems).
    Move {
        #[serde(with = "pathcodec::paths")]
        #[schemars(with = "Vec<String>")]
        sources: Vec<PathBuf>,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        destination: PathBuf,
        #[serde(default)]
        conflict: ConflictPolicy,
        #[serde(default)]
        verify: bool,
    },
    /// Rename one item inside its folder.
    Rename {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
        /// The new file name (a single name, no folder separators).
        new_name: String,
    },
    /// Rename several items with a pattern such as `{name}_{n:3}.{ext}` or `s/old/new/`.
    BulkRename {
        #[serde(with = "pathcodec::paths")]
        #[schemars(with = "Vec<String>")]
        items: Vec<PathBuf>,
        pattern: String,
        /// First value of `{n}` (default 1).
        #[serde(default)]
        counter_start: Option<u64>,
        /// Increment of `{n}` (default 1).
        #[serde(default)]
        counter_step: Option<u64>,
    },
    /// Create a folder.
    MakeDir {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        parent: PathBuf,
        name: String,
    },
    /// Move items to the trash (recoverable).
    Trash {
        #[serde(with = "pathcodec::paths")]
        #[schemars(with = "Vec<String>")]
        sources: Vec<PathBuf>,
    },
    /// Delete items permanently. Cannot be undone; the plan says so and asks for explicit confirmation.
    Delete {
        #[serde(with = "pathcodec::paths")]
        #[schemars(with = "Vec<String>")]
        sources: Vec<PathBuf>,
    },
    /// Undo a journal entry (the most recent undoable one when `entry` is omitted).
    Undo {
        #[serde(default)]
        entry: Option<String>,
    },
}

/// What planning a request produced.
pub struct Planned {
    pub plan: Plan,
    /// The scan the plan was built from; reuse it to re-plan with another conflict policy.
    pub scan: Option<Arc<Scan>>,
    pub undo: Option<UndoPlan>,
}

fn require_absolute(paths: &[PathBuf], what: &str) -> Result<()> {
    if paths.is_empty() {
        return Err(Error::Invalid(format!("no {what} given")));
    }
    if let Some(p) = paths.iter().find(|p| !p.is_absolute()) {
        return Err(Error::Invalid(format!(
            "{what} must be absolute paths, got {}",
            crate::display::path(p)
        )));
    }
    Ok(())
}

impl OpRequest {
    /// Cheap checks that need no filesystem access.
    pub fn validate(&self) -> Result<()> {
        match self {
            OpRequest::Copy {
                sources,
                destination,
                ..
            }
            | OpRequest::Move {
                sources,
                destination,
                ..
            } => {
                require_absolute(sources, "sources")?;
                require_absolute(std::slice::from_ref(destination), "destination")
            }
            OpRequest::Rename { path, new_name } => {
                require_absolute(std::slice::from_ref(path), "path")?;
                if new_name.is_empty() {
                    return Err(Error::Invalid("the new name is empty".into()));
                }
                Ok(())
            }
            OpRequest::BulkRename { items, pattern, .. } => {
                require_absolute(items, "items")?;
                Pattern::parse(pattern).map(|_| ()).map_err(Error::Invalid)
            }
            OpRequest::MakeDir { parent, name } => {
                require_absolute(std::slice::from_ref(parent), "parent")?;
                if name.is_empty() {
                    return Err(Error::Invalid("the folder name is empty".into()));
                }
                Ok(())
            }
            OpRequest::Trash { sources } | OpRequest::Delete { sources } => {
                require_absolute(sources, "sources")
            }
            OpRequest::Undo { .. } => Ok(()),
        }
    }

    /// The request for a copy/move, as the interface builds it.
    pub fn transfer(
        sources: Vec<PathBuf>,
        destination: PathBuf,
        opts: &TransferOptions,
    ) -> OpRequest {
        match opts.mode {
            super::planner::TransferMode::Copy => OpRequest::Copy {
                sources,
                destination,
                conflict: opts.policy,
                verify: opts.verify,
            },
            super::planner::TransferMode::Move => OpRequest::Move {
                sources,
                destination,
                conflict: opts.policy,
                verify: opts.verify,
            },
        }
    }
}

impl Engine {
    /// Turn a request into a plan. Reads the filesystem, changes nothing.
    ///
    /// `journal` is only needed for [`OpRequest::Undo`].
    pub fn plan_request(
        &self,
        req: &OpRequest,
        journal: Option<&Journal>,
        ctl: ScanControl<'_>,
    ) -> Result<Planned> {
        req.validate()?;
        let planned = match req {
            OpRequest::Copy {
                sources,
                destination,
                conflict,
                verify,
            } => {
                let scan = Arc::new(self.scan(sources, ctl));
                let opts = TransferOptions {
                    mode: super::planner::TransferMode::Copy,
                    policy: *conflict,
                    verify: *verify,
                };
                Planned {
                    plan: self.plan_transfer(&scan, destination, &opts),
                    scan: Some(scan),
                    undo: None,
                }
            }
            OpRequest::Move {
                sources,
                destination,
                conflict,
                verify,
            } => {
                let scan = Arc::new(self.scan(sources, ctl));
                let opts = TransferOptions {
                    mode: super::planner::TransferMode::Move,
                    policy: *conflict,
                    verify: *verify,
                };
                Planned {
                    plan: self.plan_transfer(&scan, destination, &opts),
                    scan: Some(scan),
                    undo: None,
                }
            }
            OpRequest::Trash { sources } => {
                let scan = Arc::new(self.scan(sources, ctl));
                Planned {
                    plan: self.plan_trash(&scan),
                    scan: Some(scan),
                    undo: None,
                }
            }
            OpRequest::Delete { sources } => {
                let scan = Arc::new(self.scan(sources, ctl));
                Planned {
                    plan: self.plan_delete(&scan),
                    scan: Some(scan),
                    undo: None,
                }
            }
            OpRequest::Rename { path, new_name } => Planned {
                plan: self.plan_rename(path, new_name.as_ref()),
                scan: None,
                undo: None,
            },
            OpRequest::MakeDir { parent, name } => Planned {
                plan: self.plan_mkdir(parent, name.as_ref()),
                scan: None,
                undo: None,
            },
            OpRequest::BulkRename {
                items,
                pattern,
                counter_start,
                counter_step,
            } => {
                let p = Pattern::parse(pattern)
                    .map_err(Error::Invalid)?
                    .with_counter(counter_start.unwrap_or(1), counter_step.unwrap_or(1));
                Planned {
                    plan: self.plan_bulk_rename(items, &p),
                    scan: None,
                    undo: None,
                }
            }
            OpRequest::Undo { entry } => {
                let j = journal.ok_or_else(|| Error::Invalid("undo needs the journal".into()))?;
                let id = match entry {
                    Some(id) => id.clone(),
                    None => j
                        .last_undoable()?
                        .map(|e| e.id)
                        .ok_or_else(|| Error::Invalid("nothing to undo".into()))?,
                };
                let up = self.plan_undo(j, &id)?;
                Planned {
                    plan: up.plan.clone(),
                    scan: None,
                    undo: Some(up),
                }
            }
        };
        let mut planned = planned;
        if !matches!(req, OpRequest::Undo { .. }) {
            planned.plan.request = Some(req.clone());
        }
        Ok(planned)
    }
}

/// How a plan is described to programs: its JSON form.
pub fn plan_to_json(plan: &Plan) -> Result<String> {
    Ok(serde_json::to_string_pretty(plan)?)
}
