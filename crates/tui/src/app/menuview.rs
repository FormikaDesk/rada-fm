//! What a menu is made of: its entries, what choosing one does, and the menu that is open.

use std::path::PathBuf;

use rada_core::i18n::Key;
use rada_core::model::SortKey;

use super::MenuKind;
use crate::keymap::Action;
use crate::view::ViewMode;

/// What choosing an entry of a menu does.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuCmd {
    Act(Action),
    /// Go to a folder (the hidden parts of a long path).
    Go(PathBuf),
    /// Pin a folder to the navigation pane or take it out.
    Bookmark {
        path: PathBuf,
        add: bool,
    },
    /// Open another menu beside this one.
    Sub(MenuKind),
    /// The programs that can open the file ("Open with").
    OpenWithList,
    /// Eject the disk mounted here.
    Eject(PathBuf),
    /// Mount the device that is plugged in (by its id).
    Mount(String),
    /// Change the tags of what is selected.
    Tag(TagCmd),
    SortBy(SortKey),
    /// `true`: descending.
    SortOrder(bool),
    View(ViewMode),
    /// Panels, the classic layout, or whichever fits the window.
    Layout(crate::panels::LayoutPref),
    /// Ask the network for a newer version, now.
    CheckUpdates,
    /// The version, channel and way of updating.
    About,
}

/// What a choice in the Tags menu does.
#[derive(Clone, Debug, PartialEq)]
pub enum TagCmd {
    /// Give the tag to every selected item, or take it off if they all have it.
    Toggle(rada_core::platform::Tag),
    /// Ask for the name of a new tag.
    New,
    /// Take every tag off.
    Clear,
}

/// The mark in front of an entry of a menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuIcon {
    None,
    /// The icon of an action.
    Action(Action),
    /// A button's glyph (a submenu of the command bar).
    Glyph(crate::icons::Glyph),
    /// A dot in the colour of a tag.
    Dot(Option<rada_core::platform::TagColor>),
}

pub struct MenuItem {
    pub cmd: MenuCmd,
    pub label: String,
    pub enabled: bool,
    /// Draw a separator above this entry.
    pub gap_before: bool,
    /// A choice or a switch: whether it is on (drawn with a mark).
    pub checked: Option<bool>,
    pub icon: MenuIcon,
    /// Takes something away for good or to the trash: drawn in the colour of errors.
    pub danger: bool,
}

impl MenuItem {
    fn plain(cmd: MenuCmd, label: &str, enabled: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            cmd,
            label: label.to_string(),
            enabled,
            gap_before,
            checked: None,
            icon: MenuIcon::None,
            danger: false,
        }
    }

    pub fn act(action: Action, enabled: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            icon: MenuIcon::Action(action),
            danger: matches!(action, Action::Trash | Action::DeletePermanently),
            ..MenuItem::plain(MenuCmd::Act(action), action.label(), enabled, gap_before)
        }
    }

    /// An entry that runs one of the menu's own commands.
    pub fn command(cmd: MenuCmd, label: &str, gap_before: bool) -> MenuItem {
        MenuItem::plain(cmd, label, true, gap_before)
    }

    pub fn go(label: String, path: PathBuf) -> MenuItem {
        MenuItem {
            icon: MenuIcon::Glyph(crate::icons::Glyph::TabFolder),
            ..MenuItem::plain(MenuCmd::Go(path), &label, true, false)
        }
    }

    pub fn bookmark(path: PathBuf, add: bool, gap_before: bool) -> MenuItem {
        let label = if add {
            Key::SideMenuPin.tr()
        } else {
            Key::SideMenuUnpin.tr()
        };
        MenuItem {
            icon: MenuIcon::Action(Action::Bookmark),
            ..MenuItem::plain(MenuCmd::Bookmark { path, add }, label, true, gap_before)
        }
    }

    /// One of several exclusive choices.
    pub fn choice(cmd: MenuCmd, label: &str, on: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            checked: Some(on),
            ..MenuItem::plain(cmd, label, true, gap_before)
        }
    }

    /// An action that is a switch, shown with its state.
    pub fn toggle(action: Action, label: &str, on: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            checked: Some(on),
            ..MenuItem::plain(MenuCmd::Act(action), label, true, gap_before)
        }
    }

    /// An entry that opens another menu.
    pub fn sub(kind: MenuKind, label: &str, enabled: bool, gap_before: bool) -> MenuItem {
        use crate::icons::Glyph;
        let glyph = match kind {
            MenuKind::New => Glyph::New,
            MenuKind::Sort => Glyph::Sort,
            MenuKind::View => Glyph::View,
            MenuKind::Tags => Glyph::Tag,
            MenuKind::More => Glyph::More,
            MenuKind::Help => Glyph::Help,
        };
        MenuItem {
            icon: MenuIcon::Glyph(glyph),
            ..MenuItem::plain(MenuCmd::Sub(kind), label, enabled, gap_before)
        }
    }

    /// An entry that opens the list of programs for the file.
    pub fn open_with(enabled: bool) -> MenuItem {
        MenuItem {
            icon: MenuIcon::Action(Action::OpenWith),
            ..MenuItem::plain(
                MenuCmd::OpenWithList,
                Action::OpenWith.label(),
                enabled,
                false,
            )
        }
    }

    pub fn with_icon(mut self, icon: MenuIcon) -> MenuItem {
        self.icon = icon;
        self
    }

    /// Whether the cursor can stop on it.
    pub fn selectable(&self) -> bool {
        self.enabled
    }

    /// Whether choosing it opens another menu (or window) beside this one.
    pub fn opens_menu(&self) -> bool {
        matches!(self.cmd, MenuCmd::Sub(_) | MenuCmd::OpenWithList)
    }
}

/// The head of a menu: what it is about.
pub struct MenuTitle {
    /// The icon of the file (a Nerd Font glyph or nothing) and its colour.
    pub icon: &'static str,
    pub icon_color: ratatui::style::Color,
    pub name: String,
    /// "JPEG image": the type, quiet.
    pub kind: String,
}

/// A menu: the one of the right button, a dropdown of the command bar, or a submenu of either.
pub struct MenuView {
    pub at: (u16, u16),
    pub items: Vec<MenuItem>,
    pub selected: usize,
    pub title: Option<MenuTitle>,
    /// The menu this one was opened from: it stays in view, its entry marked, and comes back
    /// with Left or Esc.
    pub parent: Option<Box<MenuView>>,
}

impl MenuView {
    pub fn new(at: (u16, u16), items: Vec<MenuItem>) -> MenuView {
        let selected = items.iter().position(MenuItem::selectable).unwrap_or(0);
        MenuView {
            at,
            items,
            selected,
            title: None,
            parent: None,
        }
    }

    pub fn titled(mut self, title: Option<MenuTitle>) -> MenuView {
        self.title = title;
        self
    }

    /// Move the cursor to the next entry it can stop on, in `dir` (1 or -1); it does not wrap.
    pub fn step(&mut self, dir: isize) {
        let mut i = self.selected as isize + dir;
        while i >= 0 && (i as usize) < self.items.len() {
            if self.items[i as usize].selectable() {
                self.selected = i as usize;
                return;
            }
            i += dir;
        }
    }

    /// The menu with its parents, outermost first.
    pub fn chain(&self) -> Vec<&MenuView> {
        let mut v = vec![self];
        let mut cur = self;
        while let Some(p) = cur.parent.as_deref() {
            v.push(p);
            cur = p;
        }
        v.reverse();
        v
    }
}
