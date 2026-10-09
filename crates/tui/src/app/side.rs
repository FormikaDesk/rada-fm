//! The sidebar's behaviour: showing and hiding it, moving the keyboard focus into it,
//! what its keys do, and its menu.

use super::*;
use crate::sidebar::{self, Item, Mode, Origin};

/// Actions that mean the same wherever the keyboard focus is; everything else is about
/// the list, and the sidebar does not pretend to do it.
const PASS_THROUGH: &[Action] = &[
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
        Some(match &item.usage {
            Some(u) => format!("{} · {} free", item.name, fmt::size(u.free)),
            None => item.name.clone(),
        })
    }

    pub(super) fn toggle_sidebar(&mut self) {
        self.side_on = !self.side_on;
        if !self.side_on {
            self.side_focus = false;
        }
        rada_core::uistate::save_in_background(
            self.svc.platform.dirs().rada_state(),
            rada_core::uistate::UiState {
                sidebar: Some(self.side_on),
            },
        );
        if self.side_on && Mode::for_width(self.term_width, true) == Mode::Hidden {
            self.toast(
                ToastKind::Info,
                "sidebar on — this terminal is too narrow to show it",
                3,
            );
        }
    }

    /// Tab: the keyboard goes to the sidebar, or back to the list.
    pub(super) fn switch_pane(&mut self) {
        if self.side_focus {
            self.side_focus = false;
            return;
        }
        if !self.side_on {
            self.toggle_sidebar();
        }
        if Mode::for_width(self.term_width, self.side_on) == Mode::Hidden {
            self.toast(
                ToastKind::Info,
                "this terminal is too narrow for the sidebar",
                3,
            );
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
                if let Some(item) = items.get(self.side_cursor) {
                    let add = !self.paths.bookmarks.contains(&item.path);
                    self.set_bookmark(item.path.clone(), add);
                }
            }
            Trash | DeletePermanently => match items.get(self.side_cursor).map(|i| &i.origin) {
                Some(Origin::Bookmark { removable: true }) => {
                    let path = items[self.side_cursor].path.clone();
                    self.set_bookmark(path, false);
                }
                _ => self.toast(
                    ToastKind::Info,
                    "only bookmarks can be removed from the sidebar",
                    3,
                ),
            },
            Filter => {
                // Typing a filter is about the list.
                self.side_focus = false;
                return false;
            }
            a if PASS_THROUGH.contains(&a) => return false,
            _ => {
                let back = self.key_for(SwitchPane).unwrap_or_else(|| "Tab".into());
                self.toast(
                    ToastKind::Info,
                    format!("the sidebar has the focus — {back} goes back to the list"),
                    3,
                );
            }
        }
        true
    }

    /// The menu of a sidebar item: open it, and add or remove it from the bookmarks.
    pub(super) fn side_menu(&self, path: &Path, at: (u16, u16)) -> MenuView {
        let mut items = vec![MenuItem::go("Open".into(), path.to_path_buf())];
        let is_state = self.paths.bookmarks.iter().any(|b| b == path);
        let is_config = self.cfg_bookmarks.iter().any(|b| b == path);
        if is_state {
            items.push(MenuItem::bookmark(path.to_path_buf(), false, true));
        } else if !is_config {
            items.push(MenuItem::bookmark(path.to_path_buf(), true, true));
        }
        MenuView {
            at,
            items,
            selected: 0,
        }
    }
}
