//! Persistent journal: an append-only JSON-lines file in `$XDG_STATE_HOME/vela/`.
//!
//! Every operation writes `begin`, then one `undo` record per *completed* step (the
//! inverse step, in execution order), then `end`. Records are flushed at least every
//! few hundred milliseconds, so even a crash mid-copy leaves enough to undo what was
//! done. Replaying the file reconstructs the history; nothing else is stored.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::ops::{OpKind, OpRequest, RunStatus, Step, Totals};
use crate::{Error, Result};

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Record {
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
}

struct Writer {
    file: File,
    buf: Vec<u8>,
    last_flush: Instant,
}

pub struct Journal {
    path: PathBuf,
    writer: Mutex<Writer>,
    counter: AtomicU64,
}

const FLUSH_EVERY: Duration = Duration::from_millis(250);
const FLUSH_BYTES: usize = 64 * 1024;

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
            writer: Mutex::new(Writer {
                file,
                buf: Vec::new(),
                last_flush: Instant::now(),
            }),
            counter: AtomicU64::new(0),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn append(&self, rec: &Record, force: bool) -> Result<()> {
        let mut line = serde_json::to_vec(rec)?;
        line.push(b'\n');
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        w.buf.extend_from_slice(&line);
        if force || w.buf.len() >= FLUSH_BYTES || w.last_flush.elapsed() >= FLUSH_EVERY {
            Self::flush_locked(&mut w, &self.path, force)?;
        }
        Ok(())
    }

    fn flush_locked(w: &mut Writer, path: &Path, sync: bool) -> Result<()> {
        let buf = std::mem::take(&mut w.buf);
        // One write call per batch: lines of concurrent instances never interleave.
        w.file
            .write_all(&buf)
            .map_err(|e| Error::io("write journal", path, e))?;
        if sync {
            w.file
                .sync_data()
                .map_err(|e| Error::io("sync journal", path, e))?;
        }
        w.last_flush = Instant::now();
        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        Self::flush_locked(&mut w, &self.path, true)
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
        self.flush()?;
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

    /// All operations, oldest first.
    pub fn entries(&self) -> Result<Vec<JournalEntry>> {
        Ok(self.replay()?.0)
    }

    /// Entries plus the redo stack (ids, latest undo last).
    fn replay(&self) -> Result<(Vec<JournalEntry>, Vec<String>)> {
        let mut order: Vec<String> = Vec::new();
        let mut map: HashMap<String, JournalEntry> = HashMap::new();
        let mut redo_stack: Vec<String> = Vec::new();
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
            Record::Undone { id, remaining, .. } => {
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
        self.read_records(|r| match r {
            Record::Undo { id: rid, step } if rid == id => steps.push(step),
            Record::Undone {
                id: rid,
                remaining: rem,
                ..
            } if rid == id => remaining = Some(rem),
            _ => {}
        })?;
        let remaining = remaining.unwrap_or_else(|| (0..steps.len()).collect());
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
