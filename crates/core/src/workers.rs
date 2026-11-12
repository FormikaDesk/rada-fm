//! Background workers. Each one owns its I/O, receives requests on a channel, and
//! answers with events tagged by a generation number so the UI can drop stale answers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::events::{CoreEvent, DirEvent, PreviewEvent};
use crate::fs::{DirItem, FileKind, FsEngine};
use crate::model::{self, Entry, EntryUpdate};
use crate::platform::{Platform, Unmounted, Volume};
use crate::preview::{self, ImageState, ImageWorker, Limits, Preview};
use crate::view::ViewKind;
use crate::views::{Problem, ViewCx, ViewProgress, ViewSink};

// ------------------------------------------------------------------------------ folders

/// List the subfolders of `dir` on a thread of its own and send them as a
/// [`CoreEvent::Complete`]. Meant for completing an address as it is typed.
pub fn complete_dirs(fs: Arc<dyn FsEngine>, dir: PathBuf, out: Sender<CoreEvent>) {
    let _ = std::thread::Builder::new()
        .name("rada-complete".into())
        .spawn(move || {
            const LIMIT: usize = 5000;
            let mut names: Vec<String> = fs
                .read_dir(&dir)
                .map(|listing| {
                    listing
                        .items
                        .into_iter()
                        .filter(|it| {
                            it.meta.as_ref().is_ok_and(|m| m.is_dir())
                                || (it.meta.as_ref().is_ok_and(|m| m.is_symlink())
                                    && fs.stat(&it.path).is_ok_and(|m| m.is_dir()))
                        })
                        .take(LIMIT)
                        .map(|it| crate::display::name(&it.name))
                        .collect()
                })
                .unwrap_or_default();
            names.sort_by_key(|n| n.to_lowercase());
            let _ = out.send(CoreEvent::Complete { dir, names });
        });
}

/// One subfolder in the navigation tree of the panels view: how many items it holds and whether
/// any of them is a folder (it can then be opened in the tree).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeKid {
    pub path: PathBuf,
    /// The name on disk.
    pub name: String,
    pub items: usize,
    pub has_dirs: bool,
    /// Why the folder cannot be listed, in a few words; `None` when it can. Without it an
    /// unreadable folder would look empty.
    pub unreadable: Option<String>,
}

/// A few words for why a folder cannot be listed.
pub(crate) fn unreadable_reason(e: &std::io::Error) -> String {
    use crate::i18n::{Key, t, tf};
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return t(Key::NavNoPermission).to_string();
    }
    let text = e.to_string();
    let os = text.split(" (os error").next().unwrap_or(&text);
    tf(Key::NavCannotRead, &[&os])
}

/// List the subfolders of `dir` with the number of items each holds, on a thread of its own, and
/// send them as a [`CoreEvent::Tree`]. `hidden` says whether names that start with a dot are
/// listed. Nothing here follows a link out of the folder: a link to a folder is a leaf.
pub fn tree_dirs(fs: Arc<dyn FsEngine>, dir: PathBuf, hidden: bool, out: Sender<CoreEvent>) {
    let _ = std::thread::Builder::new()
        .name("rada-tree".into())
        .spawn(move || {
            const LIMIT: usize = 400;
            let shown = |n: &std::ffi::OsStr| hidden || !n.to_string_lossy().starts_with('.');
            let mut kids: Vec<TreeKid> = fs
                .read_dir(&dir)
                .map(|listing| {
                    listing
                        .items
                        .into_iter()
                        .filter(|it| shown(&it.name) && it.meta.as_ref().is_ok_and(|m| m.is_dir()))
                        .take(LIMIT)
                        .map(|it| tree_kid(&*fs, it, &shown))
                        .collect()
                })
                .unwrap_or_default();
            kids.sort_by_key(|k| k.name.to_lowercase());
            let _ = out.send(CoreEvent::Tree { dir, hidden, kids });
        });
}

/// One subfolder of the tree: what it holds, or why it cannot be listed.
fn tree_kid(fs: &dyn FsEngine, it: DirItem, shown: &dyn Fn(&std::ffi::OsStr) -> bool) -> TreeKid {
    let (items, has_dirs, unreadable) = match fs.read_dir(&it.path) {
        Ok(inside) => (
            inside.items.iter().filter(|i| shown(&i.name)).count(),
            inside
                .items
                .iter()
                .any(|i| shown(&i.name) && i.meta.as_ref().is_ok_and(|m| m.is_dir())),
            None,
        ),
        Err(e) => (0, false, Some(unreadable_reason(&e))),
    };
    TreeKid {
        name: crate::display::name(&it.name),
        path: it.path,
        items,
        has_dirs,
        unreadable,
    }
}

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

/// What the worker of a list that is not a folder is told while it runs.
#[derive(Clone, Default)]
struct ViewFlags {
    /// The newest generation asked for: a search that is still running for an older one
    /// stops, and what it found is not wanted.
    latest: Arc<AtomicU64>,
    /// The generation the person asked to stop: what it found so far is the list.
    stopped: Arc<AtomicU64>,
    /// Whether hidden items are listed.
    show_hidden: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct DirLoader {
    tx: Sender<DirReq>,
    flags: ViewFlags,
}

/// How often a search that finds nothing new still says how far it is.
const PROGRESS_EVERY: Duration = Duration::from_millis(120);

/// Most unreadable folders kept to be shown: past that, only the count would be read anyway.
const MAX_PROBLEMS: usize = 500;

/// One run of the source of a list, on a thread of its own.
struct ViewRun {
    fs: Arc<dyn FsEngine>,
    platform: Arc<dyn Platform>,
    out: Sender<CoreEvent>,
    flags: ViewFlags,
    generation: u64,
    path: PathBuf,
}

impl ViewRun {
    fn stale(&self) -> bool {
        self.flags.latest.load(Ordering::SeqCst) != self.generation
    }

    fn run(self, kind: &ViewKind) {
        let cx = ViewCx {
            fs: self.fs.as_ref(),
            platform: self.platform.as_ref(),
            show_hidden: self.flags.show_hidden.load(Ordering::SeqCst),
        };
        let mut sink = ChannelSink {
            run: &self,
            problems: Vec::new(),
            progress: ViewProgress::default(),
            told: Instant::now(),
        };
        let result = crate::views::source(kind).run(&cx, &mut sink);
        let (problems, progress) = (sink.problems, sink.progress);
        if self.stale() {
            return;
        }
        let _ = self.out.send(CoreEvent::Dir(DirEvent::ViewDone {
            generation: self.generation,
            path: self.path.clone(),
            result,
            problems,
            progress,
        }));
    }
}

/// Sends what a source finds to the interface as it finds it.
struct ChannelSink<'a> {
    run: &'a ViewRun,
    problems: Vec<Problem>,
    progress: ViewProgress,
    /// When the interface was last told something.
    told: Instant,
}

impl ChannelSink<'_> {
    fn send(&mut self, entries: Vec<Entry>) {
        self.told = Instant::now();
        let _ = self.run.out.send(CoreEvent::Dir(DirEvent::ViewBatch {
            generation: self.run.generation,
            path: self.run.path.clone(),
            entries,
            progress: self.progress,
        }));
    }
}

impl ViewSink for ChannelSink<'_> {
    fn batch(&mut self, entries: Vec<Entry>) {
        if !self.run.stale() {
            self.send(entries);
        }
    }

    fn problem(&mut self, problem: Problem) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(problem);
        }
    }

    fn progress(&mut self, progress: ViewProgress) {
        self.progress = progress;
        if self.told.elapsed() >= PROGRESS_EVERY && !self.run.stale() {
            self.send(Vec::new());
        }
    }

    fn cancelled(&self) -> bool {
        self.run.stale() || self.run.flags.stopped.load(Ordering::SeqCst) == self.run.generation
    }
}

impl DirLoader {
    pub fn spawn(
        fs: Arc<dyn FsEngine>,
        platform: Arc<dyn Platform>,
        out: Sender<CoreEvent>,
    ) -> DirLoader {
        let (tx, rx) = unbounded::<DirReq>();
        let flags = ViewFlags::default();
        let seen = flags.clone();
        std::thread::Builder::new()
            .name("rada-dir".into())
            .spawn(move || dir_loop(rx, fs, platform, out, seen))
            .expect("spawn dir worker");
        DirLoader { tx, flags }
    }

    /// Read a whole directory (or, for a `tag:` path, look for the items with that tag).
    pub fn load(&self, path: PathBuf, generation: u64) {
        self.flags.latest.fetch_max(generation, Ordering::SeqCst);
        let _ = self.tx.send(DirReq::Load { path, generation });
    }

    /// Whether the lists that are not a folder show hidden items, from the next one on.
    pub fn set_show_hidden(&self, show: bool) {
        self.flags.show_hidden.store(show, Ordering::SeqCst);
    }

    /// Stop the search that runs for `generation`: what it has found is the list.
    pub fn stop_view(&self, generation: u64) {
        self.flags.stopped.store(generation, Ordering::SeqCst);
    }

    /// Re-inspect only these paths of `dir`.
    pub fn patch(&self, dir: PathBuf, paths: Vec<PathBuf>, generation: u64) {
        self.flags.latest.fetch_max(generation, Ordering::SeqCst);
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
    flags: ViewFlags,
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
            if let Some(kind) = ViewKind::of(&path) {
                // A search can take a while: on a thread of its own, so that the next folder
                // the person opens is not kept waiting behind it.
                let run = ViewRun {
                    fs: fs.clone(),
                    platform: platform.clone(),
                    out: out.clone(),
                    flags: flags.clone(),
                    generation,
                    path,
                };
                let _ = std::thread::Builder::new()
                    .name("rada-view".into())
                    .spawn(move || run.run(&kind));
                continue;
            }
            // A path that runs through a file is inside an archive. An ordinary folder costs
            // one `stat` to find that out.
            let mut denied = None;
            let (result, archive) = match crate::archive::locate(fs.as_ref(), &path) {
                Some(loc) => {
                    // A newer request waiting means nobody wants this listing any more.
                    let superseded = || !rx.is_empty();
                    match crate::archive::browse::read_dir(platform.as_ref(), &loc, &superseded) {
                        Ok((entries, view)) => (Ok(entries), Some(view)),
                        Err(crate::Error::Cancelled) => continue,
                        Err(e) => (Err(e.to_string()), None),
                    }
                }
                None => (
                    model::read_entries(fs.as_ref(), platform.as_ref(), &path).map_err(|e| {
                        // The system waiting for the person's consent is not a broken folder.
                        if let crate::Error::Io { source, .. } = &e {
                            denied = platform.explain_denied(&path, source).map(Box::new);
                        }
                        e.to_string()
                    }),
                    None,
                ),
            };
            if out
                .send(CoreEvent::Dir(DirEvent::Loaded {
                    generation,
                    path,
                    result,
                    archive,
                    denied,
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
        PreviewWorker::spawn_with(fs, None, out)
    }

    /// [`spawn`](Self::spawn), with the platform whose rules say what is hidden: the preview
    /// of a folder then leaves out what the list leaves out.
    pub fn spawn_with(
        fs: Arc<dyn FsEngine>,
        platform: Option<Arc<dyn Platform>>,
        out: Sender<CoreEvent>,
    ) -> PreviewWorker {
        let (tx, rx) = unbounded::<PreviewReq>();
        let images = ImageWorker::spawn(out.clone());
        std::thread::Builder::new()
            .name("rada-preview".into())
            .spawn(move || {
                while let Ok(mut req) = rx.recv() {
                    // Holding a key down moves through files faster than they can be read:
                    // wait a beat and keep only the newest request.
                    while let Ok(newer) = rx.recv_timeout(Duration::from_millis(30)) {
                        req = newer;
                    }
                    images.newest(req.generation);
                    // Reading inside an archive can take long: a newer request waiting means
                    // the answer is no longer wanted.
                    let superseded = || !rx.is_empty();
                    let on_device = platform.as_ref().is_some_and(|p| p.on_device(&req.path));
                    let preview = if on_device
                        && fs.lstat(&req.path).is_ok_and(|m| m.kind != FileKind::Dir)
                    {
                        // The contents of a file on a phone or a camera are not read just
                        // because the cursor passed over it: the device may send the whole file.
                        Preview::Special(crate::i18n::t(crate::i18n::Key::PreviewOnDevice).into())
                    } else {
                        preview::generate_in(
                            fs.as_ref(),
                            // What is hidden is decided by the name alone on a device: asking
                            // for the attributes of each entry would be a question per entry.
                            platform
                                .as_ref()
                                .filter(|_| !on_device)
                                .map(|p| p.attributes()),
                            &req.path,
                            &req.limits,
                            &superseded,
                        )
                    };
                    // Decoding is slow: the header-only answer goes out first, the pixels
                    // follow. The decoder is only started once the header has been sent:
                    // started before, it could finish first, and the header (still
                    // "loading") would then arrive after the picture and undo it.
                    let decode = match &preview {
                        Preview::Image(img) if matches!(img.state, ImageState::Loading) => Some((
                            // A picture inside an archive is decoded from its cached copy.
                            img.source.clone().unwrap_or_else(|| req.path.clone()),
                            req.generation,
                            img.info.clone(),
                            req.limits.image.clone(),
                        )),
                        _ => None,
                    };
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
                    if let Some((path, generation, info, limits)) = decode {
                        images.submit(path, generation, info, limits);
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
    refresh_devices: Arc<AtomicBool>,
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
        let refresh_devices = Arc::new(AtomicBool::new(true));
        Self::spawn_unmounted(
            platform.clone(),
            interval,
            out.clone(),
            stop.clone(),
            refresh_devices.clone(),
        );
        let (s, r) = (stop.clone(), refresh.clone());
        std::thread::Builder::new()
            .name("rada-volumes".into())
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
        VolumesWorker {
            stop,
            refresh,
            refresh_devices,
        }
    }

    /// The devices that are plugged in and not mounted, on a thread of their own: finding
    /// them runs programs, and a phone that does not answer must not hold the list of disks.
    fn spawn_unmounted(
        platform: Arc<dyn Platform>,
        interval: Duration,
        out: Sender<CoreEvent>,
        stop: Arc<AtomicBool>,
        refresh: Arc<AtomicBool>,
    ) {
        std::thread::Builder::new()
            .name("rada-devices".into())
            .spawn(move || {
                // The interface starts with none: an empty answer is not news.
                let mut last: Option<Vec<Unmounted>> = Some(Vec::new());
                while !stop.load(Ordering::Relaxed) {
                    let found = platform.volumes().unmounted();
                    if last.as_ref() != Some(&found) {
                        last = Some(found.clone());
                        if out.send(CoreEvent::Unmounted(found)).is_err() {
                            return;
                        }
                    }
                    // Until it is time again, or somebody asks for it now.
                    let until = Instant::now() + interval;
                    while Instant::now() < until
                        && !stop.load(Ordering::Relaxed)
                        && !refresh.swap(false, Ordering::Relaxed)
                    {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            })
            .expect("spawn devices worker");
    }

    pub fn refresh_now(&self) {
        self.refresh.store(true, Ordering::Relaxed);
        self.refresh_devices.store(true, Ordering::Relaxed);
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
