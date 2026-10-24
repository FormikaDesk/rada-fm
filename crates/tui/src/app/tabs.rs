//! Tabs. Each one has its own folder, history, cursor, selection, sort order and view; the
//! clipboard, the journal and the undo are the same for all of them.
//!
//! The front tab's state is `App::tab`, so that everything that works on "the folder" reads it
//! there. The other tabs sit in `tabs.slots` as [`TabState`]s; switching swaps the two.

use rada_core::uistate::SavedTab;

use super::{App, FilterState, LoadState, ToastKind, Viewport};
use crate::nav::NavMove;
use crate::view::ViewMode;
use rada_core::archive::ArchiveView;
use rada_core::i18n::{Key, tf};
use rada_core::model::{DirListing, SortKey, SortSpec};
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// More tabs than this and a new one is refused (every tab is a folder kept in memory).
pub const MAX_TABS: usize = 24;

/// Everything a tab remembers.
#[derive(Default)]
pub struct TabState {
    pub cwd: PathBuf,
    /// Set while looking inside an archive: the folder shown is a read-only virtual one.
    pub archive: Option<ArchiveView>,
    pub listing: DirListing,
    pub load: LoadState,
    pub visible: Vec<usize>,
    pub cursor: usize,
    pub scroll: usize,
    pub marked: BTreeSet<OsString>,
    pub sort: SortSpec,
    /// The order of this tab was chosen by the person (a click on a column title, the Sort
    /// menu, the keys). Only then is it remembered with the tab: otherwise it follows
    /// `config.toml` at every start.
    pub sort_chosen: bool,
    pub(super) remembered: HashMap<PathBuf, OsString>,
    /// Folders visited, for back and forward.
    pub nav: crate::nav::NavHistory,
    pub(super) nav_move: Option<NavMove>,
    pub filter: Option<FilterState>,
    pub(super) sel_anchor: Option<(usize, BTreeSet<OsString>)>,
    /// How the folder is shown.
    pub view: ViewMode,
    /// Number of rows the list can show; set by the renderer.
    pub viewport: Viewport,
}

/// The tabs behind the front one, which one is in front, and what is remembered between
/// sessions.
pub struct Tabs {
    /// One slot per tab; the one of the front tab is empty, its state is `App::tab`.
    pub(super) slots: Vec<TabState>,
    pub active: usize,
    /// The tabs are saved with the rest of `saved_ui` and come back at the next start.
    pub remember: bool,
    /// Where tabs are saved and the rest of what is remembered between sessions.
    pub(super) saved_ui: rada_core::uistate::UiState,
    /// The number of the last read of a folder. It is one for all the tabs and only ever goes
    /// up, so that the answer to a read asked by another tab, or before the folder changed,
    /// is told from the one that is waited for.
    dir_gen: u64,
    /// The order of the folders, put away while the list of recent files has its own.
    pub(super) sort_outside_recents: Option<SortSpec>,
}

impl Tabs {
    pub fn new(
        slots: Vec<TabState>,
        active: usize,
        remember: bool,
        saved_ui: rada_core::uistate::UiState,
    ) -> Tabs {
        Tabs {
            slots,
            active,
            remember,
            saved_ui,
            dir_gen: 0,
            sort_outside_recents: None,
        }
    }

    pub fn count(&self) -> usize {
        self.slots.len()
    }

    /// The number of the last read of a folder: an answer with another one is out of date.
    pub fn generation(&self) -> u64 {
        self.dir_gen
    }

    /// A new read of a folder is asked for: the number to ask it with.
    pub(super) fn next_generation(&mut self) -> u64 {
        self.dir_gen += 1;
        self.dir_gen
    }

    /// The tab after the front one, round to the first; none when there is only one.
    pub(super) fn next(&self) -> Option<usize> {
        (self.count() > 1).then(|| (self.active + 1) % self.count())
    }

    /// The tab before the front one, round to the last; none when there is only one.
    pub(super) fn prev(&self) -> Option<usize> {
        (self.count() > 1).then(|| (self.active + self.count() - 1) % self.count())
    }

    /// The n-th tab counting from 1 (the last one when there are fewer).
    pub(super) fn nth(&self, n: usize) -> Option<usize> {
        (n > 0 && !self.slots.is_empty()).then(|| (n - 1).min(self.count() - 1))
    }

    /// Close a tab that is not the front one.
    pub(super) fn forget(&mut self, idx: usize) {
        self.slots.remove(idx);
        if idx < self.active {
            self.active -= 1;
        }
    }
}

impl TabState {
    /// What a tab gives up when another one comes to the front: a navigation that was still on
    /// its way is given up with it.
    fn leave(&mut self) {
        self.load = LoadState::Ready;
        self.nav_move = None;
    }

    /// The tab a new one beside this starts as: the same folder, view and order, and what is
    /// remembered about the folders, with a history and a cursor of its own.
    fn opened_beside(&self) -> TabState {
        let mut fresh = TabState::fresh(self.cwd.clone(), self.view, self.sort, self.sort_chosen);
        fresh.remembered = self.remembered.clone();
        fresh
    }

    /// A tab that has not been looked at yet: it loads its folder when it comes to the front.
    pub fn fresh(cwd: PathBuf, view: ViewMode, sort: SortSpec, sort_chosen: bool) -> TabState {
        TabState {
            listing: DirListing::new(cwd.clone(), Vec::new(), sort),
            nav: crate::nav::NavHistory::new(cwd.clone()),
            cwd,
            sort,
            sort_chosen,
            view,
            ..Default::default()
        }
    }
}

/// The order a saved tab comes back in, and whether it was the person's choice.
///
/// Only an order that was chosen in the tab is kept: otherwise the tab follows `base`, the
/// order of `config.toml` as it is now. A file from a version that wrote the order of every
/// tab (`sort_chosen` absent) cannot tell: there, a tab sorted by name and not reversed is
/// taken for one that was never touched, and any other is taken for a choice; since a date
/// that was reversed used to be the newest first, and is now the one that is not, that
/// direction is turned round so that the tab looks as it did.
pub(super) fn sort_from_saved(t: &SavedTab, base: SortSpec) -> (SortSpec, bool) {
    let key = match t.sort.as_deref() {
        Some("name") => Some(SortKey::Name),
        Some("size") => Some(SortKey::Size),
        Some("date") => Some(SortKey::Modified),
        Some("type") => Some(SortKey::Type),
        _ => None,
    };
    let Some(key) = key else {
        return (base, false);
    };
    let chosen = match t.sort_chosen {
        Some(c) => c,
        None => !(key == SortKey::Name && !t.reverse),
    };
    if !chosen {
        return (base, false);
    }
    let reverse = if t.sort_chosen.is_none() && key == SortKey::Modified {
        !t.reverse
    } else {
        t.reverse
    };
    (
        SortSpec {
            key,
            reverse,
            ..base
        },
        true,
    )
}

impl App {
    /// Make `incoming` the front tab. The one that was in front is handed back, to be put in
    /// its slot.
    fn bring_to_front(&mut self, incoming: TabState) -> TabState {
        let mut out = std::mem::replace(&mut self.tab, incoming);
        out.leave();
        out
    }

    /// Tab `i`, wherever its state is.
    fn tab_at(&self, i: usize) -> Option<&TabState> {
        if i == self.tabs.active {
            Some(&self.tab)
        } else {
            self.tabs.slots.get(i)
        }
    }

    /// Read the front tab's folder again, keeping the cursor and the selection.
    fn after_switch(&mut self) {
        let cwd = self.tab.cwd.clone();
        self.windows.address = None;
        let generation = self.tabs.next_generation();
        self.tab.load = LoadState::Loading(cwd.clone());
        self.svc.loader.load(cwd.clone(), generation);
        if self.tab.archive.is_none() && self.tag_view().is_none() {
            self.svc.watcher.watch(cwd);
        }
        self.preview.invalidate();
        self.folder_info.date_cache.clear();
        self.rebuild_visible();
        self.ensure_visible();
        self.request_preview();
        self.redraw.dirty = true;
    }

    /// The folder of tab `i`.
    pub fn tab_cwd(&self, i: usize) -> &Path {
        self.tab_at(i).map_or(&self.tab.cwd, |t| &t.cwd)
    }

    /// The name on the tab: the folder's name, "Home" for the home folder.
    pub fn tab_title(&self, i: usize) -> String {
        let p = self.tab_cwd(i);
        if p == self.home() {
            return Key::HomeLabel.tr().to_string();
        }
        if rada_core::recents::is_view(p) {
            return Key::PlaceRecent.tr().to_string();
        }
        if let Some(tag) = rada_core::tagsearch::view_name(p) {
            return tf(Key::TagViewTitle, &[&tag]);
        }
        let on_disk = p
            .file_name()
            .map(rada_core::display::name)
            .unwrap_or_else(|| rada_core::display::path(p));
        // What the Finder calls the folder ("Scrivania"), when that is not its name. (On
        // Windows the tab keeps the name on disk for now: see the notes of 0.6.0-rc.8.)
        if self.svc.platform.name().starts_with("macos") {
            self.places.shown_name(p, on_disk)
        } else {
            on_disk
        }
    }

    pub fn switch_tab(&mut self, to: usize) {
        if to == self.tabs.active || to >= self.tabs.count() {
            return;
        }
        let incoming = std::mem::take(&mut self.tabs.slots[to]);
        let out = self.bring_to_front(incoming);
        self.tabs.slots[self.tabs.active] = out;
        self.tabs.active = to;
        self.after_switch();
        self.save_ui_state();
    }

    pub fn next_tab(&mut self) {
        if let Some(to) = self.tabs.next() {
            self.switch_tab(to);
        }
    }

    pub fn prev_tab(&mut self) {
        if let Some(to) = self.tabs.prev() {
            self.switch_tab(to);
        }
    }

    /// A new tab, to the right of the front one, in the folder the front one is in.
    pub fn new_tab(&mut self) {
        if self.tabs.count() >= MAX_TABS {
            self.toast(ToastKind::Warn, tf(Key::TTabLimit, &[&MAX_TABS]), 3);
            return;
        }
        if let Some(name) = self.cursor_name() {
            self.tab.remembered.insert(self.tab.cwd.clone(), name);
        }
        let fresh = self.tab.opened_beside();
        let out = self.bring_to_front(fresh);
        let at = self.tabs.active + 1;
        self.tabs.slots[self.tabs.active] = out;
        self.tabs.slots.insert(at, TabState::default());
        self.tabs.active = at;
        self.after_switch();
        self.save_ui_state();
    }

    /// Close tab `idx`. The last one cannot be closed: it goes back to the home folder.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.count() {
            return;
        }
        if self.tabs.count() == 1 {
            let home = self.svc.home.clone();
            if self.tab.cwd != home || self.tab.archive.is_some() {
                self.open_dir(home);
            }
            return;
        }
        if idx == self.tabs.active {
            self.tabs.slots.remove(idx);
            let now = idx.min(self.tabs.count() - 1);
            let incoming = std::mem::take(&mut self.tabs.slots[now]);
            // The tab that was in front is closed: what comes back is dropped.
            let _ = self.bring_to_front(incoming);
            self.tabs.active = now;
            self.after_switch();
        } else {
            self.tabs.forget(idx);
        }
        self.redraw.dirty = true;
        self.save_ui_state();
    }

    /// Alt+1 … Alt+9: the n-th tab (the last one for Alt+9 when there are fewer).
    pub fn go_tab(&mut self, n: usize) {
        if let Some(to) = self.tabs.nth(n) {
            self.switch_tab(to);
        }
    }

    /// The tabs as they are remembered between sessions.
    pub fn saved_tabs(&self) -> Vec<SavedTab> {
        (0..self.tabs.count())
            .map(|i| {
                let t = self.tab_at(i).expect("a tab for each index");
                let (cwd, archive) = (&t.cwd, &t.archive);
                let (view, sort, chosen) = (t.view, t.sort, t.sort_chosen);
                // Inside an archive the folder is not on the disk: remember where the archive is.
                let path = match archive {
                    Some(a) => a
                        .archive
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| cwd.clone()),
                    // A list of tagged items is not a place to come back to.
                    None if rada_core::tagsearch::view_name(cwd).is_some() => self.svc.home.clone(),
                    None => cwd.clone(),
                };
                SavedTab {
                    path,
                    view: Some(view.id().to_string()),
                    // The order is remembered only where the person chose it: a tab that was
                    // never sorted follows the configuration, which may change.
                    sort: chosen.then(|| sort.key.label().to_string()),
                    reverse: chosen && sort.reverse,
                    sort_chosen: Some(chosen),
                }
            })
            .collect()
    }

    /// Write what is remembered between sessions, from a thread of its own.
    pub(super) fn save_ui_state(&mut self) {
        // Asked to choose a file for another program, rada leaves its own state as it is.
        if self.session.chooser.is_some() {
            return;
        }
        if self.tabs.remember {
            self.tabs.saved_ui.tabs = self.saved_tabs();
            self.tabs.saved_ui.active_tab = self.tabs.active;
        } else {
            self.tabs.saved_ui.tabs.clear();
            self.tabs.saved_ui.active_tab = 0;
        }
        rada_core::uistate::save_in_background(
            self.svc.platform.dirs().rada_state(),
            self.tabs.saved_ui.clone(),
        );
    }
}
