//! Tabs. Each one has its own folder, history, cursor, selection, sort order and view; the
//! clipboard, the journal and the undo are the same for all of them.
//!
//! The front tab's state lives in the fields of [`App`] itself, so that everything that
//! works on "the folder" keeps doing so. The other tabs sit in `tabs` as [`TabState`]s;
//! switching swaps the two.

use rada_core::uistate::SavedTab;

use super::*;

/// More tabs than this and a new one is refused (every tab is a folder kept in memory).
pub const MAX_TABS: usize = 24;

/// Everything a tab remembers.
#[derive(Default)]
pub struct TabState {
    pub cwd: PathBuf,
    pub archive: Option<ArchiveView>,
    pub listing: DirListing,
    pub load: LoadState,
    pub visible: Vec<usize>,
    pub cursor: usize,
    pub scroll: usize,
    pub marked: BTreeSet<OsString>,
    pub sort: SortSpec,
    pub remembered: HashMap<PathBuf, OsString>,
    pub nav: crate::nav::NavHistory,
    pub nav_move: Option<NavMove>,
    pub filter: Option<FilterState>,
    pub sel_anchor: Option<(usize, BTreeSet<OsString>)>,
    pub view: ViewMode,
    pub viewport: Viewport,
}

impl TabState {
    /// A tab that has not been looked at yet: it loads its folder when it comes to the front.
    pub fn fresh(cwd: PathBuf, view: ViewMode, sort: SortSpec) -> TabState {
        TabState {
            listing: DirListing::new(cwd.clone(), Vec::new(), sort),
            nav: crate::nav::NavHistory::new(cwd.clone()),
            cwd,
            sort,
            view,
            ..Default::default()
        }
    }
}

pub(super) fn sort_from_saved(t: &SavedTab, base: SortSpec) -> SortSpec {
    let key = match t.sort.as_deref() {
        Some("name") => SortKey::Name,
        Some("size") => SortKey::Size,
        Some("date") => SortKey::Modified,
        Some("type") => SortKey::Type,
        _ => base.key,
    };
    SortSpec {
        key,
        reverse: if t.sort.is_some() {
            t.reverse
        } else {
            base.reverse
        },
        ..base
    }
}

impl App {
    /// Move the front tab's state out of the fields, leaving them empty.
    fn put_away(&mut self) -> TabState {
        TabState {
            cwd: std::mem::take(&mut self.cwd),
            archive: self.archive.take(),
            listing: std::mem::take(&mut self.listing),
            // A navigation that was still on its way is given up with the tab.
            load: LoadState::Ready,
            visible: std::mem::take(&mut self.visible),
            cursor: std::mem::take(&mut self.cursor),
            scroll: std::mem::take(&mut self.scroll),
            marked: std::mem::take(&mut self.marked),
            sort: self.sort,
            remembered: std::mem::take(&mut self.remembered),
            nav: std::mem::take(&mut self.nav),
            nav_move: None,
            filter: self.filter.take(),
            sel_anchor: self.sel_anchor.take(),
            view: self.view,
            viewport: self.viewport,
        }
    }

    /// Make `t` the front tab.
    fn bring_in(&mut self, t: TabState) {
        self.cwd = t.cwd;
        self.archive = t.archive;
        self.listing = t.listing;
        self.load = t.load;
        self.visible = t.visible;
        self.cursor = t.cursor;
        self.scroll = t.scroll;
        self.marked = t.marked;
        self.sort = t.sort;
        self.remembered = t.remembered;
        self.nav = t.nav;
        self.nav_move = t.nav_move;
        self.filter = t.filter;
        self.sel_anchor = t.sel_anchor;
        self.view = t.view;
        self.viewport = t.viewport;
    }

    /// Read the front tab's folder again, keeping the cursor and the selection.
    fn after_switch(&mut self) {
        let cwd = self.cwd.clone();
        self.address = None;
        self.dir_gen += 1;
        self.load = LoadState::Loading(cwd.clone());
        self.svc.loader.load(cwd.clone(), self.dir_gen);
        if self.archive.is_none() {
            self.svc.watcher.watch(cwd);
        }
        self.previewed = None;
        self.date_cache.clear();
        self.rebuild_visible();
        self.ensure_visible();
        self.request_preview();
        self.dirty = true;
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// The folder of tab `i`.
    pub fn tab_cwd(&self, i: usize) -> &Path {
        if i == self.active_tab {
            &self.cwd
        } else {
            self.tabs
                .get(i)
                .map(|t| t.cwd.as_path())
                .unwrap_or(&self.cwd)
        }
    }

    /// The name on the tab: the folder's name, "Home" for the home folder.
    pub fn tab_title(&self, i: usize) -> String {
        let p = self.tab_cwd(i);
        if p == self.home() {
            return "Home".to_string();
        }
        p.file_name()
            .map(rada_core::display::name)
            .unwrap_or_else(|| rada_core::display::path(p))
    }

    pub fn switch_tab(&mut self, to: usize) {
        if to == self.active_tab || to >= self.tabs.len() {
            return;
        }
        let out = self.put_away();
        self.tabs[self.active_tab] = out;
        let incoming = std::mem::take(&mut self.tabs[to]);
        self.bring_in(incoming);
        self.active_tab = to;
        self.after_switch();
        self.save_ui_state();
    }

    pub fn next_tab(&mut self) {
        let n = self.tabs.len();
        if n > 1 {
            self.switch_tab((self.active_tab + 1) % n);
        }
    }

    pub fn prev_tab(&mut self) {
        let n = self.tabs.len();
        if n > 1 {
            self.switch_tab((self.active_tab + n - 1) % n);
        }
    }

    /// A new tab, to the right of the front one, in the folder the front one is in.
    pub fn new_tab(&mut self) {
        if self.tabs.len() >= MAX_TABS {
            self.toast(ToastKind::Warn, format!("{MAX_TABS} tabs is the limit"), 3);
            return;
        }
        if let Some(name) = self.cursor_name() {
            self.remembered.insert(self.cwd.clone(), name);
        }
        let (cwd, view, sort, remembered) = (
            self.cwd.clone(),
            self.view,
            self.sort,
            self.remembered.clone(),
        );
        let out = self.put_away();
        self.tabs[self.active_tab] = out;
        let at = self.active_tab + 1;
        self.tabs.insert(at, TabState::default());
        self.active_tab = at;
        let mut fresh = TabState::fresh(cwd, view, sort);
        fresh.remembered = remembered;
        self.bring_in(fresh);
        self.after_switch();
        self.save_ui_state();
    }

    /// Close tab `idx`. The last one cannot be closed: it goes back to the home folder.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            let home = self.svc.home.clone();
            if self.cwd != home || self.archive.is_some() {
                self.open_dir(home);
            }
            return;
        }
        if idx == self.active_tab {
            let _ = self.put_away();
            self.tabs.remove(idx);
            let now = idx.min(self.tabs.len() - 1);
            let incoming = std::mem::take(&mut self.tabs[now]);
            self.bring_in(incoming);
            self.active_tab = now;
            self.after_switch();
        } else {
            self.tabs.remove(idx);
            if idx < self.active_tab {
                self.active_tab -= 1;
            }
        }
        self.dirty = true;
        self.save_ui_state();
    }

    /// Alt+1 … Alt+9: the n-th tab (the last one for Alt+9 when there are fewer).
    pub fn go_tab(&mut self, n: usize) {
        if n == 0 || self.tabs.is_empty() {
            return;
        }
        self.switch_tab((n - 1).min(self.tabs.len() - 1));
    }

    /// The tabs as they are remembered between sessions.
    pub fn saved_tabs(&self) -> Vec<SavedTab> {
        (0..self.tabs.len())
            .map(|i| {
                let (cwd, archive, view, sort) = if i == self.active_tab {
                    (&self.cwd, &self.archive, self.view, self.sort)
                } else {
                    let t = &self.tabs[i];
                    (&t.cwd, &t.archive, t.view, t.sort)
                };
                // Inside an archive the folder is not on the disk: remember where the archive is.
                let path = match archive {
                    Some(a) => a
                        .archive
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| cwd.clone()),
                    None => cwd.clone(),
                };
                SavedTab {
                    path,
                    view: Some(view.id().to_string()),
                    sort: Some(sort.key.label().to_string()),
                    reverse: sort.reverse,
                }
            })
            .collect()
    }

    /// Write what is remembered between sessions, from a thread of its own.
    pub(super) fn save_ui_state(&mut self) {
        if self.remember_tabs {
            self.saved_ui.tabs = self.saved_tabs();
            self.saved_ui.active_tab = self.active_tab;
        } else {
            self.saved_ui.tabs.clear();
            self.saved_ui.active_tab = 0;
        }
        rada_core::uistate::save_in_background(
            self.svc.platform.dirs().rada_state(),
            self.saved_ui.clone(),
        );
    }
}
