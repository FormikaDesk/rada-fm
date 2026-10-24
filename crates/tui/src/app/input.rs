//! Keyboard and mouse: from key chords and clicks to [`Action`]s, and what each action does.

use crossterm::event::{KeyEventKind, MouseButton, MouseEvent, MouseEventKind};

use super::{
    App, HistoryView, InputKind, InputView, LoadState, MenuKind, Modal, ToastKind, UpdateView,
};
use crate::hits::Target;
use crate::keymap::{Action, Scheme};
use crate::view::ViewMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::events::CoreEvent;
use rada_core::i18n::Key;
use rada_core::model::{SortKey, SortSpec};
use rada_core::ops::TransferMode;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The "filter this folder" box: while `editing`, typed characters go into it.
pub struct FilterState {
    pub text: String,
    pub editing: bool,
}

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

const WHEEL_STEP: usize = 3;

impl App {
    // ------------------------------------------------------------------ keyboard

    pub fn on_key(&mut self, key: KeyEvent) {
        self.redraw.dirty = true;
        if key.kind == KeyEventKind::Release {
            return;
        }
        // Ctrl+L, wherever it is pressed: the screen is drawn again from nothing (the key still
        // does what it does: it starts the editing of the address).
        if key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.redraw.full_redraw = true;
        }
        // The card at the bottom right goes at the first key (the key still does its job); an
        // error stays until Esc, which it takes when nothing else is open.
        let now = self.notice_now();
        if self.windows.modal.is_none()
            && self.windows.address.is_none()
            && key.code == KeyCode::Esc
            && self.windows.notices.dismiss_error(now)
        {
            return;
        }
        self.windows.notices.on_key(now);
        if self.windows.modal.is_some() {
            self.on_modal_key(key);
            return;
        }
        if self.windows.address.is_some() {
            self.on_address_key(key);
            return;
        }
        if self.filter.as_ref().is_some_and(|f| f.editing) {
            self.on_filter_key(key);
            return;
        }
        if self.session.chooser.is_some() && self.on_chooser_key(key) {
            return;
        }
        // Alt+1 … Alt+9 jump to a tab.
        if key.modifiers.contains(KeyModifiers::ALT)
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && let KeyCode::Char(c @ '1'..='9') = key.code
        {
            self.go_tab(c as usize - '0' as usize);
            return;
        }
        // In the Columns view Right goes into the folder under the cursor and does nothing on a
        // file; Left (the parent action) already goes to the folder above.
        if self.view == ViewMode::Columns
            && !self.side_nav.focus
            && key.modifiers.is_empty()
            && matches!(key.code, KeyCode::Right | KeyCode::Char('l'))
            && self.settings.keymap.action_for(&key) == Some(Action::Open)
        {
            self.columns_right();
            self.redraw.dirty = true;
            return;
        }
        if let Some(action) = self.settings.keymap.action_for(&key) {
            // In NAV of the panels view the sideways arrows open and close the folders.
            if self.frame.panels_shown
                && self.side_nav.focus
                && key.modifiers.is_empty()
                && let Some(right) = match (action, key.code) {
                    (Action::Open, KeyCode::Right | KeyCode::Char('l')) => Some(true),
                    (Action::Parent, KeyCode::Left | KeyCode::Char('h')) => Some(false),
                    _ => None,
                }
                && self.on_nav_arrow(right)
            {
                self.redraw.dirty = true;
                return;
            }
            if self.frame.panels_shown && self.preview.focus && self.on_preview_action(action) {
                self.redraw.dirty = true;
                return;
            }
            if self.side_nav.focus && self.on_side_action(action) {
                self.redraw.dirty = true;
                return;
            }
            // In the Icons view the arrows and h j k l move across the grid.
            if self.view == ViewMode::Icons
                && !self.side_nav.focus
                && key.modifiers.is_empty()
                && let Some(step) = match (action, key.code) {
                    (Action::Parent, KeyCode::Left | KeyCode::Char('h')) => Some(-1isize),
                    (Action::Open, KeyCode::Right | KeyCode::Char('l')) => Some(1),
                    _ => None,
                }
            {
                self.sel_anchor = None;
                let to = (self.cursor as isize + step).max(0) as usize;
                self.set_cursor(to);
                self.redraw.dirty = true;
                return;
            }
            self.dispatch(action);
        }
    }

    /// Do what an action says. Keys, clicks on hints and menu entries all end here.
    pub fn dispatch(&mut self, action: Action) {
        use Action::*;
        self.redraw.dirty = true;
        // A file chooser does what looks and moves, and nothing that changes a file.
        if self.session.chooser.is_some() {
            if !self
                .session
                .chooser
                .as_ref()
                .is_some_and(|c| c.allows(action))
            {
                self.toast(ToastKind::Info, Key::ChooseRefused.tr(), 2);
                return;
            }
            match action {
                Action::Open => {
                    self.chooser_navigate();
                    return;
                }
                Action::ToggleMark if !self.chooser_may_mark() => return,
                _ => {}
            }
        }
        let page = self.viewport.rows.saturating_sub(1).max(1);
        if matches!(
            action,
            Up | Down
                | Parent
                | Back
                | Forward
                | Open
                | First
                | Last
                | PageUp
                | PageDown
                | HalfPageUp
                | HalfPageDown
        ) {
            self.sel_anchor = None;
        }
        // In the Icons view, up and down are a whole row, and a page is the tiles on screen.
        let (line, page, half) = if self.view == ViewMode::Icons {
            let cols = self.viewport.grid_cols.max(1);
            let screenful = (self.viewport.grid_rows.max(1) * cols)
                .saturating_sub(cols)
                .max(cols);
            (cols, screenful, screenful / 2)
        } else {
            (1, page, page / 2)
        };
        match action {
            Up => self.set_cursor(self.cursor.saturating_sub(line)),
            Down => self.set_cursor(self.cursor + line),
            Parent => self.go_parent(),
            Back => self.go_history(true),
            Forward => self.go_history(false),
            Open => self.enter(),
            First => self.set_cursor(0),
            Last => self.set_cursor(usize::MAX),
            PageUp => self.set_cursor(self.cursor.saturating_sub(page)),
            PageDown => self.set_cursor(self.cursor + page),
            HalfPageUp => self.set_cursor(self.cursor.saturating_sub(half)),
            HalfPageDown => self.set_cursor(self.cursor + half),
            GoHome => {
                let home = self.svc.home.clone();
                self.open_dir(home);
            }

            ToggleMark => self.toggle_mark(),
            SelectAll => self.mark_all(),
            SelectUp => self.extend_selection(self.cursor.saturating_sub(line)),
            SelectDown => self.extend_selection(self.cursor + line),
            SelectToFirst => self.extend_selection(0),
            SelectToLast => self.extend_selection(usize::MAX),
            ClearSelection => {
                if self.ops.cancel() {
                    self.toast(ToastKind::Info, Key::TCancelling.tr(), 3);
                } else if !self.marked.is_empty() {
                    self.clear_marks();
                } else if self.filter.is_some() {
                    self.set_filter(None);
                } else if self.frame.size_notice_visible {
                    // Nothing else for Esc to do: it closes the advice about the window size,
                    // which then stays closed for the session.
                    self.windows.size_notice_dismissed = true;
                }
            }

            Copy => self.yank(TransferMode::Copy),
            Cut => self.yank(TransferMode::Move),
            Paste => self.start_paste(),
            Trash => self.start_trash(false),
            DeletePermanently => self.start_trash(true),
            Rename => self.start_rename(),
            BulkRename => self.start_bulk_rename(),
            NewFolder => {
                if !self.refuse_in_archive(Key::WhatCreateFolder.tr())
                    && !self.refuse_in_tag_view(Key::WhatCreateFolder.tr())
                {
                    self.windows.modal = Some(Modal::Input(InputView::new(
                        InputKind::NewDir,
                        String::new(),
                    )))
                }
            }
            ExtractHere => self.start_extract(true),
            ExtractToFolder => self.start_extract(false),
            Compress => self.start_compress(),
            Undo => self.start_undo(None),
            Redo => self.start_redo(),
            History => {
                self.windows.modal = Some(Modal::History(HistoryView {
                    entries: Vec::new(),
                    selected: 0,
                    loading: true,
                }));
                self.svc.jobs.load_history();
            }

            Filter => self.start_bar(true),
            Palette => self.start_bar(false),
            Sort => self.set_sort(self.sort.with_key(self.sort.key.next())),
            SortReverse => self.set_sort(SortSpec {
                reverse: !self.sort.reverse,
                ..self.sort
            }),
            ToggleHidden => {
                let keep = self.cursor_name();
                self.settings.show_hidden = !self.settings.show_hidden;
                self.rebuild_visible();
                self.restore_cursor(keep);
                // A folder's preview lists what the list lists.
                self.preview.invalidate();
                self.request_preview();
            }
            ToggleHex => {
                if !self.preview.toggle_hex() {
                    self.toast(ToastKind::Info, Key::THexOnlyBinary.tr(), 2);
                }
            }
            PreviewDown => self.preview.scroll_down(5),
            PreviewUp => self.preview.scroll_up(5),
            Bookmark => self.toggle_bookmark(),
            SwitchPane => self.switch_pane(),
            ToggleSidebar => self.toggle_sidebar(),

            NewFile => {
                if !self.refuse_if_busy()
                    && !self.refuse_in_archive(Key::WhatCreateFile.tr())
                    && !self.refuse_in_tag_view(Key::WhatCreateFile.tr())
                {
                    self.windows.modal = Some(Modal::Input(InputView::new(
                        InputKind::NewFile,
                        String::new(),
                    )))
                }
            }
            Refresh => self.refresh(),
            FocusAddress => self.start_address_edit(),
            ToggleDetails => self.toggle_details(),
            ViewDetails => self.set_view(ViewMode::Details),
            ViewIcons => self.set_view(ViewMode::Icons),
            ViewColumns => self.set_view(ViewMode::Columns),
            ToggleTags => self.toggle_tags_column(),
            ToggleView => self.set_view(self.view.other()),
            Properties => self.open_properties(),
            OpenWith => self.start_open_with(),
            CopyPath => self.copy_path(),
            OpenTerminal => self.open_terminal_here(),
            ContextMenu => self.open_context_menu_at_cursor(),
            NewTab => self.new_tab(),
            CloseTab => self.close_tab(self.tabs.active),
            NextTab => self.next_tab(),
            PrevTab => self.prev_tab(),

            Update => self.start_update_action(),
            QuickLook => self.quick_look(),
            ShowPackage => self.show_package(),
            ShowInFolder => self.show_in_folder(),
            Help => {
                self.windows.help_scroll = 0;
                self.windows.modal = Some(Modal::Help);
            }
            Quit => self.quit(),
        }
    }

    // ------------------------------------------------------------------ the explorer's own actions

    /// Read the folder again.
    pub fn refresh(&mut self) {
        let generation = self.tabs.next_generation();
        self.load = LoadState::Loading(self.cwd.clone());
        self.svc.loader.load(self.cwd.clone(), generation);
        self.redraw.dirty = true;
    }

    pub fn set_view(&mut self, mode: ViewMode) {
        if self.view == mode {
            return;
        }
        self.view = mode;
        self.ensure_visible();
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
        self.viewport.grid_cols = cols;
        self.viewport.grid_rows = rows;
        if self.view == ViewMode::Icons {
            self.ensure_visible();
        }
    }

    fn start_open_with(&mut self) {
        let Some((path, is_dir)) = self.current().map(|e| (e.path.clone(), e.is_dir())) else {
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
    fn copy_path(&mut self) {
        let paths = self.targets();
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

    fn open_terminal_here(&mut self) {
        let dir = match &self.archive {
            Some(a) => a.archive.parent().map(Path::to_path_buf),
            None if self.tag_view().is_some() => {
                // Where the item under the cursor is.
                Some(
                    self.folder_for_new(
                        &self
                            .current()
                            .map(|e| e.path.clone())
                            .into_iter()
                            .collect::<Vec<_>>(),
                    ),
                )
            }
            None => Some(self.cwd.clone()),
        };
        let Some(dir) = dir else { return };
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        self.after_launch();
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
    fn open_context_menu_at_cursor(&mut self) {
        let at = self
            .frame
            .hits
            .find(&Target::Row(self.cursor))
            .map(|r| (r.x.saturating_add(6), r.y.saturating_add(1)))
            .unwrap_or((8, 6));
        let on_item = self.current().is_some();
        self.windows.modal = Some(Modal::Menu(self.context_menu(on_item, at)));
    }

    /// An outside request to stop (SIGTERM, closing the terminal): cancel and leave.
    pub fn shutdown(&mut self) {
        self.ops.cancel();
        self.session.should_quit = true;
    }

    // ------------------------------------------------------------------ redo

    pub(super) fn start_redo(&mut self) {
        if self.refuse_if_busy() {
            return;
        }
        let h = self.svc.jobs.plan_redo();
        self.begin_plan(Key::PlanningRedo.tr(), h);
    }

    // ------------------------------------------------------------------ filter

    /// Set (or clear) the folder filter and keep the cursor where it makes sense.
    pub(super) fn set_filter(&mut self, f: Option<FilterState>) {
        let keep = self.cursor_name();
        self.filter = f;
        self.rebuild_visible();
        let found = keep.as_ref().is_some_and(|n| {
            self.visible
                .iter()
                .any(|&i| &self.listing.all()[i].name == n)
        });
        if found {
            self.restore_cursor(keep);
        } else {
            self.cursor = 0;
            self.scroll = 0;
        }
        self.ensure_visible();
        self.request_preview();
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(mut f) = self.filter.take() else {
            return;
        };
        match key.code {
            KeyCode::Esc => return self.set_filter(None),
            KeyCode::Enter => {
                f.editing = false;
                if f.text.is_empty() {
                    return self.set_filter(None);
                }
                self.filter = Some(f);
                return;
            }
            KeyCode::Backspace => {
                if f.text.pop().is_none() {
                    return self.set_filter(None);
                }
            }
            KeyCode::Char('u') if ctrl => f.text.clear(),
            KeyCode::Down => {
                self.filter = Some(f);
                return self.dispatch(Action::Down);
            }
            KeyCode::Up => {
                self.filter = Some(f);
                return self.dispatch(Action::Up);
            }
            KeyCode::Char(c) if !ctrl && !alt => f.text.push(c),
            _ => {}
        }
        self.set_filter(Some(f));
    }

    // ------------------------------------------------------------------ extended selection

    /// Shift+arrows and Shift+click: select everything between where the selection
    /// started and `to`, on top of what was already selected before it started.
    pub(super) fn extend_selection(&mut self, to: usize) {
        if self.visible.is_empty() {
            return;
        }
        if self.sel_anchor.is_none() {
            self.sel_anchor = Some((self.cursor, self.marked.clone()));
        }
        self.set_cursor(to);
        let Some((anchor, base)) = self.sel_anchor.clone() else {
            return;
        };
        let (lo, hi) = (anchor.min(self.cursor), anchor.max(self.cursor));
        let mut marked = base;
        for v in lo..=hi {
            if let Some(e) = self.entry_at(v) {
                marked.insert(e.name.clone());
            }
        }
        self.marked = marked;
    }

    // ------------------------------------------------------------------ mouse

    pub fn on_mouse(&mut self, m: MouseEvent) {
        if !self.settings.mouse {
            return;
        }
        // Motion only matters for what is under the pointer in the sidebar (its names show
        // in the bottom bar when it is reduced to icons): redraw only when that changes.
        if m.kind == MouseEventKind::Moved {
            let over = match self.frame.hits.at(m.column, m.row) {
                Some(Target::Place(p)) => Some(p.clone()),
                _ => None,
            };
            if self.side_nav.hover_over(over) {
                self.redraw.dirty = true;
            }
            return;
        }
        // Drag events arrive in floods and mean nothing here: no redraw.
        if !matches!(
            m.kind,
            MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::Down(
                    MouseButton::Left | MouseButton::Right | MouseButton::Middle
                )
        ) {
            return;
        }
        self.redraw.dirty = true;
        let target = self.frame.hits.at(m.column, m.row).cloned();
        if self.frame.panels_shown
            && self.windows.modal.is_none()
            && matches!(
                m.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
            && self
                .frame
                .nav_area
                .contains(ratatui::layout::Position::new(m.column, m.row))
        {
            return self.scroll_nav(m.kind == MouseEventKind::ScrollDown);
        }
        match m.kind {
            MouseEventKind::ScrollUp => self.wheel(target, false),
            MouseEventKind::ScrollDown => self.wheel(target, true),
            MouseEventKind::Down(MouseButton::Left) => {
                self.click(target, m.modifiers, (m.column, m.row))
            }
            MouseEventKind::Down(MouseButton::Right) => self.right_click(target, m.column, m.row),
            MouseEventKind::Down(MouseButton::Middle) => {
                if self.windows.modal.is_none()
                    && let Some(Target::Tab(i) | Target::TabClose(i)) = target
                {
                    self.close_tab(i);
                }
            }
            _ => {}
        }
    }

    fn wheel(&mut self, target: Option<Target>, down: bool) {
        if let Some(modal) = &mut self.windows.modal {
            match modal {
                Modal::Palette(p) => p.move_by(if down { 1 } else { -1 }),
                Modal::OpenWith(v) => v.move_by(if down { 1 } else { -1 }),
                Modal::History(h) => {
                    h.selected = if down {
                        (h.selected + 1).min(h.entries.len().saturating_sub(1))
                    } else {
                        h.selected.saturating_sub(1)
                    }
                }
                Modal::Plan(pv) => {
                    pv.scroll = if down {
                        pv.scroll + WHEEL_STEP
                    } else {
                        pv.scroll.saturating_sub(WHEEL_STEP)
                    }
                }
                Modal::Help => {
                    if down {
                        self.windows.help_down(WHEEL_STEP);
                    } else {
                        self.windows.help_up(WHEEL_STEP);
                    }
                }
                Modal::Update(UpdateView::Offer { scroll, .. }) => {
                    *scroll = if down {
                        *scroll + WHEEL_STEP
                    } else {
                        scroll.saturating_sub(WHEEL_STEP)
                    }
                }
                Modal::Result(r) => {
                    r.scroll = if down {
                        r.scroll + WHEEL_STEP
                    } else {
                        r.scroll.saturating_sub(WHEEL_STEP)
                    }
                }
                _ => {}
            }
            return;
        }
        if target == Some(Target::Preview) {
            if down {
                self.preview.scroll_down(WHEEL_STEP);
            } else {
                self.preview.scroll_up(WHEEL_STEP);
            }
            return;
        }
        self.sel_anchor = None;
        // A turn of the wheel is three rows of the list, or one row of tiles.
        let step = if self.view == ViewMode::Icons {
            self.viewport.grid_cols.max(1)
        } else {
            WHEEL_STEP
        };
        let to = if down {
            self.cursor + step
        } else {
            self.cursor.saturating_sub(step)
        };
        self.set_cursor(to);
    }

    fn click(&mut self, target: Option<Target>, mods: KeyModifiers, at: (u16, u16)) {
        if self.windows.modal.is_some() {
            return self.click_in_modal(target);
        }
        if let Some(Target::Notice(id, hit)) = target {
            return self.click_notice(id, hit);
        }
        // A click anywhere but on the address field or its suggestions ends the editing.
        if self.windows.address.is_some() {
            match &target {
                Some(Target::Suggest(i)) => return self.pick_suggestion(*i),
                Some(Target::BarHit(i)) => return self.run_bar_hit(*i),
                Some(Target::Address) => return,
                _ => self.cancel_address_edit(),
            }
        }
        // The keyboard follows the click: into the preview box when that is what was clicked.
        self.preview.focus = self.frame.panels_shown && matches!(target, Some(Target::Preview));
        match target {
            Some(Target::TreeToggle(dir)) => self.toggle_tree(&dir),
            Some(Target::Row(i)) => {
                self.side_nav.focus = false;
                self.click_row(i, mods)
            }
            Some(Target::Place(path)) => {
                self.side_nav.focus = false;
                self.open_dir(path)
            }
            Some(Target::Crumb(path)) => self.open_dir(path),
            Some(Target::Check(i)) => {
                if i < self.visible.len() {
                    self.side_nav.focus = false;
                    self.set_cursor(i);
                    if let Some(name) = self.entry_at(i).map(|e| e.name.clone())
                        && !self.marked.remove(&name)
                    {
                        self.marked.insert(name);
                    }
                    self.sel_anchor = Some((i, self.marked.clone()));
                }
            }
            Some(Target::Tab(i)) => self.switch_tab(i),
            Some(Target::TabClose(i)) => self.close_tab(i),
            Some(Target::TabNew) => self.new_tab(),
            Some(Target::TabPrev) => self.prev_tab(),
            Some(Target::TabNext) => self.next_tab(),
            Some(Target::Address) => self.start_address_edit(),
            Some(Target::CheckUpdates) => self.check_updates_now(),
            Some(Target::Menu(kind)) => {
                let at = self
                    .frame
                    .hits
                    .find(&Target::Menu(kind))
                    .map(|r| (r.x, r.y + 1))
                    .unwrap_or((at.0, at.1 + 1));
                self.open_menu(kind, at);
            }
            Some(Target::ViewMode(mode)) => self.set_view(mode),
            Some(Target::FullPreview) => {
                if self.current().is_some() {
                    self.windows.modal = Some(Modal::FullPreview);
                }
            }
            Some(Target::CrumbMore(hidden)) => {
                // The folders folded into the "…", to pick one.
                let items = hidden
                    .into_iter()
                    .map(|p| {
                        let name = p
                            .file_name()
                            .map(rada_core::display::name)
                            .unwrap_or_else(|| rada_core::display::path(&p));
                        MenuItem::go(name, p)
                    })
                    .collect();
                self.windows.modal = Some(Modal::Menu(MenuView::new((at.0, at.1 + 1), items)));
            }
            Some(Target::Act(a)) => self.dispatch(a),
            Some(Target::SortBy(key)) => {
                let spec = if self.sort.key == key {
                    SortSpec {
                        reverse: !self.sort.reverse,
                        ..self.sort
                    }
                } else {
                    self.sort.with_key(key)
                };
                self.set_sort(spec);
            }
            Some(Target::List) => self.clear_marks(),
            _ => {}
        }
    }

    fn click_row(&mut self, i: usize, mods: KeyModifiers) {
        if i >= self.visible.len() {
            return;
        }
        let double = matches!(self.last_click, Some((t, r)) if r == i && t.elapsed() < self.settings.double_click);
        self.last_click = Some((Instant::now(), i));
        if mods.contains(KeyModifiers::CONTROL) {
            // Ctrl+click adds or removes one item and becomes the new anchor.
            self.set_cursor(i);
            if let Some(name) = self.entry_at(i).map(|e| e.name.clone())
                && !self.marked.remove(&name)
            {
                self.marked.insert(name);
            }
            self.sel_anchor = Some((i, self.marked.clone()));
        } else if mods.contains(KeyModifiers::SHIFT) {
            self.extend_selection(i);
        } else if double {
            self.last_click = None;
            self.set_cursor(i);
            self.enter();
        } else {
            self.clear_marks();
            self.set_cursor(i);
            self.sel_anchor = Some((i, BTreeSet::new()));
        }
    }

    fn click_in_modal(&mut self, target: Option<Target>) {
        match (&mut self.windows.modal, target) {
            (Some(Modal::Help), _) => self.windows.modal = None,
            (Some(Modal::Properties(_)), Some(Target::Key(_))) => self.windows.modal = None,
            (Some(Modal::Properties(_)), None) => self.windows.modal = None,
            (Some(Modal::FullPreview), _) => self.windows.modal = None,
            (Some(Modal::Menu(_)), Some(Target::MenuItem(i))) => {
                if let Some(Modal::Menu(mut m)) = self.windows.modal.take() {
                    if i < m.items.len() && m.items[i].enabled {
                        m.selected = i;
                        self.menu_activate(m);
                    } else {
                        // A disabled entry does nothing, and the menu stays.
                        self.windows.modal = Some(Modal::Menu(m));
                    }
                }
            }
            (Some(Modal::Menu(_)), _) => self.windows.modal = None,
            (Some(Modal::Palette(p)), Some(Target::PaletteRow(i))) => {
                if i < p.hits.len() {
                    p.selected = i;
                    self.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                }
            }
            (Some(Modal::OpenWith(v)), Some(Target::OpenWithRow(i))) => {
                if i < v.rows().len() {
                    v.select(i);
                    self.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                }
            }
            (Some(Modal::OpenWith(v)), Some(Target::OpenWithAlways)) => v.toggle_always(),
            (Some(Modal::History(h)), Some(Target::HistoryRow(i))) => {
                if h.selected == i {
                    self.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                } else if i < h.entries.len() {
                    h.selected = i;
                }
            }
            (Some(Modal::Plan(_)), Some(Target::PlanCancel)) => self.windows.modal = None,
            (Some(Modal::Plan(_)), Some(Target::PlanRun)) => {
                if let Some(Modal::Plan(pv)) = self.windows.modal.take() {
                    if pv.can_run() {
                        self.run_plan(pv);
                    } else {
                        self.windows.modal = Some(Modal::Plan(pv));
                    }
                }
            }
            (Some(Modal::Plan(_)), Some(Target::Policy(p))) => {
                if let Some(Modal::Plan(mut pv)) = self.windows.modal.take() {
                    self.replan_with(&mut pv, p);
                    self.windows.modal = Some(Modal::Plan(pv));
                }
            }
            (Some(_), Some(Target::Key(code))) => {
                self.on_key(KeyEvent::new(code, KeyModifiers::NONE));
            }
            _ => {}
        }
    }

    fn right_click(&mut self, target: Option<Target>, x: u16, y: u16) {
        if self.session.chooser.is_some() {
            return;
        }
        if self.windows.modal.is_some() {
            if matches!(self.windows.modal, Some(Modal::Menu(_))) {
                self.windows.modal = None;
            }
            return;
        }
        if let Some(Target::Place(path)) = &target {
            self.windows.modal = Some(Modal::Menu(self.side_menu(path, (x, y))));
            return;
        }
        let on_item = match target {
            Some(Target::Row(i) | Target::Check(i)) if i < self.visible.len() => {
                let already = self
                    .entry_at(i)
                    .is_some_and(|e| self.marked.contains(&e.name));
                if !already {
                    self.clear_marks();
                }
                self.set_cursor(i);
                true
            }
            Some(Target::List) | Some(Target::Preview) => false,
            _ => return,
        };
        self.windows.modal = Some(Modal::Menu(self.context_menu(on_item, (x, y))));
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
                if let Some(path) = self.current().map(|e| e.path.clone()) {
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

    // ------------------------------------------------------------------ messages

    /// The shortest key for an action, for messages: vim's single letter when the vim
    /// keys are on, else the classic shortcut.
    pub fn key_for(&self, action: Action) -> Option<String> {
        let list = self.settings.keymap.bindings(action);
        list.iter()
            .filter(|b| b.vim && !b.classic || b.custom)
            .map(|b| b.chord.to_string())
            .min_by_key(|s| s.len())
            .or_else(|| self.settings.keymap.hint(action))
    }

    /// Is the vim scheme on? (Shown in the help footer.)
    pub fn has_scheme(&self, s: Scheme) -> bool {
        Action::ALL.iter().any(|a| {
            self.settings.keymap.bindings(*a).iter().any(|b| match s {
                Scheme::Vim => b.vim,
                Scheme::Classic => b.classic,
            })
        })
    }
}
