//! The Properties window: what is known about the item under the cursor, or about the
//! selection, taken from the listing already in memory (nothing is read from the disk).

use rada_core::i18n::{Key, tf};

use super::{App, Modal};
use crate::fmt;
use rada_core::model::Entry;

pub struct PropsView {
    pub title: String,
    /// Label and value, in order.
    pub rows: Vec<(String, String)>,
    pub scroll: usize,
}

impl App {
    pub fn open_properties(&mut self) {
        let view = if self.tab.sel.marked.len() > 1 {
            self.selection_properties()
        } else {
            let Some(e) = self.tab.current() else { return };
            self.item_properties(e)
        };
        self.windows.modal = Some(Modal::Properties(view));
    }

    fn selection_properties(&self) -> PropsView {
        let chosen: Vec<&Entry> = self
            .tab
            .listing
            .all()
            .iter()
            .filter(|e| self.tab.sel.marked.contains(&e.name))
            .collect();
        let size: u64 = chosen.iter().filter(|e| !e.is_dir()).map(|e| e.size).sum();
        let folders = chosen.iter().filter(|e| e.is_dir()).count();
        let mut rows = vec![
            (
                Key::PropItems.tr().to_string(),
                tf(
                    Key::PropItemsValue,
                    &[
                        &chosen.len(),
                        &(chosen.len() - folders),
                        &fmt::count(folders as u64, Key::FolderOne, Key::FolderMany),
                    ],
                ),
            ),
            (
                Key::PropSize.tr().to_string(),
                tf(
                    Key::PropSizeNoFolders,
                    &[&fmt::size(size), &fmt::thousands(size)],
                ),
            ),
            (
                Key::PropLocation.tr().to_string(),
                match self.tab.view_title() {
                    Some(title) => title,
                    None => fmt::short_path(&self.tab.cwd, self.home()),
                },
            ),
        ];
        if self.tab.archive.is_some() {
            rows.push((
                Key::PropInside.tr().to_string(),
                Key::PropInArchive.tr().to_string(),
            ));
        }
        PropsView {
            title: if chosen.len() == 1 {
                Key::PropsSelectedOne.tr().to_string()
            } else {
                tf(Key::PropsSelectedMany, &[&chosen.len()])
            },
            rows,
            scroll: 0,
        }
    }

    fn item_properties(&self, e: &Entry) -> PropsView {
        let when = |t| self.settings.date_with_day(t, self.now());
        let mut rows: Vec<(String, String)> = vec![
            (
                Key::PropName.tr().into(),
                e.path
                    .file_name()
                    .map(rada_core::display::name)
                    .unwrap_or_else(|| rada_core::display::name(&e.name)),
            ),
            (
                Key::PropType.tr().into(),
                rada_core::i18n::type_label(&e.type_label).into_owned(),
            ),
            (
                Key::PropLocation.tr().into(),
                fmt::short_path(e.path.parent().unwrap_or(&self.tab.cwd), self.home()),
            ),
        ];
        if let Some(l) = &e.link {
            rows.push((
                Key::PropLinkTo.tr().into(),
                rada_core::display::path(&l.target),
            ));
        }
        if !e.is_dir() {
            rows.push((
                Key::PropSize.tr().into(),
                tf(
                    Key::PropSizeBytes,
                    &[&fmt::size(e.size), &fmt::thousands(e.size)],
                ),
            ));
        }
        rows.push((Key::PropModified.tr().into(), when(e.mtime)));
        if e.created.is_some() {
            rows.push((Key::PropCreated.tr().into(), when(e.created)));
        }
        if let Some(m) = e.mode {
            rows.push((
                Key::PropPermissions.tr().into(),
                format!("{}  {:04o}", rada_core::preview::mode_string(m), m & 0o7777),
            ));
        }
        let mut flags: Vec<&str> = Vec::new();
        if e.hidden {
            flags.push(Key::FlagHidden.tr());
        }
        if e.system {
            flags.push(Key::FlagSystem.tr());
        }
        if e.readonly {
            flags.push(Key::FlagReadOnly.tr());
        }
        if e.executable {
            flags.push(Key::FlagExecutable.tr());
        }
        if !flags.is_empty() {
            rows.push((Key::PropAttributes.tr().into(), flags.join(", ")));
        }
        if let Some(err) = &e.error {
            rows.push((Key::PropProblem.tr().into(), err.clone()));
        }
        if self.tab.archive.is_some() {
            rows.push((Key::PropInside.tr().into(), Key::PropInArchive.tr().into()));
        }
        PropsView {
            title: e.display.clone(),
            rows,
            scroll: 0,
        }
    }
}
