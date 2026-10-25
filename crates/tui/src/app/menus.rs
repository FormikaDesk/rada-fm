//! The menus: the one on the right button, and the dropdowns of the command bar.

use rada_core::i18n::Key;

use super::{
    App, InputKind, InputView, MenuCmd, MenuIcon, MenuItem, MenuTitle, MenuView, Modal, TagCmd,
};
use crate::keymap::Action;
use crate::panels::LayoutPref;
use crate::view::ViewMode;
use rada_core::model::{Entry, SortKey};
use rada_core::ops::OpRequest;

/// Which dropdown a button of the command bar opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    New,
    Sort,
    View,
    More,
    Tags,
    Help,
}

impl App {
    /// Open a dropdown with its corner at `at`.
    pub fn open_menu(&mut self, kind: MenuKind, at: (u16, u16)) {
        let items = self.menu_items(kind);
        self.windows.modal = Some(Modal::Menu(MenuView::new(at, items)));
    }

    pub(super) fn menu_items(&self, kind: MenuKind) -> Vec<MenuItem> {
        use Action::*;
        let ro = self.tab.archive.is_some();
        // Nothing is made *in* a list of tagged items.
        let nowhere = ro || self.tab.tag_view().is_some();
        match kind {
            MenuKind::New => vec![
                MenuItem::act(NewFolder, !nowhere, false),
                MenuItem::act(NewFile, !nowhere, false),
            ],
            MenuKind::Sort => {
                let key = |k: SortKey, label: &str, gap: bool| {
                    MenuItem::choice(MenuCmd::SortBy(k), label, self.tab.sort.key == k, gap)
                };
                vec![
                    key(SortKey::Name, Key::ColName.tr(), false),
                    key(SortKey::Modified, Key::SortModified.tr(), false),
                    key(SortKey::Type, Key::ColType.tr(), false),
                    key(SortKey::Size, Key::ColSize.tr(), false),
                    MenuItem::choice(
                        MenuCmd::SortOrder(false),
                        Key::Ascending.tr(),
                        !self.tab.sort.descending(),
                        true,
                    ),
                    MenuItem::choice(
                        MenuCmd::SortOrder(true),
                        Key::Descending.tr(),
                        self.tab.sort.descending(),
                        false,
                    ),
                ]
            }
            MenuKind::View => vec![
                MenuItem::choice(
                    MenuCmd::View(ViewMode::Details),
                    Key::ViewDetails.tr(),
                    self.tab.view == ViewMode::Details,
                    false,
                ),
                MenuItem::choice(
                    MenuCmd::View(ViewMode::Icons),
                    Key::ViewIcons.tr(),
                    self.tab.view == ViewMode::Icons,
                    false,
                ),
                MenuItem::choice(
                    MenuCmd::View(ViewMode::Columns),
                    Key::ViewColumns.tr(),
                    self.tab.view == ViewMode::Columns,
                    false,
                ),
                MenuItem::toggle(
                    ToggleDetails,
                    Key::MenuDetailsPane.tr(),
                    self.details_visible(),
                    true,
                ),
                MenuItem::toggle(
                    ToggleHidden,
                    Key::MenuHiddenFiles.tr(),
                    self.settings.show_hidden,
                    false,
                ),
                MenuItem::toggle(
                    ToggleSidebar,
                    Key::MenuNavPane.tr(),
                    self.side_nav.wanted,
                    false,
                ),
                MenuItem::toggle(
                    ToggleTags,
                    Key::MenuTagColumn.tr(),
                    self.settings.show_tags,
                    false,
                ),
                MenuItem::choice(
                    MenuCmd::Layout(LayoutPref::Auto),
                    Key::MenuLayoutAuto.tr(),
                    self.settings.layout_pref == LayoutPref::Auto,
                    true,
                ),
                MenuItem::choice(
                    MenuCmd::Layout(LayoutPref::Panels),
                    Key::MenuLayoutPanels.tr(),
                    self.settings.layout_pref == LayoutPref::Panels,
                    false,
                ),
                MenuItem::choice(
                    MenuCmd::Layout(LayoutPref::Classic),
                    Key::MenuLayoutClassic.tr(),
                    self.settings.layout_pref == LayoutPref::Classic,
                    false,
                ),
            ],
            MenuKind::Tags => self.tag_menu_items(),
            MenuKind::Help => vec![
                MenuItem::act(Help, true, false),
                MenuItem::command(MenuCmd::CheckUpdates, Key::MenuCheckUpdates.tr(), true),
                MenuItem::command(MenuCmd::About, Key::MenuAbout.tr(), false),
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

    /// The head of a submenu: the tags of what is selected get a title, the others none.
    pub(super) fn submenu_title(&self, kind: MenuKind) -> Option<MenuTitle> {
        match kind {
            MenuKind::Tags => {
                let what = if self.tab.sel.marked.len() > 1 {
                    crate::fmt::count(
                        self.tab.sel.marked.len() as u64,
                        Key::ItemOne,
                        Key::ItemMany,
                    )
                } else {
                    self.tab
                        .current()
                        .map(|e| e.display.clone())
                        .unwrap_or_default()
                };
                Some(MenuTitle {
                    icon: "",
                    icon_color: self.settings.th.accent,
                    name: rada_core::i18n::tf(Key::TagsOf, &[&what]),
                    kind: String::new(),
                })
            }
            _ => None,
        }
    }

    /// The head of the menu of an item: its icon, its name and what kind of file it is; with
    /// several items selected, how many.
    fn item_menu_title(&self) -> Option<MenuTitle> {
        let th = &self.settings.th;
        if self.tab.sel.marked.len() > 1 {
            return Some(MenuTitle {
                icon: crate::icons::ui(self.settings.icons, crate::icons::Glyph::Checked),
                icon_color: th.accent,
                name: crate::fmt::count(
                    self.tab.sel.marked.len() as u64,
                    Key::ItemOne,
                    Key::ItemMany,
                ),
                kind: String::new(),
            });
        }
        let e = self.tab.current()?;
        let (icon, icon_color) = crate::icons::icon(e, self.settings.icons, th);
        Some(MenuTitle {
            icon,
            icon_color,
            name: e.display.clone(),
            kind: rada_core::i18n::type_label(&e.type_label).into_owned(),
        })
    }

    /// The menu of the right button (or the Menu key): on an item, or on empty space.
    pub(super) fn context_menu(&self, on_item: bool, at: (u16, u16)) -> MenuView {
        use Action::*;
        let has_clip = self.ops.clipboard.is_some();
        let mut items: Vec<MenuItem> = Vec::new();
        let ro = self.tab.archive.is_some();
        let nowhere = ro || self.tab.tag_view().is_some();
        let on_archive = self.on_archive_item();
        if on_item {
            let cur = self.tab.current();
            // Opening.
            items.push(MenuItem::act(Open, true, false));
            if self.tab.tag_view().is_some() {
                items.push(MenuItem::act(ShowInFolder, true, false));
            }
            if cur.is_some_and(|e| e.package) {
                items.push(MenuItem::act(ShowPackage, true, false));
            }
            items.push(MenuItem::open_with(
                cur.is_some_and(|e| !e.is_navigable_dir()) && !ro,
            ));
            if self.svc.platform.opener().can_quick_look() {
                items.push(MenuItem::act(QuickLook, !ro, false));
            }
            if on_archive || ro {
                items.push(MenuItem::act(ExtractHere, true, true));
                items.push(MenuItem::act(ExtractToFolder, true, false));
            }
            // The clipboard.
            items.push(MenuItem::act(Cut, !ro, true));
            items.push(MenuItem::act(Copy, true, false));
            items.push(MenuItem::act(Paste, has_clip && !nowhere, false));
            // Changing it.
            items.push(MenuItem::act(Rename, !ro, true));
            if !ro {
                items.push(MenuItem::act(Compress, true, false));
            }
            // About it.
            let mut gap = true;
            if self.tags_available() {
                items.push(MenuItem::sub(MenuKind::Tags, Key::MenuTags.tr(), !ro, gap));
                gap = false;
            }
            if let Some(e) = cur
                && e.is_navigable_dir()
            {
                let pinned = self.places.is_bookmarked(&e.path);
                items.push(MenuItem::bookmark(e.path.clone(), !pinned, gap));
                gap = false;
            }
            items.push(MenuItem::act(CopyPath, true, gap));
            items.push(MenuItem::act(Properties, true, true));
            // Taking it away: last, and red.
            items.push(MenuItem::act(Trash, !ro, true));
            if !ro {
                items.push(MenuItem::act(DeletePermanently, true, false));
            }
        } else {
            items.push(MenuItem::sub(
                MenuKind::New,
                Key::MenuNew.tr(),
                !nowhere,
                false,
            ));
            items.push(MenuItem::act(Paste, has_clip && !nowhere, false));
            items.push(MenuItem::sub(
                MenuKind::Sort,
                Key::MenuSortBy.tr(),
                true,
                true,
            ));
            items.push(MenuItem::sub(
                MenuKind::View,
                Key::MenuView.tr(),
                true,
                false,
            ));
            items.push(MenuItem::act(Refresh, true, true));
            items.push(MenuItem::act(SelectAll, true, false));
            if ro {
                items.push(MenuItem::act(ExtractHere, true, true));
                items.push(MenuItem::act(ExtractToFolder, true, false));
            } else {
                items.push(MenuItem::act(OpenTerminal, true, true));
            }
        }
        MenuView::new(at, items).titled(if on_item {
            self.item_menu_title()
        } else {
            None
        })
    }

    /// Do what a chosen menu entry says. The menu is already closed.
    pub(super) fn run_menu(&mut self, cmd: MenuCmd, at: (u16, u16)) {
        match cmd {
            MenuCmd::Act(action) => self.dispatch(action),
            MenuCmd::Go(path) => self.open_dir(path),
            MenuCmd::Bookmark { path, add } => self.set_bookmark(path, add),
            MenuCmd::Sub(kind) => self.open_menu(kind, at),
            MenuCmd::OpenWithList => self.dispatch(Action::OpenWith),
            MenuCmd::SortBy(key) => {
                // Choosing the key that is already used leaves the order alone.
                if key != self.tab.sort.key {
                    self.set_sort(self.tab.sort.with_key(key));
                }
            }
            MenuCmd::SortOrder(descending) => {
                if descending != self.tab.sort.descending() {
                    self.set_sort(self.tab.sort.with_descending(descending));
                }
            }
            MenuCmd::View(mode) => self.set_view(mode),
            MenuCmd::Layout(pref) => self.set_layout_pref(pref),
            MenuCmd::Eject(mount) => self.start_eject(&mount),
            MenuCmd::Mount(id) => self.start_mount(&id),
            MenuCmd::Tag(cmd) => self.run_tag_cmd(cmd),
            MenuCmd::CheckUpdates => self.check_updates_now(),
            MenuCmd::About => self.open_about(),
        }
    }
}

impl App {
    /// Whether this system keeps tags, and these items may have them.
    pub fn tags_available(&self) -> bool {
        self.svc.platform.tags().is_some() && self.tab.archive.is_none()
    }

    /// The entries the tag menu is about: the selection, or the item under the cursor.
    fn tag_targets(&self) -> Vec<&Entry> {
        if !self.tab.sel.marked.is_empty() {
            self.tab
                .listing
                .all()
                .iter()
                .filter(|e| self.tab.sel.marked.contains(&e.name))
                .collect()
        } else {
            self.tab.current().into_iter().collect()
        }
    }

    /// The entries of the Tags submenu: the seven colours, the other tags the items carry, a
    /// new tag, and removing them all. A tag is ticked when every item has it.
    pub(super) fn tag_menu_items(&self) -> Vec<MenuItem> {
        use rada_core::platform::{Tag, TagColor as Hue};
        let targets = self.tag_targets();
        let everyone = |name: &str| {
            !targets.is_empty()
                && targets
                    .iter()
                    .all(|e| e.tags.iter().any(|t| t.is_called(name)))
        };
        // The colours are called what the system calls them (a Mac in Italian says "Blu"): a
        // tag of another name has no colour for the Finder. A tag with the colour counts
        // whatever its name.
        let lang = self.tags.lang.as_str();
        let has_colour = |c: Hue| {
            !targets.is_empty()
                && targets.iter().all(|e| {
                    e.tags
                        .iter()
                        .any(|t| t.color == Some(c) || t.is_called(c.label(lang)))
                })
        };
        let mut items: Vec<MenuItem> = Hue::ALL
            .into_iter()
            .map(|c| {
                MenuItem::choice(
                    MenuCmd::Tag(TagCmd::Toggle(Tag::standard_in(c, lang))),
                    c.label(lang),
                    has_colour(c),
                    false,
                )
                .with_icon(MenuIcon::Dot(Some(c)))
            })
            .collect();
        // The tags of their own that the items already carry, so that they can be taken off.
        let mut others: Vec<Tag> = Vec::new();
        for e in &targets {
            for t in &e.tags {
                if Hue::from_name(&t.name).is_none() && !others.iter().any(|o| o.is_called(&t.name))
                {
                    others.push(t.clone());
                }
            }
        }
        for (i, t) in others.into_iter().enumerate() {
            let on = everyone(&t.name);
            items.push(
                MenuItem::choice(MenuCmd::Tag(TagCmd::Toggle(t.clone())), &t.name, on, i == 0)
                    .with_icon(MenuIcon::Dot(t.color)),
            );
        }
        items.push(
            MenuItem::command(MenuCmd::Tag(TagCmd::New), Key::MenuNewTag.tr(), true)
                .with_icon(MenuIcon::Glyph(crate::icons::Glyph::New)),
        );
        items.last_mut().unwrap().enabled = !targets.is_empty();
        items.push(
            MenuItem::command(
                MenuCmd::Tag(TagCmd::Clear),
                Key::MenuRemoveAllTags.tr(),
                false,
            )
            .with_icon(MenuIcon::Glyph(crate::icons::Glyph::Close)),
        );
        items.last_mut().unwrap().enabled = targets.iter().any(|e| !e.tags.is_empty());
        items
    }

    pub(super) fn run_tag_cmd(&mut self, cmd: TagCmd) {
        use rada_core::ops::TagChange;
        let items = self.tab.targets();
        if items.is_empty()
            || self.refuse_in_archive(Key::WhatChangeTags.tr())
            || self.refuse_if_busy()
        {
            return;
        }
        let change = match cmd {
            TagCmd::Toggle(tag) => TagChange::Toggle { tag },
            TagCmd::Clear => TagChange::Clear,
            TagCmd::New => {
                self.windows.modal = Some(Modal::Input(InputView::new(
                    InputKind::NewTag { items },
                    String::new(),
                )));
                return;
            }
        };
        let h = self
            .svc
            .jobs
            .plan_request(OpRequest::SetTags { items, change });
        self.begin_plan(Key::PlanningGeneric.tr(), h);
    }
}
