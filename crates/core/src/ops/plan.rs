//! The plan: what an operation *will* do, as data, before anything is touched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs::{Fingerprint, Stamp};
use crate::platform::TrashedItem;
use crate::{display, pathcodec};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Copy,
    Move,
    Rename,
    BulkRename,
    MakeDir,
    Trash,
    Delete,
    Undo,
}

impl OpKind {
    pub fn verb(self) -> &'static str {
        match self {
            OpKind::Copy => "Copy",
            OpKind::Move => "Move",
            OpKind::Rename => "Rename",
            OpKind::BulkRename => "Bulk rename",
            OpKind::MakeDir => "New folder",
            OpKind::Trash => "Move to trash",
            OpKind::Delete => "Delete permanently",
            OpKind::Undo => "Undo",
        }
    }
}

/// One atomic action. The whole engine (planning, execution, journal, undo) speaks
/// only this vocabulary, which is what makes "every operation is undoable" tractable:
/// each step knows its own inverse (see [`Step::inverse`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    MakeDir {
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
        mode: Option<u32>,
    },
    /// Apply the final permissions/time of a directory created earlier in the same run
    /// (after its children exist, so a read-only source dir stays writable meanwhile).
    FinishDir {
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
        mode: Option<u32>,
        mtime: Option<Stamp>,
    },
    CopyFile {
        #[serde(with = "pathcodec::path")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        size: u64,
        mode: Option<u32>,
        mtime: Option<Stamp>,
        atime: Option<Stamp>,
        /// Re-read and compare the copy.
        verify: bool,
        /// Cross-device move: remove the source once the copy is verified.
        remove_source: bool,
    },
    CopySymlink {
        #[serde(with = "pathcodec::path")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        #[serde(with = "pathcodec::path")]
        target: PathBuf,
        remove_source: bool,
    },
    Rename {
        #[serde(with = "pathcodec::path")]
        from: PathBuf,
        #[serde(with = "pathcodec::path")]
        to: PathBuf,
    },
    TrashItem {
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
    },
    /// Permanent removal of a file or symlink. With `expect`, refuses to remove a
    /// file that changed since it was recorded (used by undo).
    RemoveFile {
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
        expect: Option<Fingerprint>,
    },
    /// Removal of an *empty* directory.
    RemoveDir {
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
    },
    Restore {
        item: TrashedItem,
    },
}

impl Step {
    /// The path this step is "about", for progress and error messages.
    pub fn path(&self) -> &Path {
        match self {
            Step::MakeDir { path, .. }
            | Step::FinishDir { path, .. }
            | Step::TrashItem { path }
            | Step::RemoveFile { path, .. }
            | Step::RemoveDir { path } => path,
            Step::CopyFile { src, .. } | Step::CopySymlink { src, .. } => src,
            Step::Rename { from, .. } => from,
            Step::Restore { item } => &item.original,
        }
    }

    /// Where the step writes, if anywhere.
    pub fn destination(&self) -> Option<&Path> {
        match self {
            Step::MakeDir { path, .. } => Some(path),
            Step::CopyFile { dst, .. } | Step::CopySymlink { dst, .. } => Some(dst),
            Step::Rename { to, .. } => Some(to),
            Step::Restore { item } => Some(&item.original),
            _ => None,
        }
    }

    pub fn bytes(&self) -> u64 {
        match self {
            Step::CopyFile { size, .. } => *size,
            _ => 0,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Step::MakeDir { path, .. } => format!("create folder {}", display::path(path)),
            Step::FinishDir { path, .. } => format!("finish folder {}", display::path(path)),
            Step::CopyFile {
                src,
                dst,
                remove_source,
                ..
            } => format!(
                "{} {} -> {}",
                if *remove_source { "move" } else { "copy" },
                display::path(src),
                display::path(dst)
            ),
            Step::CopySymlink {
                src,
                dst,
                remove_source,
                ..
            } => format!(
                "{} link {} -> {}",
                if *remove_source { "move" } else { "copy" },
                display::path(src),
                display::path(dst)
            ),
            Step::Rename { from, to } => {
                format!("rename {} -> {}", display::path(from), display::path(to))
            }
            Step::TrashItem { path } => format!("trash {}", display::path(path)),
            Step::RemoveFile { path, .. } => format!("delete {}", display::path(path)),
            Step::RemoveDir { path } => format!("remove folder {}", display::path(path)),
            Step::Restore { item } => format!("restore {}", display::path(&item.original)),
        }
    }

    /// The step that reverses this one, given what executing it produced.
    /// `None` when there is nothing to reverse (or it cannot be reversed).
    pub fn inverse(&self, result: &StepResult) -> Option<Step> {
        if result.status != StepStatus::Done {
            return None;
        }
        match self {
            Step::MakeDir { path, .. } if result.created => {
                Some(Step::RemoveDir { path: path.clone() })
            }
            Step::MakeDir { .. } | Step::FinishDir { .. } => None,
            Step::CopyFile {
                src,
                dst,
                remove_source,
                size,
                mode,
                mtime,
                atime,
                ..
            } => {
                if *remove_source {
                    Some(Step::CopyFile {
                        src: dst.clone(),
                        dst: src.clone(),
                        size: *size,
                        mode: *mode,
                        mtime: *mtime,
                        atime: *atime,
                        verify: true,
                        remove_source: true,
                    })
                } else {
                    Some(Step::RemoveFile {
                        path: dst.clone(),
                        expect: result.after.clone(),
                    })
                }
            }
            Step::CopySymlink {
                src,
                dst,
                target,
                remove_source,
            } => {
                if *remove_source {
                    Some(Step::CopySymlink {
                        src: dst.clone(),
                        dst: src.clone(),
                        target: target.clone(),
                        remove_source: true,
                    })
                } else {
                    Some(Step::RemoveFile {
                        path: dst.clone(),
                        expect: result.after.clone(),
                    })
                }
            }
            Step::Rename { from, to } => Some(Step::Rename {
                from: to.clone(),
                to: from.clone(),
            }),
            Step::TrashItem { .. } => result.trashed.clone().map(|item| Step::Restore { item }),
            Step::RemoveDir { path } => Some(Step::MakeDir {
                path: path.clone(),
                mode: result.removed_dir_mode.map(|m| m | 0o700),
            }),
            // Permanent deletion and restore are not reversed here.
            Step::RemoveFile { .. } | Step::Restore { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Done,
    /// Nothing to do (already in the desired state).
    NoOp,
    /// Deliberately left in place (e.g. a directory that is not empty).
    Kept,
}

/// What executing a step produced.
#[derive(Clone, Debug)]
pub struct StepResult {
    pub status: StepStatus,
    pub note: Option<String>,
    /// For `MakeDir`: this run created the directory.
    pub created: bool,
    /// Fingerprint of what was written.
    pub after: Option<Fingerprint>,
    pub trashed: Option<TrashedItem>,
    pub removed_dir_mode: Option<u32>,
}

impl StepResult {
    pub fn done() -> Self {
        StepResult {
            status: StepStatus::Done,
            note: None,
            created: false,
            after: None,
            trashed: None,
            removed_dir_mode: None,
        }
    }
    pub fn noop(note: impl Into<String>) -> Self {
        StepResult {
            status: StepStatus::NoOp,
            note: Some(note.into()),
            ..Self::done()
        }
    }
    pub fn kept(note: impl Into<String>) -> Self {
        StepResult {
            status: StepStatus::Kept,
            note: Some(note.into()),
            ..Self::done()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    /// The plan cannot be executed.
    Blocking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WarningKind {
    Irreversible,
    InsideItself,
    NotWritable,
    LowSpace,
    Conflict,
    Overwrite,
    Unreadable,
    SpecialFile,
    BrokenSymlink,
    Symlink,
    CircularLink,
    MountLoop,
    CrossDevice,
    SameLocation,
    HardLinks,
    InvalidName,
    Missing,
    Merge,
    /// An undo step was refused because the item changed since the operation.
    ModifiedSince,
    /// An undo step cannot be carried out (target occupied, source gone, ...).
    CannotUndo,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub kind: WarningKind,
    pub severity: Severity,
    pub message: String,
    pub count: u64,
    /// A few representative paths (never all of them: plans can be huge).
    pub examples: Vec<PathBuf>,
}

/// Collects warnings grouped by kind so that 10 000 symlinks give one line, not 10 000.
#[derive(Default)]
pub struct WarningSet {
    by_kind: BTreeMap<(WarningKind, Severity), (u64, Vec<PathBuf>, Option<String>)>,
}

const MAX_EXAMPLES: usize = 5;

impl WarningSet {
    pub fn add(&mut self, kind: WarningKind, severity: Severity, path: Option<&Path>) {
        self.add_with(kind, severity, path, None);
    }

    /// `detail` is appended to the example list's message the first time only.
    pub fn add_with(
        &mut self,
        kind: WarningKind,
        severity: Severity,
        path: Option<&Path>,
        detail: Option<String>,
    ) {
        let e = self.by_kind.entry((kind, severity)).or_default();
        e.0 += 1;
        if let Some(p) = path {
            if e.1.len() < MAX_EXAMPLES {
                e.1.push(p.to_path_buf());
            }
        }
        if e.2.is_none() {
            e.2 = detail;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_kind.is_empty()
    }

    pub fn has_blocking(&self) -> bool {
        self.by_kind.keys().any(|(_, s)| *s == Severity::Blocking)
    }

    pub fn finish(self) -> Vec<Warning> {
        let mut out: Vec<Warning> = self
            .by_kind
            .into_iter()
            .map(|((kind, severity), (count, examples, detail))| Warning {
                kind,
                severity,
                message: message_for(kind, count, detail.as_deref()),
                count,
                examples,
            })
            .collect();
        // Most serious first, then by kind: stable and deterministic.
        out.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.kind.cmp(&b.kind)));
        out
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn message_for(kind: WarningKind, n: u64, detail: Option<&str>) -> String {
    let extra = detail.map(|d| format!(" ({d})")).unwrap_or_default();
    match kind {
        WarningKind::Irreversible => "PERMANENT: this cannot be undone".to_string(),
        WarningKind::InsideItself => format!(
            "{} is being copied or moved into itself",
            plural(n, "folder", "folders")
        ),
        WarningKind::NotWritable => format!("destination is not writable{extra}"),
        WarningKind::LowSpace => format!("not enough free space{extra}"),
        WarningKind::Conflict => format!(
            "{} already exist{}",
            plural(n, "item", "items"),
            if n == 1 { "s" } else { "" }
        ),
        WarningKind::Overwrite => format!(
            "{} will be replaced (the old version goes to the trash, so undo restores it)",
            plural(n, "item", "items")
        ),
        WarningKind::Unreadable => format!(
            "{} cannot be read and will probably fail{extra}",
            plural(n, "item", "items")
        ),
        WarningKind::SpecialFile => format!(
            "{} (device, socket or pipe) will be skipped",
            plural(n, "special file", "special files")
        ),
        WarningKind::BrokenSymlink => format!(
            "{} point to nothing; they are kept as broken links",
            plural(n, "symlink", "symlinks")
        ),
        WarningKind::Symlink => format!(
            "{} will be kept as links (not followed)",
            plural(n, "symlink", "symlinks")
        ),
        WarningKind::CircularLink => format!(
            "{} form a loop; kept as links, not followed",
            plural(n, "symlink", "symlinks")
        ),
        WarningKind::MountLoop => format!(
            "{} loop back to a parent (mount cycle) and will be skipped",
            plural(n, "folder", "folders")
        ),
        WarningKind::CrossDevice => {
            "different filesystem: files are copied, verified, then removed from the source"
                .to_string()
        }
        WarningKind::SameLocation => format!(
            "{} already in the destination folder",
            plural(n, "item is", "items are")
        ),
        WarningKind::HardLinks => format!(
            "{} have several hard links; copies become independent files",
            plural(n, "file", "files")
        ),
        WarningKind::InvalidName => format!("invalid name{extra}"),
        WarningKind::Missing => format!("{} no longer exist", plural(n, "item", "items")),
        WarningKind::Merge => format!(
            "{} will be merged with existing folders",
            plural(n, "folder", "folders")
        ),
        WarningKind::ModifiedSince => format!(
            "{} changed since the operation and will be left untouched",
            plural(n, "item", "items")
        ),
        WarningKind::CannotUndo => {
            format!("{} cannot be undone{extra}", plural(n, "step", "steps"))
        }
        WarningKind::Other => detail.unwrap_or("see details").to_string(),
    }
}

/// How to resolve a destination that already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Leave existing items alone, do not copy the clashing source.
    #[default]
    Skip,
    /// Copy under a new name (`name (1).ext`).
    KeepBoth,
    /// Replace; the old item is moved to the trash first so that undo can bring it back.
    Overwrite,
}

impl ConflictPolicy {
    pub fn next(self) -> Self {
        match self {
            ConflictPolicy::Skip => ConflictPolicy::KeepBoth,
            ConflictPolicy::KeepBoth => ConflictPolicy::Overwrite,
            ConflictPolicy::Overwrite => ConflictPolicy::Skip,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConflictPolicy::Skip => "skip existing",
            ConflictPolicy::KeepBoth => "keep both",
            ConflictPolicy::Overwrite => "overwrite (old goes to trash)",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    /// Top-level items the user selected.
    pub items: u64,
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub kind: OpKind,
    pub title: String,
    pub destination: Option<PathBuf>,
    pub steps: Vec<Step>,
    pub totals: Totals,
    pub warnings: Vec<Warning>,
    pub policy: ConflictPolicy,
    /// False for permanent deletion: the journal records it but cannot undo it.
    pub reversible: bool,
    /// Old name -> new name, for rename previews.
    pub renames: Vec<(PathBuf, PathBuf)>,
}

impl Plan {
    pub fn empty(kind: OpKind, title: impl Into<String>) -> Plan {
        Plan {
            kind,
            title: title.into(),
            destination: None,
            steps: Vec::new(),
            totals: Totals::default(),
            warnings: Vec::new(),
            policy: ConflictPolicy::Skip,
            reversible: true,
            renames: Vec::new(),
        }
    }

    pub fn blocking(&self) -> impl Iterator<Item = &Warning> {
        self.warnings
            .iter()
            .filter(|w| w.severity == Severity::Blocking)
    }

    pub fn is_executable(&self) -> bool {
        self.blocking().next().is_none() && !self.steps.is_empty()
    }

    pub fn has_conflicts(&self) -> bool {
        self.warnings
            .iter()
            .any(|w| matches!(w.kind, WarningKind::Conflict | WarningKind::Overwrite))
    }

    /// Bytes that will be moved through memory/IO.
    pub fn total_bytes(&self) -> u64 {
        self.steps.iter().map(Step::bytes).sum()
    }
}
