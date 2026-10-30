//! Persistent journal: an append-only JSON-lines file in `$XDG_STATE_HOME/rada/`.
//!
//! Every operation writes `begin`, then for each step that creates something a `pending`
//! record *before* the step and an `undo` record (the inverse step, in execution order) once
//! it has completed, then `end`. Every record is handed to the operating system as soon as it
//! is written, so even a killed process (SIGKILL, power button on the process, not on the
//! disk) leaves an exact account: at most the very last step is in doubt, and `pending`
//! says which one, so that the next start can settle it (see `ops::recover`). Replaying the
//! file reconstructs the history; nothing else is stored.
//!
//! A line that cannot be read is one of three things. The last line of the file without its
//! newline is a write cut short by a crash: it is skipped, and the next record that is written
//! closes it with a mark (`#torn`) so that it does not swallow that record. A well-formed line
//! of a record type this version does not know was written by a newer rada: it is skipped. Any
//! other line that cannot be read, with more lines after it, is damage: the operations that
//! had not finished before it are neither settled nor undone (`JournalEntry::damaged`), and
//! the person is told where the file is.
//!
//! An operation that is running holds a lock (see `oplock`) from its `begin` record to its
//! `end` record. That is how another rada tells a copy that has been going on for two days
//! from one whose process is dead: the lock of a dead process is free.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fs::Stamp;
use crate::i18n::{t, tf};
use crate::oplock::OpLock;
use crate::ops::{OpKind, OpRequest, RunStatus, Step, Totals};
use crate::pathcodec;
use crate::{Error, Result};

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

fn is_false(b: &bool) -> bool {
    !*b
}

impl Intent {
    /// The link target a symlink step was to give its destination.
    fn pointing_to(mut self, to: &Path) -> Intent {
        if let Intent::Create { target, .. } | Intent::Move { target, .. } = &mut self {
            *target = Some(to.to_path_buf());
        }
        self
    }

    /// Marks a step whose result nothing can be checked against.
    fn unverifiable(mut self) -> Intent {
        if let Intent::Create { unverifiable, .. } = &mut self {
            *unverifiable = true;
        }
        self
    }

    /// The name the step gives its temporary file (see `ExecHandler::temp_tag`), so that
    /// settling the step removes that file and no other.
    pub fn with_temp(mut self, tag: Option<String>) -> Intent {
        if let Intent::Create { temp, .. } | Intent::Move { temp, .. } | Intent::Link { temp, .. } =
            &mut self
        {
            *temp = tag;
        }
        self
    }

    /// The intent of a step, for the steps that create something.
    pub fn of(step: &Step) -> Option<Intent> {
        Some(match step {
            Step::CopyFile {
                src,
                dst,
                size,
                mtime,
                remove_source,
                ..
            } => {
                let expected = Some(*size);
                if *remove_source {
                    Intent::moved(src, dst, false, expected, *mtime)
                } else {
                    Intent::create(dst, expected, *mtime)
                }
            }
            Step::CopySymlink {
                src,
                dst,
                target,
                remove_source,
            } => {
                let intent = if *remove_source {
                    Intent::moved(src, dst, true, None, None)
                } else {
                    Intent::create(dst, None, None)
                };
                intent.pointing_to(target)
            }
            Step::HardLink {
                existing,
                link,
                src_link,
                src_existing,
            } => Intent::Link {
                existing: existing.clone(),
                link: link.clone(),
                src_link: src_link.clone(),
                src_existing: src_existing.clone(),
                temp: None,
            },
            Step::ExtractFile {
                dst, size, mtime, ..
            } => Intent::create(dst, Some(*size), *mtime),
            Step::ExtractSymlink { dst, target, .. } => {
                Intent::create(dst, None, None).pointing_to(target)
            }
            Step::Compress { dst, .. } => Intent::create(dst, None, None).unverifiable(),
            Step::MakeDir { path, .. } => Intent::Dir { path: path.clone() },
            Step::MakeFile { path, .. } => Intent::create(path, Some(0), None),
            Step::Rename { from, to } => Intent::Rename {
                from: from.clone(),
                to: to.clone(),
            },
            _ => return None,
        })
    }

    fn create(dst: &Path, size: Option<u64>, mtime: Option<Stamp>) -> Intent {
        Intent::Create {
            dst: dst.to_path_buf(),
            temp: None,
            size,
            mtime,
            target: None,
            unverifiable: false,
        }
    }

    fn moved(
        src: &Path,
        dst: &Path,
        symlink: bool,
        size: Option<u64>,
        mtime: Option<Stamp>,
    ) -> Intent {
        Intent::Move {
            src: src.to_path_buf(),
            dst: dst.to_path_buf(),
            symlink,
            temp: None,
            size,
            mtime,
            target: None,
        }
    }
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

/// An operation the process of which died, and what is known about the step it died in.
#[derive(Clone, Debug)]
pub struct Interrupted {
    pub entry: JournalEntry,
    pub in_doubt: Option<Intent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryStatus {
    /// No `end` record: the process died or was killed mid-operation.
    Interrupted,
    Finished(RunStatus),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoState {
    NotUndone,
    Partial { remaining: usize },
    Undone,
}

/// One operation as shown in the history (no steps loaded).
#[derive(Clone, Debug)]
pub struct JournalEntry {
    pub id: String,
    pub time: i64,
    pub kind: OpKind,
    pub title: String,
    pub reversible: bool,
    pub totals: Totals,
    pub status: EntryStatus,
    pub done: u64,
    pub failed: u64,
    pub bytes: u64,
    pub undo_steps: usize,
    pub undo_state: UndoState,
    /// The first step that ran without its way back being written (the journal could not be
    /// written, and the person chose to go on): undo covers only what comes before.
    pub unrecorded_from: Option<usize>,
    pub request: Option<OpRequest>,
    /// Fully undone, and nothing but other undos happened since: `Ctrl+Y` can redo it.
    pub redoable: bool,
    /// The line of the first unreadable record of the journal, when this operation had not
    /// finished before it: what the journal says about it cannot be trusted, so it is neither
    /// settled nor undone.
    pub damaged: Option<usize>,
}

impl JournalEntry {
    /// Can `u` offer to undo this?
    pub fn is_undoable(&self) -> bool {
        self.reversible
            && self.damaged.is_none()
            && self.undo_steps > 0
            && self.undo_state == UndoState::NotUndone
            && self.kind != OpKind::Undo
    }

    pub fn can_retry_undo(&self) -> bool {
        self.reversible
            && self.damaged.is_none()
            && matches!(self.undo_state, UndoState::Partial { .. })
    }

    /// The operation in a few words, as the Undo button says what it would undo:
    /// "copy of 3 items", "new folder", "trash of 1 item".
    pub fn describe(&self) -> String {
        use crate::i18n::Key::*;
        let items = crate::i18n::count(self.totals.items.max(1), ItemOne, ItemMany);
        match self.kind {
            OpKind::Copy => tf(DoneCopy, &[&items]),
            OpKind::Move => tf(DoneMove, &[&items]),
            OpKind::Rename | OpKind::BulkRename => tf(DoneRename, &[&items]),
            OpKind::MakeDir => t(DoneNewFolder).to_string(),
            OpKind::MakeFile => t(DoneNewFile).to_string(),
            OpKind::Trash => tf(DoneTrash, &[&items]),
            OpKind::Delete => tf(DoneDelete, &[&items]),
            OpKind::Extract => tf(DoneExtract, &[&items]),
            OpKind::Compress => tf(DoneCompress, &[&items]),
            OpKind::SetTags => tf(DoneTags, &[&items]),
            OpKind::Undo => t(DoneUndo).to_string(),
        }
    }
}

struct Writer {
    file: File,
}

pub struct Journal {
    path: PathBuf,
    writer: Mutex<Writer>,
    counter: AtomicU64,
    /// The locks of the operations this process is running, by id.
    running: Mutex<HashMap<String, OpLock>>,
    /// The record that is to fail, and how many of its kind are still to be written before it
    /// (tests).
    #[cfg(any(test, feature = "testutil"))]
    fault: Mutex<Option<(RecordKind, usize)>>,
}

/// The records an operation writes for each of its steps.
#[cfg(any(test, feature = "testutil"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    /// The intent, before a step that creates something.
    Pending,
    /// The way back of a step that finished.
    Undo,
    /// The hash of a file that undo will check before it removes its last name.
    Digest,
}

/// Closes a line that a crash cut short (see the module documentation).
const TORN: &[u8] = b"#torn";

/// The record types this version writes; a well-formed line of another type is from a newer
/// rada.
const KNOWN: [&str; 7] = [
    "pending",
    "begin",
    "undo",
    "digest",
    "end",
    "undone",
    "undone_step",
];

/// Does the file end with a newline (or is it empty)?
fn ends_with_newline(file: &File) -> std::io::Result<bool> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    let mut last = [0u8; 1];
    #[cfg(unix)]
    let n = std::os::unix::fs::FileExt::read_at(file, &mut last, len - 1)?;
    #[cfg(windows)]
    let n = std::os::windows::fs::FileExt::seek_read(file, &mut last, len - 1)?;
    Ok(n == 0 || last[0] == b'\n')
}

/// What a line is: a record, `None` for a line to skip without worry (a write cut by a
/// crash, a record of a newer rada), or why it is damaged. `complete`: it ended with a newline.
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

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Journal {
    /// Open (creating directories and the file) the journal at `path`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Journal> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            crate::statefile::create_private_dir(dir)
                .map_err(|e| Error::io("create journal folder", dir, e))?;
        }
        let file = crate::statefile::private_file_options()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)
            .map_err(|e| Error::io("open journal", &path, e))?;
        Ok(Journal {
            path,
            writer: Mutex::new(Writer { file }),
            counter: AtomicU64::new(0),
            running: Mutex::new(HashMap::new()),
            #[cfg(any(test, feature = "testutil"))]
            fault: Mutex::new(None),
        })
    }

    /// The `nth` (from 1) record of `kind` written from now on fails, once, as a disk that is
    /// full for a moment would. Records of other kinds are written as they are, so the test
    /// does not depend on how many records a step makes.
    #[cfg(any(test, feature = "testutil"))]
    pub fn fail_record(&self, kind: RecordKind, nth: usize) {
        *self.fault.lock().unwrap() = Some((kind, nth.max(1)));
    }

    #[cfg(any(test, feature = "testutil"))]
    fn injected_fault(&self, rec: &Record) -> Result<()> {
        let kind = match rec {
            Record::Pending { .. } => RecordKind::Pending,
            Record::Undo { .. } => RecordKind::Undo,
            Record::Digest { .. } => RecordKind::Digest,
            _ => return Ok(()),
        };
        let mut fault = self.fault.lock().unwrap();
        match *fault {
            Some((k, 1)) if k == kind => {
                *fault = None;
                Err(Error::io(
                    "write journal",
                    &self.path,
                    std::io::Error::other("no space left on device"),
                ))
            }
            Some((k, n)) if k == kind => {
                *fault = Some((k, n - 1));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock_dir(&self) -> PathBuf {
        self.path.parent().unwrap_or(Path::new("")).join("locks")
    }

    fn take_lock(&self, name: &str) -> Result<Option<OpLock>> {
        let dir = self.lock_dir();
        OpLock::try_take(&dir, name).map_err(|e| Error::io("lock", dir.join(name), e))
    }

    fn running(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, OpLock>>> {
        self.running
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))
    }

    /// Write one record. It goes to the operating system at once, in one `write` call (lines
    /// of concurrent instances never interleave); `force` also waits for the disk.
    fn append(&self, rec: &Record, force: bool) -> Result<()> {
        #[cfg(any(test, feature = "testutil"))]
        self.injected_fault(rec)?;
        let mut line = serde_json::to_vec(rec)?;
        line.push(b'\n');
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        // A process that died in the middle of a write left a line without its end: close it,
        // in the same write, so that this record is not glued to it.
        if !ends_with_newline(&w.file).map_err(|e| Error::io("read journal", &self.path, e))? {
            line.splice(0..0, TORN.iter().chain(b"\n").copied());
        }
        w.file
            .write_all(&line)
            .map_err(|e| Error::io("write journal", &self.path, e))?;
        if force {
            w.file
                .sync_data()
                .map_err(|e| Error::io("sync journal", &self.path, e))?;
        }
        Ok(())
    }

    /// Wait until everything written so far is on the disk.
    pub fn flush(&self) -> Result<()> {
        let w = self
            .writer
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        w.file
            .sync_data()
            .map_err(|e| Error::io("sync journal", &self.path, e))
    }

    // ---------------------------------------------------------------- writing

    pub fn begin(
        &self,
        kind: OpKind,
        title: &str,
        reversible: bool,
        totals: Totals,
        request: Option<&OpRequest>,
        redo_of: Option<&str>,
    ) -> Result<String> {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let id = format!("{ms:x}-{:x}-{n}", std::process::id());
        // Before the record: whoever sees the operation in the file finds its lock taken.
        let lock = self
            .take_lock(&format!("{id}.lock"))?
            .ok_or_else(|| Error::Journal(format!("operation {id} is locked already")))?;
        let written = self.append(
            &Record::Begin {
                id: id.clone(),
                time: now_secs(),
                kind,
                title: title.to_string(),
                reversible,
                totals,
                request: request.cloned(),
                redo_of: redo_of.map(str::to_string),
            },
            true,
        );
        match written {
            Ok(()) => self.running()?.insert(id.clone(), lock),
            Err(e) => {
                lock.release();
                return Err(e);
            }
        };
        Ok(id)
    }

    /// A step that creates something is about to run.
    pub fn pending(&self, id: &str, intent: Intent) -> Result<()> {
        self.append(
            &Record::Pending {
                id: id.to_string(),
                intent,
            },
            false,
        )
    }

    pub fn record_undo(&self, id: &str, step: Step) -> Result<()> {
        self.append(
            &Record::Undo {
                id: id.to_string(),
                step,
            },
            false,
        )
    }

    pub fn record_digest(&self, id: &str, path: &Path, hash: &str) -> Result<()> {
        self.append(
            &Record::Digest {
                id: id.to_string(),
                path: path.to_path_buf(),
                hash: hash.to_string(),
            },
            false,
        )
    }

    pub fn end(
        &self,
        id: &str,
        status: RunStatus,
        done: u64,
        failed: u64,
        bytes: u64,
    ) -> Result<()> {
        self.end_unrecorded(id, status, done, failed, bytes, None)
    }

    /// [`Journal::end`] for a run in which the steps from `unrecorded_from` on were not
    /// recorded: the entry says so, and undo covers only what comes before.
    pub fn end_unrecorded(
        &self,
        id: &str,
        status: RunStatus,
        done: u64,
        failed: u64,
        bytes: u64,
        unrecorded_from: Option<usize>,
    ) -> Result<()> {
        self.append(
            &Record::End {
                id: id.to_string(),
                status,
                done,
                failed,
                bytes,
                unrecorded_from,
            },
            true,
        )?;
        // Only once the operation is on record as finished: if the record could not be
        // written the lock file stays, and the operation is settled like an interrupted one.
        if let Some(lock) = self.running()?.remove(id) {
            lock.release();
        }
        Ok(())
    }

    /// One step of an undo has just been carried out.
    pub fn mark_step_undone(&self, id: &str, index: usize) -> Result<()> {
        self.append(
            &Record::UndoneStep {
                id: id.to_string(),
                index,
            },
            false,
        )
    }

    /// Record that (part of) an entry has been undone. `remaining` are the undo-step
    /// indices that are still outstanding; empty means fully undone.
    pub fn mark_undone(&self, id: &str, remaining: Vec<usize>) -> Result<()> {
        self.append(
            &Record::Undone {
                id: id.to_string(),
                time: now_secs(),
                remaining,
            },
            true,
        )
    }

    // ---------------------------------------------------------------- reading

    /// Hand every readable record, with its line number, to `f`. The answer is the line of
    /// the first damaged one, if any (see the module documentation).
    fn read_records(&self, mut f: impl FnMut(usize, Record)) -> Result<Option<usize>> {
        let file = File::open(&self.path).map_err(|e| Error::io("read journal", &self.path, e))?;
        let mut reader = BufReader::new(file);
        let (mut line, mut n, mut damage) = (Vec::new(), 0, None);
        loop {
            line.clear();
            let got = reader
                .read_until(b'\n', &mut line)
                .map_err(|e| Error::io("read journal", &self.path, e))?;
            if got == 0 {
                return Ok(damage);
            }
            n += 1;
            let complete = line.last() == Some(&b'\n');
            if complete {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            match classify(&line, complete) {
                Ok(Some(r)) => f(n, r),
                Ok(None) => tracing::debug!("journal line {n}: not a record of this version"),
                Err(e) => {
                    tracing::warn!("journal line {n} cannot be read: {e}");
                    damage.get_or_insert(n);
                }
            }
        }
    }

    /// Operations that never wrote their `end` record (the process died), with the step that was
    /// in doubt, if any: the last `pending` record, when no `undo` record came after it.
    pub fn interrupted(&self) -> Result<Vec<Interrupted>> {
        let entries = self.entries()?;
        let mut last: HashMap<String, (Intent, bool)> = HashMap::new();
        self.read_records(|_, r| match r {
            Record::Pending { id, intent } => {
                last.insert(id, (intent, false));
            }
            Record::Undo { id, .. } => {
                if let Some(l) = last.get_mut(&id) {
                    l.1 = true;
                }
            }
            _ => {}
        })?;
        Ok(entries
            .into_iter()
            .filter(|e| e.status == EntryStatus::Interrupted && e.damaged.is_none())
            .map(|entry| {
                let in_doubt = last
                    .remove(&entry.id)
                    .and_then(|(intent, resolved)| (!resolved).then_some(intent));
                Interrupted { entry, in_doubt }
            })
            .collect())
    }

    /// Settle an interrupted operation: undo records for steps found completed but not
    /// recorded, then an `end` record so that it is never looked at again.
    pub fn settle(&self, entry: &JournalEntry, adopted: Vec<Step>) -> Result<()> {
        let extra = adopted.len();
        for step in adopted {
            self.record_undo(&entry.id, step)?;
        }
        self.end(
            &entry.id,
            RunStatus::Interrupted,
            (entry.undo_steps + extra) as u64,
            0,
            0,
        )
    }

    /// Take the lock of an interrupted operation, which is what allows settling it. `None`
    /// when a running process holds it. `created` on the lock says that no lock file existed:
    /// the operation was begun by a rada that did not lock, so only its process id and its age
    /// say whether it is still running.
    pub fn claim(&self, id: &str) -> Result<Option<OpLock>> {
        self.take_lock(&format!("{id}.lock"))
    }

    /// Is `id` still without an `end` record? (Asked again once the lock is held, because the
    /// operation may have finished between the listing and the lock.)
    pub fn is_interrupted(&self, id: &str) -> Result<bool> {
        Ok(self
            .entries()?
            .iter()
            .any(|e| e.id == id && e.status == EntryStatus::Interrupted))
    }

    /// Hold the undo of `id` for as long as the lock lives. A second rada undoing the same
    /// operation would run the same steps against the same files.
    pub fn lock_undo(&self, id: &str) -> Result<OpLock> {
        self.take_lock(&format!("{id}.undo.lock"))?.ok_or_else(|| {
            Error::Journal(format!(
                "\"{id}\" is being undone by another rada right now; wait for it to finish"
            ))
        })
    }

    /// The line (counted from 1) of the first record in the middle of the file that cannot be
    /// read. A write cut short at the end of the file, and a record of a newer rada, are not
    /// damage. Operations that finished before that line can still be trusted; those after it
    /// can neither be settled nor undone by rada.
    pub fn damage(&self) -> Result<Option<usize>> {
        self.read_records(|_, _| {})
    }

    /// All operations, oldest first.
    pub fn entries(&self) -> Result<Vec<JournalEntry>> {
        Ok(self.replay()?.0)
    }

    /// Entries plus the redo stack (ids, latest undo last).
    fn replay(&self) -> Result<(Vec<JournalEntry>, Vec<String>)> {
        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, JournalEntry> = HashMap::new();
        let mut redo_stack: Vec<String> = Vec::new();
        // Undo steps still outstanding for entries whose undo is under way.
        let mut left: HashMap<String, usize> = HashMap::new();
        // The line each operation's `end` record is on.
        let mut ended: HashMap<String, usize> = HashMap::new();
        let damage = self.read_records(|n, r| match r {
            Record::Begin {
                id,
                time,
                kind,
                title,
                reversible,
                totals,
                request,
                redo_of,
            } => {
                // A new operation ends the redo history, unless it *is* a redo.
                match redo_of {
                    Some(r) => redo_stack.retain(|x| *x != r),
                    None => redo_stack.clear(),
                }
                order.push(id.clone());
                map.insert(
                    id.clone(),
                    JournalEntry {
                        id,
                        time,
                        kind,
                        title,
                        reversible,
                        totals,
                        status: EntryStatus::Interrupted,
                        done: 0,
                        failed: 0,
                        bytes: 0,
                        undo_steps: 0,
                        undo_state: UndoState::NotUndone,
                        unrecorded_from: None,
                        request,
                        redoable: false,
                        damaged: None,
                    },
                );
            }
            Record::Pending { .. } | Record::Digest { .. } => {}
            Record::Undo { id, .. } => {
                if let Some(e) = map.get_mut(&id) {
                    e.undo_steps += 1;
                }
            }
            Record::End {
                id,
                status,
                done,
                failed,
                bytes,
                unrecorded_from,
            } => {
                ended.insert(id.clone(), n);
                if let Some(e) = map.get_mut(&id) {
                    e.unrecorded_from = unrecorded_from;
                    e.status = EntryStatus::Finished(status);
                    e.done = done;
                    e.failed = failed;
                    e.bytes = bytes;
                }
            }
            Record::UndoneStep { id, .. } => {
                if let Some(e) = map.get_mut(&id) {
                    let l = left.entry(id.clone()).or_insert(e.undo_steps);
                    *l = l.saturating_sub(1);
                    e.undo_state = if *l == 0 {
                        UndoState::Undone
                    } else {
                        UndoState::Partial { remaining: *l }
                    };
                }
            }
            Record::Undone { id, remaining, .. } => {
                left.insert(id.clone(), remaining.len());
                if remaining.is_empty() {
                    redo_stack.retain(|x| *x != id);
                    redo_stack.push(id.clone());
                }
                if let Some(e) = map.get_mut(&id) {
                    e.undo_state = if remaining.is_empty() {
                        UndoState::Undone
                    } else {
                        UndoState::Partial {
                            remaining: remaining.len(),
                        }
                    };
                }
            }
        })?;
        if let Some(bad) = damage {
            for (id, e) in map.iter_mut() {
                if !ended.get(id).is_some_and(|&n| n < bad) {
                    e.damaged = Some(bad);
                }
            }
        }
        for id in &redo_stack {
            if let Some(e) = map.get_mut(id) {
                e.redoable = e.request.is_some() && e.reversible && e.damaged.is_none();
            }
        }
        Ok((
            order.into_iter().filter_map(|id| map.remove(&id)).collect(),
            redo_stack,
        ))
    }

    /// The operation `Ctrl+Y` would plan again: the most recently undone one, provided
    /// nothing but other undos happened since.
    pub fn last_redoable(&self) -> Result<Option<JournalEntry>> {
        let (entries, stack) = self.replay()?;
        Ok(stack
            .iter()
            .rev()
            .find_map(|id| entries.iter().find(|e| e.id == *id && e.redoable).cloned()))
    }

    /// The most recent operation that `u` can undo.
    pub fn last_undoable(&self) -> Result<Option<JournalEntry>> {
        Ok(self
            .entries()?
            .into_iter()
            .rev()
            .find(JournalEntry::is_undoable))
    }

    /// The hashes recorded for the files of `id` (see `record_digest`), by path.
    pub fn load_digests(&self, id: &str) -> Result<HashMap<PathBuf, String>> {
        let mut out = HashMap::new();
        self.read_records(|_, r| {
            if let Record::Digest {
                id: rid,
                path,
                hash,
            } = r
                && rid == id
            {
                out.insert(path, hash);
            }
        })?;
        Ok(out)
    }

    /// Undo steps of one entry, in execution order, with the indices still outstanding.
    pub fn load_undo(&self, id: &str) -> Result<(Vec<Step>, Vec<usize>)> {
        let mut steps = Vec::new();
        let mut remaining: Option<Vec<usize>> = None;
        // Steps carried out since the last full account (or from the start).
        let mut done_since: Vec<usize> = Vec::new();
        self.read_records(|_, r| match r {
            Record::Undo { id: rid, step } if rid == id => steps.push(step),
            Record::Undone {
                id: rid,
                remaining: rem,
                ..
            } if rid == id => {
                remaining = Some(rem);
                done_since.clear();
            }
            Record::UndoneStep { id: rid, index } if rid == id => done_since.push(index),
            _ => {}
        })?;
        let mut remaining = remaining.unwrap_or_else(|| (0..steps.len()).collect());
        remaining.retain(|i| !done_since.contains(i));
        Ok((steps, remaining))
    }
}
