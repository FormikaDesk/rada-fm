//! Live updates: a `notify` watcher with debouncing, on its own thread.
//!
//! The UI never re-reads a directory because a key was pressed; it re-reads (or patches)
//! because the filesystem said something changed. Bursts are coalesced: 5 000 file
//! creations produce one event with `rescan = true`, not 5 000.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, select, unbounded};
use notify::{Config, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};

use crate::events::{CoreEvent, WatchEvent};

const QUIET: Duration = Duration::from_millis(60);
const MAX_WAIT: Duration = Duration::from_millis(400);
const RESCAN_ABOVE: usize = 256;

enum Ctl {
    Watch(PathBuf),
    Stop,
}

pub struct DirWatcher {
    ctl: Sender<Ctl>,
}

impl DirWatcher {
    pub fn spawn(out: Sender<CoreEvent>) -> DirWatcher {
        let (ctl_tx, ctl_rx) = unbounded();
        std::thread::Builder::new()
            .name("rada-watch".into())
            .spawn(move || run(ctl_rx, out))
            .expect("spawn watcher thread");
        DirWatcher { ctl: ctl_tx }
    }

    /// Watch `dir` (non-recursively), replacing the previous directory.
    pub fn watch(&self, dir: PathBuf) {
        let _ = self.ctl.send(Ctl::Watch(dir));
    }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        let _ = self.ctl.send(Ctl::Stop);
    }
}

type Raw = notify::Result<notify::Event>;

enum Backend {
    Native(RecommendedWatcher),
    Poll(PollWatcher),
}

impl Backend {
    fn watch(&mut self, p: &Path) -> notify::Result<()> {
        match self {
            Backend::Native(w) => w.watch(p, RecursiveMode::NonRecursive),
            Backend::Poll(w) => w.watch(p, RecursiveMode::NonRecursive),
        }
    }
    fn unwatch(&mut self, p: &Path) {
        let _ = match self {
            Backend::Native(w) => w.unwatch(p),
            Backend::Poll(w) => w.unwatch(p),
        };
    }
}

fn native(raw_tx: Sender<Raw>) -> notify::Result<Backend> {
    RecommendedWatcher::new(
        move |ev| {
            let _ = raw_tx.send(ev);
        },
        Config::default(),
    )
    .map(Backend::Native)
}

fn polling(raw_tx: Sender<Raw>) -> notify::Result<Backend> {
    PollWatcher::new(
        move |ev| {
            let _ = raw_tx.send(ev);
        },
        Config::default().with_poll_interval(Duration::from_secs(1)),
    )
    .map(Backend::Poll)
}

fn run(ctl: Receiver<Ctl>, out: Sender<CoreEvent>) {
    let (raw_tx, raw_rx) = unbounded::<Raw>();
    let mut backend = match native(raw_tx.clone()) {
        Ok(b) => b,
        Err(e) => {
            let _ = out.send(CoreEvent::Watch(WatchEvent::Degraded(format!(
                "native file watching unavailable ({e}); polling"
            ))));
            match polling(raw_tx.clone()) {
                Ok(b) => b,
                Err(e) => {
                    let _ = out.send(CoreEvent::Watch(WatchEvent::Failed(e.to_string())));
                    return;
                }
            }
        }
    };

    let mut current: Option<PathBuf> = None;
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut rescan = false;
    let mut first_pending: Option<Instant> = None;
    let mut last_event: Option<Instant> = None;

    loop {
        let wait = match (first_pending, last_event) {
            (Some(first), Some(last)) => {
                let due = (last + QUIET).min(first + MAX_WAIT);
                due.saturating_duration_since(Instant::now())
            }
            _ => Duration::from_secs(3600),
        };
        select! {
            recv(ctl) -> msg => match msg {
                Ok(Ctl::Watch(dir)) => {
                    if let Some(old) = current.take() {
                        backend.unwatch(&old);
                    }
                    pending.clear();
                    rescan = false;
                    first_pending = None;
                    last_event = None;
                    if let Err(e) = backend.watch(&dir) {
                        // inotify limit reached, or the path vanished: fall back to polling.
                        tracing::warn!("watch {}: {e}", dir.display());
                        if let Ok(mut p) = polling(raw_tx.clone()) {
                            if p.watch(&dir).is_ok() {
                                let _ = out.send(CoreEvent::Watch(WatchEvent::Degraded(format!("watching {} by polling ({e})", dir.display()))));
                                backend = p;
                                current = Some(dir);
                                continue;
                            }
                        }
                        let _ = out.send(CoreEvent::Watch(WatchEvent::Failed(format!("cannot watch {}: {e}", dir.display()))));
                    } else {
                        current = Some(dir);
                    }
                }
                Ok(Ctl::Stop) | Err(_) => return,
            },
            recv(raw_rx) -> ev => {
                let Ok(ev) = ev else { return };
                let Some(dir) = current.clone() else { continue };
                match ev {
                    Ok(ev) => {
                        if matches!(ev.kind, EventKind::Access(_) | EventKind::Other) && !ev.need_rescan() {
                            continue;
                        }
                        if ev.need_rescan() {
                            rescan = true;
                        }
                        for p in ev.paths {
                            if p == dir {
                                rescan = true; // the directory itself changed or vanished
                            } else if p.parent() == Some(dir.as_path()) {
                                pending.insert(p);
                            }
                        }
                        let now = Instant::now();
                        first_pending.get_or_insert(now);
                        last_event = Some(now);
                    }
                    Err(e) => {
                        tracing::warn!("watch error: {e}");
                        rescan = true;
                        let now = Instant::now();
                        first_pending.get_or_insert(now);
                        last_event = Some(now);
                    }
                }
            },
            default(wait) => {
                if let (Some(dir), Some(_)) = (current.clone(), first_pending) {
                    let many = pending.len() > RESCAN_ABOVE;
                    let paths: Vec<PathBuf> = if many { Vec::new() } else { pending.drain().collect() };
                    pending.clear();
                    let _ = out.send(CoreEvent::Watch(WatchEvent::Changed { dir, paths, rescan: rescan || many }));
                    rescan = false;
                    first_pending = None;
                    last_event = None;
                }
            }
        }
    }
}
