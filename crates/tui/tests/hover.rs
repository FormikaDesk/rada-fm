//! The light highlight of what the pointer is over: rows, buttons, menu entries, tabs and the
//! rest, from the map of clicks; one state for the whole screen; a new frame only when what
//! is under the pointer changes.

mod common;

use common::*;
use crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};
use rada_tui::hits::Target;
use rada_tui::keymap::Action;
use rada_tui::theme::{ColorDepth, Theme};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

fn scene() -> H {
    let mut h = demo_scene(140, 35);
    h.keys("g");
    let _ = h.screen();
    h
}

fn buffer(h: &mut H) -> Buffer {
    let _ = h.screen();
    h.term.backend().buffer().clone()
}

fn area(h: &mut H, t: &Target) -> Rect {
    let _ = h.screen();
    h.app
        .frame
        .hits
        .find(t)
        .unwrap_or_else(|| panic!("{t:?} is not on screen"))
}

fn move_to(h: &mut H, x: u16, y: u16) {
    h.mouse(MouseEventKind::Moved, x, y, KeyModifiers::NONE);
}

/// Put the pointer on the middle of `t` and draw.
fn hover(h: &mut H, t: &Target) -> Rect {
    let r = area(h, t);
    move_to(h, r.x + r.width / 2, r.y);
    r
}

fn cells(r: Rect) -> impl Iterator<Item = (u16, u16)> {
    (r.y..r.bottom()).flat_map(move |y| (r.x..r.right()).map(move |x| (x, y)))
}

/// How many cells of `r` have the hover tone as their background.
fn lit(h: &mut H, r: Rect) -> usize {
    let mark = h.app.settings.th.mark;
    let b = buffer(h);
    cells(r).filter(|p| b[*p].bg == mark).count()
}

fn underlined(h: &mut H, r: Rect) -> usize {
    let b = buffer(h);
    cells(r)
        .filter(|p| b[*p].modifier.contains(Modifier::UNDERLINED))
        .count()
}

fn all_lit(h: &mut H, r: Rect) -> bool {
    lit(h, r) == (r.width * r.height) as usize
}

#[test]
fn a_row_of_the_list_is_lit_under_the_pointer() {
    let mut h = scene();
    let row = Target::Row(3);
    let r = area(&mut h, &row);
    assert_eq!(lit(&mut h, r), 0, "nothing is lit before the pointer comes");
    hover(&mut h, &row);
    assert!(all_lit(&mut h, r), "the whole row is lit");
    // Another row is not.
    let other = area(&mut h, &Target::Row(4));
    assert_eq!(lit(&mut h, other), 0);
    // Away from anything that can be clicked, nothing is lit.
    move_to(&mut h, 0, 0);
    assert_eq!(lit(&mut h, r), 0);
}

#[test]
fn a_checkbox_lights_the_row_it_belongs_to() {
    let mut h = scene();
    let r = area(&mut h, &Target::Row(3));
    hover(&mut h, &Target::Check(3));
    assert!(all_lit(&mut h, r));
}

#[test]
fn the_row_of_the_cursor_and_the_selected_rows_are_left_as_they_are() {
    let mut h = scene();
    let cursor = h.app.tab.sel.cursor;
    let r = hover(&mut h, &Target::Row(cursor));
    assert_eq!(lit(&mut h, r), 0, "the cursor row");
    // Select the next one with Space, which moves the cursor on: the marked row stays marked.
    h.key(KeyCode::Char(' '));
    let marked = Target::Row(cursor);
    let r = hover(&mut h, &marked);
    assert_eq!(h.marked_names().len(), 1);
    assert_eq!(lit(&mut h, r), 0, "a selected row");
}

#[test]
fn a_button_of_the_bar_is_lit() {
    let mut h = scene();
    let cut = Target::Act(Action::Cut);
    let r = hover(&mut h, &cut);
    assert!(all_lit(&mut h, r));
    let neighbour = area(&mut h, &Target::Act(Action::Copy));
    assert_eq!(lit(&mut h, neighbour), 0);
}

#[test]
fn the_entries_of_the_context_menu_are_lit_but_the_one_the_keys_are_on_is_not() {
    let mut h = scene();
    h.press("shift+f10");
    let first = area(&mut h, &Target::MenuItem(0));
    let second = hover(&mut h, &Target::MenuItem(1));
    assert!(all_lit(&mut h, second));
    assert_eq!(lit(&mut h, first), 0, "the first entry is the current one");
    hover(&mut h, &Target::MenuItem(0));
    assert_eq!(
        lit(&mut h, first),
        0,
        "and stays as it is under the pointer"
    );
}

#[test]
fn a_window_over_the_list_takes_the_pointer_only_for_its_own_buttons() {
    let mut h = scene();
    h.press("ctrl+c");
    h.keys("j");
    h.press("ctrl+v");
    plan_modal(&mut h);
    let run = hover(&mut h, &Target::PlanRun);
    assert!(
        underlined(&mut h, run) > 0 || lit(&mut h, run) > 0,
        "the main button of the plan"
    );
    let cancel = hover(&mut h, &Target::PlanCancel);
    assert!(all_lit(&mut h, cancel));
    // A row of the list under the window is out of reach, and so is the window's own body.
    move_to(&mut h, 31, 12);
    let mark = h.app.settings.th.mark;
    let after = buffer(&mut h);
    assert!(
        cells(Rect::new(30, 12, 74, 1)).all(|p| after[p].bg != mark),
        "the list is behind the window"
    );
}

#[test]
fn the_buttons_of_the_preview_are_lit() {
    let mut h = scene();
    let r = hover(&mut h, &Target::FullPreview);
    assert!(all_lit(&mut h, r));
}

#[test]
fn a_tab_is_lit_unless_it_is_the_one_in_front() {
    let mut h = scene();
    h.press("ctrl+t");
    let _ = h.screen();
    let behind = hover(&mut h, &Target::Tab(0));
    assert!(all_lit(&mut h, behind));
    let front = hover(&mut h, &Target::Tab(1));
    assert_eq!(lit(&mut h, front), 0);
    let close = hover(&mut h, &Target::TabClose(0));
    assert!(all_lit(&mut h, close), "and its cross");
}

#[test]
fn a_path_segment_and_the_commands_of_the_bar_are_underlined_where_the_field_is_the_tone() {
    let mut h = scene();
    let crumb = h
        .app
        .frame
        .hits
        .all()
        .find(|(_, t)| matches!(t, Target::Crumb(_)))
        .map(|(_, t)| t.clone())
        .expect("a path segment");
    let r = hover(&mut h, &crumb);
    assert!(underlined(&mut h, r) > 0);
    // The command line: the rows under the bar.
    h.press("ctrl+p");
    h.keys("do");
    let hit = Target::BarHit(1);
    let r = hover(&mut h, &hit);
    assert!(underlined(&mut h, r) > 0 || lit(&mut h, r) > 0);
}

#[test]
fn a_place_of_the_sidebar_is_lit_as_before() {
    let mut h = scene();
    let place = h
        .app
        .frame
        .hits
        .all()
        .find(|(_, t)| matches!(t, Target::Place(_)))
        .map(|(_, t)| t.clone())
        .expect("a place");
    let r = hover(&mut h, &place);
    assert!(all_lit(&mut h, r));
}

#[test]
fn a_move_that_stays_on_the_same_thing_asks_for_no_new_frame() {
    let mut h = scene();
    let r = area(&mut h, &Target::Row(3));
    move_to(&mut h, r.x + 5, r.y);
    assert!(h.app.redraw.dirty, "coming onto a row asks for a frame");
    let _ = h.screen();
    h.app.redraw.dirty = false;
    move_to(&mut h, r.x + 20, r.y);
    move_to(&mut h, r.x + 1, r.y);
    assert!(!h.app.redraw.dirty, "still on the same row");
    // The checkbox is part of the row.
    move_to(&mut h, r.x, r.y);
    assert!(!h.app.redraw.dirty, "the checkbox of the same row");
    // The next row is another thing.
    move_to(&mut h, r.x + 5, r.y + 1);
    assert!(h.app.redraw.dirty);
    let _ = h.screen();
    h.app.redraw.dirty = false;
    // Leaving for something that is not clicked asks for a frame once, and then nothing.
    move_to(&mut h, 0, 0);
    assert!(h.app.redraw.dirty);
    let _ = h.screen();
    h.app.redraw.dirty = false;
    move_to(&mut h, 1, 0);
    move_to(&mut h, 2, 0);
    assert!(!h.app.redraw.dirty);
}

#[test]
fn nothing_is_lit_with_the_mouse_off() {
    let mut h = scene();
    h.app.settings.mouse = false;
    let r = area(&mut h, &Target::Row(3));
    h.app.redraw.dirty = false;
    move_to(&mut h, r.x + 5, r.y);
    assert!(!h.app.redraw.dirty, "the mouse is not listened to");
    assert_eq!(lit(&mut h, r), 0);
}

#[test]
fn with_sixteen_colours_it_is_an_underline_and_the_background_is_not_touched() {
    let mut h = scene();
    h.app.settings.th = Theme::named("rada", ColorDepth::Ansi16).unwrap();
    let before = buffer(&mut h);
    let r = hover(&mut h, &Target::Row(3));
    let after = buffer(&mut h);
    assert!(cells(r).all(|p| after[p].modifier.contains(Modifier::UNDERLINED)));
    assert!(
        cells(r).all(|p| after[p].bg == before[p].bg),
        "no background"
    );
    // The row of the cursor stays as it is.
    let cursor = h.app.tab.sel.cursor;
    let r = hover(&mut h, &Target::Row(cursor));
    assert_eq!(underlined(&mut h, r), 0);
}

#[test]
fn a_frame_without_the_pointer_is_what_it_always_was() {
    let mut h = scene();
    let plain = buffer(&mut h);
    let r = area(&mut h, &Target::Row(3));
    move_to(&mut h, r.x + 5, r.y);
    let _ = buffer(&mut h);
    move_to(&mut h, 0, 0);
    let again = buffer(&mut h);
    assert_eq!(plain, again, "leaving takes the highlight away completely");
}
