//! The border of a popup, cell by cell, on the buffer the interface really draws: a menu, the
//! submenu beside it, the window that asks before something is deleted for good, and the
//! stacked notifications, in a dark theme and in Carta.
//!
//! The rule: no cell of the border is a colour of its own. A menu or a notification is the
//! colour of what lies behind it, border cells included; a window is one tone from its line
//! to its inside. Nothing is drawn beyond the border (no shadow), and a menu opened near the
//! bottom stays above the status bar and the key hints.

mod common;

use common::*;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use rada_tui::app::{MenuCmd, Modal, Notice, NoticeAction, NoticeKind};
use rada_tui::hits::Target;
use rada_tui::theme::{ColorDepth, Theme};
use rada_tui::ui;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

const THEMES: [&str; 2] = ["rada", "rada-light"];

fn scene(theme: &str, w: u16, h: u16, files: usize) -> H {
    let sb = rada_core::testutil::Sandbox::new();
    for i in 0..files {
        sb.write(format!("w/file-{i:02}.txt"), "x");
    }
    let dir = sb.path("w");
    let mut hh = H::new(sb, dir, w, h);
    // The list takes the width: what lies behind the cards is the list.
    hh.app.details = Some(false);
    hh.app.th = Theme::named(theme, ColorDepth::True).unwrap();
    hh
}

fn buffer(h: &mut H) -> Buffer {
    h.term.draw(|f| ui::draw(f, &mut h.app)).unwrap();
    h.term.backend().buffer().clone()
}

/// Where the rounded border of a popup is: a `╭` in the colour of the border, with its `╮`
/// to the right and its `╰` below (the cards of the panes are in another colour).
fn cards(b: &Buffer, th: &Theme) -> Vec<Rect> {
    let mut out = Vec::new();
    for y in 0..b.area.height {
        for x in 0..b.area.width {
            let c = &b[(x, y)];
            if c.symbol() != "╭" || c.fg != th.popup_border {
                continue;
            }
            let mut x1 = x + 1;
            while x1 < b.area.width && b[(x1, y)].symbol() == "─" {
                x1 += 1;
            }
            let mut y1 = y + 1;
            // The side of a notification is a heavy line in the colour of its state.
            while y1 < b.area.height && matches!(b[(x, y1)].symbol(), "│" | "┃") {
                y1 += 1;
            }
            if x1 < b.area.width
                && y1 < b.area.height
                && b[(x1, y)].symbol() == "╮"
                && b[(x, y1)].symbol() == "╰"
                && b[(x1, y1)].symbol() == "╯"
            {
                out.push(Rect::new(x, y, x1 - x + 1, y1 - y + 1));
            }
        }
    }
    out
}

fn inside_any(rs: &[Rect], x: u16, y: u16) -> bool {
    rs.iter()
        .any(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom())
}

/// The cells of the border of `r`, each with its neighbour straight out and straight in.
/// A cell of a border, the cell straight out of it and the one straight into it.
type EdgeCells = ((u16, u16), Option<(u16, u16)>, (u16, u16));

fn border(b: &Buffer, r: Rect) -> Vec<EdgeCells> {
    let mut out = Vec::new();
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let (l, rt, t, bt) = (x == r.x, x == r.right() - 1, y == r.y, y == r.bottom() - 1);
            if !(l || rt || t || bt) {
                continue;
            }
            let dx: i32 = if l {
                -1
            } else if rt {
                1
            } else {
                0
            };
            let dy: i32 = if t {
                -1
            } else if bt {
                1
            } else {
                0
            };
            let (ox, oy) = (x as i32 + dx, y as i32 + dy);
            let outside =
                (ox >= 0 && oy >= 0 && (ox as u16) < b.area.width && (oy as u16) < b.area.height)
                    .then_some((ox as u16, oy as u16));
            out.push((
                (x, y),
                outside,
                ((x as i32 - dx) as u16, (y as i32 - dy) as u16),
            ));
        }
    }
    out
}

/// A menu or a notification: each cell of the border is the colour of the cell just outside
/// it, and the inside is the plain background. So, wherever the background around is plain
/// (nearly all of the screen), the cell of the border, the cell outside it and the cell inside
/// it are one colour; where a row of another colour (a selected row) goes on beyond the line,
/// the line keeps that colour on its side. No band of the popup's own colour is left.
fn assert_behind_rule(
    before: &Buffer,
    after: &Buffer,
    th: &Theme,
    r: Rect,
    all: &[Rect],
    what: &str,
) {
    let plain = th.popup;
    let norm = |c: Color| if c == Color::Reset { plain } else { c };
    for (cell, outside, inside) in border(after, r) {
        let bg = after[cell].bg;
        assert_eq!(
            after[inside].bg, plain,
            "{what} {}: inside {cell:?}",
            th.name
        );
        let Some(o) = outside else { continue };
        // Against the cell of another card (a submenu beside its parent) there is nothing to
        // compare with.
        if inside_any(all, o.0, o.1) {
            continue;
        }
        let was = norm(before[o].bg);
        assert_eq!(
            bg, was,
            "{what} {}: {cell:?} against what was outside",
            th.name
        );
        if was == plain {
            assert_eq!(
                bg, after[inside].bg,
                "{what} {}: {cell:?} against inside",
                th.name
            );
        }
    }
}

/// Nothing is drawn in the ring of cells around the cards: it is as it was without them.
fn assert_no_shadow(before: &Buffer, after: &Buffer, rs: &[Rect], what: &str) {
    for r in rs {
        let ring = Rect::new(
            r.x.saturating_sub(1),
            r.y.saturating_sub(1),
            r.width + 2,
            r.height + 2,
        )
        .intersection(after.area);
        for y in ring.y..ring.bottom() {
            for x in ring.x..ring.right() {
                if inside_any(rs, x, y) {
                    continue;
                }
                assert_eq!(
                    after[(x, y)].bg,
                    before[(x, y)].bg,
                    "{what}: ({x}, {y}) beside the card {r:?}"
                );
            }
        }
    }
}

fn open_context_menu(h: &mut H, row: usize) {
    let target = Target::Row(row);
    h.app.cursor = row;
    h.right_click(&target);
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
}

/// Move the cursor of the menu to the entry that opens `Tag` and open it.
fn open_tag_submenu(h: &mut H) {
    for _ in 0..20 {
        let on_sub = matches!(
            &h.app.modal,
            Some(Modal::Menu(m)) if matches!(m.items[m.selected].cmd, MenuCmd::Sub(_))
        );
        if on_sub {
            break;
        }
        h.key(KeyCode::Down);
    }
    h.key(KeyCode::Right);
    match &h.app.modal {
        Some(Modal::Menu(m)) => assert!(m.parent.is_some(), "the submenu is open"),
        _ => panic!("no menu"),
    }
}

#[test]
fn a_menu_and_its_submenu_have_a_border_of_the_colour_behind_them_and_no_shadow() {
    for name in THEMES {
        let mut h = scene(name, 120, 36, 12);
        let th = h.app.th.clone();
        // The menu is opened on the third row: the cursor is there before and after.
        h.app.cursor = 3;
        let before = buffer(&mut h);
        open_context_menu(&mut h, 3);
        let one = buffer(&mut h);
        let rs = cards(&one, &th);
        assert_eq!(rs.len(), 1, "{name}: one menu");
        assert_behind_rule(&before, &one, &th, rs[0], &rs, "menu");
        assert_no_shadow(&before, &one, &rs, "menu");
        open_tag_submenu(&mut h);
        let two = buffer(&mut h);
        let rs = cards(&two, &th);
        assert_eq!(rs.len(), 2, "{name}: the menu and its submenu");
        for r in &rs {
            assert_behind_rule(&before, &two, &th, *r, &rs, "menu or submenu");
        }
        assert_no_shadow(&before, &two, &rs, "submenu");
    }
}

#[test]
fn a_menu_opened_near_the_bottom_stays_above_the_status_bar_and_the_key_hints() {
    for name in THEMES {
        let (w, hgt) = (120u16, 30u16);
        let mut h = scene(name, w, hgt, 60);
        let th = h.app.th.clone();
        h.keys("G");
        // Drawn once, so that the rows are known.
        let _ = buffer(&mut h);
        let bars = 1 + ui::hints_height(true, hgt);
        let top_of_bars = hgt - bars;
        // The last visible row of the list, then the last of the files.
        let last_row = (0..h.app.visible.len())
            .rev()
            .find(|i| h.app.hits.find(&Target::Row(*i)).is_some())
            .expect("a visible row");
        open_context_menu(&mut h, last_row);
        for chain in 0..2 {
            if chain == 1 {
                open_tag_submenu(&mut h);
            }
            let after = buffer(&mut h);
            let rs = cards(&after, &th);
            assert_eq!(rs.len(), chain + 1, "{name}");
            for r in &rs {
                assert!(
                    r.bottom() < top_of_bars,
                    "{name}: {r:?} not above the bars that begin at row {top_of_bars}"
                );
            }
        }
    }
}

#[test]
fn a_submenu_that_opens_on_the_left_has_no_dark_strip_and_no_empty_row() {
    for name in THEMES {
        // The menu opened at the right edge: its submenu has no room beside it on that side.
        let mut h = scene(name, 100, 30, 12);
        let th = h.app.th.clone();
        h.app.cursor = 3;
        let before = buffer(&mut h);
        let (_, y) = h.where_is(&Target::Row(3));
        h.mouse(
            MouseEventKind::Down(MouseButton::Right),
            98,
            y,
            KeyModifiers::NONE,
        );
        assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
        open_tag_submenu(&mut h);
        let after = buffer(&mut h);
        let rs = cards(&after, &th);
        assert_eq!(rs.len(), 2, "{name}");
        assert!(
            rs[1].right() <= rs[0].x,
            "{name}: on the left of its parent: {rs:?}"
        );
        for r in &rs {
            assert_behind_rule(&before, &after, &th, *r, &rs, "submenu");
        }
        assert_no_shadow(&before, &after, &rs, "submenu on the left");
    }
}

#[test]
fn the_window_that_asks_before_deleting_is_one_tone_inside_its_line() {
    for name in THEMES {
        let mut h = scene(name, 120, 36, 6);
        let th = h.app.th.clone();
        let before = buffer(&mut h);
        h.keys("D");
        plan_modal(&mut h);
        let after = buffer(&mut h);
        let rs = cards(&after, &th);
        assert_eq!(rs.len(), 1, "{name}: one window");
        let r = rs[0];
        let tone = th.modal_surface();
        for (cell, _, inside) in border(&after, r) {
            assert_eq!(after[cell].bg, tone, "{name}: {cell:?}");
            assert_eq!(after[inside].bg, tone, "{name}: inside {cell:?}");
        }
        // The screen behind went down: every cell outside is the dimmed colour it had.
        // (Not the bars at the bottom: the key hints change with the window.)
        let bars = 1 + ui::hints_height(true, after.area.height);
        for y in 0..after.area.height - bars {
            for x in 0..after.area.width {
                if inside_any(&rs, x, y) || after[(x, y)].symbol().contains('\u{10EEEE}') {
                    continue;
                }
                assert_eq!(
                    after[(x, y)].bg,
                    th.dimmed(before[(x, y)].bg),
                    "{name}: ({x}, {y}) behind the window"
                );
            }
        }
        // And the window is a little lighter than the screen it is over.
        assert_ne!(tone, th.dimmed(th.canvas), "{name}");
    }
}

#[test]
fn stacked_notifications_have_a_border_of_the_colour_behind_them_and_a_side_to_match() {
    for name in THEMES {
        let mut h = scene(name, 120, 36, 6);
        let th = h.app.th.clone();
        let before = buffer(&mut h);
        h.app.notify(
            Notice::new(NoticeKind::Ok, "3 items moved to the trash")
                .with_action(NoticeAction::Undo),
        );
        h.app.notify(Notice::new(
            NoticeKind::Error,
            "cannot rename setup.sh: denied",
        ));
        let after = buffer(&mut h);
        let rs = cards(&after, &th);
        assert_eq!(rs.len(), 2, "{name}: two cards");
        for r in &rs {
            assert_behind_rule(&before, &after, &th, *r, &rs, "notification");
            // The coloured side keeps the colour behind it, like the rest of the line.
            for y in r.y + 1..r.bottom() - 1 {
                assert_eq!(after[(r.x, y)].symbol(), "┃", "{name}: side of {r:?}");
                assert_ne!(after[(r.x, y)].fg, th.popup_border, "{name}");
            }
        }
        assert_no_shadow(&before, &after, &rs, "notification");
    }
}

#[test]
fn a_notification_is_still_a_card_with_its_colour_at_the_side() {
    // The colour of the state is on the left, the border's own line on the other sides.
    let mut h = scene("rada", 120, 36, 6);
    let th = h.app.th.clone();
    h.app
        .notify(Notice::new(NoticeKind::Ok, "3 items moved to the trash"));
    let b = buffer(&mut h);
    let r = cards(&b, &th)[0];
    assert_eq!(b[(r.x, r.y + 1)].fg, th.success);
    assert_eq!(b[(r.right() - 1, r.y + 1)].fg, th.popup_border);
}

/// A submenu is right against its menu, on the right of it and on the left of it, and nothing
/// of the list shows between the two lines: not the cursor row, which is in another colour and
/// passes behind them.
#[test]
fn a_submenu_is_right_against_its_menu_and_nothing_of_the_list_shows_between() {
    for name in THEMES {
        for (side, click_x) in [("right", 31u16), ("left", 98u16)] {
            let mut h = scene(name, 100, 30, 12);
            let th = h.app.th.clone();
            // The last row: the menu has to move up, so that the cursor row is behind it.
            h.app.cursor = 11;
            let before = buffer(&mut h);
            let (_, y) = h.where_is(&Target::Row(11));
            h.mouse(
                MouseEventKind::Down(MouseButton::Right),
                click_x,
                y,
                KeyModifiers::NONE,
            );
            assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
            open_tag_submenu(&mut h);
            let after = buffer(&mut h);
            let rs = cards(&after, &th);
            assert_eq!(rs.len(), 2, "{name} {side}: {rs:?}");
            let (parent, child) = (rs[0], rs[1]);
            let (left, right) = if side == "right" {
                assert_eq!(child.x, parent.right(), "{name}: {parent:?} {child:?}");
                (parent, child)
            } else {
                assert_eq!(child.right(), parent.x, "{name}: {parent:?} {child:?}");
                (child, parent)
            };
            // The two lines, over the rows both cards have: on the plain background. The
            // cursor row (another colour) is among them.
            let top = parent.y.max(child.y);
            let bottom = parent.bottom().min(child.bottom());
            let mut coloured_behind = false;
            for y in top..bottom {
                for x in [left.right() - 1, right.x] {
                    assert_eq!(after[(x, y)].bg, th.popup, "{name} {side}: ({x}, {y})");
                    coloured_behind |=
                        before[(x, y)].bg != th.popup && before[(x, y)].bg != Color::Reset;
                }
            }
            // The test means something: a row of another colour really was behind them.
            assert!(
                coloured_behind,
                "{name} {side}: nothing coloured behind the seam"
            );
        }
    }
}
