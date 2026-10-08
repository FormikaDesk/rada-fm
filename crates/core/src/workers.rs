//! Background workers. Each one owns its I/O, receives requests on a channel, and
//! answers with events tagged by a generation number so the UI can drop stale answers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::events::{CoreEvent, DirEvent, PreviewEvent};
use crate::fs::FsEngine;
use crate::model::{self, EntryUpdate};
use crate::platform::{Platform, Volume};
use crate::preview::{self, ImageState, ImageWorker, Limits, Preview};

// ------------------------------------------------------------------------------ folders

enum DirReq {
    Load {
        path: PathBuf,
        generation: u64,
    },
    Patch {
        dir: PathBuf,
        paths: Vec<PathBuf>,
        generation: u64,
    },
}

#[derive(Clone)]
pub struct DirLoader {
    tx: Sender<DirReq>,
}

impl DirLoader {
    pub fn spawn(
        fs: Arc<dyn FsEngine>,
        platform: Arc<dyn Platform>,
        out: Sender<CoreEvent>,
    ) -> DirLoader {
        let (tx, rx) = unbounded::<DirReq>();
        std::thread::Builder::new()
            .name("vela-dir".into())
            .spawn(move || dir_loop(rx, fs, platform, out))
            .expect("spawn dir worker");
        DirLoader { tx }
    }

    /// Read a whole directory.
    pub fn load(&self, path: PathBuf, generation: u64) {
        let _ = self.tx.send(DirReq::Load { path, generation });
    }

    /// Re-inspect only these paths of `dir`.
    pub fn patch(&self, dir: PathBuf, paths: Vec<PathBuf>, generation: u64) {
        let _ = self.tx.send(DirReq::Patch {
            dir,
            paths,
            generation,
        });
    }
}

fn dir_loop(
    rx: Receiver<DirReq>,
    fs: Arc<dyn FsEngine>,
    platform: Arc<dyn Platform>,
    out: Sender<CoreEvent>,
) {
    while let Ok(first) = rx.recv() {
        // Coalesce: only the newest full load matters; patches for the same dir merge.
        let mut load: Option<(PathBuf, u64)> = None;
        let mut patches: HashMap<PathBuf, (Vec<PathBuf>, u64)> = HashMap::new();
        let mut absorb = |req: DirReq, load: &mut Option<(PathBuf, u64)>| match req {
            DirReq::Load { path, generation } => {
                *load = Some((path, generation));
                patches.clear();
            }
            DirReq::Patch {
                dir,
                paths,
                generation,
            } => {
                let e = patches.entry(dir).or_insert((Vec::new(), generation));
                e.0.extend(paths);
                e.1 = generation;
            }
        };
        absorb(first, &mut load);
        while let Ok(more) = rx.try_recv() {
            absorb(more, &mut load);
        }

        if let Some((path, generation)) = load {
            let result = model::read_entries(fs.as_ref(), platform.as_ref(), &path)
                .map_err(|e| e.to_string());
            if out
                .send(CoreEvent::Dir(DirEvent::Loaded {
                    generation,
                    path,
                    result,
                }))
                .is_err()
            {
                return;
            }
        }
        for (dir, (mut paths, generation)) in patches {
            paths.sort();
            paths.dedup();
            let updates: Vec<EntryUpdate> = paths
                .into_iter()
                .map(
                    |p| match model::read_entry(fs.as_ref(), platform.as_ref(), &p) {
                        Some(e) => EntryUpdate::Upsert(e),
                        None => EntryUpdate::Remove(
                            p.file_name().map(|n| n.to_os_string()).unwrap_or_default(),
                        ),
                    },
                )
                .collect();
            if out
                .send(CoreEvent::Dir(DirEvent::Patched {
                    generation,
                    dir,
                    updates,
                }))
                .is_err()
            {
                return;
            }
        }
    }
}

// ------------------------------------------------------------------------------ preview

struct PreviewReq {
    path: PathBuf,
    generation: u64,
    limits: Limits,
}

#[derive(Clone)]
pub struct PreviewWorker {
    tx: Sender<PreviewReq>,
}

impl PreviewWorker {
    pub fn spawn(fs: Arc<dyn FsEngine>, out: Sender<CoreEvent>) -> PreviewWorker {
        let (tx, rx) = unbounded::<PreviewReq>();
        let images = ImageWorker::spawn(out.clone());
        std::thread::Builder::new()
            .name("vela-preview".into())
            .spawn(move || {
                while let Ok(mut req) = rx.recv() {
                    // Holding a key down moves through files faster than they can be read:
                    // wait a beat and keep only the newest request.
                    while let Ok(newer) = rx.recv_timeout(Duration::from_millis(30)) {
                        req = newer;
                    }
                    images.newest(req.generation);
                    let preview = preview::generate(fs.as_ref(), &req.path, &req.limits);
                    // Decoding is slow: the header-only answer goes out now, the pixels follow.
                    if let Preview::Image(img) = &preview {
                        if matches!(img.state, ImageState::Loading) {
                            images.submit(
                                req.path.clone(),
                                req.generation,
                                img.info.clone(),
                                req.limits.image.clone(),
                            );
                        }
                    }
                    let name = req
                        .path
                        .file_name()
                        .map(|n| n.to_os_string())
                        .unwrap_or_default();
                    let ev = PreviewEvent {
                        generation: req.generation,
                        path: req.path,
                        name,
                        preview,
                    };
                    if out.send(CoreEvent::Preview(ev)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn preview worker");
        PreviewWorker { tx }
    }

    pub fn request(&self, path: PathBuf, generation: u64, limits: Limits) {
        let _ = self.tx.send(PreviewReq {
            path,
            generation,
            limits,
        });
    }
}

// ------------------------------------------------------------------------------ volumes

pub struct VolumesWorker {
    stop: Arc<AtomicBool>,
    refresh: Arc<AtomicBool>,
}

impl VolumesWorker {
    /// Lists volumes off the UI thread: when the system reports a mount change, and
    /// otherwise every `interval` (never on a keypress).
    pub fn spawn(
        platform: Arc<dyn Platform>,
        interval: Duration,
        out: Sender<CoreEvent>,
    ) -> VolumesWorker {
        let stop = Arc::new(AtomicBool::new(false));
        let refresh = Arc::new(AtomicBool::new(true));
        let (s, r) = (stop.clone(), refresh.clone());
        std::thread::Builder::new()
            .name("vela-volumes".into())
            .spawn(move || {
                let mut last: Option<Vec<Volume>> = None;
                let mut last_sent = Instant::now();
                let mut next = Instant::now();
                while !s.load(Ordering::Relaxed) {
                    let changed = platform
                        .volumes()
                        .wait_for_change(Duration::from_millis(500));
                    if !(changed || r.swap(false, Ordering::Relaxed) || Instant::now() >= next) {
                        continue;
                    }
                    next = Instant::now() + interval;
                    match platform.volumes().list() {
                        Ok(v) => {
                            let same = last.as_ref().is_some_and(|l| same_mounts(l, &v));
                            // Free-space figures drift; resend now and then even if mounts are equal.
                            if !same || last_sent.elapsed() > Duration::from_secs(30) {
                                last_sent = Instant::now();
                                last = Some(v.clone());
                                if out.send(CoreEvent::Volumes(v)).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!("volumes: {e}");
                            if last.is_none() {
                                last = Some(Vec::new());
                                let _ = out.send(CoreEvent::Volumes(Vec::new()));
                            }
                        }
                    }
                }
            })
            .expect("spawn volumes worker");
        VolumesWorker { stop, refresh }
    }

    pub fn refresh_now(&self) {
        self.refresh.store(true, Ordering::Relaxed);
    }
}

impl Drop for VolumesWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn same_mounts(a: &[Volume], b: &[Volume]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.mount_point == y.mount_point
                && x.device == y.device
                && x.responsive == y.responsive
                && x.fs_type == y.fs_type
        })
}
