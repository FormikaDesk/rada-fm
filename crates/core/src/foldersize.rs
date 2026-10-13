//! The size of a folder: counted in the background, for the folders on screen only.
//!
//! Counting walks the whole tree below a folder, so it is never done where the interface can
//! wait for it. A [`SizeWorker`] does it on a thread of its own, one folder at a time, and
//! stops as soon as the person leaves the folder (see [`SizeWorker::newest`]). The walk is
//! careful about what it enters:
//!
//! * links are not followed, and neither is another filesystem (a mounted disk below the
//!   folder is a different place, and counting it would be both slow and wrong);
//! * `/proc`, `/sys` and `/dev` are not walked, nor is a network share or a disk that does
//!   not answer (see [`Avoid`]);
//! * it gives up after a time or a number of entries ([`SizeLimits`]) and then says "more
//!   than this much" instead of a figure that would be false.
//!
//! The answers are kept in a [`SizeCache`]. The watcher drops what it hears has changed; an
//! answer is also dropped when the folder's own modification time moves, and in any case
//! after [`SizeCache::MAX_AGE`] (a change deep inside a folder reaches nobody above it).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Sender, unbounded};

use crate::events::CoreEvent;
use crate::fs::{FileKind, FsEngine, FsMeta};
use crate::platform::{Volume, VolumeKind};

/// What the person chose with `folder_sizes` in the configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SizeMode {
    /// Count the size of the folders on screen.
    #[default]
    Auto,
    /// Show nothing in the Size column for a folder.
    Off,
    /// Only count the items in each folder, which is quick: no walk below.
    Items,
}

impl SizeMode {
    pub fn parse(s: &str) -> Option<SizeMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "on" | "true" => Some(SizeMode::Auto),
            "off" | "false" | "never" => Some(SizeMode::Off),
            "items" | "count" => Some(SizeMode::Items),
            _ => None,
        }
    }
}

/// How long and how far one count may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeLimits {
    pub max_time: Duration,
    pub max_entries: u64,
}

impl Default for SizeLimits {
    fn default() -> Self {
        SizeLimits {
            max_time: Duration::from_secs(3),
            max_entries: 200_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SizeSettings {
    pub mode: SizeMode,
    pub limits: SizeLimits,
}

/// What a count found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderSize {
    /// The items directly in the folder.
    pub items: u64,
    /// The bytes in the files below it; `None` when only the items were counted.
    pub bytes: Option<u64>,
    /// False when the count stopped at a limit: `bytes` is then "at least this much".
    pub complete: bool,
}

/// How a request for a folder's size ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeOutcome {
    Counted(FolderSize),
    /// A place that is not counted (`/proc`, a network share…).
    Skipped,
    /// The folder could not be read.
    Unreadable,
}

/// Places that are never walked: the system's own virtual folders and what the volumes list
/// says is slow or not answering.
#[derive(Clone, Debug, Default)]
pub struct Avoid {
    paths: Vec<PathBuf>,
}

impl Avoid {
    /// These places and everything below them, besides the system's own.
    pub fn new(paths: Vec<PathBuf>) -> Avoid {
        let mut all = Avoid::system().paths;
        all.extend(paths);
        Avoid { paths: all }
    }

    /// Only the virtual folders every system has.
    pub fn system() -> Avoid {
        let paths = if cfg!(unix) {
            ["/proc", "/sys", "/dev"].map(PathBuf::from).to_vec()
        } else {
            Vec::new()
        };
        Avoid { paths }
    }

    /// The system's places and the mount points of network shares and of volumes that do not
    /// answer.
    pub fn for_volumes(volumes: &[Volume]) -> Avoid {
        Avoid::new(
            volumes
                .iter()
                .filter(|v| v.kind == VolumeKind::Network || !v.responsive)
                .map(|v| v.mount_point.clone())
                .collect(),
        )
    }

    pub fn covers(&self, p: &Path) -> bool {
        self.paths.iter().any(|a| p.starts_with(a))
    }
}

/// The Windows "reparse point" attribute: a junction or a link, which is not followed.
const REPARSE_POINT: u32 = 0x400;

fn is_link_like(m: &FsMeta) -> bool {
    m.is_symlink() || (cfg!(windows) && m.os_attrs.is_some_and(|a| a & REPARSE_POINT != 0))
}

/// Whether a folder is on another filesystem than the one counted from (a disk mounted
/// below it). Where the system gives no device numbers nothing is told apart.
fn on_other_filesystem(root_dev: Option<u64>, m: &FsMeta) -> bool {
    matches!((root_dev, m.dev), (Some(a), Some(b)) if a != b)
}

/// Count `root`. `cancelled` is asked at every entry: when it says yes the count is dropped
/// (`None`). Reaching a limit is not a cancellation: the figure so far is returned, marked
/// as incomplete.
pub fn measure(
    fs: &dyn FsEngine,
    root: &Path,
    mode: SizeMode,
    limits: &SizeLimits,
    avoid: &Avoid,
    cancelled: &dyn Fn() -> bool,
) -> Option<SizeOutcome> {
    if avoid.covers(root) || mode == SizeMode::Off {
        return Some(SizeOutcome::Skipped);
    }
    let root_dev = fs.lstat(root).ok().and_then(|m| m.dev);
    let Ok(first) = fs.read_dir(root) else {
        return Some(SizeOutcome::Unreadable);
    };
    let items = first.len() as u64;
    if mode == SizeMode::Items {
        return Some(SizeOutcome::Counted(FolderSize {
            items,
            bytes: None,
            complete: true,
        }));
    }

    let started = Instant::now();
    let mut entries = 0u64;
    let mut bytes = 0u64;
    let mut linked: HashSet<(u64, u64)> = HashSet::new();
    let mut pending = vec![first];
    while let Some(batch) = pending.pop() {
        for item in batch {
            if cancelled() {
                return None;
            }
            entries += 1;
            if entries > limits.max_entries || started.elapsed() > limits.max_time {
                return Some(SizeOutcome::Counted(FolderSize {
                    items,
                    bytes: Some(bytes),
                    complete: false,
                }));
            }
            let Ok(meta) = item.meta else { continue };
            match meta.kind {
                FileKind::File => {
                    // A file with several names is counted once.
                    let again = meta.nlink.is_some_and(|n| n > 1)
                        && match (meta.dev, meta.ino) {
                            (Some(d), Some(i)) => !linked.insert((d, i)),
                            _ => false,
                        };
                    if !again {
                        bytes = bytes.saturating_add(meta.size);
                    }
                }
                FileKind::Dir => {
                    if is_link_like(&meta)
                        || on_other_filesystem(root_dev, &meta)
                        || avoid.covers(&item.path)
                    {
                        continue;
                    }
                    if let Ok(more) = fs.read_dir(&item.path) {
                        pending.push(more);
                    }
                }
                // Links are not followed; sockets and devices have no size worth adding.
                _ => {}
            }
        }
    }
    Some(SizeOutcome::Counted(FolderSize {
        items,
        bytes: Some(bytes),
        complete: true,
    }))
}

// ---------------------------------------------------------------------------------- cache

#[derive(Clone, Debug)]
enum Slot {
    Pending,
    Known {
        outcome: SizeOutcome,
        at: Instant,
        /// The folder's modification time when it was counted.
        mtime: Option<SystemTime>,
    },
}

/// What is known of a folder's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// Never asked for, or the answer is out of date.
    Unknown,
    /// Being counted.
    Pending,
    Known(SizeOutcome),
}

#[derive(Debug, Default)]
pub struct SizeCache {
    slots: HashMap<PathBuf, Slot>,
}

impl SizeCache {
    /// An answer older than this is counted again when the folder is on screen.
    pub const MAX_AGE: Duration = Duration::from_secs(300);
    const MAX_SLOTS: usize = 20_000;

    /// What is known of `path`, whose modification time now is `mtime`.
    pub fn lookup(&self, path: &Path, mtime: Option<SystemTime>, now: Instant) -> Lookup {
        match self.slots.get(path) {
            None => Lookup::Unknown,
            Some(Slot::Pending) => Lookup::Pending,
            Some(Slot::Known {
                outcome,
                at,
                mtime: counted_at,
            }) => {
                if now.saturating_duration_since(*at) > Self::MAX_AGE || *counted_at != mtime {
                    Lookup::Unknown
                } else {
                    Lookup::Known(*outcome)
                }
            }
        }
    }

    /// The answer for `path` as it was last counted, current or not.
    pub fn last(&self, path: &Path) -> Option<SizeOutcome> {
        match self.slots.get(path) {
            Some(Slot::Known { outcome, .. }) => Some(*outcome),
            _ => None,
        }
    }

    pub fn mark_pending(&mut self, path: PathBuf) {
        if self.slots.len() >= Self::MAX_SLOTS {
            self.slots.clear();
        }
        self.slots.insert(path, Slot::Pending);
    }

    pub fn store(
        &mut self,
        path: PathBuf,
        outcome: SizeOutcome,
        mtime: Option<SystemTime>,
        now: Instant,
    ) {
        self.slots.insert(
            path,
            Slot::Known {
                outcome,
                at: now,
                mtime,
            },
        );
    }

    /// The folder changed: what was counted for it, and for the folders it lies in, is no
    /// longer true.
    pub fn invalidate(&mut self, path: &Path) {
        self.slots.retain(|k, _| !path.starts_with(k));
    }

    /// Everything counted for the folders directly in `dir` (the watcher lost track of what
    /// changed there).
    pub fn invalidate_children(&mut self, dir: &Path) {
        self.slots.retain(|k, _| !k.starts_with(dir));
    }

    /// The counts waiting for an answer were dropped (the person left the folder): they are
    /// asked for again if they are needed again.
    pub fn forget_pending(&mut self) {
        self.slots.retain(|_, s| !matches!(s, Slot::Pending));
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

// ---------------------------------------------------------------------------------- worker

/// One folder's answer.
pub struct SizeEvent {
    pub path: PathBuf,
    pub outcome: SizeOutcome,
}

struct SizeReq {
    epoch: u64,
    paths: Vec<PathBuf>,
    settings: SizeSettings,
    avoid: Arc<Avoid>,
}

/// Counts folders one after the other on a thread of its own.
#[derive(Clone)]
pub struct SizeWorker {
    tx: Sender<SizeReq>,
    newest: Arc<AtomicU64>,
}

impl SizeWorker {
    pub fn spawn(fs: Arc<dyn FsEngine>, out: Sender<CoreEvent>) -> SizeWorker {
        let (tx, rx) = unbounded::<SizeReq>();
        let newest = Arc::new(AtomicU64::new(0));
        let current = newest.clone();
        std::thread::Builder::new()
            .name("rada-sizes".into())
            .spawn(move || {
                while let Ok(req) = rx.recv() {
                    let stale = || current.load(Ordering::Relaxed) != req.epoch;
                    for path in req.paths {
                        if stale() {
                            break;
                        }
                        let Some(outcome) = measure(
                            fs.as_ref(),
                            &path,
                            req.settings.mode,
                            &req.settings.limits,
                            &req.avoid,
                            &stale,
                        ) else {
                            break;
                        };
                        if out
                            .send(CoreEvent::Size(SizeEvent { path, outcome }))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            })
            .expect("spawn size worker");
        SizeWorker { tx, newest }
    }

    /// Count these folders, in this order, unless a newer epoch is announced first. `epoch`
    /// should be the one last given to [`newest`](Self::newest) (0 before any): a request of
    /// any other is dropped.
    pub fn request(
        &self,
        epoch: u64,
        paths: Vec<PathBuf>,
        settings: SizeSettings,
        avoid: Arc<Avoid>,
    ) {
        let _ = self.tx.send(SizeReq {
            epoch,
            paths,
            settings,
            avoid,
        });
    }

    /// Announce the epoch now wanted (the person went to another folder): everything asked for
    /// under another is dropped, and the count in progress stops at its next entry.
    pub fn newest(&self, epoch: u64) {
        self.newest.store(epoch, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::LocalFs;
    use crate::testutil::Sandbox;
    use std::cell::Cell;

    fn never() -> bool {
        false
    }

    fn count(sb: &Sandbox, rel: &str, mode: SizeMode, limits: SizeLimits) -> SizeOutcome {
        measure(
            &LocalFs,
            &sb.path(rel),
            mode,
            &limits,
            &Avoid::system(),
            &never,
        )
        .unwrap()
    }

    fn counted(o: SizeOutcome) -> FolderSize {
        match o {
            SizeOutcome::Counted(c) => c,
            other => panic!("expected a count, got {other:?}"),
        }
    }

    #[test]
    fn it_adds_up_the_files_below_and_counts_the_items_in_the_folder() {
        let sb = Sandbox::new();
        sb.write("d/a", vec![0u8; 100]);
        sb.write("d/sub/b", vec![0u8; 250]);
        sb.write("d/sub/deeper/c", vec![0u8; 5]);
        sb.mkdir("d/empty");
        let c = counted(count(&sb, "d", SizeMode::Auto, SizeLimits::default()));
        assert_eq!(c.bytes, Some(355));
        assert_eq!(c.items, 3, "a, sub and empty");
        assert!(c.complete);
        let e = counted(count(&sb, "d/empty", SizeMode::Auto, SizeLimits::default()));
        assert_eq!((e.bytes, e.items), (Some(0), 0));
    }

    #[test]
    fn the_items_mode_does_not_walk_below() {
        let sb = Sandbox::new();
        sb.write("d/a", "x");
        sb.write("d/sub/b", "y");
        let c = counted(count(&sb, "d", SizeMode::Items, SizeLimits::default()));
        assert_eq!((c.items, c.bytes, c.complete), (2, None, true));
    }

    #[test]
    fn off_counts_nothing_at_all() {
        let sb = Sandbox::new();
        sb.write("d/a", "x");
        assert_eq!(
            count(&sb, "d", SizeMode::Off, SizeLimits::default()),
            SizeOutcome::Skipped
        );
    }

    #[test]
    fn a_missing_folder_is_unreadable() {
        let sb = Sandbox::new();
        assert_eq!(
            count(&sb, "nowhere", SizeMode::Auto, SizeLimits::default()),
            SizeOutcome::Unreadable
        );
    }

    #[test]
    fn links_are_not_followed_and_a_file_with_two_names_counts_once() {
        let sb = Sandbox::new();
        sb.write("outside/big", vec![0u8; 10_000]);
        sb.write("d/a", vec![0u8; 10]);
        sb.symlink(sb.path("outside"), "d/to-folder");
        sb.symlink(sb.path("outside/big"), "d/to-file");
        std::fs::hard_link(sb.path("d/a"), sb.path("d/a-again")).unwrap();
        let c = counted(count(&sb, "d", SizeMode::Auto, SizeLimits::default()));
        assert_eq!(c.bytes, Some(10), "nothing from the targets, 10 once");
    }

    #[test]
    fn it_stops_at_the_entry_limit_and_says_at_least() {
        let sb = Sandbox::new();
        for i in 0..50 {
            sb.write(format!("d/f{i:02}"), vec![0u8; 10]);
        }
        let limits = SizeLimits {
            max_entries: 20,
            ..SizeLimits::default()
        };
        let c = counted(count(&sb, "d", SizeMode::Auto, limits));
        assert!(!c.complete);
        let b = c.bytes.unwrap();
        assert!(b > 0 && b < 500, "a part of it: {b}");
        assert_eq!(c.items, 50, "the folder's own items are still all there");
    }

    #[test]
    fn it_stops_at_the_time_limit() {
        let sb = Sandbox::new();
        for i in 0..20 {
            sb.write(format!("d/f{i:02}"), "x");
        }
        let limits = SizeLimits {
            max_time: Duration::ZERO,
            ..SizeLimits::default()
        };
        let c = counted(count(&sb, "d", SizeMode::Auto, limits));
        assert!(!c.complete);
    }

    #[test]
    fn a_count_that_is_cancelled_gives_no_answer() {
        let sb = Sandbox::new();
        for i in 0..30 {
            sb.write(format!("d/sub/f{i:02}"), "x");
        }
        // Cancelled after the tenth question: the walk drops everything.
        let asked = Cell::new(0);
        let cancelled = || {
            asked.set(asked.get() + 1);
            asked.get() > 10
        };
        let out = measure(
            &LocalFs,
            &sb.path("d"),
            SizeMode::Auto,
            &SizeLimits::default(),
            &Avoid::system(),
            &cancelled,
        );
        assert!(out.is_none());
        assert!(asked.get() <= 12, "it stopped soon: {}", asked.get());
    }

    #[test]
    fn the_places_to_avoid_are_not_walked() {
        let sb = Sandbox::new();
        sb.write("d/a", vec![0u8; 7]);
        sb.write("d/share/big", vec![0u8; 1000]);
        let avoid = Avoid::new(vec![sb.path("d/share")]);
        let below = measure(
            &LocalFs,
            &sb.path("d"),
            SizeMode::Auto,
            &SizeLimits::default(),
            &avoid,
            &never,
        )
        .unwrap();
        assert_eq!(counted(below).bytes, Some(7), "the share is left out");
        let on_it = measure(
            &LocalFs,
            &sb.path("d/share"),
            SizeMode::Auto,
            &SizeLimits::default(),
            &avoid,
            &never,
        );
        assert_eq!(on_it, Some(SizeOutcome::Skipped));
    }

    #[test]
    #[cfg(unix)]
    fn the_virtual_folders_of_the_system_are_never_counted() {
        let a = Avoid::system();
        for p in ["/proc", "/proc/1/maps", "/sys/kernel", "/dev", "/dev/shm/x"] {
            assert!(a.covers(Path::new(p)), "{p}");
        }
        for p in ["/", "/home/u", "/devices", "/processing", "/system"] {
            assert!(!a.covers(Path::new(p)), "{p}");
        }
        // The answer comes before any reading.
        let out = measure(
            &LocalFs,
            Path::new("/proc"),
            SizeMode::Auto,
            &SizeLimits::default(),
            &a,
            &never,
        );
        assert_eq!(out, Some(SizeOutcome::Skipped));
    }

    #[test]
    fn network_shares_and_silent_disks_are_avoided() {
        let vol = |mount: &str, kind, responsive| Volume {
            mount_point: PathBuf::from(mount),
            label: None,
            fs_type: "x".into(),
            device: "x".into(),
            kind,
            drive_letter: None,
            total: None,
            available: None,
            read_only: false,
            responsive,
        };
        let a = Avoid::for_volumes(&[
            vol("/mnt/nas", VolumeKind::Network, true),
            vol("/mnt/hung", VolumeKind::Fixed, false),
            vol("/mnt/disk", VolumeKind::Fixed, true),
        ]);
        assert!(a.covers(Path::new("/mnt/nas/music")));
        assert!(a.covers(Path::new("/mnt/hung")));
        assert!(!a.covers(Path::new("/mnt/disk/x")));
    }

    #[test]
    fn a_folder_on_another_device_is_told_apart() {
        let sb = Sandbox::new();
        sb.mkdir("d");
        let mut m = LocalFs.lstat(&sb.path("d")).unwrap();
        let dev = m.dev;
        assert!(!on_other_filesystem(dev, &m), "the same device");
        if let Some(d) = m.dev {
            m.dev = Some(d + 1);
            assert!(on_other_filesystem(Some(d), &m), "a mounted disk");
        }
        assert!(!on_other_filesystem(None, &m), "no numbers, no difference");
    }

    #[test]
    fn the_cache_drops_what_the_folder_says_has_changed() {
        let mut c = SizeCache::default();
        let t0 = Instant::now();
        let m1 = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10));
        let m2 = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(20));
        let p = PathBuf::from("/d/sub");
        assert_eq!(c.lookup(&p, m1, t0), Lookup::Unknown);
        c.mark_pending(p.clone());
        assert_eq!(c.lookup(&p, m1, t0), Lookup::Pending);
        let answer = SizeOutcome::Counted(FolderSize {
            items: 1,
            bytes: Some(9),
            complete: true,
        });
        c.store(p.clone(), answer, m1, t0);
        assert_eq!(c.lookup(&p, m1, t0), Lookup::Known(answer));
        // The folder was modified since.
        assert_eq!(c.lookup(&p, m2, t0), Lookup::Unknown);
        // Too old.
        let later = t0 + SizeCache::MAX_AGE + Duration::from_secs(1);
        assert_eq!(c.lookup(&p, m1, later), Lookup::Unknown);
        // The old figure is still there for a screen that wants something to show.
        assert_eq!(c.last(&p), Some(answer));
    }

    #[test]
    fn the_cache_invalidates_a_folder_and_the_ones_above_it() {
        let mut c = SizeCache::default();
        let now = Instant::now();
        let done = SizeOutcome::Skipped;
        for p in ["/a", "/a/b", "/a/b/c", "/a/x", "/z"] {
            c.store(PathBuf::from(p), done, None, now);
        }
        c.invalidate(Path::new("/a/b/c"));
        assert!(c.last(Path::new("/a/b/c")).is_none());
        assert!(c.last(Path::new("/a/b")).is_none(), "it lies in /a/b");
        assert!(c.last(Path::new("/a")).is_none());
        assert!(c.last(Path::new("/a/x")).is_some());
        assert!(c.last(Path::new("/z")).is_some());

        c.store(PathBuf::from("/q/1"), done, None, now);
        c.store(PathBuf::from("/q/2"), done, None, now);
        c.invalidate_children(Path::new("/q"));
        assert!(c.last(Path::new("/q/1")).is_none() && c.last(Path::new("/q/2")).is_none());
        assert!(c.last(Path::new("/a/x")).is_some());

        c.mark_pending(PathBuf::from("/p"));
        c.forget_pending();
        assert_eq!(
            c.lookup(Path::new("/p"), None, now),
            Lookup::Unknown,
            "a count that was dropped is asked for again"
        );
    }

    fn drain(rx: &crossbeam_channel::Receiver<CoreEvent>, n: usize) -> Vec<SizeEvent> {
        let mut got = Vec::new();
        while got.len() < n {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(CoreEvent::Size(e)) => got.push(e),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        got
    }

    #[test]
    fn the_worker_answers_each_folder_in_the_order_asked() {
        let sb = Sandbox::new();
        sb.write("a/f", vec![0u8; 11]);
        sb.write("b/f", vec![0u8; 22]);
        let (tx, rx) = unbounded();
        let w = SizeWorker::spawn(Arc::new(LocalFs), tx);
        w.newest(1);
        w.request(
            1,
            vec![sb.path("b"), sb.path("a")],
            SizeSettings::default(),
            Arc::new(Avoid::system()),
        );
        let got = drain(&rx, 2);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].path, sb.path("b"));
        assert_eq!(counted(got[0].outcome).bytes, Some(22));
        assert_eq!(counted(got[1].outcome).bytes, Some(11));
    }

    #[test]
    fn a_newer_epoch_drops_what_is_waiting_and_stops_the_count_in_progress() {
        let sb = Sandbox::new();
        sb.write("slow/f", "x");
        sb.write("quick/f", "x");
        let (tx, rx) = unbounded();
        let w = SizeWorker::spawn(Arc::new(LocalFs), tx);
        let avoid = Arc::new(Avoid::system());
        w.newest(2);
        // Epoch 1 was left behind before it began: it is not answered, the next is.
        w.request(
            1,
            vec![sb.path("slow"), sb.path("quick")],
            SizeSettings::default(),
            avoid.clone(),
        );
        w.request(
            2,
            vec![sb.path("quick")],
            SizeSettings::default(),
            avoid.clone(),
        );
        let got = drain(&rx, 1);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, sb.path("quick"));
        assert_eq!(counted(got[0].outcome).bytes, Some(1));
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "no second answer: the first epoch's work was dropped"
        );
    }
}
