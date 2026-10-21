//! Everything background workers tell the UI, on one channel.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::journal::JournalEntry;
use crate::model::{Entry, EntryUpdate};
use crate::ops::{ErrorChoice, ExecReport, Plan, Progress, Scan, UndoPlan};
use crate::platform::{Unmounted, Volume};
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
        /// Set when the folder could not be opened because the system is waiting for the
        /// person to allow it (macOS asks per terminal and per folder).
        denied: Option<Box<crate::platform::AccessDenied>>,
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
    /// The devices that are plugged in and not mounted (sticks, phones), when that list changes.
    Unmounted(Vec<Unmounted>),
    /// A device was mounted, or the system said why not.
    Mounted {
        /// What was mounted (the id the list gave it) and its name.
        id: String,
        name: String,
        result: Result<(), String>,
    },
    /// Recent folders, bookmarks and standard places (from the places worker).
    Paths(crate::places::PathLists),
    Job(JobEvent),
    /// The subfolders of a folder, for completing an address being typed.
    Complete {
        dir: PathBuf,
        /// Names of the subfolders, sorted; empty when the folder cannot be read.
        names: Vec<String>,
    },
    /// The folders zoxide knows that match what is typed in the command bar (at most five).
    Zoxide {
        query: String,
        dirs: Vec<PathBuf>,
    },
    /// Something a background thread wants the user to know (a program that would not start).
    Note {
        text: String,
        error: bool,
    },
    /// The size of a folder on screen, counted in the background.
    Size(crate::foldersize::SizeEvent),
    /// The update worker: a newer version, progress, the end.
    Update(crate::update::UpdateEvent),
    /// The font offered at the first start was installed (a sentence to show), or why not.
    FontInstalled(Result<String, String>),
    /// A disk was ejected, or the system said why not.
    Ejected {
        mount: PathBuf,
        name: String,
        result: Result<(), String>,
    },
    /// The programs that can open a file (for "Open with…"), in the order to list them, and
    /// the one chosen to be used always for this kind of file, if any.
    OpenChoices {
        path: PathBuf,
        kind: Option<String>,
        apps: Vec<crate::platform::OpenApp>,
        always: Option<String>,
    },
}
