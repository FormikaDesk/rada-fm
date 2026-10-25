//! Keyboard and mouse: from key chords and clicks to [`Action`]s, and what each action does.

use crossterm::event::KeyEventKind;

use super::{App, ToastKind};
use crate::keymap::{Action, Scheme};
use crate::view::ViewMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::i18n::Key;
use rada_core::model::SortSpec;
use rada_core::ops::TransferMode;
use std::ffi::OsString;

/// The "filter this folder" box: while `editing`, typed characters go into it.
pub struct FilterState {
    pub text: String,
    pub editing: bool,
}

/// Actions after which an extended selection is not extended any more.
fn moves_cursor(action: Action) -> bool {
    use Action::*;
    matches!(
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
    )
}

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
        if self.key_goes_to_card(key) || self.key_goes_to_open_input(key) {
            return;
        }
        if self.key_changes_tab_or_column(key) {
            return;
        }
        if let Some(action) = self.settings.keymap.action_for(&key) {
            self.on_action_key(key, action);
        }
    }

    /// The card at the bottom right goes at the first key (the key still does its job); an
    /// error stays until Esc, which it takes when nothing else is open.
    fn key_goes_to_card(&mut self, key: KeyEvent) -> bool {
        let now = self.notice_now();
        if self.windows.modal.is_none()
            && self.windows.address.is_none()
            && key.code == KeyCode::Esc
            && self.windows.notices.dismiss_error(now)
        {
            return true;
        }
        self.windows.notices.on_key(now);
        false
    }

    /// A window, the address bar, the filter box or the file chooser takes the key.
    fn key_goes_to_open_input(&mut self, key: KeyEvent) -> bool {
        if self.windows.modal.is_some() {
            self.on_modal_key(key);
            return true;
        }
        if self.windows.address.is_some() {
            self.on_address_key(key);
            return true;
        }
        if self.tab.filter.as_ref().is_some_and(|f| f.editing) {
            self.on_filter_key(key);
            return true;
        }
        self.session.chooser.is_some() && self.on_chooser_key(key)
    }

    /// Alt+1 … Alt+9 jump to a tab; in the Columns view Right goes into the folder under the
    /// cursor and does nothing on a file (Left, the parent action, already goes to the folder
    /// above).
    fn key_changes_tab_or_column(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::ALT)
            && !key.modifiers.contains(KeyModifiers::CONTROL)
            && let KeyCode::Char(c @ '1'..='9') = key.code
        {
            self.go_tab(c as usize - '0' as usize);
            return true;
        }
        if self.tab.view == ViewMode::Columns
            && !self.side_nav.focus
            && key.modifiers.is_empty()
            && matches!(key.code, KeyCode::Right | KeyCode::Char('l'))
            && self.settings.keymap.action_for(&key) == Some(Action::Open)
        {
            self.columns_right();
            self.redraw.dirty = true;
            return true;
        }
        false
    }

    fn on_action_key(&mut self, key: KeyEvent, action: Action) {
        if self.pane_takes_action(key, action) || self.step_across_grid(key, action) {
            return;
        }
        self.dispatch(action);
    }

    /// The NAV box, the preview or the sidebar has the keyboard and wants this action.
    fn pane_takes_action(&mut self, key: KeyEvent, action: Action) -> bool {
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
            return true;
        }
        if self.frame.panels_shown && self.preview.focus && self.on_preview_action(action) {
            self.redraw.dirty = true;
            return true;
        }
        if self.side_nav.focus && self.on_side_action(action) {
            self.redraw.dirty = true;
            return true;
        }
        false
    }

    /// In the Icons view the arrows and h j k l move across the grid.
    fn step_across_grid(&mut self, key: KeyEvent, action: Action) -> bool {
        if self.tab.view == ViewMode::Icons
            && !self.side_nav.focus
            && key.modifiers.is_empty()
            && let Some(step) = match (action, key.code) {
                (Action::Parent, KeyCode::Left | KeyCode::Char('h')) => Some(-1isize),
                (Action::Open, KeyCode::Right | KeyCode::Char('l')) => Some(1),
                _ => None,
            }
        {
            self.tab.sel.drop_anchor();
            let to = (self.tab.sel.cursor as isize + step).max(0) as usize;
            self.set_cursor(to);
            self.redraw.dirty = true;
            return true;
        }
        false
    }

    /// Do what an action says. Keys, clicks on hints and menu entries all end here.
    pub fn dispatch(&mut self, action: Action) {
        self.redraw.dirty = true;
        if self.chooser_gate(action) {
            return;
        }
        if moves_cursor(action) {
            self.tab.sel.drop_anchor();
        }
        let steps = self.cursor_steps();
        let _done = self.move_action(action, steps)
            || self.select_action(action, steps.0)
            || self.file_action(action)
            || self.view_action(action)
            || self.window_action(action);
    }

    /// A file chooser does what looks and moves, and nothing that changes a file. True when the
    /// action was refused or done here.
    fn chooser_gate(&mut self, action: Action) -> bool {
        if self.session.chooser.is_none() {
            return false;
        }
        if !self
            .session
            .chooser
            .as_ref()
            .is_some_and(|c| c.allows(action))
        {
            self.toast(ToastKind::Info, Key::ChooseRefused.tr(), 2);
            return true;
        }
        match action {
            Action::Open => {
                self.chooser_navigate();
                true
            }
            Action::ToggleMark => !self.chooser_may_mark(),
            _ => false,
        }
    }

    /// How far a line, a page and half a page move the cursor. In the Icons view, up and down
    /// are a whole row, and a page is the tiles on screen.
    fn cursor_steps(&self) -> (usize, usize, usize) {
        let page = self.tab.viewport.rows.saturating_sub(1).max(1);
        if self.tab.view == ViewMode::Icons {
            let cols = self.tab.viewport.grid_cols.max(1);
            let screenful = (self.tab.viewport.grid_rows.max(1) * cols)
                .saturating_sub(cols)
                .max(cols);
            (cols, screenful, screenful / 2)
        } else {
            (1, page, page / 2)
        }
    }

    /// Actions that move the cursor or the folder. False for any other action.
    fn move_action(&mut self, action: Action, (line, page, half): (usize, usize, usize)) -> bool {
        use Action::*;
        match action {
            Up => self.set_cursor(self.tab.sel.cursor.saturating_sub(line)),
            Down => self.set_cursor(self.tab.sel.cursor + line),
            Parent => self.go_parent(),
            Back => self.go_history(true),
            Forward => self.go_history(false),
            Open => self.enter(),
            First => self.set_cursor(0),
            Last => self.set_cursor(usize::MAX),
            PageUp => self.set_cursor(self.tab.sel.cursor.saturating_sub(page)),
            PageDown => self.set_cursor(self.tab.sel.cursor + page),
            HalfPageUp => self.set_cursor(self.tab.sel.cursor.saturating_sub(half)),
            HalfPageDown => self.set_cursor(self.tab.sel.cursor + half),
            GoHome => {
                let home = self.svc.home.clone();
                self.open_dir(home);
            }
            _ => return false,
        }
        true
    }

    /// Actions that mark entries. False for any other action.
    fn select_action(&mut self, action: Action, line: usize) -> bool {
        use Action::*;
        match action {
            ToggleMark => self.toggle_mark(),
            SelectAll => self.tab.mark_all(),
            SelectUp => self.extend_selection(self.tab.sel.cursor.saturating_sub(line)),
            SelectDown => self.extend_selection(self.tab.sel.cursor + line),
            SelectToFirst => self.extend_selection(0),
            SelectToLast => self.extend_selection(usize::MAX),
            ClearSelection => self.clear_or_cancel(),
            _ => return false,
        }
        true
    }

    /// Esc: stop the operation that runs, else drop the marks, else the filter, else close the
    /// advice about the window size.
    fn clear_or_cancel(&mut self) {
        if self.ops.cancel() {
            self.toast(ToastKind::Info, Key::TCancelling.tr(), 3);
        } else if !self.tab.sel.marked.is_empty() {
            self.tab.sel.clear_marks();
        } else if self.tab.filter.is_some() {
            self.set_filter(None);
        } else if self.frame.size_notice_visible {
            // Nothing else for Esc to do: it closes the advice about the window size,
            // which then stays closed for the session.
            self.windows.size_notice_dismissed = true;
        }
    }

    /// Actions that start an operation on files. False for any other action.
    fn file_action(&mut self, action: Action) -> bool {
        use Action::*;
        match action {
            Copy => self.yank(TransferMode::Copy),
            Cut => self.yank(TransferMode::Move),
            Paste => self.start_paste(),
            Trash => self.start_trash(false),
            DeletePermanently => self.start_trash(true),
            Rename => self.start_rename(),
            BulkRename => self.start_bulk_rename(),
            NewFolder => self.ask_new_folder(),
            NewFile => self.ask_new_file(),
            ExtractHere => self.start_extract(true),
            ExtractToFolder => self.start_extract(false),
            Compress => self.start_compress(),
            Undo => self.start_undo(None),
            Redo => self.start_redo(),
            History => self.open_history(),
            OpenWith => self.start_open_with(),
            CopyPath => self.copy_path(),
            _ => return false,
        }
        true
    }

    /// Actions that change how the folder is listed or shown. False for any other action.
    fn view_action(&mut self, action: Action) -> bool {
        use Action::*;
        match action {
            Filter => self.start_bar(true),
            Palette => self.start_bar(false),
            Sort => self.set_sort(self.tab.sort.with_key(self.tab.sort.key.next())),
            SortReverse => self.set_sort(SortSpec {
                reverse: !self.tab.sort.reverse,
                ..self.tab.sort
            }),
            ToggleHidden => self.toggle_hidden(),
            ToggleHex => self.toggle_hex(),
            PreviewDown => self.preview.scroll_down(5),
            PreviewUp => self.preview.scroll_up(5),
            Refresh => self.refresh(),
            ToggleDetails => self.toggle_details(),
            ViewDetails => self.set_view(ViewMode::Details),
            ViewIcons => self.set_view(ViewMode::Icons),
            ViewColumns => self.set_view(ViewMode::Columns),
            ToggleTags => self.toggle_tags_column(),
            ToggleView => self.set_view(self.tab.view.other()),
            _ => return false,
        }
        true
    }

    /// Actions about the bookmarks, the panes, the tabs and the windows. False for any other
    /// action.
    fn window_action(&mut self, action: Action) -> bool {
        use Action::*;
        match action {
            Bookmark => self.toggle_bookmark(),
            SwitchPane => self.switch_pane(),
            ToggleSidebar => self.toggle_sidebar(),
            FocusAddress => self.start_address_edit(),
            Properties => self.open_properties(),
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
            Help => self.open_help(),
            Quit => self.quit(),
            _ => return false,
        }
        true
    }

    // ------------------------------------------------------------------ the explorer's own actions

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
        let keep = self.tab.cursor_name();
        self.tab.filter = f;
        self.rebuild_visible();
        let found = keep.as_ref().is_some_and(|n| {
            self.tab
                .visible
                .iter()
                .any(|&i| &self.tab.listing.all()[i].name == n)
        });
        if found {
            self.tab.restore_cursor(keep);
        } else {
            self.tab.sel.cursor = 0;
            self.tab.sel.scroll = 0;
        }
        self.tab.ensure_visible();
        self.request_preview();
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(mut f) = self.tab.filter.take() else {
            return;
        };
        match key.code {
            KeyCode::Esc => return self.set_filter(None),
            KeyCode::Enter => {
                f.editing = false;
                if f.text.is_empty() {
                    return self.set_filter(None);
                }
                self.tab.filter = Some(f);
                return;
            }
            KeyCode::Backspace => {
                if f.text.pop().is_none() {
                    return self.set_filter(None);
                }
            }
            KeyCode::Char('u') if ctrl => f.text.clear(),
            KeyCode::Down => {
                self.tab.filter = Some(f);
                return self.dispatch(Action::Down);
            }
            KeyCode::Up => {
                self.tab.filter = Some(f);
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
        if self.tab.visible.is_empty() {
            return;
        }
        self.tab.sel.begin_extend();
        self.set_cursor(to);
        let Some(span) = self.tab.sel.span() else {
            return;
        };
        let names: Vec<OsString> = span
            .filter_map(|v| self.tab.entry_at(v).map(|e| e.name.clone()))
            .collect();
        self.tab.sel.mark_span(names);
    }

    // ------------------------------------------------------------------ mouse

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
