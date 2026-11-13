//! Reading metadata in the background: one file after the other on a thread of its own, the
//! answers kept for as long as the file stays as it was.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Sender, unbounded};

use super::{Meta, MetaLimits, MetaTools};
use crate::events::CoreEvent;
use crate::fs::{FileKind, Fingerprint};

/// What was read, by file, with the fingerprint the file had: an answer is only given back
/// while the file still has it.
#[derive(Debug, Default)]
pub struct MetaCache {
    slots: HashMap<PathBuf, (Fingerprint, Arc<Meta>)>,
}

impl MetaCache {
    const MAX_SLOTS: usize = 4096;

    pub fn get(&self, path: &Path, now: &Fingerprint) -> Option<Arc<Meta>> {
        self.slots
            .get(path)
            .filter(|(fp, _)| fp.diff(now).is_none())
            .map(|(_, m)| m.clone())
    }

    pub fn put(&mut self, path: PathBuf, fp: Fingerprint, meta: Arc<Meta>) {
        if self.slots.len() >= Self::MAX_SLOTS && !self.slots.contains_key(&path) {
            self.slots.clear();
        }
        self.slots.insert(path, (fp, meta));
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

/// The fingerprint of the file `path` leads to (a link is followed, as reading does).
pub fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let m = std::fs::metadata(path).ok()?;
    if !m.is_file() {
        return None;
    }
    #[cfg(unix)]
    let (ctime, nlink) = {
        use std::os::unix::fs::MetadataExt;
        let secs = u64::try_from(m.ctime()).ok();
        let at = secs.map(|s| {
            std::time::UNIX_EPOCH
                + std::time::Duration::new(s, m.ctime_nsec().clamp(0, 999_999_999) as u32)
        });
        (at, m.nlink())
    };
    #[cfg(not(unix))]
    let (ctime, nlink) = (None, 1);
    Some(Fingerprint::new(
        FileKind::File,
        m.len(),
        m.modified().ok(),
        ctime,
        nlink,
    ))
}

/// One file's answer.
#[derive(Clone, Debug)]
pub struct MetaEvent {
    /// The request it answers.
    pub epoch: u64,
    pub path: PathBuf,
    /// The file as it was read; `None` when it is not a regular file.
    pub fingerprint: Option<Fingerprint>,
    pub meta: Arc<Meta>,
    /// How many files of the request have been answered, this one included, and how many it
    /// asked for.
    pub done: usize,
    pub total: usize,
}

struct MetaReq {
    epoch: u64,
    paths: Vec<PathBuf>,
}

/// Reads files one after the other on a thread of its own.
#[derive(Clone)]
pub struct MetaWorker {
    tx: Sender<MetaReq>,
    newest: Arc<AtomicU64>,
}

impl MetaWorker {
    pub fn spawn(tools: MetaTools, limits: MetaLimits, out: Sender<CoreEvent>) -> MetaWorker {
        let (tx, rx) = unbounded::<MetaReq>();
        let newest = Arc::new(AtomicU64::new(0));
        let current = newest.clone();
        std::thread::Builder::new()
            .name("rada-meta".into())
            .spawn(move || {
                let mut cache = MetaCache::default();
                while let Ok(req) = rx.recv() {
                    let run = Run {
                        tools: &tools,
                        limits: &limits,
                        newest: &current,
                        out: &out,
                    };
                    if !run.answer(req, &mut cache) {
                        return;
                    }
                }
            })
            .expect("spawn metadata worker");
        MetaWorker { tx, newest }
    }

    /// Read these files, in this order, unless a newer epoch is announced first. `epoch`
    /// should be the one last given to [`newest`](Self::newest): a request of any other is
    /// dropped.
    pub fn request(&self, epoch: u64, paths: Vec<PathBuf>) {
        let _ = self.tx.send(MetaReq { epoch, paths });
    }

    /// From now on only requests of `epoch` are answered: the files of older ones still to be
    /// read are not, and the one being read is given up.
    pub fn newest(&self, epoch: u64) {
        self.newest.store(epoch, Ordering::SeqCst);
    }
}

struct Run<'a> {
    tools: &'a MetaTools,
    limits: &'a MetaLimits,
    newest: &'a AtomicU64,
    out: &'a Sender<CoreEvent>,
}

impl Run<'_> {
    /// Answer `req` file by file. False when nobody listens any more.
    fn answer(&self, req: MetaReq, cache: &mut MetaCache) -> bool {
        let stale = || self.newest.load(Ordering::SeqCst) != req.epoch;
        let total = req.paths.len();
        for (i, path) in req.paths.into_iter().enumerate() {
            if stale() {
                break;
            }
            let fp = fingerprint(&path);
            let meta = match fp.as_ref().and_then(|f| cache.get(&path, f)) {
                Some(known) => known,
                None => {
                    let read = Arc::new(super::read(&path, self.tools, self.limits, &stale));
                    // A read given up half way says nothing about the file.
                    if stale() {
                        break;
                    }
                    if let Some(f) = &fp {
                        cache.put(path.clone(), f.clone(), read.clone());
                    }
                    read
                }
            };
            let ev = MetaEvent {
                epoch: req.epoch,
                path,
                fingerprint: fp,
                meta,
                done: i + 1,
                total,
            };
            if self.out.send(CoreEvent::Meta(ev)).is_err() {
                return false;
            }
        }
        true
    }
}
