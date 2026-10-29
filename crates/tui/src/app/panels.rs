//! What the panels view needs besides drawing: which layout the window gets, the tree of
//! folders of the NAV box and the keys of NAV and ANTEPRIMA.
//!
//! Tab takes the keyboard from the folder to NAV, from NAV to ANTEPRIMA and back to the folder.

use super::{App, ToastKind};
use crate::keymap::Action;
use crate::panels::{NavKind, NavMode, NavRow};
use crate::sidebar::{self, Origin};
use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What git said about the folder shown: the answer, and the folder it is about.
pub struct GitView {
    pub dir: PathBuf,
    pub status: rada_core::git::GitStatus,
}

/// What is worked out about the folder shown, and again when it changes.
#[derive(Default)]
pub struct FolderInfo {
    /// What git said about the folder shown (panels view), with the folder it was about.
    pub git: Option<GitView>,
    /// The folder and the generation of its listing git was last asked about, and when.
    pub(super) git_asked: Option<(PathBuf, u64)>,
    pub(super) git_busy: Option<Instant>,
    /// What is known of the size of folders (see `sizes.rs`).
    pub(crate) sizes: super::sizes::Sizes,
    /// Date strings of the visible rows, worked out once and kept until the folder, the
    /// clock's hour or the style changes.
    pub(super) date_cache: Vec<Option<String>>,
    date_cache_key: (u64, usize),
}

impl FolderInfo {
    /// Forget what git said and what was asked of it.
    pub(super) fn forget_git(&mut self) {
        self.git = None;
        self.git_asked = None;
        self.git_busy = None;
    }

    /// Keep the dates worked out only while they are for this hour and this many entries
    /// (`key`).
    pub(super) fn dates_for(&mut self, key: (u64, usize)) {
        if self.date_cache_key != key || self.date_cache.len() != key.1 {
            self.date_cache.clear();
            self.date_cache.resize(key.1, None);
            self.date_cache_key = key;
        }
    }

    /// The date text of an entry that [`App::warm_dates`] has seen.
    pub fn date_text(&self, listing_index: usize) -> &str {
        self.date_cache
            .get(listing_index)
            .and_then(|s| s.as_deref())
            .unwrap_or("")
    }
}

/// A question to git that has had no answer after this long is not waited for.
const GIT_WAIT: Duration = Duration::from_secs(6);

/// The dots a line of tags has: where they start in the TAGS section, and how many.
pub(crate) fn tag_range(kind: &NavKind) -> Option<(usize, usize)> {
    match kind {
        NavKind::Tags { first, len, .. } => Some((*first, *len)),
        _ => None,
    }
}

impl App {
    /// Whether a window `width` × `height` is drawn as panels.
    pub fn panels_wanted(&self, width: u16, height: u16) -> Option<crate::panels::Columns> {
        crate::panels::columns_for(
            self.settings.layout_pref,
            self.settings.layout,
            self.session.chooser.is_some(),
            width,
            height,
        )
    }

    /// Choose the layout (the View menu): it is remembered, and the window decides what is drawn
    /// when the choice is `auto`.
    pub fn set_layout_pref(&mut self, pref: crate::panels::LayoutPref) {
        if self.settings.layout_pref == pref {
            return;
        }
        self.settings.layout_pref = pref;
        if pref == crate::panels::LayoutPref::Classic {
            self.side_nav.focus = false;
            self.preview.focus = false;
        }
        self.tabs.saved_ui.layout = Some(pref.id().to_string());
        self.save_ui_state();
        self.redraw.dirty = true;
    }

    /// The folder the NAV tree starts at.
    pub fn tree_root(&self) -> &Path {
        self.home()
    }

    /// Make the tree lead to the current folder and ask for what it still lacks. Called by the
    /// drawing: it only sends requests, the answers arrive as events.
    pub fn want_tree(&mut self) {
        if self.settings.nav_mode == NavMode::Path {
            // Only the folder shown is asked about: its subfolders are what the box lists.
            let want = self
                .tab
                .view_title()
                .is_none()
                .then(|| self.tab.cwd.clone());
            if self.side_nav.tree.open.iter().ne(want.iter()) {
                self.side_nav.tree.open = want.into_iter().collect();
            }
            for dir in self
                .side_nav
                .tree
                .wanted(self.settings.show_hidden, self.tabs.generation())
            {
                self.svc.tree_dirs(dir, self.settings.show_hidden);
            }
            return;
        }
        let root = self.svc.home.clone();
        self.side_nav.tree.follow(&root, &self.tab.cwd);
        for dir in self
            .side_nav
            .tree
            .wanted(self.settings.show_hidden, self.tabs.generation())
        {
            self.svc.tree_dirs(dir, self.settings.show_hidden);
        }
    }

    /// Ask git about the folder shown if it was not asked yet, and not while another question
    /// is still out. Called by the drawing: it only sends a request, the answer arrives as an
    /// event. A virtual folder (a tag, the recent files, an archive) is not in a repository.
    pub fn want_git(&mut self) {
        if !self.svc.git.enabled() || self.tab.archive.is_some() || self.tab.view_title().is_some()
        {
            if self.folder_info.git.take().is_some() {
                self.redraw.dirty = true;
            }
            return;
        }
        let key = (self.tab.cwd.clone(), self.tabs.generation());
        if self.folder_info.git_asked.as_ref() == Some(&key) {
            return;
        }
        if self
            .folder_info
            .git_busy
            .is_some_and(|t| t.elapsed() < GIT_WAIT)
        {
            return;
        }
        self.folder_info.git_asked = Some(key);
        self.folder_info.git_busy = Some(Instant::now());
        self.svc.git_status(&self.tab.cwd);
    }

    /// What git said about a folder.
    pub(super) fn on_git(&mut self, dir: PathBuf, status: Option<rada_core::git::GitStatus>) {
        self.folder_info.git_busy = None;
        if dir == self.tab.cwd {
            self.folder_info.git = status.map(|status| GitView { dir, status });
        }
        // The folder may have changed while git was thinking: the next frame asks again.
        self.redraw.dirty = true;
    }

    /// What git says about the folder shown, when it is the one it was asked about.
    pub fn git_here(&self) -> Option<&rada_core::git::GitStatus> {
        self.folder_info
            .git
            .as_ref()
            .filter(|g| g.dir == self.tab.cwd)
            .map(|g| &g.status)
    }

    /// The command that shows or hides the details (Alt+P, F6, the button at the top right): in
    /// the view of two columns it puts ANTEPRIMA in the place of NAV, and NAV back. The
    /// keyboard follows the box it was in.
    pub(super) fn toggle_left_preview(&mut self) {
        if !self.side_nav.wanted {
            self.toggle_sidebar();
            self.side_nav.left_preview = true;
            self.redraw.dirty = true;
            return;
        }
        let had_focus = self.side_nav.focus || self.preview.focus;
        self.side_nav.left_preview = !self.side_nav.left_preview;
        if had_focus {
            self.side_nav.focus = !self.side_nav.left_preview;
            self.preview.focus = self.side_nav.left_preview;
        }
        self.redraw.dirty = true;
    }

    /// Tab in the panels view: the folder, NAV, ANTEPRIMA, the folder. With two columns the
    /// box on the left is the only one after the folder, whichever of the two it is.
    pub(super) fn switch_pane_panels(&mut self) {
        if self.frame.panels_two {
            if self.side_nav.focus || self.preview.focus {
                self.side_nav.focus = false;
                self.preview.focus = false;
                return;
            }
            if !self.side_nav.wanted {
                self.toggle_sidebar();
            }
            if self.side_nav.left_preview {
                self.preview.focus = true;
            } else {
                let rows = self.nav_rows();
                self.side_nav.nav_cursor = Self::nav_current(&rows, &self.tab.cwd).unwrap_or(0);
                self.side_nav.focus = true;
            }
            return;
        }
        if self.side_nav.focus {
            self.side_nav.focus = false;
            self.preview.focus = self.frame.details_shown != crate::ui::DetailsMode::Hidden;
            return;
        }
        if self.preview.focus {
            self.preview.focus = false;
            return;
        }
        if !self.side_nav.wanted {
            self.toggle_sidebar();
        }
        let rows = self.nav_rows();
        self.side_nav.nav_cursor = Self::nav_current(&rows, &self.tab.cwd).unwrap_or(0);
        self.side_nav.focus = true;
    }

    /// The wheel over the NAV box: it scrolls the rows, three at a time, and not the cursor.
    pub(super) fn scroll_nav(&mut self, down: bool) {
        const STEP: usize = 3;
        self.frame.nav_scroll = if down {
            self.frame.nav_scroll + STEP
        } else {
            self.frame.nav_scroll.saturating_sub(STEP)
        };
        self.redraw.dirty = true;
    }

    /// An arrow clicked or pressed on a folder of the tree: open or close it.
    pub fn toggle_tree(&mut self, dir: &Path) {
        self.side_nav.tree.toggle(dir);
        self.redraw.dirty = true;
    }

    /// Left and right in NAV: they open and close the folders of the tree and move between
    /// the dots of the tags. `true`: dealt with.
    pub(super) fn on_nav_arrow(&mut self, right: bool) -> bool {
        let rows = self.nav_rows();
        let Some(row) = rows.get(self.side_nav.nav_cursor).cloned() else {
            return false;
        };
        match row.kind {
            NavKind::Folder { open } => {
                match (right, open) {
                    (true, Some(false)) => self.side_nav.tree.toggle(&row.path),
                    (true, Some(true)) => {
                        // Into the folder: its first child.
                        if rows
                            .get(self.side_nav.nav_cursor + 1)
                            .is_some_and(|r| r.depth > row.depth)
                        {
                            self.side_nav.nav_cursor += 1;
                        }
                    }
                    (false, Some(true)) => self.side_nav.tree.toggle(&row.path),
                    (false, _) => {
                        // To the folder this one is in.
                        if let Some(up) = rows[..self.side_nav.nav_cursor]
                            .iter()
                            .rposition(|r| r.depth < row.depth)
                        {
                            self.side_nav.nav_cursor = up;
                        }
                    }
                    (true, None) => {}
                }
                true
            }
            NavKind::Tags { .. } => {
                self.step_tag(right, &rows);
                true
            }
            _ => false,
        }
    }

    /// Left and Right on the lines of tag dots: from one dot to the next, and on to the next
    /// line when the line ends.
    fn step_tag(&mut self, right: bool, rows: &[NavRow]) {
        let shown = rows
            .iter()
            .filter_map(|r| tag_range(&r.kind))
            .map(|(first, len)| first + len)
            .max()
            .unwrap_or(1);
        let now = self.side_nav.nav_tag.min(shown - 1);
        self.side_nav.nav_tag = if right {
            (now + 1).min(shown - 1)
        } else {
            now.saturating_sub(1)
        };
        let dot = self.side_nav.nav_tag;
        if let Some(at) = rows
            .iter()
            .position(|r| tag_range(&r.kind).is_some_and(|(f, n)| (f..f + n).contains(&dot)))
        {
            self.side_nav.nav_cursor = at;
        }
    }

    /// The cursor of NAV has moved onto a line of tag dots: the dot is the one in the same
    /// column as before, or the last that line has.
    fn keep_tag_column(&mut self, rows: &[NavRow]) {
        let Some((first, len)) = rows
            .get(self.side_nav.nav_cursor)
            .and_then(|r| tag_range(&r.kind))
        else {
            return;
        };
        let dot = self.side_nav.nav_tag;
        let column = rows
            .iter()
            .filter_map(|r| tag_range(&r.kind))
            .find(|(f, n)| (*f..f + n).contains(&dot))
            .map_or(0, |(f, _)| dot - f);
        self.side_nav.nav_tag = first + column.min(len - 1);
    }

    /// The name of the tag whose dot the keyboard or the pointer is on in NAV: the dots have
    /// no names of their own.
    pub fn nav_tag_name(&self) -> Option<String> {
        if !self.frame.panels_shown {
            return None;
        }
        if self.side_nav.focus {
            let rows = self.nav_rows();
            let (first, len) = tag_range(&rows.get(self.side_nav.nav_cursor)?.kind)?;
            let path = self.nav_tag_path(first, len);
            return rada_core::tagsearch::view_name(&path);
        }
        rada_core::tagsearch::view_name(self.hovered_place()?)
    }

    /// Where the dot of the keyboard goes, among the `len` dots of a line from `first`.
    fn nav_tag_path(&self, first: usize, len: usize) -> PathBuf {
        let dot = self.side_nav.nav_tag.clamp(first, first + len.max(1) - 1);
        sidebar::tag_items(self)
            .get(dot)
            .map(|i| i.path.clone())
            .unwrap_or_default()
    }

    /// A key while the NAV box has the focus (the classic pane's keys, on the rows of this
    /// one). `true`: dealt with here.
    pub(super) fn on_nav_action(&mut self, action: Action) -> bool {
        use Action::*;
        let rows = self.nav_rows();
        let last = rows.len().saturating_sub(1);
        self.side_nav.nav_cursor = self.side_nav.nav_cursor.min(last);
        let here = rows.get(self.side_nav.nav_cursor).cloned();
        match action {
            Up => self.side_nav.nav_cursor = nav_step(&rows, self.side_nav.nav_cursor, false),
            Down => self.side_nav.nav_cursor = nav_step(&rows, self.side_nav.nav_cursor, true),
            First => self.side_nav.nav_cursor = 0,
            Last => self.side_nav.nav_cursor = last,
            Open => {
                if !self.nav_open(here) {
                    return true;
                }
            }
            Parent | ClearSelection | SwitchPane => {
                if action == SwitchPane {
                    self.switch_pane_panels();
                } else {
                    self.side_nav.focus = false;
                }
            }
            Bookmark => self.nav_bookmark(here),
            Trash | DeletePermanently => self.nav_remove_bookmark(here),
            Filter => {
                self.side_nav.focus = false;
                return false;
            }
            a if super::side::PASS_THROUGH.contains(&a) => return false,
            _ => {
                let back = self.key_for(SwitchPane).unwrap_or_else(|| "Tab".into());
                self.toast(ToastKind::Info, tf(Key::TSidebarFocus, &[&back]), 3);
            }
        }
        self.keep_tag_column(&rows);
        true
    }

    /// Enter on a row of NAV: go to its folder. False for a row that is only a heading, a
    /// gap or a "more": nothing is done.
    fn nav_open(&mut self, here: Option<NavRow>) -> bool {
        if let Some(row) = here {
            let path = match row.kind {
                NavKind::Tags { first, len, .. } => self.nav_tag_path(first, len),
                NavKind::Heading | NavKind::Gap | NavKind::More => return false,
                _ => row.path,
            };
            self.side_nav.focus = false;
            self.open_dir(path);
        }
        true
    }

    /// The bookmark key on a folder of NAV: pin it, or take it out.
    fn nav_bookmark(&mut self, here: Option<NavRow>) {
        if let Some(row) = here
            && matches!(row.kind, NavKind::Folder { .. } | NavKind::Favorite(_))
        {
            let add = !self.places.paths.bookmarks.contains(&row.path);
            self.set_bookmark(row.path, add);
        }
    }

    /// Delete on a row of NAV: only a pinned folder can be taken out.
    fn nav_remove_bookmark(&mut self, here: Option<NavRow>) {
        let items = sidebar::items(self, false);
        match here.map(|r| r.kind) {
            Some(NavKind::Favorite(n))
                if matches!(items[n].origin, Origin::Bookmark { removable: true }) =>
            {
                let path = items[n].path.clone();
                self.set_bookmark(path, false);
            }
            _ => self.toast(ToastKind::Info, Key::TOnlyBookmarksRemovable.tr(), 3),
        }
    }

    /// A key while ANTEPRIMA has the focus: the arrows scroll the preview. `true`: dealt with.
    pub(super) fn on_preview_action(&mut self, action: Action) -> bool {
        use Action::*;
        match action {
            Up => self.preview.scroll_up(1),
            Down => self.preview.scroll_down(1),
            First => self.preview.scroll = 0,
            PageUp | PreviewUp => self.preview.scroll_up(10),
            PageDown | PreviewDown => self.preview.scroll_down(10),
            // Tab goes on round: back to the folder.
            Parent | ClearSelection | SwitchPane => self.preview.focus = false,
            Filter => {
                self.preview.focus = false;
                return false;
            }
            _ => return false,
        }
        true
    }
}

/// The next row from `from`, up or down, that can be chosen; `from` when there is none.
fn nav_step(rows: &[NavRow], from: usize, down: bool) -> usize {
    let last = rows.len().saturating_sub(1);
    let mut i = from;
    loop {
        let next = if down {
            (i + 1).min(last)
        } else {
            i.saturating_sub(1)
        };
        if next == i {
            return from;
        }
        i = next;
        if rows[i].selectable() {
            return i;
        }
    }
}
