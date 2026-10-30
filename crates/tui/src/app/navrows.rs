//! The rows of the NAV box: Home and its tree, the pinned folders, the tags and the trash, or,
//! in `percorso` mode, the places and the way to the folder shown.

use std::path::{Path, PathBuf};

use rada_core::i18n::{Key, tf};
use rada_core::platform::PlaceKind;

use super::App;
use crate::panels::{NavKind, NavMode, NavRow, TAG_LINES_MOST, tag_lines};
use crate::sidebar::{self, Origin, Section};

/// The rows the way to the folder needs to have a blank row above its title: the blank row, the
/// title, "… more" and the folder shown. With a row less the blank row goes.
const WAY_MIN: usize = 4;
/// The least it is cut down to.
const WAY_LEAST: usize = WAY_MIN - 1;

fn plain_row(kind: NavKind, name: String, path: PathBuf) -> NavRow {
    NavRow {
        kind,
        depth: 0,
        path,
        name,
        count: None,
        unreadable: None,
    }
}

fn gap_row() -> NavRow {
    plain_row(NavKind::Gap, String::new(), PathBuf::new())
}

fn heading_row(name: &str) -> NavRow {
    plain_row(NavKind::Heading, name.to_string(), PathBuf::new())
}

/// The folders the person pinned, under a title and a blank row; none when there are none. The
/// standard folders are in the tree under Home: only what was added is here.
fn pinned_rows(items: &[sidebar::Item]) -> Vec<NavRow> {
    let favorites: Vec<usize> = (0..items.len())
        .filter(|&n| {
            items[n].section == Section::Pinned
                && matches!(items[n].origin, Origin::Bookmark { .. })
        })
        .collect();
    if favorites.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![
        gap_row(),
        plain_row(
            NavKind::Heading,
            Key::SecPinned.tr().to_string(),
            PathBuf::new(),
        ),
    ];
    for n in favorites {
        let kind = NavKind::Favorite(n);
        rows.push(plain_row(
            kind,
            items[n].name.clone(),
            items[n].path.clone(),
        ));
    }
    rows
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
    let mut rows = vec![heading_row(Key::PanelPath.tr())];
    if room >= WAY_MIN {
        rows.insert(0, gap_row());
    }
    let head = rows.len();
    let avail = room.max(WAY_LEAST) - head;
    let hidden = if chain.len() > avail {
        chain.len() + 1 - avail
    } else {
        0
    };
    if hidden > 0 {
        rows.push(plain_row(
            NavKind::More,
            tf(Key::NavMore, &[&hidden]),
            PathBuf::new(),
        ));
    }
    rows.extend(chain.into_iter().skip(hidden));
    rows.extend(fit_kids(kids, avail.saturating_sub(rows.len() - head)));
    rows
}

/// Makes the blocks below the places fit in `height` rows beside the `rest` (the places, the
/// trash and the least of the way): first the blank row above TAGS goes, then the disks but
/// the main one.
fn squeeze(
    devices: Vec<NavRow>,
    tags: &mut Vec<NavRow>,
    rest: usize,
    height: usize,
) -> Vec<NavRow> {
    let needed = |disks: &[NavRow], tags: &[NavRow]| rest + block_len(disks) + tags.len();
    if needed(&devices, tags) > height {
        tags.retain(|r| r.kind != NavKind::Gap);
    }
    if needed(&devices, tags) > height {
        return main_disk_only(devices);
    }
    devices
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
    /// The rows of the NAV box: the tree from Home, the favourites, the tags and the trash.
    pub fn nav_rows(&self) -> Vec<NavRow> {
        if self.settings.nav_mode == NavMode::Path {
            return self.nav_rows_path();
        }
        let mut rows = self.home_rows();
        let items = sidebar::items(self, false);
        rows.extend(pinned_rows(&items));
        rows.extend(self.tags_rows(TAG_LINES_MOST));
        if let Some(t) = items.iter().find(|i| i.section == Section::Trash) {
            rows.push(gap_row());
            rows.push(plain_row(NavKind::Trash, t.name.clone(), t.path.clone()));
        }
        rows
    }

    /// Home, and the tree under it when it is open.
    fn home_rows(&self) -> Vec<NavRow> {
        let mut rows = Vec::new();
        let home = self.home().to_path_buf();
        let home_name = self
            .places
            .paths
            .places
            .iter()
            .find(|p| p.kind == PlaceKind::Home)
            .map(|p| p.name())
            .unwrap_or_else(|| Key::HomeLabel.tr().to_string());
        let open = self.side_nav.tree.is_open(&home);
        rows.push(NavRow {
            kind: NavKind::Folder { open: Some(open) },
            depth: 0,
            path: home.clone(),
            name: home_name,
            count: None,
            unreadable: None,
        });
        if open {
            self.tree_rows(&mut rows, &home, 1);
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
        let mut tags = self.tags_rows(1);
        let trash = items
            .iter()
            .find(|i| i.section == Section::Trash)
            .map(|t| plain_row(NavKind::Trash, t.name.clone(), t.path.clone()));
        let (chain, kids) = self.way_rows(&items);
        let height = self.frame.nav_area.height as usize;
        let rest = places.len() + trash.iter().len();
        let min_way = if chain.is_empty() { 0 } else { WAY_LEAST };
        if height > 0 {
            // The lines of the user's own tags come after everything else: they take what is
            // left over the places, the disks, the least of the way and the first line of dots.
            let spare = height.saturating_sub(rest + min_way + block_len(&devices) + tags.len());
            // (Where tags are not kept there is no block, and nothing to give lines to.)
            if spare > 0 && !tags.is_empty() {
                tags = self.tags_rows(tags.len() - 1 + spare.min(TAG_LINES_MOST - 1));
            }
            devices = squeeze(devices, &mut tags, rest + min_way, height);
        }
        let room = match height {
            0 => usize::MAX,
            h => h.saturating_sub(rest + block_len(&devices) + tags.len()),
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

    /// The blank row and the lines of tag dots, or nothing where tags are not kept: the seven
    /// colours, then the user's own tags, in `lines` lines if they fit (the colours are
    /// always there, and the box's width says how many dots a line has).
    fn tags_rows(&self, lines: usize) -> Vec<NavRow> {
        if !self.tags_supported() {
            return Vec::new();
        }
        let total = sidebar::tag_items(self).len();
        let (layout, more) = tag_lines(total, self.frame.nav_area.width as usize, lines);
        let last = layout.len() - 1;
        let lines = layout.into_iter().enumerate().map(|(i, (first, len))| {
            let title = if i == 0 { Key::SecTags.tr() } else { "" };
            let more = if i == last { more } else { 0 };
            plain_row(
                NavKind::Tags { first, len, more },
                title.to_string(),
                PathBuf::new(),
            )
        });
        std::iter::once(gap_row()).chain(lines).collect()
    }

    /// The folders of the way to the folder shown (see [`Self::nav_rows_path`]) and its
    /// subfolders; both empty when the folder shown is a place itself, or a tag, the recent
    /// files or the like, which are not in a tree of folders.
    fn way_rows(&self, items: &[sidebar::Item]) -> (Vec<NavRow>, Vec<NavRow>) {
        if self.tab.view_title().is_some() {
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
            .any(|n| rada_core::places::same_folder(&items[*n].path, &self.tab.cwd))
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
            .filter(|n| self.tab.cwd.starts_with(&items[*n].path))
            .max_by_key(|n| items[*n].path.components().count());
        let root = match place {
            Some(n) => items[n].path.clone(),
            None => self.path_root(),
        };
        let mut chain: Vec<&Path> = self
            .tab
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
            name: self.places.shown_name(dir, on_disk),
            count: here.then(|| self.tab.listing.all().len()),
            unreadable: None,
        }
    }

    /// The subfolders of the folder shown, as rows `depth` levels in.
    fn kid_rows(&self, depth: usize) -> Vec<NavRow> {
        let depth = depth.min(crate::panels::NAV_INDENT_MAX);
        let kids = self.side_nav.tree.kids(&self.tab.cwd).unwrap_or(&[]);
        kids.iter()
            .map(|k| NavRow {
                kind: NavKind::Folder { open: None },
                depth,
                path: k.path.clone(),
                name: self.places.shown_name(&k.path, k.name.clone()),
                count: Some(k.items),
                unreadable: k.unreadable.clone(),
            })
            .collect()
    }

    /// Where the way shown in NAV starts: Home, or the root of the disk the folder is on.
    fn path_root(&self) -> PathBuf {
        if self.tab.cwd.starts_with(self.home()) {
            return self.home().to_path_buf();
        }
        self.devices
            .volumes
            .iter()
            .map(|v| &v.mount_point)
            .filter(|m| self.tab.cwd.starts_with(m))
            .max_by_key(|m| m.components().count())
            .cloned()
            .or_else(|| self.tab.cwd.ancestors().last().map(Path::to_path_buf))
            .unwrap_or_else(|| self.tab.cwd.clone())
    }

    fn tree_rows(&self, rows: &mut Vec<NavRow>, dir: &Path, depth: usize) {
        // A path that loops back (a link) must not make the tree endless.
        if depth > 24 {
            return;
        }
        let Some(kids) = self.side_nav.tree.kids(dir) else {
            return;
        };
        for k in kids {
            let open = k.has_dirs.then(|| self.side_nav.tree.is_open(&k.path));
            rows.push(NavRow {
                kind: NavKind::Folder { open },
                depth,
                path: k.path.clone(),
                name: self.places.shown_name(&k.path, k.name.clone()),
                count: Some(k.items),
                unreadable: k.unreadable.clone(),
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
}
