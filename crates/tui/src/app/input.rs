//! Keyboard and mouse: from key chords and clicks to [`Action`]s, and what each action does.

use crossterm::event::{KeyEventKind, MouseButton, MouseEvent, MouseEventKind};

use super::*;
use crate::keymap::Scheme;

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
    /// Open another menu in its place.
    Sub(MenuKind),
    SortBy(SortKey),
    /// `true`: descending.
    SortOrder(bool),
    View(ViewMode),
}

pub struct MenuItem {
    pub cmd: MenuCmd,
    pub label: String,
    pub enabled: bool,
    /// Draw a separator above this entry.
    pub gap_before: bool,
    /// A choice or a switch: whether it is on (drawn with a mark).
    pub checked: Option<bool>,
}

impl MenuItem {
    pub fn act(action: Action, enabled: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            cmd: MenuCmd::Act(action),
            label: action.label().to_string(),
            enabled,
            gap_before,
            checked: None,
        }
    }

    pub fn go(label: String, path: PathBuf) -> MenuItem {
        MenuItem {
            cmd: MenuCmd::Go(path),
            label,
            enabled: true,
            gap_before: false,
            checked: None,
        }
    }

    pub fn bookmark(path: PathBuf, add: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            cmd: MenuCmd::Bookmark { path, add },
            label: if add {
                "Pin to navigation".into()
            } else {
                "Unpin from navigation".into()
            },
            enabled: true,
            gap_before,
            checked: None,
        }
    }

    /// One of several exclusive choices.
    pub fn choice(cmd: MenuCmd, label: &str, on: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            cmd,
            label: label.to_string(),
            enabled: true,
            gap_before,
            checked: Some(on),
        }
    }

    /// An action that is a switch, shown with its state.
    pub fn toggle(action: Action, label: &str, on: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            cmd: MenuCmd::Act(action),
            label: label.to_string(),
            enabled: true,
            gap_before,
            checked: Some(on),
        }
    }

    /// An entry that opens another menu.
    pub fn sub(kind: MenuKind, label: &str, enabled: bool, gap_before: bool) -> MenuItem {
        MenuItem {
            cmd: MenuCmd::Sub(kind),
            label: label.to_string(),
            enabled,
            gap_before,
            checked: None,
        }
    }
}

/// A context menu opened with the right mouse button.
pub struct MenuView {
    pub at: (u16, u16),
    pub items: Vec<MenuItem>,
    pub selected: usize,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const WHEEL_STEP: usize = 3;

impl App {
    // ------------------------------------------------------------------ keyboard

    pub fn on_key(&mut self, key: KeyEvent) {
        self.dirty = true;
        if key.kind == KeyEventKind::Release {
            return;
        }
        if self.modal.is_some() {
            self.on_modal_key(key);
            return;
        }
        if self.address.is_some() {
            self.on_address_key(key);
            return;
        }
        if self.filter.as_ref().is_some_and(|f| f.editing) {
            self.on_filter_key(key);
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
        if let Some(action) = self.keymap.action_for(&key) {
            if self.side_focus && self.on_side_action(action) {
                self.dirty = true;
                return;
            }
            // In the Icons view the arrows and h j k l move across the grid.
            if self.view == ViewMode::Icons
                && !self.side_focus
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
                self.dirty = true;
                return;
            }
            self.dispatch(action);
        }
    }

    /// Do what an action says. Keys, clicks on hints and menu entries all end here.
    pub fn dispatch(&mut self, action: Action) {
        use Action::*;
        self.dirty = true;
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
                if let Some(r) = &self.running {
                    r.handle.cancel.cancel();
                    self.toast(ToastKind::Info, "cancelling…", 3);
                } else if !self.marked.is_empty() {
                    self.clear_marks();
                } else if self.filter.is_some() {
                    self.set_filter(None);
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
                if !self.refuse_in_archive("create a folder") {
                    self.modal = Some(Modal::Input(InputView::new(
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
                self.modal = Some(Modal::History(HistoryView {
                    entries: Vec::new(),
                    selected: 0,
                    loading: true,
                }));
                self.svc.jobs.load_history();
            }

            Filter => {
                let text = self.filter.take().map(|f| f.text).unwrap_or_default();
                self.filter = Some(FilterState {
                    text,
                    editing: true,
                });
            }
            Palette => self.open_palette(),
            Sort => self.set_sort(SortSpec {
                key: self.sort.key.next(),
                ..self.sort
            }),
            SortReverse => self.set_sort(SortSpec {
                reverse: !self.sort.reverse,
                ..self.sort
            }),
            ToggleHidden => {
                let keep = self.cursor_name();
                self.show_hidden = !self.show_hidden;
                self.rebuild_visible();
                self.restore_cursor(keep);
                self.request_preview();
            }
            ToggleHex => {
                if matches!(self.preview.content, Some(Preview::Binary(_))) {
                    self.preview.hex = !self.preview.hex;
                    self.preview.scroll = 0;
                } else {
                    self.toast(
                        ToastKind::Info,
                        "the hex dump is available for binary files",
                        2,
                    );
                }
            }
            PreviewDown => self.preview.scroll = self.preview.scroll.saturating_add(5),
            PreviewUp => self.preview.scroll = self.preview.scroll.saturating_sub(5),
            Bookmark => self.toggle_bookmark(),
            SwitchPane => self.switch_pane(),
            ToggleSidebar => self.toggle_sidebar(),

            NewFile => {
                if !self.refuse_if_busy() && !self.refuse_in_archive("create a file") {
                    self.modal = Some(Modal::Input(InputView::new(
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
            ToggleView => self.set_view(self.view.other()),
            Properties => self.open_properties(),
            OpenWith => self.start_open_with(),
            CopyPath => self.copy_path(),
            OpenTerminal => self.open_terminal_here(),
            ContextMenu => self.open_context_menu_at_cursor(),
            NewTab => self.new_tab(),
            CloseTab => self.close_tab(self.active_tab),
            NextTab => self.next_tab(),
            PrevTab => self.prev_tab(),

            Help => {
                self.help_scroll = 0;
                self.modal = Some(Modal::Help);
            }
            Quit => self.quit(),
        }
    }

    // ------------------------------------------------------------------ the explorer's own actions

    /// Read the folder again.
    pub fn refresh(&mut self) {
        self.dir_gen += 1;
        self.load = LoadState::Loading(self.cwd.clone());
        self.svc.loader.load(self.cwd.clone(), self.dir_gen);
        self.dirty = true;
    }

    pub fn set_view(&mut self, mode: ViewMode) {
        if self.view == mode {
            return;
        }
        self.view = mode;
        self.ensure_visible();
        self.save_ui_state();
        self.dirty = true;
    }

    /// Whether the last frame showed the details pane.
    pub fn details_visible(&self) -> bool {
        self.details_shown != crate::ui::DetailsMode::Hidden
    }

    pub fn toggle_details(&mut self) {
        let want = !self.details_visible();
        self.details = Some(want);
        self.saved_ui.details = Some(want);
        self.save_ui_state();
        if want && self.term_width < 60 {
            self.toast(
                ToastKind::Info,
                "details pane on — this terminal is too narrow to show it",
                3,
            );
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
            self.toast(ToastKind::Info, "Open with… is for files", 3);
            return;
        }
        if self.refuse_in_archive("open with a program") {
            return;
        }
        self.modal = Some(Modal::Input(InputView::new(
            InputKind::OpenWith { path },
            String::new(),
        )));
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
        self.copied_text = Some(text);
        self.toast(
            ToastKind::Ok,
            if paths.len() == 1 {
                "path copied"
            } else {
                "paths copied"
            },
            2,
        );
    }

    fn open_terminal_here(&mut self) {
        let dir = match &self.archive {
            Some(a) => a.archive.parent().map(Path::to_path_buf),
            None => Some(self.cwd.clone()),
        };
        let Some(dir) = dir else { return };
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
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
            .hits
            .find(&Target::Row(self.cursor))
            .map(|r| (r.x.saturating_add(6), r.y.saturating_add(1)))
            .unwrap_or((8, 6));
        let on_item = self.current().is_some();
        self.modal = Some(Modal::Menu(self.context_menu(on_item, at)));
    }

    /// An outside request to stop (SIGTERM, closing the terminal): cancel and leave.
    pub fn shutdown(&mut self) {
        if let Some(r) = &self.running {
            r.handle.cancel.cancel();
        }
        self.should_quit = true;
    }

    // ------------------------------------------------------------------ redo

    pub(super) fn start_redo(&mut self) {
        if self.refuse_if_busy() {
            return;
        }
        let h = self.svc.jobs.plan_redo();
        self.begin_plan("Planning redo", h);
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
        if !self.mouse {
            return;
        }
        // Motion only matters for what is under the pointer in the sidebar (its names show
        // in the bottom bar when it is reduced to icons): redraw only when that changes.
        if m.kind == MouseEventKind::Moved {
            let over = match self.hits.at(m.column, m.row) {
                Some(Target::Place(p)) => Some(p.clone()),
                _ => None,
            };
            if over != self.hover {
                self.hover = over;
                self.dirty = true;
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
        self.dirty = true;
        let target = self.hits.at(m.column, m.row).cloned();
        match m.kind {
            MouseEventKind::ScrollUp => self.wheel(target, false),
            MouseEventKind::ScrollDown => self.wheel(target, true),
            MouseEventKind::Down(MouseButton::Left) => {
                self.click(target, m.modifiers, (m.column, m.row))
            }
            MouseEventKind::Down(MouseButton::Right) => self.right_click(target, m.column, m.row),
            MouseEventKind::Down(MouseButton::Middle) => {
                if self.modal.is_none()
                    && let Some(Target::Tab(i) | Target::TabClose(i)) = target
                {
                    self.close_tab(i);
                }
            }
            _ => {}
        }
    }

    fn wheel(&mut self, target: Option<Target>, down: bool) {
        if let Some(modal) = &mut self.modal {
            match modal {
                Modal::Palette(p) => p.move_by(if down { 1 } else { -1 }),
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
                    self.help_scroll = if down {
                        self.help_scroll + WHEEL_STEP
                    } else {
                        self.help_scroll.saturating_sub(WHEEL_STEP)
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
            self.preview.scroll = if down {
                self.preview.scroll.saturating_add(WHEEL_STEP)
            } else {
                self.preview.scroll.saturating_sub(WHEEL_STEP)
            };
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
        if self.modal.is_some() {
            return self.click_in_modal(target);
        }
        // A click anywhere but on the address field or its suggestions ends the editing.
        if self.address.is_some() {
            match &target {
                Some(Target::Suggest(i)) => return self.pick_suggestion(*i),
                Some(Target::Address) => return,
                _ => self.cancel_address_edit(),
            }
        }
        match target {
            Some(Target::Row(i)) => {
                self.side_focus = false;
                self.click_row(i, mods)
            }
            Some(Target::Place(path)) => {
                self.side_focus = false;
                self.open_dir(path)
            }
            Some(Target::Crumb(path)) => self.open_dir(path),
            Some(Target::Check(i)) => {
                if i < self.visible.len() {
                    self.side_focus = false;
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
            Some(Target::Menu(kind)) => {
                let at = self
                    .hits
                    .find(&Target::Menu(kind))
                    .map(|r| (r.x, r.y + 1))
                    .unwrap_or((at.0, at.1 + 1));
                self.open_menu(kind, at);
            }
            Some(Target::ViewMode(mode)) => self.set_view(mode),
            Some(Target::FullPreview) => {
                if self.current().is_some() {
                    self.modal = Some(Modal::FullPreview);
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
                self.modal = Some(Modal::Menu(MenuView {
                    at: (at.0, at.1 + 1),
                    items,
                    selected: 0,
                }));
            }
            Some(Target::Act(a)) => self.dispatch(a),
            Some(Target::SortBy(key)) => {
                let spec = if self.sort.key == key {
                    SortSpec {
                        reverse: !self.sort.reverse,
                        ..self.sort
                    }
                } else {
                    SortSpec { key, ..self.sort }
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
        let double =
            matches!(self.last_click, Some((t, r)) if r == i && t.elapsed() < DOUBLE_CLICK);
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
        match (&mut self.modal, target) {
            (Some(Modal::Help), _) => self.modal = None,
            (Some(Modal::Properties(_)), Some(Target::Key(_))) => self.modal = None,
            (Some(Modal::Properties(_)), None) => self.modal = None,
            (Some(Modal::FullPreview), _) => self.modal = None,
            (Some(Modal::Menu(m)), Some(Target::MenuItem(i))) => {
                let at = (
                    m.at.0.saturating_add(2),
                    m.at.1.saturating_add(1 + i as u16),
                );
                let item = m.items.get(i).map(|it| (it.cmd.clone(), it.enabled));
                self.modal = None;
                if let Some((cmd, true)) = item {
                    self.run_menu(cmd, at);
                }
            }
            (Some(Modal::Menu(_)), _) => self.modal = None,
            (Some(Modal::Palette(p)), Some(Target::PaletteRow(i))) => {
                if i < p.hits.len() {
                    p.selected = i;
                    self.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                }
            }
            (Some(Modal::History(h)), Some(Target::HistoryRow(i))) => {
                if h.selected == i {
                    self.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                } else if i < h.entries.len() {
                    h.selected = i;
                }
            }
            (Some(Modal::Plan(_)), Some(Target::Policy(p))) => {
                if let Some(Modal::Plan(mut pv)) = self.modal.take() {
                    self.replan_with(&mut pv, p);
                    self.modal = Some(Modal::Plan(pv));
                }
            }
            (Some(_), Some(Target::Key(code))) => {
                self.on_key(KeyEvent::new(code, KeyModifiers::NONE));
            }
            _ => {}
        }
    }

    fn right_click(&mut self, target: Option<Target>, x: u16, y: u16) {
        if self.modal.is_some() {
            if matches!(self.modal, Some(Modal::Menu(_))) {
                self.modal = None;
            }
            return;
        }
        if let Some(Target::Place(path)) = &target {
            self.modal = Some(Modal::Menu(self.side_menu(path, (x, y))));
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
        self.modal = Some(Modal::Menu(self.context_menu(on_item, (x, y))));
    }

    pub(super) fn on_menu_key(&mut self, mut m: MenuView, key: KeyEvent) {
        let n = m.items.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {}
            KeyCode::Down | KeyCode::Char('j') => {
                m.selected = (m.selected + 1).min(n.saturating_sub(1));
                self.modal = Some(Modal::Menu(m));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                m.selected = m.selected.saturating_sub(1);
                self.modal = Some(Modal::Menu(m));
            }
            KeyCode::Enter => {
                if let Some(it) = m.items.get(m.selected) {
                    if it.enabled {
                        let cmd = it.cmd.clone();
                        let at = (
                            m.at.0.saturating_add(2),
                            m.at.1.saturating_add(1 + m.selected as u16),
                        );
                        self.run_menu(cmd, at);
                    } else {
                        self.modal = Some(Modal::Menu(m));
                    }
                }
            }
            _ => self.modal = Some(Modal::Menu(m)),
        }
    }

    // ------------------------------------------------------------------ messages

    /// "Copied 342 files — press u to undo"
    pub(super) fn done_message(&self, r: &Running) -> String {
        let t = &r.totals;
        let what = |files_first: bool| {
            if files_first && t.files > 0 {
                fmt::count(t.files, "file", "files")
            } else if files_first && t.dirs > 0 {
                fmt::count(t.dirs, "folder", "folders")
            } else {
                fmt::count(t.items.max(1), "item", "items")
            }
        };
        let base = match r.kind {
            OpKind::Copy => format!("Copied {}", what(true)),
            OpKind::Move => format!("Moved {}", what(true)),
            OpKind::Rename | OpKind::BulkRename => {
                format!("Renamed {}", fmt::count(t.items.max(1), "item", "items"))
            }
            OpKind::MakeDir => "Folder created".to_string(),
            OpKind::MakeFile => "File created".to_string(),
            OpKind::Trash => format!("Moved {} to the trash", what(false)),
            OpKind::Delete => format!("Deleted {} permanently", what(false)),
            OpKind::Undo => "Undone".to_string(),
            OpKind::Extract => format!("Extracted {}", what(true)),
            OpKind::Compress => "Archive created".to_string(),
        };
        let tail = match r.kind {
            OpKind::Undo => self
                .key_for(Action::Redo)
                .map(|k| format!("press {k} to redo")),
            _ if r.reversible => self
                .key_for(Action::Undo)
                .map(|k| format!("press {k} to undo")),
            _ => None,
        };
        match tail {
            Some(t) => format!("{base} — {t}"),
            None => base,
        }
    }

    /// The shortest key for an action, for messages: vim's single letter when the vim
    /// keys are on, else the classic shortcut.
    pub fn key_for(&self, action: Action) -> Option<String> {
        let list = self.keymap.bindings(action);
        list.iter()
            .filter(|b| b.vim && !b.classic || b.custom)
            .map(|b| b.chord.to_string())
            .min_by_key(|s| s.len())
            .or_else(|| self.keymap.hint(action))
    }

    /// Is the vim scheme on? (Shown in the help footer.)
    pub fn has_scheme(&self, s: Scheme) -> bool {
        Action::ALL.iter().any(|a| {
            self.keymap.bindings(*a).iter().any(|b| match s {
                Scheme::Vim => b.vim,
                Scheme::Classic => b.classic,
            })
        })
    }
}
