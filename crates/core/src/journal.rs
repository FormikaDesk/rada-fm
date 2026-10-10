//! Persistent journal: an append-only JSON-lines file in `$XDG_STATE_HOME/rada/`.
//!
//! Every operation writes `begin`, then for each step that creates something a `pending`
//! record *before* the step and an `undo` record (the inverse step, in execution order) once
//! it has completed, then `end`. Every record is handed to the operating system as soon as it
//! is written, so even a killed process (SIGKILL, power button on the process, not on the
//! disk) leaves an exact account: at most the very last step is in doubt, and `pending`
//! says which one, so that the next start can settle it (see `ops::recover`). Replaying the
//! file reconstructs the history; nothing else is stored.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

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
    },
    /// A cross-device move of a file (`symlink: false`) or a link.
    Move {
        #[serde(with = "pathcodec::path")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        dst: PathBuf,
        symlink: bool,
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
    },
}

impl Intent {
    /// The intent of a step, for the steps that create something.
    pub fn of(step: &Step) -> Option<Intent> {
        Some(match step {
            Step::CopyFile {
                src,
                dst,
                remove_source,
                ..
            } => {
                if *remove_source {
                    Intent::Move {
                        src: src.clone(),
                        dst: dst.clone(),
                        symlink: false,
                    }
                } else {
                    Intent::Create { dst: dst.clone() }
                }
            }
            Step::CopySymlink {
                src,
                dst,
                remove_source,
                ..
            } => {
                if *remove_source {
                    Intent::Move {
                        src: src.clone(),
                        dst: dst.clone(),
                        symlink: true,
                    }
                } else {
                    Intent::Create { dst: dst.clone() }
                }
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
            },
            Step::ExtractFile { dst, .. }
            | Step::ExtractSymlink { dst, .. }
            | Step::Compress { dst, .. } => Intent::Create { dst: dst.clone() },
            Step::MakeDir { path, .. } => Intent::Dir { path: path.clone() },
            Step::MakeFile { path, .. } => Intent::Create { dst: path.clone() },
            Step::Rename { from, to } => Intent::Rename {
                from: from.clone(),
                to: to.clone(),
            },
            _ => return None,
        })
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
    End {
        id: String,
        status: RunStatus,
        done: u64,
        failed: u64,
        bytes: u64,
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
    pub request: Option<OpRequest>,
    /// Fully undone, and nothing but other undos happened since: `Ctrl+Y` can redo it.
    pub redoable: bool,
}

impl JournalEntry {
    /// Can `u` offer to undo this?
    pub fn is_undoable(&self) -> bool {
        self.reversible
            && self.undo_steps > 0
            && self.undo_state == UndoState::NotUndone
            && self.kind != OpKind::Undo
    }

    pub fn can_retry_undo(&self) -> bool {
        self.reversible && matches!(self.undo_state, UndoState::Partial { .. })
    }

    /// The operation in a few words, as the Undo button says what it would undo:
    /// "copy of 3 items", "new folder", "trash of 1 item".
    pub fn describe(&self) -> String {
        let n = self.totals.items.max(1);
        let items = if n == 1 {
            "1 item".to_string()
        } else {
            format!("{n} items")
        };
        match self.kind {
            OpKind::Copy => format!("copy of {items}"),
            OpKind::Move => format!("move of {items}"),
            OpKind::Rename | OpKind::BulkRename => format!("rename of {items}"),
            OpKind::MakeDir => "new folder".to_string(),
            OpKind::MakeFile => "new file".to_string(),
            OpKind::Trash => format!("trash of {items}"),
            OpKind::Delete => format!("deletion of {items}"),
            OpKind::Extract => format!("extraction of {items}"),
            OpKind::Compress => format!("compression of {items}"),
            OpKind::Undo => "undo".to_string(),
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
            create_private_dir(dir).map_err(|e| Error::io("create journal folder", dir, e))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::io("open journal", &path, e))?;
        Ok(Journal {
            path,
            writer: Mutex::new(Writer { file }),
            counter: AtomicU64::new(0),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write one record. It goes to the operating system at once, in one `write` call (lines
    /// of concurrent instances never interleave); `force` also waits for the disk.
    fn append(&self, rec: &Record, force: bool) -> Result<()> {
        let mut line = serde_json::to_vec(rec)?;
        line.push(b'\n');
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
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
        self.append(
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
        )?;
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

    pub fn end(
        &self,
        id: &str,
        status: RunStatus,
        done: u64,
        failed: u64,
        bytes: u64,
    ) -> Result<()> {
        self.append(
            &Record::End {
                id: id.to_string(),
                status,
                done,
                failed,
                bytes,
            },
            true,
        )
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

    fn read_records(&self, mut f: impl FnMut(Record)) -> Result<()> {
        let file = File::open(&self.path).map_err(|e| Error::io("read journal", &self.path, e))?;
        for (n, line) in BufReader::new(file).split(b'\n').enumerate() {
            let line = line.map_err(|e| Error::io("read journal", &self.path, e))?;
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<Record>(&line) {
                Ok(r) => f(r),
                // A torn last line after a crash, or a record from a newer version.
                Err(e) => tracing::warn!("journal line {}: {e}", n + 1),
            }
        }
        Ok(())
    }

    /// Operations that never wrote their `end` record (the process died), with the step that was
    /// in doubt, if any: the last `pending` record, when no `undo` record came after it.
    pub fn interrupted(&self) -> Result<Vec<Interrupted>> {
        let entries = self.entries()?;
        let mut last: HashMap<String, (Intent, bool)> = HashMap::new();
        self.read_records(|r| match r {
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
            .filter(|e| e.status == EntryStatus::Interrupted)
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
        self.read_records(|r| match r {
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
                        request,
                        redoable: false,
                    },
                );
            }
            Record::Pending { .. } => {}
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
            } => {
                if let Some(e) = map.get_mut(&id) {
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
        for id in &redo_stack {
            if let Some(e) = map.get_mut(id) {
                e.redoable = e.request.is_some() && e.reversible;
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

    /// Undo steps of one entry, in execution order, with the indices still outstanding.
    pub fn load_undo(&self, id: &str) -> Result<(Vec<Step>, Vec<usize>)> {
        let mut steps = Vec::new();
        let mut remaining: Option<Vec<usize>> = None;
        // Steps carried out since the last full account (or from the start).
        let mut done_since: Vec<usize> = Vec::new();
        self.read_records(|r| match r {
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

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}
