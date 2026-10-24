//! The sidebar's behaviour: showing and hiding it, moving the keyboard focus into it,
//! what its keys do, and its menu.

use super::{App, MenuCmd, MenuItem, MenuView, Modal, ToastKind};
use crate::fmt;
use crate::keymap::Action;
use crate::sidebar::{self, Item, Mode, Origin};
use crossterm::event::{KeyCode, KeyEvent};
use rada_core::events::CoreEvent;
use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};

/// Actions that mean the same wherever the keyboard focus is; everything else is about
/// the list, and the sidebar does not pretend to do it.
pub(super) const PASS_THROUGH: &[Action] = &[
    Action::Help,
    Action::Palette,
    Action::Quit,
    Action::ToggleSidebar,
    Action::Back,
    Action::Forward,
    Action::GoHome,
    Action::ToggleHidden,
    Action::Sort,
    Action::SortReverse,
];

impl App {
    /// A folder could not be opened because the system is waiting for the person to allow it:
    /// say so with the way to do it, and remember it for the sidebar.
    pub(super) fn on_access_denied(&mut self, d: rada_core::platform::AccessDenied) {
        self.devices.deny(d.clone());
        if self.modal.is_none() {
            self.modal = Some(Modal::Access(d));
        } else {
            self.toast(ToastKind::Error, d.message(), 6);
        }
    }

    /// Keys of the window about a refused folder: open the page of the system settings, or
    /// close.
    pub(super) fn on_access_key(&mut self, d: rada_core::platform::AccessDenied, key: KeyEvent) {
        let url = match key.code {
            KeyCode::Enter | KeyCode::Char('o') => Some(d.settings_url),
            KeyCode::Char('f') => Some(d.full_disk_url),
            KeyCode::Esc | KeyCode::Char('q') => return,
            _ => None,
        };
        if let Some(url) = url {
            let platform = self.svc.platform.clone();
            let notes = self.svc.notes.clone();
            self.after_launch();
            std::thread::spawn(move || {
                if let Err(e) = platform.opener().open_url(url) {
                    let _ = notes.send(CoreEvent::Note {
                        text: tf(Key::TCannotOpenSettings, &[&e]),
                        error: true,
                    });
                }
            });
            self.toast(ToastKind::Info, Key::TSettingsOpened.tr(), 6);
            return;
        }
        self.modal = Some(Modal::Access(d));
    }

    pub fn config_bookmarks(&self) -> &[PathBuf] {
        &self.cfg_bookmarks
    }

    /// The items the sidebar shows right now.
    pub fn side_items(&self) -> Vec<Item> {
        sidebar::items(self, self.side_mode == Mode::Rail)
    }

    /// What the footer says about the item under the keyboard focus or the mouse, in the
    /// icon-only mode where the names are not on screen.
    pub fn side_description(&self) -> Option<String> {
        if self.side_mode != Mode::Rail {
            return None;
        }
        let items = self.side_items();
        let item = if self.side_focus {
            items.get(self.side_cursor)
        } else {
            let h = self.hover.as_ref()?;
            items.iter().find(|i| &i.path == h)
        }?;
        if item.locked
            && let Some(d) = self.devices.denied_for(&item.path)
        {
            return Some(d.message());
        }
        Some(match &item.usage {
            Some(u) => tf(Key::SideFree, &[&item.name, &fmt::size(u.free)]),
            None => item.name.clone(),
        })
    }

    pub(super) fn toggle_sidebar(&mut self) {
        self.side_on = !self.side_on;
        if !self.side_on {
            self.side_focus = false;
        }
        self.saved_ui.sidebar = Some(self.side_on);
        self.save_ui_state();
        if self.side_on && Mode::for_width(self.term_width, true) == Mode::Hidden {
            self.toast(ToastKind::Info, Key::TSidebarOnNarrow.tr(), 3);
        }
    }

    /// Tab: the keyboard goes to the sidebar, or back to the list.
    pub(super) fn switch_pane(&mut self) {
        if self.panels_shown {
            return self.switch_pane_panels();
        }
        if self.side_focus {
            self.side_focus = false;
            return;
        }
        if !self.side_on {
            self.toggle_sidebar();
        }
        if Mode::for_width(self.term_width, self.side_on) == Mode::Hidden {
            self.toast(ToastKind::Info, Key::TSidebarNarrow.tr(), 3);
            return;
        }
        // Make the next frame's mode current before looking at the items.
        self.side_mode = Mode::for_width(self.term_width, self.side_on);
        let items = self.side_items();
        if items.is_empty() {
            return;
        }
        self.side_cursor = sidebar::current_index(&items, &self.cwd).unwrap_or(0);
        self.side_focus = true;
    }

    /// A key while the sidebar has the focus. `true`: dealt with here.
    pub(super) fn on_side_action(&mut self, action: Action) -> bool {
        use Action::*;
        if self.panels_shown {
            return self.on_nav_action(action);
        }
        let items = self.side_items();
        let last = items.len().saturating_sub(1);
        self.side_cursor = self.side_cursor.min(last);
        match action {
            Up => self.side_cursor = self.side_cursor.saturating_sub(1),
            Down => self.side_cursor = (self.side_cursor + 1).min(last),
            First => self.side_cursor = 0,
            Last => self.side_cursor = last,
            Open => {
                if let Some(item) = items.get(self.side_cursor) {
                    self.side_focus = false;
                    self.open_dir(item.path.clone());
                }
            }
            Parent | ClearSelection | SwitchPane => self.side_focus = false,
            Bookmark => {
                if let Some(item) = items.get(self.side_cursor)
                    && !matches!(item.origin, Origin::Unmounted(_))
                {
                    let add = !self.paths.bookmarks.contains(&item.path);
                    self.set_bookmark(item.path.clone(), add);
                }
            }
            Trash | DeletePermanently => match items.get(self.side_cursor).map(|i| &i.origin) {
                Some(Origin::Device(_)) => {
                    let mount = items[self.side_cursor].path.clone();
                    self.start_eject(&mount);
                }
                Some(Origin::Unmounted(_)) => {
                    self.toast(ToastKind::Info, Key::TCannotEject.tr(), 3)
                }
                Some(Origin::Bookmark { removable: true }) => {
                    let path = items[self.side_cursor].path.clone();
                    self.set_bookmark(path, false);
                }
                _ => self.toast(ToastKind::Info, Key::TOnlyBookmarksRemovable.tr(), 3),
            },
            Filter => {
                // Typing a filter is about the list.
                self.side_focus = false;
                return false;
            }
            a if PASS_THROUGH.contains(&a) => return false,
            _ => {
                let back = self.key_for(SwitchPane).unwrap_or_else(|| "Tab".into());
                self.toast(ToastKind::Info, tf(Key::TSidebarFocus, &[&back]), 3);
            }
        }
        true
    }

    /// The menu of a sidebar item: open it, and add or remove it from the bookmarks.
    pub(super) fn side_menu(&self, path: &Path, at: (u16, u16)) -> MenuView {
        let mut items = vec![MenuItem::go(
            Key::SideMenuOpen.tr().into(),
            path.to_path_buf(),
        )];
        if rada_core::tagsearch::view_name(path).is_some() {
            // A tag is not a folder: nothing to pin or eject.
            return MenuView::new(at, items);
        }
        if let Some(id) = self.devices.unmounted_id(path) {
            // Plugged in and not mounted: there is no folder to pin, and "Open" is "Mount".
            items.clear();
            items.push(MenuItem::command(
                MenuCmd::Mount(id),
                Key::SideMenuMount.tr(),
                true,
            ));
            return MenuView::new(at, items);
        }
        let is_state = self.paths.bookmarks.iter().any(|b| b == path);
        let is_config = self.cfg_bookmarks.iter().any(|b| b == path);
        if is_state {
            items.push(MenuItem::bookmark(path.to_path_buf(), false, true));
        } else if !is_config {
            items.push(MenuItem::bookmark(path.to_path_buf(), true, true));
        }
        if self.ejectable(path).is_some() {
            items.push(MenuItem::command(
                MenuCmd::Eject(path.to_path_buf()),
                Key::SideMenuEject.tr(),
                true,
            ));
        }
        MenuView::new(at, items)
    }
}
