//! The size of folders as the interface sees it: asked for the folders on screen, shown as
//! "···" until the background worker has counted, kept while the folder stays as it is. The
//! counting itself is `rada_core::foldersize`; nothing here waits for it.

use rada_core::i18n::Key;
use std::collections::HashMap;
use std::time::Instant;

use rada_core::foldersize::{
    Avoid, FolderSize, Lookup, SizeCache, SizeEvent, SizeMode, SizeOutcome,
};
use rada_core::fs::FileKind;

use super::App;
use crate::fmt;
use rada_core::model::Entry;
use rada_core::platform::Volume;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What the Size column and the details pane say about a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderSizeView {
    /// Nothing: counting is off, or the place is one that is not counted.
    Nothing,
    /// Being counted.
    Counting,
    Counted(FolderSize),
}

pub struct Sizes {
    cache: SizeCache,
    /// Grows each time the front folder changes, which drops the counts of the one left.
    epoch: u64,
    /// The folder the epoch belongs to.
    dir: PathBuf,
    /// Asked for while drawing, sent right after.
    wanted: Vec<PathBuf>,
    /// Sent and not yet answered.
    outstanding: usize,
    /// The modification time each of those folders had when it was asked for: the answer is
    /// kept for that state of the folder.
    asked: HashMap<PathBuf, Option<std::time::SystemTime>>,
    avoid: Arc<Avoid>,
}

impl Default for Sizes {
    fn default() -> Self {
        Sizes {
            cache: SizeCache::default(),
            epoch: 0,
            dir: PathBuf::new(),
            wanted: Vec::new(),
            outstanding: 0,
            asked: HashMap::new(),
            avoid: Arc::new(Avoid::system()),
        }
    }
}

impl Sizes {
    pub fn set_volumes(&mut self, volumes: &[Volume], device_root: Option<&Path>) {
        self.avoid = Arc::new(Avoid::for_volumes_and(volumes, device_root));
    }
}

impl App {
    pub fn folder_size_mode(&self) -> SizeMode {
        self.svc.size_settings.mode
    }

    /// Count folders as `settings` say from now on (what was counted is forgotten).
    pub fn set_folder_size_settings(&mut self, settings: rada_core::foldersize::SizeSettings) {
        self.svc.size_settings = settings;
        let s = &mut self.folder_sizes;
        s.cache.clear();
        s.wanted.clear();
        s.asked.clear();
        s.outstanding = 0;
        s.epoch += 1;
        self.svc.sizes.newest(s.epoch);
        self.redraw.dirty = true;
    }

    /// The folder in front changed: what was being counted for the old one is dropped.
    fn follow_folder_for_sizes(&mut self) {
        if self.folder_sizes.dir == self.cwd {
            return;
        }
        let s = &mut self.folder_sizes;
        s.dir = self.cwd.clone();
        s.epoch += 1;
        s.wanted.clear();
        s.asked.clear();
        s.outstanding = 0;
        s.cache.forget_pending();
        self.svc.sizes.newest(s.epoch);
    }

    /// Ask for the sizes of these entries (indices into the listing), those not known yet.
    /// Called while drawing: it only notes what is wanted and sends it to the worker.
    pub fn want_folder_sizes(&mut self, indices: impl Iterator<Item = usize>) {
        if self.folder_size_mode() == SizeMode::Off
            || self.archive.is_some()
            || self.tag_view().is_some()
        {
            return;
        }
        self.follow_folder_for_sizes();
        let now = Instant::now();
        let mut wanted = Vec::new();
        for i in indices {
            let Some(e) = self.listing.all().get(i) else {
                continue;
            };
            if e.kind != FileKind::Dir {
                continue;
            }
            if self.folder_sizes.cache.lookup(&e.path, e.mtime, now) == Lookup::Unknown {
                wanted.push((e.path.clone(), e.mtime));
            }
        }
        // The counted size of a folder that changed is shown as unknown until it is counted
        // again (a stale figure would be a false one).
        for (path, mtime) in wanted {
            self.folder_sizes.cache.mark_pending(path.clone());
            self.folder_sizes.asked.insert(path.clone(), mtime);
            self.folder_sizes.wanted.push(path);
        }
        self.send_wanted_sizes();
    }

    fn send_wanted_sizes(&mut self) {
        let s = &mut self.folder_sizes;
        if s.wanted.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut s.wanted);
        s.outstanding += paths.len();
        self.svc
            .sizes
            .request(s.epoch, paths, self.svc.size_settings, s.avoid.clone());
    }

    /// What to show for this entry, if it is a folder.
    pub fn folder_size_view(&self, e: &Entry) -> FolderSizeView {
        if self.folder_size_mode() == SizeMode::Off || e.kind != FileKind::Dir {
            return FolderSizeView::Nothing;
        }
        match self
            .folder_sizes
            .cache
            .lookup(&e.path, e.mtime, Instant::now())
        {
            Lookup::Known(SizeOutcome::Counted(c)) => FolderSizeView::Counted(c),
            Lookup::Known(_) => FolderSizeView::Nothing,
            Lookup::Pending | Lookup::Unknown => FolderSizeView::Counting,
        }
    }

    /// The text of the Size column for a folder, `width` columns wide at most.
    pub fn folder_size_text(&self, e: &Entry, width: usize) -> String {
        match self.folder_size_view(e) {
            FolderSizeView::Nothing => String::new(),
            FolderSizeView::Counting => "···".to_string(),
            FolderSizeView::Counted(c) => match c.bytes {
                Some(b) if c.complete => fmt::size(b),
                Some(b) => format!("> {}", fmt::size(b)),
                None => {
                    let full = fmt::count(c.items, Key::ItemOne, Key::ItemMany);
                    if full.chars().count() <= width {
                        full
                    } else {
                        fmt::thousands(c.items)
                    }
                }
            },
        }
    }

    /// The folder under the cursor, as an index into the listing.
    pub fn current_index(&self) -> Option<usize> {
        self.visible.get(self.cursor).copied()
    }

    /// "12 items · 4.8 MB" for the details pane, once the folder has been counted; while it
    /// is being counted the figure is "···".
    pub fn folder_summary(&self, e: &Entry) -> Option<String> {
        match self.folder_size_view(e) {
            FolderSizeView::Nothing => None,
            FolderSizeView::Counting => Some(format!(
                "{} · ···",
                rada_core::i18n::type_label(&e.type_label)
            )),
            FolderSizeView::Counted(c) => {
                let items = fmt::count(c.items, Key::ItemOne, Key::ItemMany);
                Some(match c.bytes {
                    Some(b) if c.complete => format!("{items} · {}", fmt::size(b)),
                    Some(b) => format!("{items} · > {}", fmt::size(b)),
                    None => items,
                })
            }
        }
    }

    /// An answer of the worker.
    pub(super) fn on_size_event(&mut self, ev: SizeEvent) {
        let s = &mut self.folder_sizes;
        let mtime = s.asked.remove(&ev.path).flatten();
        s.outstanding = s.outstanding.saturating_sub(1);
        s.cache.store(ev.path, ev.outcome, mtime, Instant::now());
        // The order by size is redone when the batch is in, not folder by folder: rows that
        // jump while being looked at are worse than a short wait.
        if s.outstanding == 0 {
            self.sync_folder_sizes();
        }
    }

    /// Give the listing the sizes counted so far (it is ordered again if it goes by size).
    pub(super) fn sync_folder_sizes(&mut self) {
        let cache = &self.folder_sizes.cache;
        let keep = self.cursor_name();
        let moved = self
            .listing
            .set_folder_sizes(|e| match cache.last(&e.path) {
                Some(SizeOutcome::Counted(FolderSize { bytes: Some(b), .. })) => Some(b),
                _ => None,
            });
        if moved {
            self.rebuild_visible();
            self.restore_cursor(keep);
        }
    }

    /// The watcher heard of a change in `dir`: what was counted there may be false now.
    pub(super) fn forget_folder_sizes(&mut self, dir: &Path, paths: &[PathBuf], rescan: bool) {
        let cache = &mut self.folder_sizes.cache;
        if rescan {
            cache.invalidate_children(dir);
        } else {
            for p in paths {
                cache.invalidate(p);
            }
        }
    }
}
