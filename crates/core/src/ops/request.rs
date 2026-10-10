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

use super::archives::ExtractInto;
use super::engine::Engine;
use super::plan::{ConflictPolicy, Plan};
use super::planner::TransferOptions;
use super::rename::Pattern;
use super::scan::{Scan, ScanControl};
use super::undo::UndoPlan;
use crate::archive::{self, ArchiveKind};
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
    /// Create an empty file.
    MakeFile {
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
    /// Take the contents of an archive out into a folder. With `only`, just those members
    /// (paths inside the archive); without, everything.
    Extract {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        archive: PathBuf,
        /// The folder to extract into (or to make the new folder in).
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        destination: PathBuf,
        #[serde(default)]
        into: ExtractInto,
        /// Members to extract, as paths inside the archive; empty means all.
        #[serde(with = "pathcodec::paths", default)]
        #[schemars(with = "Vec<String>")]
        only: Vec<PathBuf>,
        #[serde(default)]
        conflict: ConflictPolicy,
    },
    /// Put items into a new archive.
    Compress {
        #[serde(with = "pathcodec::paths")]
        #[schemars(with = "Vec<String>")]
        sources: Vec<PathBuf>,
        /// The archive to create (full path, including its extension).
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        archive: PathBuf,
        format: ArchiveKind,
        #[serde(default)]
        conflict: ConflictPolicy,
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
            OpRequest::MakeFile { parent, name } => {
                require_absolute(std::slice::from_ref(parent), "parent")?;
                if name.is_empty() {
                    return Err(Error::Invalid("the file name is empty".into()));
                }
                Ok(())
            }
            OpRequest::Trash { sources } | OpRequest::Delete { sources } => {
                require_absolute(sources, "sources")
            }
            OpRequest::Extract {
                archive,
                destination,
                only,
                ..
            } => {
                require_absolute(std::slice::from_ref(archive), "archive")?;
                require_absolute(std::slice::from_ref(destination), "destination")?;
                if let Some(p) = only.iter().find(|p| p.is_absolute()) {
                    return Err(Error::Invalid(format!(
                        "members to extract are paths inside the archive, got {}",
                        crate::display::path(p)
                    )));
                }
                Ok(())
            }
            OpRequest::Compress {
                sources, archive, ..
            } => {
                require_absolute(sources, "sources")?;
                require_absolute(std::slice::from_ref(archive), "archive")
            }
            OpRequest::Undo { .. } => Ok(()),
        }
    }

    /// The same request with another way of settling name clashes; `None` for requests that
    /// have no such thing.
    pub fn with_conflict(&self, policy: ConflictPolicy) -> Option<OpRequest> {
        let mut r = self.clone();
        match &mut r {
            OpRequest::Copy { conflict, .. }
            | OpRequest::Move { conflict, .. }
            | OpRequest::Extract { conflict, .. }
            | OpRequest::Compress { conflict, .. } => *conflict = policy,
            _ => return None,
        }
        Some(r)
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
        self.refuse_archive_writes(req)?;
        let planned = match req {
            OpRequest::Copy {
                sources,
                destination,
                conflict,
                ..
            } if self.archive_sources(sources)?.is_some() => {
                // Copying out of an archive is an extraction of those members.
                let (archive, only) = self.archive_sources(sources)?.expect("checked");
                let plan = self.plan_extract(
                    &archive,
                    &only,
                    destination,
                    &ExtractInto::Here,
                    *conflict,
                    ctl,
                )?;
                Planned {
                    plan,
                    scan: None,
                    undo: None,
                }
            }
            OpRequest::Extract {
                archive,
                destination,
                into,
                only,
                conflict,
            } => Planned {
                plan: self.plan_extract(archive, only, destination, into, *conflict, ctl)?,
                scan: None,
                undo: None,
            },
            OpRequest::Compress {
                sources,
                archive,
                format,
                conflict,
            } => {
                let scan = Arc::new(self.scan(sources, ctl));
                Planned {
                    plan: self.plan_compress(&scan, archive, *format, *conflict),
                    scan: Some(scan),
                    undo: None,
                }
            }
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
            OpRequest::MakeFile { parent, name } => Planned {
                plan: self.plan_mkfile(parent, name.as_ref()),
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

impl Engine {
    /// The archive all of `sources` lie in, with their paths inside it; `None` when none of
    /// them is in an archive. Items from two archives, or from an archive and a folder, are
    /// refused: one extraction reads one archive.
    pub(super) fn archive_sources(
        &self,
        sources: &[PathBuf],
    ) -> Result<Option<(PathBuf, Vec<PathBuf>)>> {
        let mut found: Option<PathBuf> = None;
        let mut inner = Vec::new();
        let mut outside = 0usize;
        for s in sources {
            match archive::locate(&*self.fs, s) {
                Some(loc) if !loc.inner.as_os_str().is_empty() => {
                    match &found {
                        Some(a) if *a != loc.archive => {
                            return Err(Error::Invalid(
                                "the selection is in two different archives; copy them one archive at a time".into(),
                            ));
                        }
                        _ => found = Some(loc.archive),
                    }
                    inner.push(loc.inner);
                }
                _ => outside += 1,
            }
        }
        match found {
            None => Ok(None),
            Some(_) if outside > 0 => Err(Error::Invalid(
                "the selection mixes items from an archive with items outside it".into(),
            )),
            Some(a) => Ok(Some((a, inner))),
        }
    }

    /// Archives are read-only folders: nothing may be written, moved, renamed or deleted in
    /// them, and the message says what to do instead.
    fn refuse_archive_writes(&self, req: &OpRequest) -> Result<()> {
        let inside = |p: &PathBuf| archive::is_inside(&*self.fs, p);
        let refuse = |p: &PathBuf, what: &str| {
            Err(Error::Invalid(format!(
                "{} is inside an archive, which is read-only: {what}",
                crate::display::path(p)
            )))
        };
        match req {
            OpRequest::Copy { destination, .. } | OpRequest::Extract { destination, .. }
                if inside(destination) =>
            {
                refuse(destination, "choose a folder outside it")
            }
            OpRequest::Move {
                sources,
                destination,
                ..
            } => {
                if let Some(p) = sources.iter().find(|p| inside(p)) {
                    return refuse(p, "copy it out instead (an archive cannot be changed)");
                }
                if inside(destination) {
                    return refuse(destination, "choose a folder outside it");
                }
                Ok(())
            }
            OpRequest::Trash { sources } | OpRequest::Delete { sources } => {
                match sources.iter().find(|p| inside(p)) {
                    Some(p) => refuse(p, "an archive cannot be changed"),
                    None => Ok(()),
                }
            }
            OpRequest::BulkRename { items, .. } => match items.iter().find(|p| inside(p)) {
                Some(p) => refuse(p, "an archive cannot be changed"),
                None => Ok(()),
            },
            OpRequest::Rename { path, .. } if inside(path) => {
                refuse(path, "an archive cannot be changed")
            }
            OpRequest::MakeDir { parent, .. } | OpRequest::MakeFile { parent, .. }
                if inside(parent) =>
            {
                refuse(parent, "an archive cannot be changed")
            }
            OpRequest::Compress { archive, .. } if inside(archive) => {
                refuse(archive, "choose a folder outside it")
            }
            _ => Ok(()),
        }
    }
}

/// How a plan is described to programs: its JSON form.
pub fn plan_to_json(plan: &Plan) -> Result<String> {
    Ok(serde_json::to_string_pretty(plan)?)
}
