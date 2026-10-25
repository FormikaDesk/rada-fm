//! The palette of places: what it lists for the folder and the disks, and its keys.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::i18n::{Key, tf};

use super::{App, Modal};
use crate::fmt;
use crate::palette::{PaletteItem, PaletteKind, PaletteView};

/// The list of the palette while it is made: every path once, a disk instead of its mount point.
struct Items {
    items: Vec<PaletteItem>,
    seen: HashSet<PathBuf>,
    mounts: HashSet<PathBuf>,
    home: PathBuf,
}

impl Items {
    fn add(
        &mut self,
        path: &Path,
        kind: PaletteKind,
        label: Option<String>,
        detail: Option<String>,
    ) {
        if kind != PaletteKind::Disk && self.mounts.contains(path) {
            return;
        }
        if !self.seen.insert(path.to_path_buf()) {
            return;
        }
        let home = &self.home;
        let label = label.unwrap_or_else(|| {
            if path == home {
                Key::HomeLabel.tr().to_string()
            } else {
                path.file_name()
                    .map(rada_core::display::name)
                    .unwrap_or_else(|| rada_core::display::path(path))
            }
        });
        let detail = detail.unwrap_or_else(|| fmt::short_path(path, home));
        self.items.push(PaletteItem {
            label,
            detail,
            path: path.to_path_buf(),
            kind,
        });
    }

    fn plain(&mut self, path: &Path, kind: PaletteKind) {
        self.add(path, kind, None, None);
    }
}

impl App {
    pub(super) fn palette_items(&self) -> Vec<PaletteItem> {
        // A mount point is listed once, as a disk (so "/" is not both "System" and a disk).
        let mut list = Items {
            items: Vec::new(),
            seen: HashSet::new(),
            mounts: self
                .devices
                .volumes
                .iter()
                .map(|v| v.mount_point.clone())
                .collect(),
            home: self.svc.home.clone(),
        };
        self.add_places(&mut list);
        self.add_disks(&mut list);
        self.add_nearby(&mut list);
        list.items
    }

    fn add_places(&self, list: &mut Items) {
        for p in &self.places.bookmarks() {
            list.plain(p, PaletteKind::Bookmark);
        }
        for p in self
            .places
            .paths
            .recents
            .iter()
            .filter(|p| **p != self.tab.cwd)
        {
            list.plain(p, PaletteKind::Recent);
        }
        for place in &self.places.paths.places {
            list.plain(&place.path, PaletteKind::Place);
        }
        list.add(
            Path::new(std::path::MAIN_SEPARATOR_STR),
            PaletteKind::Place,
            Some(Key::DiskSystem.tr().into()),
            Some(std::path::MAIN_SEPARATOR_STR.into()),
        );
    }

    fn add_disks(&self, list: &mut Items) {
        for v in &self.devices.volumes {
            let label = v.label.clone().unwrap_or_else(|| {
                if v.mount_point == Path::new(std::path::MAIN_SEPARATOR_STR) {
                    Key::DiskSystem.tr().to_string()
                } else {
                    rada_core::display::path(&v.mount_point)
                }
            });
            let free = v
                .available
                .map(|a| tf(Key::PalFreeDetail, &[&fmt::size(a)]))
                .unwrap_or_else(|| {
                    if v.responsive {
                        String::new()
                    } else {
                        Key::PalNotResponding.tr().into()
                    }
                });
            let detail = format!("{}{free}", rada_core::display::path(&v.mount_point));
            list.add(&v.mount_point, PaletteKind::Disk, Some(label), Some(detail));
        }
    }

    /// The folders above this one and the ones inside it.
    fn add_nearby(&self, list: &mut Items) {
        if self.tab.tag_view().is_none() {
            for anc in self.tab.cwd.ancestors().skip(1).take(4) {
                list.plain(anc, PaletteKind::Parent);
            }
        }
        let dirs: Vec<PathBuf> = self
            .tab
            .listing
            .all()
            .iter()
            .filter(|e| e.is_dir())
            .take(400)
            .map(|e| e.path.clone())
            .collect();
        for p in &dirs {
            list.plain(p, PaletteKind::Folder);
        }
    }

    pub(super) fn open_palette(&mut self) {
        let items = self.palette_items();
        self.windows.modal = Some(Modal::Palette(Box::new(PaletteView::new(
            items,
            self.svc.home.clone(),
        ))));
    }

    /// New recents/volumes arrived while the palette is open: refresh its list, keep the query.
    pub(super) fn refresh_palette(&mut self) {
        if matches!(self.windows.modal, Some(Modal::Palette(_))) {
            let items = self.palette_items();
            if let Some(Modal::Palette(p)) = &mut self.windows.modal {
                p.set_items(items);
            }
        }
    }

    pub(super) fn on_palette_key(&mut self, mut p: Box<PaletteView>, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter => {
                if let Some(item) = p.current() {
                    let path = item.path.clone();
                    self.open_dir(path);
                }
            }
            code => {
                press(&mut p, code, ctrl);
                self.windows.modal = Some(Modal::Palette(p));
            }
        }
    }
}

/// What a key does to the palette: move the choice, or edit the query.
fn press(p: &mut PaletteView, code: KeyCode, ctrl: bool) {
    match code {
        KeyCode::Down => p.move_by(1),
        KeyCode::Up => p.move_by(-1),
        KeyCode::Char('n') | KeyCode::Char('j') if ctrl => p.move_by(1),
        KeyCode::Char('p') | KeyCode::Char('k') if ctrl => p.move_by(-1),
        KeyCode::PageDown => p.move_by(8),
        KeyCode::PageUp => p.move_by(-8),
        KeyCode::Char('u') if ctrl => p.clear(),
        KeyCode::Backspace => p.backspace(),
        KeyCode::Char(c) if !ctrl => p.insert(c),
        _ => {}
    }
}
