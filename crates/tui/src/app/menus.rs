//! The menus: the one on the right button, and the dropdowns of the command bar.

use crossterm::event::{KeyCode, KeyEvent};
use rada_core::i18n::Key;

use super::{
    App, InputKind, InputView, MenuCmd, MenuIcon, MenuItem, MenuTitle, MenuView, Modal, TagCmd,
};
use crate::keymap::Action;
use crate::panels::LayoutPref;
use crate::registry::{Avail, Surface};
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
    /// The recipes for what is selected.
    Actions,
}

impl App {
    /// Open a dropdown with its corner at `at`.
    pub fn open_menu(&mut self, kind: MenuKind, at: (u16, u16)) {
        let items = self.menu_items(kind);
        self.windows.modal = Some(Modal::Menu(MenuView::new(at, items)));
    }

    pub fn menu_items(&self, kind: MenuKind) -> Vec<MenuItem> {
        use Action::*;
        match kind {
            // Nothing is made *in* a list of tagged items.
            MenuKind::New => self.acts(Surface::NewMenu, &[(NewFolder, false), (NewFile, false)]),
            MenuKind::Sort => self.sort_menu_items(),
            MenuKind::View => self.view_menu_items(),
            MenuKind::Tags => self.tag_menu_items(),
            MenuKind::Help => vec![
                MenuItem::act(Help, true, false),
                MenuItem::command(MenuCmd::CheckUpdates, Key::MenuCheckUpdates.tr(), true),
                MenuItem::command(MenuCmd::About, Key::MenuAbout.tr(), false),
            ],
            MenuKind::More => self.more_menu_items(),
            MenuKind::Actions => self.recipe_menu_items(),
        }
    }

    fn sort_menu_items(&self) -> Vec<MenuItem> {
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

    fn view_menu_items(&self) -> Vec<MenuItem> {
        let mut items = self.view_mode_items();
        items.extend(self.view_toggle_items());
        items.extend(self.layout_items());
        items
    }

    fn view_mode_items(&self) -> Vec<MenuItem> {
        let mode = |m: ViewMode, label: &str| {
            MenuItem::choice(MenuCmd::View(m), label, self.tab.view == m, false)
        };
        vec![
            mode(ViewMode::Details, Key::ViewDetails.tr()),
            mode(ViewMode::Icons, Key::ViewIcons.tr()),
            mode(ViewMode::Columns, Key::ViewColumns.tr()),
        ]
    }

    fn view_toggle_items(&self) -> Vec<MenuItem> {
        use Action::*;
        vec![
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
        ]
    }

    fn layout_items(&self) -> Vec<MenuItem> {
        let layout = |pref: LayoutPref, label: &str, gap: bool| {
            MenuItem::choice(
                MenuCmd::Layout(pref),
                label,
                self.settings.layout_pref == pref,
                gap,
            )
        };
        vec![
            layout(LayoutPref::Auto, Key::MenuLayoutAuto.tr(), true),
            layout(LayoutPref::Panels, Key::MenuLayoutPanels.tr(), false),
            layout(LayoutPref::Classic, Key::MenuLayoutClassic.tr(), false),
        ]
    }

    /// The entry of `action` in the menu `surface`, greyed or left out as its card says.
    fn act(&self, surface: Surface, action: Action, gap: bool) -> Option<MenuItem> {
        match action.spec().availability(&self.facts(surface)) {
            Avail::Enabled => Some(MenuItem::act(action, true, gap)),
            Avail::Disabled(_) => Some(MenuItem::act(action, false, gap)),
            Avail::Hidden => None,
        }
    }

    /// The entries of several actions, each with whether a separator goes above it.
    fn acts(&self, surface: Surface, actions: &[(Action, bool)]) -> Vec<MenuItem> {
        actions
            .iter()
            .filter_map(|(action, gap)| self.act(surface, *action, *gap))
            .collect()
    }

    fn more_menu_items(&self) -> Vec<MenuItem> {
        use Action::*;
        self.acts(
            Surface::MoreMenu,
            &[
                (Redo, false),
                (History, false),
                (BulkRename, false),
                (ExtractHere, true),
                (ExtractToFolder, false),
                (OpenTerminal, true),
                (Refresh, false),
                (Help, true),
            ],
        )
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
    pub fn context_menu(&self, on_item: bool, at: (u16, u16)) -> MenuView {
        let items = if on_item {
            self.item_menu_items()
        } else {
            self.space_menu_items()
        };
        MenuView::new(at, items).titled(if on_item {
            self.item_menu_title()
        } else {
            None
        })
    }

    fn item_menu_items(&self) -> Vec<MenuItem> {
        let mut items = self.item_opening_items();
        // Only when some recipe is for what is selected: without recipes the menu is as it was.
        if !self.recipe_list().is_empty() {
            items.push(MenuItem::sub(
                MenuKind::Actions,
                Key::MenuActions.tr(),
                true,
                true,
            ));
        }
        items.extend(self.item_edit_items());
        items.extend(self.item_about_items());
        items
    }

    fn item_opening_items(&self) -> Vec<MenuItem> {
        use Action::*;
        let on = Surface::ItemMenu;
        let facts = self.facts(on);
        let mut items = self.acts(
            on,
            &[(Open, false), (ShowInFolder, false), (ShowPackage, false)],
        );
        items.push(MenuItem::open_with(
            OpenWith.spec().availability(&facts) == Avail::Enabled,
        ));
        items.extend(self.acts(
            on,
            &[
                (QuickLook, false),
                (ExtractHere, true),
                (ExtractToFolder, false),
            ],
        ));
        items
    }

    /// The clipboard, and changing the item.
    fn item_edit_items(&self) -> Vec<MenuItem> {
        use Action::*;
        self.acts(
            Surface::ItemMenu,
            &[
                (Cut, true),
                (Copy, false),
                (Paste, false),
                (Rename, true),
                (Compress, false),
            ],
        )
    }

    /// About the item, and taking it away: last, and red.
    fn item_about_items(&self) -> Vec<MenuItem> {
        use Action::*;
        let on = Surface::ItemMenu;
        let ro = self.facts(on).read_only;
        let cur = self.tab.current();
        let mut items = Vec::new();
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
        items.extend(self.acts(
            on,
            &[
                (CopyPath, gap),
                (Properties, true),
                (Trash, true),
                (DeletePermanently, false),
            ],
        ));
        items
    }

    fn space_menu_items(&self) -> Vec<MenuItem> {
        use Action::*;
        let on = Surface::SpaceMenu;
        let nowhere = self.facts(on).nowhere();
        let mut items = vec![MenuItem::sub(
            MenuKind::New,
            Key::MenuNew.tr(),
            !nowhere,
            false,
        )];
        items.extend(self.act(on, Paste, false));
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
        items.extend(self.acts(
            on,
            &[
                (Refresh, true),
                (SelectAll, false),
                (ExtractHere, true),
                (ExtractToFolder, false),
                (OpenTerminal, true),
            ],
        ));
        items
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
            MenuCmd::Recipe(ask) => self.continue_recipe(*ask),
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
        let targets = self.tag_targets();
        let mut items = colour_items(&targets, self.tags.lang.as_str());
        items.extend(own_tag_items(&targets));
        items.extend(tag_commands(&targets));
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

    pub(super) fn on_menu_key(&mut self, mut m: MenuView, key: KeyEvent) {
        match key.code {
            // Esc closes the menu in front: a submenu gives the menu it came from back.
            KeyCode::Esc | KeyCode::Char('q') => {
                if let Some(p) = m.parent.take() {
                    self.windows.modal = Some(Modal::Menu(*p));
                }
            }
            KeyCode::Left | KeyCode::Char('h') if m.parent.is_some() => {
                if let Some(p) = m.parent.take() {
                    self.windows.modal = Some(Modal::Menu(*p));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                m.step(1);
                self.windows.modal = Some(Modal::Menu(m));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                m.step(-1);
                self.windows.modal = Some(Modal::Menu(m));
            }
            KeyCode::Right | KeyCode::Char('l') => {
                // Right opens a submenu; on any other entry it does nothing.
                if m.items.get(m.selected).is_some_and(MenuItem::opens_menu) {
                    self.menu_activate(m);
                } else {
                    self.windows.modal = Some(Modal::Menu(m));
                }
            }
            KeyCode::Enter => self.menu_activate(m),
            _ => self.windows.modal = Some(Modal::Menu(m)),
        }
    }

    /// Choose the entry under the cursor: run it, or open the menu it stands for beside this
    /// one (which stays in view, its entry marked).
    pub(super) fn menu_activate(&mut self, m: MenuView) {
        let Some(it) = m.items.get(m.selected) else {
            return;
        };
        if !it.enabled {
            self.windows.modal = Some(Modal::Menu(m));
            return;
        }
        match it.cmd.clone() {
            MenuCmd::Sub(kind) => {
                let items = self.menu_items(kind);
                let title = self.submenu_title(kind);
                let mut child = MenuView::new((0, 0), items).titled(title);
                child.parent = Some(Box::new(m));
                self.windows.modal = Some(Modal::Menu(child));
            }
            MenuCmd::OpenWithList => {
                if let Some(path) = self.tab.current().map(|e| e.path.clone()) {
                    self.open_with_window(path, Some(Box::new(m)));
                } else {
                    self.windows.modal = None;
                }
            }
            cmd => {
                let at = (
                    m.at.0.saturating_add(2),
                    m.at.1.saturating_add(1 + m.selected as u16),
                );
                self.windows.modal = None;
                self.run_menu(cmd, at);
            }
        }
    }
}

/// Whether every one of `targets` carries a tag called `name`.
fn all_have(targets: &[&Entry], name: &str) -> bool {
    !targets.is_empty()
        && targets
            .iter()
            .all(|e| e.tags.iter().any(|t| t.is_called(name)))
}

/// The seven colours, ticked when every item has the colour. They are called what the system
/// calls them (a Mac in Italian says "Blu"): a tag of another name has no colour for the
/// Finder. A tag with the colour counts whatever its name.
fn colour_items(targets: &[&Entry], lang: &str) -> Vec<MenuItem> {
    use rada_core::platform::{Tag, TagColor as Hue};
    let has_colour = |c: Hue| {
        !targets.is_empty()
            && targets.iter().all(|e| {
                e.tags
                    .iter()
                    .any(|t| t.color == Some(c) || t.is_called(c.label(lang)))
            })
    };
    Hue::ALL
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
        .collect()
}

/// The tags of their own that the items already carry, so that they can be taken off.
fn own_tag_items(targets: &[&Entry]) -> Vec<MenuItem> {
    use rada_core::platform::{Tag, TagColor as Hue};
    let mut others: Vec<Tag> = Vec::new();
    for e in targets {
        for t in &e.tags {
            if Hue::from_name(&t.name).is_none() && !others.iter().any(|o| o.is_called(&t.name)) {
                others.push(t.clone());
            }
        }
    }
    others
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let on = all_have(targets, &t.name);
            MenuItem::choice(
                MenuCmd::Tag(TagCmd::Toggle(t.clone())),
                &rada_core::display::text(&t.name),
                on,
                i == 0,
            )
            .with_icon(MenuIcon::Dot(t.color))
        })
        .collect()
}

/// A new tag, and removing them all.
fn tag_commands(targets: &[&Entry]) -> Vec<MenuItem> {
    let mut new = MenuItem::command(MenuCmd::Tag(TagCmd::New), Key::MenuNewTag.tr(), true)
        .with_icon(MenuIcon::Glyph(crate::icons::Glyph::New));
    new.enabled = !targets.is_empty();
    let mut clear = MenuItem::command(
        MenuCmd::Tag(TagCmd::Clear),
        Key::MenuRemoveAllTags.tr(),
        false,
    )
    .with_icon(MenuIcon::Glyph(crate::icons::Glyph::Close));
    clear.enabled = targets.iter().any(|e| !e.tags.is_empty());
    vec![new, clear]
}
