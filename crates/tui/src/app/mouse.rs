//! The mouse: what a click, a double click, the wheel and the right button do.

use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use rada_core::model::{SortKey, SortSpec};
use rada_core::ops::ConflictPolicy;

use super::{App, MenuItem, MenuKind, MenuView, Modal, UpdateView};
use crate::hits::Target;
use crate::view::ViewMode;

const WHEEL_STEP: usize = 3;

/// A button went down or the wheel turned: the events that mean something here.
fn is_press_or_wheel(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::Down(MouseButton::Left | MouseButton::Right | MouseButton::Middle)
    )
}

/// Where a text scrolled to `at` is after one turn of the wheel.
fn wheel_scrolled(at: usize, down: bool) -> usize {
    if down {
        at + WHEEL_STEP
    } else {
        at.saturating_sub(WHEEL_STEP)
    }
}

impl App {
    pub fn on_mouse(&mut self, m: MouseEvent) {
        if !self.settings.mouse {
            return;
        }
        // Motion only matters for what is under the pointer (it is lit, and the names of the
        // sidebar show in the bottom bar when it is reduced to icons): redraw only when that
        // changes.
        if m.kind == MouseEventKind::Moved {
            self.hover(m.column, m.row);
            return;
        }
        // Drag events arrive in floods and mean nothing here: no redraw.
        if !is_press_or_wheel(m.kind) {
            return;
        }
        self.redraw.dirty = true;
        let target = self.frame.hits.at(m.column, m.row).cloned();
        if self.wheel_over_nav(&m) {
            return self.scroll_nav(m.kind == MouseEventKind::ScrollDown);
        }
        match m.kind {
            MouseEventKind::ScrollUp => self.wheel(target, false),
            MouseEventKind::ScrollDown => self.wheel(target, true),
            MouseEventKind::Down(MouseButton::Left) => {
                self.click(target, m.modifiers, (m.column, m.row))
            }
            MouseEventKind::Down(MouseButton::Right) => self.right_click(target, m.column, m.row),
            MouseEventKind::Down(MouseButton::Middle) => self.middle_click(target),
            _ => {}
        }
    }

    /// The middle button on a tab closes it.
    fn middle_click(&mut self, target: Option<Target>) {
        if self.windows.modal.is_none()
            && let Some(Target::Tab(i) | Target::TabClose(i)) = target
        {
            self.close_tab(i);
        }
    }

    fn hover(&mut self, column: u16, row: u16) {
        let under = self.frame.hits.at(column, row).and_then(Target::lit);
        if self.hover.moved(column, row, under) {
            self.redraw.dirty = true;
        }
    }

    /// The wheel turned over the NAV box, with nothing open over it.
    fn wheel_over_nav(&self, m: &MouseEvent) -> bool {
        self.frame.panels_shown
            && self.windows.modal.is_none()
            && matches!(
                m.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
            && self
                .frame
                .nav_area
                .contains(ratatui::layout::Position::new(m.column, m.row))
    }

    fn wheel(&mut self, target: Option<Target>, down: bool) {
        if self.windows.modal.is_some() {
            return self.wheel_in_modal(down);
        }
        if target == Some(Target::Preview) {
            if down {
                self.preview.scroll_down(WHEEL_STEP);
            } else {
                self.preview.scroll_up(WHEEL_STEP);
            }
            return;
        }
        self.tab.sel.drop_anchor();
        // A turn of the wheel is three rows of the list, or one row of tiles.
        let step = if self.tab.view == ViewMode::Icons {
            self.tab.viewport.grid_cols.max(1)
        } else {
            WHEEL_STEP
        };
        let to = if down {
            self.tab.sel.cursor + step
        } else {
            self.tab.sel.cursor.saturating_sub(step)
        };
        self.set_cursor(to);
    }

    fn wheel_in_modal(&mut self, down: bool) {
        let Some(modal) = &mut self.windows.modal else {
            return;
        };
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
            Modal::Plan(pv) => pv.scroll = wheel_scrolled(pv.scroll, down),
            Modal::Help => {
                if down {
                    self.windows.help_down(WHEEL_STEP);
                } else {
                    self.windows.help_up(WHEEL_STEP);
                }
            }
            Modal::Update(UpdateView::Offer { scroll, .. }) => {
                *scroll = wheel_scrolled(*scroll, down)
            }
            Modal::Result(r) => r.scroll = wheel_scrolled(r.scroll, down),
            _ => {}
        }
    }

    pub(super) fn click(&mut self, target: Option<Target>, mods: KeyModifiers, at: (u16, u16)) {
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
        self.click_target(target, mods, at);
    }

    fn click_target(&mut self, target: Option<Target>, mods: KeyModifiers, at: (u16, u16)) {
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
            Some(Target::Check(i)) => self.click_check(i),
            Some(Target::Tab(i)) => self.switch_tab(i),
            Some(Target::TabClose(i)) => self.close_tab(i),
            Some(Target::TabNew) => self.new_tab(),
            Some(Target::TabPrev) => self.prev_tab(),
            Some(Target::TabNext) => self.next_tab(),
            Some(Target::Address) => self.start_address_edit(),
            Some(Target::CheckUpdates) => self.check_updates_now(),
            Some(Target::Menu(kind)) => self.click_menu_title(kind, at),
            Some(Target::ViewMode(mode)) => self.set_view(mode),
            Some(Target::FullPreview) => {
                if self.tab.current().is_some() {
                    self.windows.modal = Some(Modal::FullPreview);
                }
            }
            Some(Target::CrumbMore(hidden)) => self.click_crumb_more(hidden, at),
            Some(Target::Act(a)) => self.dispatch(a),
            Some(Target::SortBy(key)) => self.click_sort_title(key),
            Some(Target::List) => self.tab.sel.clear_marks(),
            _ => {}
        }
    }

    fn click_check(&mut self, i: usize) {
        if i < self.tab.visible.len() {
            self.side_nav.focus = false;
            self.set_cursor(i);
            self.toggle_mark_at(i);
        }
    }

    /// A title of the menu bar: its menu opens under it.
    fn click_menu_title(&mut self, kind: MenuKind, at: (u16, u16)) {
        let at = self
            .frame
            .hits
            .find(&Target::Menu(kind))
            .map(|r| (r.x, r.y + 1))
            .unwrap_or((at.0, at.1 + 1));
        self.open_menu(kind, at);
    }

    /// The "…" of a long path: the folders folded into it, to pick one.
    fn click_crumb_more(&mut self, hidden: Vec<PathBuf>, at: (u16, u16)) {
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

    /// A column title: sort by it, or the other way round if the list already is.
    fn click_sort_title(&mut self, key: SortKey) {
        let spec = if self.tab.sort.key == key {
            SortSpec {
                reverse: !self.tab.sort.reverse,
                ..self.tab.sort
            }
        } else {
            self.tab.sort.with_key(key)
        };
        self.set_sort(spec);
    }

    /// Mark or unmark row `i`, which becomes where a selection extended next starts.
    fn toggle_mark_at(&mut self, i: usize) {
        if let Some(name) = self.tab.entry_at(i).map(|e| e.name.clone()) {
            self.tab.sel.toggle(name);
        }
        self.tab.sel.anchor_at(i);
    }

    fn click_row(&mut self, i: usize, mods: KeyModifiers) {
        if i >= self.tab.visible.len() {
            return;
        }
        let double = matches!(self.last_click, Some((t, r)) if r == i && t.elapsed() < self.settings.double_click);
        self.last_click = Some((Instant::now(), i));
        if mods.contains(KeyModifiers::CONTROL) {
            // Ctrl+click adds or removes one item and becomes the new anchor.
            self.set_cursor(i);
            self.toggle_mark_at(i);
        } else if mods.contains(KeyModifiers::SHIFT) {
            self.extend_selection(i);
        } else if double {
            self.last_click = None;
            self.set_cursor(i);
            self.enter();
        } else {
            self.tab.sel.clear_marks();
            self.set_cursor(i);
            self.tab.sel.anchor_alone(i);
        }
    }

    fn click_in_modal(&mut self, target: Option<Target>) {
        match (&mut self.windows.modal, target) {
            (Some(Modal::Help), _) => self.windows.modal = None,
            (Some(Modal::Properties(_)), Some(Target::Key(_))) => self.windows.modal = None,
            (Some(Modal::Properties(_)), None) => self.windows.modal = None,
            (Some(Modal::FullPreview), _) => self.windows.modal = None,
            (Some(Modal::Menu(_)), Some(Target::MenuItem(i))) => self.click_menu_item(i),
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
            (Some(Modal::Plan(_)), Some(Target::PlanRun)) => self.click_plan_run(),
            (Some(Modal::Plan(_)), Some(Target::Policy(p))) => self.click_plan_policy(p),
            (Some(_), Some(Target::Key(code))) => {
                self.on_key(KeyEvent::new(code, KeyModifiers::NONE));
            }
            _ => {}
        }
    }

    fn click_menu_item(&mut self, i: usize) {
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

    fn click_plan_run(&mut self) {
        if let Some(Modal::Plan(pv)) = self.windows.modal.take() {
            if pv.can_run() {
                self.run_plan(pv);
            } else {
                self.windows.modal = Some(Modal::Plan(pv));
            }
        }
    }

    fn click_plan_policy(&mut self, policy: ConflictPolicy) {
        if let Some(Modal::Plan(mut pv)) = self.windows.modal.take() {
            self.replan_with(&mut pv, policy);
            self.windows.modal = Some(Modal::Plan(pv));
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
            Some(Target::Row(i) | Target::Check(i)) if i < self.tab.visible.len() => {
                let already = self
                    .tab
                    .entry_at(i)
                    .is_some_and(|e| self.tab.sel.marked.contains(&e.name));
                if !already {
                    self.tab.sel.clear_marks();
                }
                self.set_cursor(i);
                true
            }
            Some(Target::List) | Some(Target::Preview) => false,
            _ => return,
        };
        self.windows.modal = Some(Modal::Menu(self.context_menu(on_item, (x, y))));
    }
}
