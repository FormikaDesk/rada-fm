//! The journal an operation that runs other programs writes, read the way rada 0.6.1 reads
//! it: none of its lines may count as damage there.
//!
//! The types below are the ones 0.6.1 reads a journal with (`OpKind`, `Step`, `Intent`,
//! `Record`, the list of record types and `classify`), copied from its sources; only the
//! types they are built from (a fingerprint, a time, a tag) are the ones of today, and the
//! request of a `begin` record is taken as any JSON. A line 0.6.1 does not know is skipped; a
//! line of a type it knows that it cannot read is damage, and stops it from settling or
//! undoing what was running.
#![allow(dead_code)]

use std::path::PathBuf;

use rada_core::archive::ArchiveKind;
use rada_core::archive::writer::CompressItem;
use rada_core::fs::{Fingerprint, Stamp};
use rada_core::ops::{RunStatus, Totals};
use rada_core::pathcodec;
use rada_core::platform::TrashedItem;
use serde::{Deserialize, Serialize};

type OpRequest = serde_json::Value;

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Copy,
    Move,
    Rename,
    BulkRename,
    MakeDir,
    /// Create an empty file.
    MakeFile,
    Trash,
    Delete,
    Undo,
    /// Take files out of an archive.
    Extract,
    /// Put files into a new archive.
    Compress,
    /// Change the tags of files (the Finder's tags on macOS, `user.xdg.tags` on Linux).
    SetTags,
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
        /// Permissions to give the folder once the whole run is over (it was made writable
        /// for the run): used when undo brings back a folder that was read-only.
        #[serde(default)]
        restore_mode: Option<u32>,
    },
    /// A new, empty file.
    MakeFile {
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
        /// The folder this one is a copy of: its owner and extended attributes are carried
        /// over, once the folder is full.
        #[serde(with = "pathcodec::opt_path", default)]
        src: Option<PathBuf>,
        #[serde(default)]
        uid: Option<u32>,
        #[serde(default)]
        gid: Option<u32>,
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
        /// Owner and group of the source, given to the copy when this process may.
        #[serde(default)]
        uid: Option<u32>,
        #[serde(default)]
        gid: Option<u32>,
        /// Bytes really stored, when that is less than `size` (a sparse file): what progress
        /// and free space are counted in.
        #[serde(default)]
        stored: Option<u64>,
        /// Later steps of this plan make more names for this file: its fingerprint must not
        /// depend on its link count.
        #[serde(default)]
        link_primary: bool,
    },
    /// Another name for a file this plan copied: the copies of files that were hard links of
    /// each other stay hard links of each other.
    HardLink {
        /// The copy already made.
        #[serde(with = "pathcodec::path")]
        existing: PathBuf,
        /// The new name.
        #[serde(with = "pathcodec::path")]
        link: PathBuf,
        /// Cross-device move: the source name that goes away with this one.
        #[serde(with = "pathcodec::opt_path", default)]
        src_link: Option<PathBuf>,
        /// Cross-device move: where `existing` came from (for the undo).
        #[serde(with = "pathcodec::opt_path", default)]
        src_existing: Option<PathBuf>,
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
    /// One file out of an archive, written under a temporary name and renamed into place
    /// when complete. The members of one archive are extracted in the order the archive
    /// holds them, in a single pass.
    ExtractFile {
        /// The member as a path (`/home/u/photos.zip/2024/a.jpg`): what the user sees.
        #[serde(with = "pathcodec::path")]
        src: PathBuf,
        /// The archive file.
        #[serde(with = "pathcodec::path")]
        archive: PathBuf,
        /// The member's position in the archive's list when the plan was made.
        entry: usize,
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        /// Declared size: what is written never exceeds it.
        size: u64,
        mode: Option<u32>,
        mtime: Option<Stamp>,
    },
    /// A symbolic link stored in an archive, created as a link and never followed.
    ExtractSymlink {
        #[serde(with = "pathcodec::path")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        #[serde(with = "pathcodec::path")]
        target: PathBuf,
    },
    /// A whole new archive, written under a temporary name and renamed into place.
    Compress {
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        format: ArchiveKind,
        items: Vec<CompressItem>,
        /// Bytes of file data that will be read.
        size: u64,
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
    /// Replace the tags of a file or folder. `before` is what it has now, checked when the
    /// step runs: tags changed in the meantime are left alone, so that undo never overwrites
    /// what somebody did after the operation.
    SetTags {
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
        before: Vec<rada_core::platform::Tag>,
        after: Vec<rada_core::platform::Tag>,
    },
}

/// What a step was about to do, written before it starts. Small on purpose: enough to see on
/// disk, after a crash, whether the step happened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Intent {
    /// A copy (of a file or a link) to `dst`, source left in place.
    Create {
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<String>,
        /// What the file is expected to be once written, so that recovery does not take
        /// for it a file somebody else put there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mtime: Option<Stamp>,
        /// For a link: where it was to point, so that recovery does not take for it a link
        /// (or a file) somebody else put there.
        #[serde(
            with = "pathcodec::opt_path",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        target: Option<PathBuf>,
        /// The step declares nothing to check what it wrote against (a new archive), so
        /// recovery leaves whatever it finds. Absent in records written before this existed.
        #[serde(default, skip_serializing_if = "is_false")]
        unverifiable: bool,
    },
    /// A cross-device move of a file (`symlink: false`) or a link.
    Move {
        #[serde(with = "pathcodec::path")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        symlink: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<String>,
        /// What the file is expected to be once written, so that recovery does not take
        /// for it a file somebody else put there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mtime: Option<Stamp>,
        /// For a link: where it was to point (see `Create`).
        #[serde(
            with = "pathcodec::opt_path",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        target: Option<PathBuf>,
    },
    Dir {
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
    },
    Rename {
        #[serde(with = "pathcodec::path")]
        from: PathBuf,
        #[serde(with = "pathcodec::path")]
        to: PathBuf,
    },
    Link {
        #[serde(with = "pathcodec::path")]
        existing: PathBuf,
        #[serde(with = "pathcodec::path")]
        link: PathBuf,
        #[serde(with = "pathcodec::opt_path", default)]
        src_link: Option<PathBuf>,
        #[serde(with = "pathcodec::opt_path", default)]
        src_existing: Option<PathBuf>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<String>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Record {
    /// Written before a step that creates something starts.
    Pending {
        id: String,
        intent: Intent,
    },
    Begin {
        id: String,
        time: i64,
        kind: OpKind,
        title: String,
        reversible: bool,
        totals: Totals,
        /// What was asked, so the operation can be planned again (redo).
        #[serde(default)]
        request: Option<OpRequest>,
        /// Set when this operation re-did an undone one.
        #[serde(default)]
        redo_of: Option<String>,
    },
    Undo {
        id: String,
        step: Step,
    },
    /// The hash of the contents of a file an undo step will remove, taken when the file was
    /// made: for files with several names, whose fingerprint cannot see a change of contents.
    Digest {
        id: String,
        #[serde(with = "pathcodec::path")]
        path: PathBuf,
        hash: String,
    },
    End {
        id: String,
        status: RunStatus,
        done: u64,
        failed: u64,
        bytes: u64,
        /// The first step that ran without its way back being written, when the journal
        /// could not be written during the run: undo covers what comes before.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unrecorded_from: Option<usize>,
    },
    Undone {
        id: String,
        time: i64,
        /// Indices (into the entry's undo steps) that are still not undone.
        remaining: Vec<usize>,
    },
    /// One undo step has been carried out (written as it happens, so that an undo that is
    /// cut short can be taken up again where it stopped).
    UndoneStep {
        id: String,
        index: usize,
    },
}
const TORN: &[u8] = b"#torn";

const KNOWN: [&str; 7] = [
    "pending",
    "begin",
    "undo",
    "digest",
    "end",
    "undone",
    "undone_step",
];

fn classify(line: &[u8], complete: bool) -> std::result::Result<Option<Record>, String> {
    let (body, closed) = match line.strip_suffix(TORN) {
        Some(body) => (body, true),
        None => (line, false),
    };
    let err = match serde_json::from_slice::<Record>(body) {
        Ok(r) => return Ok(Some(r)),
        Err(e) => e,
    };
    if !complete || closed {
        return Ok(None);
    }
    let unknown_type = serde_json::from_slice::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("t")?.as_str().map(|t| !KNOWN.contains(&t)))
        .unwrap_or(false);
    if unknown_type {
        Ok(None)
    } else {
        Err(err.to_string())
    }
}

/// What 0.6.1 makes of a journal: how many lines it reads, how many it skips, and the ones
/// it takes for damage, with why.
pub struct Reading {
    pub read: usize,
    pub skipped: usize,
    pub damaged: Vec<(usize, String)>,
}

pub fn read(journal: &str) -> Reading {
    let mut reading = Reading {
        read: 0,
        skipped: 0,
        damaged: Vec::new(),
    };
    for (n, line) in journal.lines().enumerate() {
        match classify(line.as_bytes(), true) {
            Ok(Some(_)) => reading.read += 1,
            Ok(None) => reading.skipped += 1,
            Err(why) => reading.damaged.push((n + 1, format!("{why}: {line}"))),
        }
    }
    reading
}
