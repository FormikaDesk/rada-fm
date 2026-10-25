//! The palette of places: what it lists for the folder and the disks, and its keys.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::i18n::{Key, tf};

use super::{App, Modal};
use crate::fmt;
use crate::palette::{PaletteItem, PaletteKind, PaletteView};

impl App {
    pub(super) fn palette_items(&self) -> Vec<PaletteItem> {
        let mut items: Vec<PaletteItem> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let home = self.svc.home.clone();
        let cwd = self.tab.cwd.clone();
        // A mount point is listed once, as a disk (so "/" is not both "System" and a disk).
        let mounts: std::collections::HashSet<PathBuf> = self
            .devices
            .volumes
            .iter()
            .map(|v| v.mount_point.clone())
            .collect();
        let mut add = |items: &mut Vec<PaletteItem>,
                       path: &Path,
                       kind: PaletteKind,
                       label: Option<String>,
                       detail: Option<String>| {
            if kind != PaletteKind::Disk && mounts.contains(path) {
                return;
            }
            if !seen.insert(path.to_path_buf()) {
                return;
            }
            let label = label.unwrap_or_else(|| {
                if path == home {
                    Key::HomeLabel.tr().to_string()
                } else {
                    path.file_name()
                        .map(rada_core::display::name)
                        .unwrap_or_else(|| rada_core::display::path(path))
                }
            });
            let detail = detail.unwrap_or_else(|| fmt::short_path(path, &home));
            items.push(PaletteItem {
                label,
                detail,
                path: path.to_path_buf(),
                kind,
            });
        };
        for p in &self.places.bookmarks() {
            add(&mut items, p, PaletteKind::Bookmark, None, None);
        }
        for p in self.places.paths.recents.iter().filter(|p| **p != cwd) {
            add(&mut items, p, PaletteKind::Recent, None, None);
        }
        for place in &self.places.paths.places {
            add(&mut items, &place.path, PaletteKind::Place, None, None);
        }
        add(
            &mut items,
            Path::new(std::path::MAIN_SEPARATOR_STR),
            PaletteKind::Place,
            Some(Key::DiskSystem.tr().into()),
            Some(std::path::MAIN_SEPARATOR_STR.into()),
        );
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
            add(
                &mut items,
                &v.mount_point,
                PaletteKind::Disk,
                Some(label),
                Some(detail),
            );
        }
        if self.tab.tag_view().is_none() {
            for anc in cwd.ancestors().skip(1).take(4) {
                add(&mut items, anc, PaletteKind::Parent, None, None);
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
            add(&mut items, p, PaletteKind::Folder, None, None);
        }
        items
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
            KeyCode::Down => {
                p.move_by(1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Up => {
                p.move_by(-1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char('n') | KeyCode::Char('j') if ctrl => {
                p.move_by(1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char('p') | KeyCode::Char('k') if ctrl => {
                p.move_by(-1);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::PageDown => {
                p.move_by(8);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::PageUp => {
                p.move_by(-8);
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char('u') if ctrl => {
                p.clear();
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Backspace => {
                p.backspace();
                self.windows.modal = Some(Modal::Palette(p));
            }
            KeyCode::Char(c) if !ctrl => {
                p.insert(c);
                self.windows.modal = Some(Modal::Palette(p));
            }
            _ => self.windows.modal = Some(Modal::Palette(p)),
        }
    }
}
