//! The explorer's own commands: refreshing, the view, the details pane, copying the path, opening a terminal.

use std::path::Path;

use rada_core::events::CoreEvent;
use rada_core::i18n::Key;

use super::{App, LoadState, Modal, ToastKind};
use crate::hits::Target;
use crate::view::ViewMode;

impl App {
    /// Read the folder again.
    pub fn refresh(&mut self) {
        let generation = self.tabs.next_generation();
        self.tab.load = LoadState::Loading(self.tab.cwd.clone());
        self.svc.loader.load(self.tab.cwd.clone(), generation);
        self.redraw.dirty = true;
    }

    pub fn set_view(&mut self, mode: ViewMode) {
        if self.tab.view == mode {
            return;
        }
        self.tab.view = mode;
        self.tab.ensure_visible();
        self.save_ui_state();
        self.redraw.dirty = true;
    }

    /// Show or hide the Tag column of the list, and remember it.
    pub fn toggle_tags_column(&mut self) {
        self.settings.show_tags = !self.settings.show_tags;
        self.tabs.saved_ui.tags_column = Some(self.settings.show_tags);
        self.save_ui_state();
        self.redraw.dirty = true;
    }

    /// Whether the last frame showed the details pane.
    pub fn details_visible(&self) -> bool {
        self.frame.details_shown != crate::ui::DetailsMode::Hidden
    }

    pub fn toggle_details(&mut self) {
        if self.frame.panels_shown && self.frame.panels_two {
            return self.toggle_left_preview();
        }
        if self.session.chooser.is_some() {
            return self.toggle_chooser_preview();
        }
        let want = !self.details_visible();
        self.settings.details = Some(want);
        self.tabs.saved_ui.details = Some(want);
        self.save_ui_state();
        if want && self.frame.term_width() < 60 {
            self.toast(ToastKind::Info, Key::TDetailsOnNarrow.tr(), 3);
        }
    }

    /// A file chooser shows its preview beside the list from 70 columns; below that, the key
    /// puts it in the place of the list and takes it away again. None of it is remembered.
    fn toggle_chooser_preview(&mut self) {
        if self.frame.term_width() < crate::ui::CHOOSER_PREVIEW_FROM {
            self.settings.details = Some(self.settings.details != Some(true));
        }
    }

    /// The renderer tells how many tiles fit across and down in the Icons view.
    pub fn set_grid(&mut self, cols: usize, rows: usize) {
        self.tab.viewport.grid_cols = cols;
        self.tab.viewport.grid_rows = rows;
        if self.tab.view == ViewMode::Icons {
            self.tab.ensure_visible();
        }
    }

    pub(super) fn start_open_with(&mut self) {
        let Some((path, is_dir)) = self.tab.current().map(|e| (e.path.clone(), e.is_dir())) else {
            return;
        };
        if is_dir {
            self.toast(ToastKind::Info, Key::TOpenWithFiles.tr(), 3);
            return;
        }
        if self.refuse_in_archive(Key::WhatOpenWithProgram.tr()) {
            return;
        }
        self.open_with_window(path, None);
    }

    /// Put the path of the selection (or of the item under the cursor) on the system clipboard.
    pub(super) fn copy_path(&mut self) {
        let paths = self.tab.targets();
        if paths.is_empty() {
            return;
        }
        let text = paths
            .iter()
            .map(|p| rada_core::display::path(p))
            .collect::<Vec<_>>()
            .join("\n");
        self.ops.copied_text = Some(text);
        self.toast(
            ToastKind::Ok,
            if paths.len() == 1 {
                Key::TPathCopied.tr()
            } else {
                Key::TPathsCopied.tr()
            },
            2,
        );
    }

    pub(super) fn open_terminal_here(&mut self) {
        let dir = match &self.tab.archive {
            Some(a) => a.archive.parent().map(Path::to_path_buf),
            None if self.tab.tag_view().is_some() => {
                // Where the item under the cursor is.
                Some(
                    self.folder_for_new(
                        &self
                            .tab
                            .current()
                            .map(|e| e.path.clone())
                            .into_iter()
                            .collect::<Vec<_>>(),
                    ),
                )
            }
            None => Some(self.tab.cwd.clone()),
        };
        let Some(dir) = dir else { return };
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        self.redraw.after_launch();
        std::thread::spawn(move || {
            if let Err(e) = platform.opener().open_terminal(&dir) {
                let _ = notes.send(CoreEvent::Note {
                    text: e.to_string(),
                    error: true,
                });
            }
        });
    }

    /// Shift+F10 or the Menu key: the context menu, at the item under the cursor.
    pub(super) fn open_context_menu_at_cursor(&mut self) {
        let at = self
            .frame
            .hits
            .find(&Target::Row(self.tab.sel.cursor))
            .map(|r| (r.x.saturating_add(6), r.y.saturating_add(1)))
            .unwrap_or((8, 6));
        let on_item = self.tab.current().is_some();
        self.windows.modal = Some(Modal::Menu(self.context_menu(on_item, at)));
    }

    /// An outside request to stop (SIGTERM, closing the terminal): cancel and leave.
    pub fn shutdown(&mut self) {
        self.ops.cancel();
        self.session.should_quit = true;
    }
}
