//! The menus: the one on the right button, and the dropdowns of the command bar.

use super::*;

/// Which dropdown a button of the command bar opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    New,
    Sort,
    View,
    More,
}

impl App {
    /// Open a dropdown with its corner at `at`.
    pub fn open_menu(&mut self, kind: MenuKind, at: (u16, u16)) {
        let items = self.menu_items(kind);
        self.modal = Some(Modal::Menu(MenuView {
            at,
            items,
            selected: 0,
        }));
    }

    fn menu_items(&self, kind: MenuKind) -> Vec<MenuItem> {
        use Action::*;
        let ro = self.archive.is_some();
        match kind {
            MenuKind::New => vec![
                MenuItem::act(NewFolder, !ro, false),
                MenuItem::act(NewFile, !ro, false),
            ],
            MenuKind::Sort => {
                let key = |k: SortKey, label: &str, gap: bool| {
                    MenuItem::choice(MenuCmd::SortBy(k), label, self.sort.key == k, gap)
                };
                vec![
                    key(SortKey::Name, "Name", false),
                    key(SortKey::Modified, "Date modified", false),
                    key(SortKey::Type, "Type", false),
                    key(SortKey::Size, "Size", false),
                    MenuItem::choice(
                        MenuCmd::SortOrder(false),
                        "Ascending",
                        !self.sort.reverse,
                        true,
                    ),
                    MenuItem::choice(
                        MenuCmd::SortOrder(true),
                        "Descending",
                        self.sort.reverse,
                        false,
                    ),
                ]
            }
            MenuKind::View => vec![
                MenuItem::choice(
                    MenuCmd::View(ViewMode::Details),
                    "Details",
                    self.view == ViewMode::Details,
                    false,
                ),
                MenuItem::choice(
                    MenuCmd::View(ViewMode::Icons),
                    "Icons",
                    self.view == ViewMode::Icons,
                    false,
                ),
                MenuItem::toggle(ToggleDetails, "Details pane", self.details_visible(), true),
                MenuItem::toggle(ToggleHidden, "Hidden files", self.show_hidden, false),
                MenuItem::toggle(ToggleSidebar, "Navigation pane", self.side_on, false),
            ],
            MenuKind::More => {
                let mut v = vec![
                    MenuItem::act(Redo, true, false),
                    MenuItem::act(History, true, false),
                    MenuItem::act(BulkRename, !ro, false),
                ];
                if ro || self.on_archive_item() {
                    v.push(MenuItem::act(ExtractHere, true, true));
                    v.push(MenuItem::act(ExtractToFolder, true, false));
                }
                v.push(MenuItem::act(OpenTerminal, !ro, true));
                v.push(MenuItem::act(Refresh, true, false));
                v.push(MenuItem::act(Help, true, true));
                v
            }
        }
    }

    /// The menu of the right button (or the Menu key): on an item, or on empty space.
    pub(super) fn context_menu(&self, on_item: bool, at: (u16, u16)) -> MenuView {
        use Action::*;
        let has_clip = self.clipboard.is_some();
        let mut items: Vec<MenuItem> = Vec::new();
        let ro = self.archive.is_some();
        let on_archive = self.on_archive_item();
        if on_item {
            let cur = self.current();
            items.push(MenuItem::act(Open, true, false));
            items.push(MenuItem::act(
                OpenWith,
                cur.is_some_and(|e| !e.is_dir()) && !ro,
                false,
            ));
            if on_archive || ro {
                items.push(MenuItem::act(ExtractHere, true, true));
                items.push(MenuItem::act(ExtractToFolder, true, false));
            }
            items.push(MenuItem::act(Cut, !ro, !(on_archive || ro)));
            items.push(MenuItem::act(Copy, true, false));
            items.push(MenuItem::act(Paste, has_clip && !ro, false));
            items.push(MenuItem::act(Rename, !ro, true));
            if !ro {
                items.push(MenuItem::act(Compress, true, false));
            }
            if let Some(e) = cur
                && e.is_dir()
            {
                let pinned =
                    self.paths.bookmarks.contains(&e.path) || self.cfg_bookmarks.contains(&e.path);
                items.push(MenuItem::bookmark(e.path.clone(), !pinned, true));
            }
            items.push(MenuItem::act(
                CopyPath,
                true,
                cur.is_none_or(|e| !e.is_dir()),
            ));
            items.push(MenuItem::act(Trash, !ro, true));
            items.push(MenuItem::act(Properties, true, true));
        } else {
            items.push(MenuItem::sub(MenuKind::New, "New", !ro, false));
            items.push(MenuItem::act(Paste, has_clip && !ro, false));
            items.push(MenuItem::sub(MenuKind::Sort, "Sort by", true, true));
            items.push(MenuItem::sub(MenuKind::View, "View", true, false));
            items.push(MenuItem::act(Refresh, true, true));
            items.push(MenuItem::act(SelectAll, true, false));
            if ro {
                items.push(MenuItem::act(ExtractHere, true, true));
                items.push(MenuItem::act(ExtractToFolder, true, false));
            } else {
                items.push(MenuItem::act(OpenTerminal, true, true));
            }
        }
        MenuView {
            at,
            items,
            selected: 0,
        }
    }

    /// Do what a chosen menu entry says. The menu is already closed.
    pub(super) fn run_menu(&mut self, cmd: MenuCmd, at: (u16, u16)) {
        match cmd {
            MenuCmd::Act(action) => self.dispatch(action),
            MenuCmd::Go(path) => self.open_dir(path),
            MenuCmd::Bookmark { path, add } => self.set_bookmark(path, add),
            MenuCmd::Sub(kind) => self.open_menu(kind, at),
            MenuCmd::SortBy(key) => {
                // Choosing the key that is already used leaves the order alone.
                if key != self.sort.key {
                    self.set_sort(SortSpec { key, ..self.sort });
                }
            }
            MenuCmd::SortOrder(reverse) => {
                if reverse != self.sort.reverse {
                    self.set_sort(SortSpec {
                        reverse,
                        ..self.sort
                    });
                }
            }
            MenuCmd::View(mode) => self.set_view(mode),
        }
    }
}
