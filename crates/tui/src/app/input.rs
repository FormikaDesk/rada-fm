//! Keyboard and mouse: from key chords and clicks to [`Action`]s, and what each action does.

use crossterm::event::{KeyEventKind, MouseButton, MouseEvent, MouseEventKind};

use super::*;
use crate::keymap::Scheme;

/// The "filter this folder" box: while `editing`, typed characters go into it.
pub struct FilterState {
    pub text: String,
    pub editing: bool,
}

pub struct MenuItem {
    pub action: Action,
    pub enabled: bool,
    /// Draw a separator above this entry.
    pub gap_before: bool,
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
        if self.filter.as_ref().is_some_and(|f| f.editing) {
            self.on_filter_key(key);
            return;
        }
        if let Some(action) = self.keymap.action_for(&key) {
            self.dispatch(action);
        }
    }

    /// Do what an action says. Keys, clicks on hints and menu entries all end here.
    pub fn dispatch(&mut self, action: Action) {
        use Action::*;
        self.dirty = true;
        let page = self.view_rows.saturating_sub(1).max(1);
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
        match action {
            Up => self.set_cursor(self.cursor.saturating_sub(1)),
            Down => self.set_cursor(self.cursor + 1),
            Parent => self.go_parent(),
            Back => self.go_history(true),
            Forward => self.go_history(false),
            Open => self.enter(),
            First => self.set_cursor(0),
            Last => self.set_cursor(usize::MAX),
            PageUp => self.set_cursor(self.cursor.saturating_sub(page)),
            PageDown => self.set_cursor(self.cursor + page),
            HalfPageUp => self.set_cursor(self.cursor.saturating_sub(page / 2)),
            HalfPageDown => self.set_cursor(self.cursor + page / 2),
            GoHome => {
                let home = self.svc.home.clone();
                self.open_dir(home);
            }

            ToggleMark => self.toggle_mark(),
            SelectAll => self.mark_all(),
            SelectUp => self.extend_selection(self.cursor.saturating_sub(1)),
            SelectDown => self.extend_selection(self.cursor + 1),
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
                self.modal = Some(Modal::Input(InputView::new(
                    InputKind::NewDir,
                    String::new(),
                )))
            }
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

            Help => {
                self.help_scroll = 0;
                self.modal = Some(Modal::Help);
            }
            Quit => self.quit(),
        }
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
        // Motion and drag events arrive in floods and mean nothing here: no redraw.
        if !matches!(
            m.kind,
            MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
        ) {
            return;
        }
        self.dirty = true;
        let target = self.hits.at(m.column, m.row).cloned();
        match m.kind {
            MouseEventKind::ScrollUp => self.wheel(target, false),
            MouseEventKind::ScrollDown => self.wheel(target, true),
            MouseEventKind::Down(MouseButton::Left) => self.click(target, m.modifiers),
            MouseEventKind::Down(MouseButton::Right) => self.right_click(target, m.column, m.row),
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
        let to = if down {
            self.cursor + WHEEL_STEP
        } else {
            self.cursor.saturating_sub(WHEEL_STEP)
        };
        self.set_cursor(to);
    }

    fn click(&mut self, target: Option<Target>, mods: KeyModifiers) {
        if self.modal.is_some() {
            return self.click_in_modal(target);
        }
        match target {
            Some(Target::Row(i)) => self.click_row(i, mods),
            Some(Target::Crumb(path)) => self.open_dir(path),
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
            (Some(Modal::Menu(m)), Some(Target::MenuItem(i))) => {
                let item = m.items.get(i).map(|it| (it.action, it.enabled));
                self.modal = None;
                if let Some((action, true)) = item {
                    self.dispatch(action);
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
        let on_item = match target {
            Some(Target::Row(i)) if i < self.visible.len() => {
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

    pub(super) fn context_menu(&self, on_item: bool, at: (u16, u16)) -> MenuView {
        use Action::*;
        let has_clip = self.clipboard.is_some();
        let mut items: Vec<MenuItem> = Vec::new();
        let mut add = |action: Action, enabled: bool, gap_before: bool| {
            items.push(MenuItem {
                action,
                enabled,
                gap_before,
            })
        };
        if on_item {
            add(Open, true, false);
            add(Copy, true, true);
            add(Cut, true, false);
            add(Paste, has_clip, false);
            add(Rename, true, true);
            add(Trash, true, false);
            add(DeletePermanently, true, false);
            add(NewFolder, true, true);
            add(SelectAll, true, false);
        } else {
            add(Paste, has_clip, false);
            add(NewFolder, true, false);
            add(SelectAll, true, false);
        }
        add(Undo, true, true);
        add(Redo, true, false);
        add(Help, true, true);
        MenuView {
            at,
            items,
            selected: 0,
        }
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
                        self.dispatch(it.action);
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
            OpKind::Trash => format!("Moved {} to the trash", what(false)),
            OpKind::Delete => format!("Deleted {} permanently", what(false)),
            OpKind::Undo => "Undone".to_string(),
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
