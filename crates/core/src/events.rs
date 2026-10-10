//! Everything background workers tell the UI, on one channel.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::journal::JournalEntry;
use crate::model::{Entry, EntryUpdate};
use crate::ops::{ErrorChoice, ExecReport, Plan, Progress, Scan, UndoPlan};
use crate::platform::Volume;
use crate::preview::Preview;

pub type JobId = u64;

#[derive(Debug)]
pub struct FailureInfo {
    pub index: usize,
    pub description: String,
    pub message: String,
    pub path: PathBuf,
    pub attempt: u32,
}

pub enum JobEvent {
    Scanning {
        job: JobId,
        files: u64,
        dirs: u64,
        bytes: u64,
        current: PathBuf,
    },
    Planned {
        job: JobId,
        plan: Box<Plan>,
        /// Kept so that changing the conflict policy re-plans without re-reading the disk.
        scan: Option<Arc<Scan>>,
        undo: Option<Box<UndoPlan>>,
    },
    PlanFailed {
        job: JobId,
        message: String,
    },
    Progress {
        job: JobId,
        progress: Progress,
    },
    /// The worker is blocked until `reply` gets an answer.
    AskFailure {
        job: JobId,
        info: FailureInfo,
        reply: Sender<ErrorChoice>,
    },
    Finished {
        job: JobId,
        title: String,
        report: ExecReport,
        journal_id: Option<String>,
        journal_errors: u64,
    },
    History {
        job: JobId,
        entries: Vec<JournalEntry>,
    },
    /// At start, operations a dead process had left half done were settled.
    Recovered {
        items: Vec<crate::ops::Recovered>,
    },
    /// What the next undo would undo, in a few words; `None` when there is nothing to undo.
    UndoLabel {
        job: JobId,
        label: Option<String>,
    },
    NothingToUndo {
        job: JobId,
    },
    NothingToRedo {
        job: JobId,
    },
    Error {
        job: JobId,
        message: String,
    },
}

pub enum DirEvent {
    Loaded {
        generation: u64,
        path: PathBuf,
        result: Result<Vec<Entry>, String>,
        /// Set when `path` is an archive or a folder inside one.
        archive: Option<crate::archive::ArchiveView>,
    },
    Patched {
        generation: u64,
        dir: PathBuf,
        updates: Vec<EntryUpdate>,
    },
}

pub enum WatchEvent {
    /// Something in `dir` changed. `rescan` means "paths are unknown, reload everything".
    Changed {
        dir: PathBuf,
        paths: Vec<PathBuf>,
        rescan: bool,
    },
    /// Native notifications are unavailable; the watcher fell back to polling.
    Degraded(String),
    Failed(String),
}

pub struct PreviewEvent {
    pub generation: u64,
    pub path: PathBuf,
    pub name: OsString,
    pub preview: Preview,
}

pub enum CoreEvent {
    Dir(DirEvent),
    Watch(WatchEvent),
    Preview(PreviewEvent),
    Volumes(Vec<Volume>),
    /// Recent folders, bookmarks and standard places (from the places worker).
    Paths(crate::places::PathLists),
    Job(JobEvent),
}
