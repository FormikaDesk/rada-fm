//! What the panels view needs besides drawing: which layout the window gets, the tree of
//! folders of the NAV box and the keys of NAV and ANTEPRIMA.
//!
//! Tab takes the keyboard from the folder to NAV, from NAV to ANTEPRIMA and back to the folder.

use rada_core::platform::{PlaceKind, TagColor as Hue};

use super::{App, ToastKind};
use crate::keymap::Action;
use crate::panels::{NavKind, NavMode, NavRow};
use crate::sidebar::{self, Origin, Section};
use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What git said about the folder shown: the answer, and the folder it is about.
pub struct GitView {
    pub dir: PathBuf,
    pub status: rada_core::git::GitStatus,
}

/// A question to git that has had no answer after this long is not waited for.
const GIT_WAIT: Duration = Duration::from_secs(6);

/// The least the way to the folder is cut down to: the blank row, its title, "… more" and the
/// folder shown.
const WAY_MIN: usize = 4;

fn plain_row(kind: NavKind, name: String, path: PathBuf) -> NavRow {
    NavRow {
        kind,
        depth: 0,
        path,
        name,
        count: None,
    }
}

fn gap_row() -> NavRow {
    plain_row(NavKind::Gap, String::new(), PathBuf::new())
}

fn heading_row(name: &str) -> NavRow {
    plain_row(NavKind::Heading, name.to_string(), PathBuf::new())
}

/// The rows of the places of the sidebar whose section `keep` accepts.
fn favorite_rows(items: &[sidebar::Item], keep: impl Fn(Section) -> bool) -> Vec<NavRow> {
    (0..items.len())
        .filter(|n| keep(items[*n].section))
        .map(|n| {
            plain_row(
                NavKind::Favorite(n),
                items[n].name.clone(),
                items[n].path.clone(),
            )
        })
        .collect()
}

/// Rows a block takes with its blank row and its title, none when it is empty.
fn block_len(rows: &[NavRow]) -> usize {
    if rows.is_empty() { 0 } else { rows.len() + 2 }
}

/// The devices as one disk and "… 2 more", when there are more than that.
fn main_disk_only(mut disks: Vec<NavRow>) -> Vec<NavRow> {
    if disks.len() > 2 {
        let others = disks.len() - 1;
        disks.truncate(1);
        disks.push(plain_row(
            NavKind::More,
            tf(Key::NavMoreDisks, &[&others]),
            PathBuf::new(),
        ));
    }
    disks
}

/// The way to the folder shown with its title, in `room` rows: the subfolders go first, then
/// the oldest folders of the way (a row says how many).
fn fit_way(chain: Vec<NavRow>, kids: Vec<NavRow>, room: usize) -> Vec<NavRow> {
    if chain.is_empty() {
        return Vec::new();
    }
    let avail = room.max(WAY_MIN) - 2;
    let hidden = if chain.len() > avail {
        chain.len() + 1 - avail
    } else {
        0
    };
    let mut rows = vec![gap_row(), heading_row(Key::PanelPath.tr())];
    if hidden > 0 {
        rows.push(plain_row(
            NavKind::More,
            tf(Key::NavMore, &[&hidden]),
            PathBuf::new(),
        ));
    }
    rows.extend(chain.into_iter().skip(hidden));
    rows.extend(fit_kids(kids, avail.saturating_sub(rows.len() - 2)));
    rows
}

/// The subfolders that fit in `room` rows, the last of them "… 3 more" if not all do.
fn fit_kids(mut kids: Vec<NavRow>, room: usize) -> Vec<NavRow> {
    if kids.len() <= room {
        return kids;
    }
    if room == 0 {
        return Vec::new();
    }
    let depth = kids[0].depth;
    let hidden = kids.len() - (room - 1);
    kids.truncate(room - 1);
    kids.push(NavRow {
        depth,
        ..plain_row(NavKind::More, tf(Key::NavMore, &[&hidden]), PathBuf::new())
    });
    kids
}

impl App {
    /// Whether a window `width` × `height` is drawn as panels.
    pub fn panels_wanted(&self, width: u16, height: u16) -> Option<crate::panels::Columns> {
        crate::panels::columns_for(
            self.layout_pref,
            self.layout,
            self.chooser.is_some(),
            width,
            height,
        )
    }

    /// Choose the layout (the View menu): it is remembered, and the window decides what is drawn
    /// when the choice is `auto`.
    pub fn set_layout_pref(&mut self, pref: crate::panels::LayoutPref) {
        if self.layout_pref == pref {
            return;
        }
        self.layout_pref = pref;
        if pref == crate::panels::LayoutPref::Classic {
            self.side_focus = false;
            self.preview_focus = false;
        }
        self.saved_ui.layout = Some(pref.id().to_string());
        self.save_ui_state();
        self.dirty = true;
    }

    /// The folder the NAV tree starts at.
    pub fn tree_root(&self) -> &Path {
        self.home()
    }

    /// Make the tree lead to the current folder and ask for what it still lacks. Called by the
    /// drawing: it only sends requests, the answers arrive as events.
    pub fn want_tree(&mut self) {
        if self.nav_mode == NavMode::Path {
            // Only the folder shown is asked about: its subfolders are what the box lists.
            let want = self.view_title().is_none().then(|| self.cwd.clone());
            if self.tree.open.iter().ne(want.iter()) {
                self.tree.open = want.into_iter().collect();
            }
            for dir in self.tree.wanted(self.show_hidden, self.dir_gen) {
                self.svc.tree_dirs(dir, self.show_hidden);
            }
            return;
        }
        let root = self.svc.home.clone();
        self.tree.follow(&root, &self.cwd);
        for dir in self.tree.wanted(self.show_hidden, self.dir_gen) {
            self.svc.tree_dirs(dir, self.show_hidden);
        }
    }

    /// Ask git about the folder shown if it was not asked yet, and not while another question
    /// is still out. Called by the drawing: it only sends a request, the answer arrives as an
    /// event. A virtual folder (a tag, the recent files, an archive) is not in a repository.
    pub fn want_git(&mut self) {
        if !self.svc.git.enabled() || self.archive.is_some() || self.view_title().is_some() {
            if self.git.take().is_some() {
                self.dirty = true;
            }
            return;
        }
        let key = (self.cwd.clone(), self.dir_gen);
        if self.git_asked.as_ref() == Some(&key) {
            return;
        }
        if self.git_busy.is_some_and(|t| t.elapsed() < GIT_WAIT) {
            return;
        }
        self.git_asked = Some(key);
        self.git_busy = Some(Instant::now());
        self.svc.git_status(&self.cwd);
    }

    /// What git said about a folder.
    pub(super) fn on_git(&mut self, dir: PathBuf, status: Option<rada_core::git::GitStatus>) {
        self.git_busy = None;
        if dir == self.cwd {
            self.git = status.map(|status| GitView { dir, status });
        }
        // The folder may have changed while git was thinking: the next frame asks again.
        self.dirty = true;
    }

    /// What git says about the folder shown, when it is the one it was asked about.
    pub fn git_here(&self) -> Option<&rada_core::git::GitStatus> {
        self.git
            .as_ref()
            .filter(|g| g.dir == self.cwd)
            .map(|g| &g.status)
    }

    /// The rows of the NAV box: the tree from Home, the favourites, the tags and the trash.
    pub fn nav_rows(&self) -> Vec<NavRow> {
        if self.nav_mode == NavMode::Path {
            return self.nav_rows_path();
        }
        let mut rows = Vec::new();
        let home = self.home().to_path_buf();
        let home_name = self
            .paths
            .places
            .iter()
            .find(|p| p.kind == PlaceKind::Home)
            .map(|p| p.name())
            .unwrap_or_else(|| Key::HomeLabel.tr().to_string());
        let open = self.tree.is_open(&home);
        rows.push(NavRow {
            kind: NavKind::Folder { open: Some(open) },
            depth: 0,
            path: home.clone(),
            name: home_name,
            count: None,
        });
        if open {
            self.tree_rows(&mut rows, &home, 1);
        }
        let items = sidebar::items(self, false);
        let favorites: Vec<usize> = items
            .iter()
            .enumerate()
            // The standard folders are in the tree under Home: only what was added is here.
            .filter(|(_, i)| {
                i.section == Section::Pinned && matches!(i.origin, Origin::Bookmark { .. })
            })
            .map(|(n, _)| n)
            .collect();
        let gap = || NavRow {
            kind: NavKind::Gap,
            depth: 0,
            path: PathBuf::new(),
            name: String::new(),
            count: None,
        };
        if !favorites.is_empty() {
            rows.push(gap());
            rows.push(NavRow {
                kind: NavKind::Heading,
                depth: 0,
                path: PathBuf::new(),
                name: Key::SecPinned.tr().to_string(),
                count: None,
            });
            for n in favorites {
                rows.push(NavRow {
                    kind: NavKind::Favorite(n),
                    depth: 0,
                    path: items[n].path.clone(),
                    name: items[n].name.clone(),
                    count: None,
                });
            }
        }
        if self.tags_supported() {
            rows.push(gap());
            rows.push(NavRow {
                kind: NavKind::Tags,
                depth: 0,
                path: PathBuf::new(),
                name: Key::SecTags.tr().to_string(),
                count: None,
            });
        }
        if let Some(t) = items.iter().find(|i| i.section == Section::Trash) {
            rows.push(gap());
            rows.push(NavRow {
                kind: NavKind::Trash,
                depth: 0,
                path: t.path.clone(),
                name: t.name.clone(),
                count: None,
            });
        }
        rows
    }

    /// The rows of the NAV box in `percorso` mode: the places as in the classic sidebar (Home,
    /// the standard folders, the favourites, the devices, the tags), then, when the folder shown
    /// is not itself one of them, under a title, the way to it: the nearest place that holds it,
    /// the folders down to it, the folder with its count and its subfolders. The trash is the
    /// last row, whatever is shown. The places come first and stay in sight: when the box is too
    /// short the way gives up rows, and if that is not enough the devices do.
    fn nav_rows_path(&self) -> Vec<NavRow> {
        let items = sidebar::items(self, false);
        let places = favorite_rows(&items, |s| matches!(s, Section::Home | Section::Pinned));
        let mut devices = favorite_rows(&items, |s| s == Section::Devices);
        let tags = self.tags_rows();
        let trash = items
            .iter()
            .find(|i| i.section == Section::Trash)
            .map(|t| plain_row(NavKind::Trash, t.name.clone(), t.path.clone()));
        let (chain, kids) = self.way_rows(&items);
        let height = self.nav_area.height as usize;
        let fixed =
            |disks: &[NavRow]| places.len() + block_len(disks) + tags.len() + trash.iter().len();
        let min_way = if chain.is_empty() { 0 } else { WAY_MIN };
        if height > 0 && fixed(&devices) + min_way > height {
            devices = main_disk_only(devices);
        }
        let room = match height {
            0 => usize::MAX,
            h => h.saturating_sub(fixed(&devices)),
        };
        let mut rows = places;
        if !devices.is_empty() {
            rows.push(gap_row());
            rows.push(heading_row(Key::SecDevices.tr()));
            rows.extend(devices);
        }
        rows.extend(tags);
        rows.extend(fit_way(chain, kids, room));
        rows.extend(trash);
        rows
    }

    /// The blank row and the row of the seven tag colours, or nothing where tags are not kept.
    fn tags_rows(&self) -> Vec<NavRow> {
        if !self.tags_supported() {
            return Vec::new();
        }
        let tags = plain_row(NavKind::Tags, Key::SecTags.tr().to_string(), PathBuf::new());
        vec![gap_row(), tags]
    }

    /// The folders of the way to the folder shown (see [`Self::nav_rows_path`]) and its
    /// subfolders; both empty when the folder shown is a place itself, or a tag, the recent
    /// files or the like, which are not in a tree of folders.
    fn way_rows(&self, items: &[sidebar::Item]) -> (Vec<NavRow>, Vec<NavRow>) {
        if self.view_title().is_some() {
            return Default::default();
        }
        // The places a folder can be in or be: not the recent files, a tag, a disk that is not
        // mounted.
        let places: Vec<usize> = (0..items.len())
            .filter(|n| {
                !matches!(
                    items[*n].origin,
                    Origin::Recent | Origin::Tag(_) | Origin::Unmounted(_)
                )
            })
            .collect();
        if places
            .iter()
            .any(|n| rada_core::places::same_folder(&items[*n].path, &self.cwd))
        {
            return Default::default();
        }
        let chain = self.chain_rows(items, &places);
        let kids = self.kid_rows(chain.last().map_or(0, |r| r.depth + 1));
        (chain, kids)
    }

    /// The nearest place that holds the folder shown and the folders down to it, as rows.
    fn chain_rows(&self, items: &[sidebar::Item], places: &[usize]) -> Vec<NavRow> {
        // The nearest place that holds the folder: the longest path.
        let place = places
            .iter()
            .copied()
            .filter(|n| self.cwd.starts_with(&items[*n].path))
            .max_by_key(|n| items[*n].path.components().count());
        let root = match place {
            Some(n) => items[n].path.clone(),
            None => self.path_root(),
        };
        let mut chain: Vec<&Path> = self
            .cwd
            .ancestors()
            .take_while(|a| a.starts_with(&root))
            .collect();
        chain.reverse();
        let last = chain.len().saturating_sub(1);
        let mut rows = Vec::new();
        for (depth, dir) in chain.iter().enumerate() {
            rows.push(match place {
                // The place itself, with its icon.
                Some(n) if depth == 0 => plain_row(
                    NavKind::Favorite(n),
                    items[n].name.clone(),
                    items[n].path.clone(),
                ),
                _ => self.way_row(dir, depth, depth == last),
            });
        }
        rows
    }

    /// The row of a folder of the way, `depth` levels down it; the folder shown (`here`) has
    /// its count of items.
    fn way_row(&self, dir: &Path, depth: usize, here: bool) -> NavRow {
        let on_disk = dir
            .file_name()
            .map(rada_core::display::name)
            .unwrap_or_else(|| rada_core::display::path(dir));
        NavRow {
            kind: NavKind::Folder { open: None },
            depth: depth.min(crate::panels::NAV_INDENT_MAX),
            path: dir.to_path_buf(),
            name: self.shown_name(dir, on_disk),
            count: here.then(|| self.listing.all().len()),
        }
    }

    /// The subfolders of the folder shown, as rows `depth` levels in.
    fn kid_rows(&self, depth: usize) -> Vec<NavRow> {
        let depth = depth.min(crate::panels::NAV_INDENT_MAX);
        let kids = self.tree.kids(&self.cwd).unwrap_or(&[]);
        kids.iter()
            .map(|k| NavRow {
                kind: NavKind::Folder { open: None },
                depth,
                path: k.path.clone(),
                name: self.shown_name(&k.path, k.name.clone()),
                count: Some(k.items),
            })
            .collect()
    }

    /// Where the way shown in NAV starts: Home, or the root of the disk the folder is on.
    fn path_root(&self) -> PathBuf {
        if self.cwd.starts_with(self.home()) {
            return self.home().to_path_buf();
        }
        self.volumes
            .iter()
            .map(|v| &v.mount_point)
            .filter(|m| self.cwd.starts_with(m))
            .max_by_key(|m| m.components().count())
            .cloned()
            .or_else(|| self.cwd.ancestors().last().map(Path::to_path_buf))
            .unwrap_or_else(|| self.cwd.clone())
    }

    fn tree_rows(&self, rows: &mut Vec<NavRow>, dir: &Path, depth: usize) {
        // A path that loops back (a link) must not make the tree endless.
        if depth > 24 {
            return;
        }
        let Some(kids) = self.tree.kids(dir) else {
            return;
        };
        for k in kids {
            let open = k.has_dirs.then(|| self.tree.is_open(&k.path));
            rows.push(NavRow {
                kind: NavKind::Folder { open },
                depth,
                path: k.path.clone(),
                name: self.shown_name(&k.path, k.name.clone()),
                count: Some(k.items),
            });
            if open == Some(true) {
                self.tree_rows(rows, &k.path, depth + 1);
            }
        }
    }

    /// The row of the NAV box that is the folder shown, for the highlight.
    pub fn nav_current(rows: &[NavRow], cwd: &Path) -> Option<usize> {
        // The deepest tree row wins over a favourite of the same folder.
        rows.iter()
            .position(|r| matches!(r.kind, NavKind::Folder { .. }) && r.path == cwd)
            .or_else(|| {
                rows.iter().position(|r| {
                    matches!(r.kind, NavKind::Favorite(_) | NavKind::Trash) && r.path == cwd
                })
            })
    }

    /// The command that shows or hides the details (Alt+P, F6, the button at the top right): in
    /// the view of two columns it puts ANTEPRIMA in the place of NAV, and NAV back. The
    /// keyboard follows the box it was in.
    pub(super) fn toggle_left_preview(&mut self) {
        if !self.side_on {
            self.toggle_sidebar();
            self.left_preview = true;
            self.dirty = true;
            return;
        }
        let had_focus = self.side_focus || self.preview_focus;
        self.left_preview = !self.left_preview;
        if had_focus {
            self.side_focus = !self.left_preview;
            self.preview_focus = self.left_preview;
        }
        self.dirty = true;
    }

    /// Tab in the panels view: the folder, NAV, ANTEPRIMA, the folder. With two columns the
    /// box on the left is the only one after the folder, whichever of the two it is.
    pub(super) fn switch_pane_panels(&mut self) {
        if self.panels_two {
            if self.side_focus || self.preview_focus {
                self.side_focus = false;
                self.preview_focus = false;
                return;
            }
            if !self.side_on {
                self.toggle_sidebar();
            }
            if self.left_preview {
                self.preview_focus = true;
            } else {
                let rows = self.nav_rows();
                self.nav_cursor = Self::nav_current(&rows, &self.cwd).unwrap_or(0);
                self.side_focus = true;
            }
            return;
        }
        if self.side_focus {
            self.side_focus = false;
            self.preview_focus = self.details_shown != crate::ui::DetailsMode::Hidden;
            return;
        }
        if self.preview_focus {
            self.preview_focus = false;
            return;
        }
        if !self.side_on {
            self.toggle_sidebar();
        }
        let rows = self.nav_rows();
        self.nav_cursor = Self::nav_current(&rows, &self.cwd).unwrap_or(0);
        self.side_focus = true;
    }

    /// The wheel over the NAV box: it scrolls the rows, three at a time, and not the cursor.
    pub(super) fn scroll_nav(&mut self, down: bool) {
        const STEP: usize = 3;
        self.nav_scroll = if down {
            self.nav_scroll + STEP
        } else {
            self.nav_scroll.saturating_sub(STEP)
        };
        self.dirty = true;
    }

    /// An arrow clicked or pressed on a folder of the tree: open or close it.
    pub fn toggle_tree(&mut self, dir: &Path) {
        self.tree.toggle(dir);
        self.dirty = true;
    }

    /// Left and right in NAV: they open and close the folders of the tree and move between
    /// the dots of the tags. `true`: dealt with.
    pub(super) fn on_nav_arrow(&mut self, right: bool) -> bool {
        let rows = self.nav_rows();
        let Some(row) = rows.get(self.nav_cursor).cloned() else {
            return false;
        };
        match row.kind {
            NavKind::Folder { open } => {
                match (right, open) {
                    (true, Some(false)) => self.tree.toggle(&row.path),
                    (true, Some(true)) => {
                        // Into the folder: its first child.
                        if rows
                            .get(self.nav_cursor + 1)
                            .is_some_and(|r| r.depth > row.depth)
                        {
                            self.nav_cursor += 1;
                        }
                    }
                    (false, Some(true)) => self.tree.toggle(&row.path),
                    (false, _) => {
                        // To the folder this one is in.
                        if let Some(up) = rows[..self.nav_cursor]
                            .iter()
                            .rposition(|r| r.depth < row.depth)
                        {
                            self.nav_cursor = up;
                        }
                    }
                    (true, None) => {}
                }
                true
            }
            NavKind::Tags => {
                let last = Hue::ALL.len() - 1;
                self.nav_tag = if right {
                    (self.nav_tag + 1).min(last)
                } else {
                    self.nav_tag.saturating_sub(1)
                };
                true
            }
            _ => false,
        }
    }

    /// A key while the NAV box has the focus (the classic pane's keys, on the rows of this
    /// one). `true`: dealt with here.
    pub(super) fn on_nav_action(&mut self, action: Action) -> bool {
        use Action::*;
        let rows = self.nav_rows();
        let last = rows.len().saturating_sub(1);
        self.nav_cursor = self.nav_cursor.min(last);
        let step = |from: usize, down: bool| -> usize {
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
        };
        let here = rows.get(self.nav_cursor).cloned();
        match action {
            Up => self.nav_cursor = step(self.nav_cursor, false),
            Down => self.nav_cursor = step(self.nav_cursor, true),
            First => self.nav_cursor = 0,
            Last => self.nav_cursor = last,
            Open => {
                if let Some(row) = here {
                    let path = match row.kind {
                        NavKind::Tags => {
                            let c = Hue::ALL[self.nav_tag.min(Hue::ALL.len() - 1)];
                            rada_core::tagsearch::view_path(c.label(&self.tag_lang))
                        }
                        NavKind::Heading | NavKind::Gap | NavKind::More => return true,
                        _ => row.path,
                    };
                    self.side_focus = false;
                    self.open_dir(path);
                }
            }
            Parent | ClearSelection | SwitchPane => {
                if action == SwitchPane {
                    self.switch_pane_panels();
                } else {
                    self.side_focus = false;
                }
            }
            Bookmark => {
                if let Some(row) = here
                    && matches!(row.kind, NavKind::Folder { .. } | NavKind::Favorite(_))
                {
                    let add = !self.paths.bookmarks.contains(&row.path);
                    self.set_bookmark(row.path, add);
                }
            }
            Trash | DeletePermanently => {
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
            Filter => {
                self.side_focus = false;
                return false;
            }
            a if super::side::PASS_THROUGH.contains(&a) => return false,
            _ => {
                let back = self.key_for(SwitchPane).unwrap_or_else(|| "Tab".into());
                self.toast(ToastKind::Info, tf(Key::TSidebarFocus, &[&back]), 3);
            }
        }
        true
    }

    /// A key while ANTEPRIMA has the focus: the arrows scroll the preview. `true`: dealt with.
    pub(super) fn on_preview_action(&mut self, action: Action) -> bool {
        use Action::*;
        match action {
            Up => self.preview.scroll = self.preview.scroll.saturating_sub(1),
            Down => self.preview.scroll = self.preview.scroll.saturating_add(1),
            First => self.preview.scroll = 0,
            PageUp | PreviewUp => self.preview.scroll = self.preview.scroll.saturating_sub(10),
            PageDown | PreviewDown => self.preview.scroll = self.preview.scroll.saturating_add(10),
            // Tab goes on round: back to the folder.
            Parent | ClearSelection | SwitchPane => self.preview_focus = false,
            Filter => {
                self.preview_focus = false;
                return false;
            }
            _ => return false,
        }
        true
    }
}
