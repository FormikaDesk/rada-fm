//! The soft look on a real frame: round ends, the shade behind the cursor row, and what each of them becomes without Nerd Font glyphs, in 256 colours and
//! in the basic profile.

mod common;

use common::*;
use rada_tui::hits::Target;
use rada_tui::theme::{ColorDepth, Theme};
use rada_tui::{IconSet, ui};
use ratatui::style::Color;

const CAP_LEFT: &str = "\u{e0b6}";
const CAP_RIGHT: &str = "\u{e0b4}";

fn put_cursor(h: &mut H, name: &str) {
    h.keys("g");
    for _ in 0..40 {
        if h.cursor_name() == name {
            break;
        }
        h.keys("j");
    }
    assert_eq!(h.cursor_name(), name);
}

fn scene(icons: IconSet) -> H {
    let mut h = demo_scene(170, 40);
    h.app.icons = icons;
    put_cursor(&mut h, "photo.jpg");
    h
}

/// The cells of the row of the cursor, as the list drew them.
fn cursor_cells(h: &mut H) -> Vec<ratatui::buffer::Cell> {
    let _ = h.screen();
    let r = h.app.hits.find(&Target::Row(h.app.cursor)).unwrap();
    let buf = h.term.backend().buffer().clone();
    (r.x..r.x + r.width)
        .map(|x| buf[(x, r.y)].clone())
        .collect()
}

#[test]
fn with_nerd_icons_the_cursor_row_is_shaded_across_and_has_round_ends() {
    let mut h = scene(IconSet::Nerd);
    let th = h.app.th.clone();
    let cells = cursor_cells(&mut h);
    let w = cells.len();
    // The ends are half circles in the colour of the shade at that end, on the background.
    assert_eq!(cells[0].symbol(), CAP_LEFT);
    assert_eq!(cells[0].fg, th.grad_row_start);
    assert_eq!(cells[w - 1].symbol(), CAP_RIGHT);
    assert_eq!(cells[w - 1].fg, th.grad_row_end);
    // Between them every cell has the colour of the shade at its place: it goes from the
    // first colour to the last.
    for (i, c) in cells.iter().enumerate().take(w - 1).skip(1) {
        assert_eq!(c.bg, th.row_shade(i, w), "cell {i} of {w}");
    }
    assert_ne!(cells[1].bg, cells[w - 2].bg);
    // The text on it is bold and white.
    let name = cells.iter().find(|c| c.symbol() == "p").unwrap();
    assert_eq!(name.fg, th.cursor_text);
    assert!(name.modifier.contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn selected_rows_are_tinted_with_round_ends_too() {
    let mut h = scene(IconSet::Nerd);
    let th = h.app.th.clone();
    put_cursor(&mut h, "backup.zip");
    h.press("space");
    put_cursor(&mut h, "photo.jpg");
    let _ = h.screen();
    let r = h.app.hits.find(&Target::Row(6)).unwrap();
    let buf = h.term.backend().buffer().clone();
    assert_eq!(buf[(r.x, r.y)].symbol(), CAP_LEFT);
    assert_eq!(buf[(r.x, r.y)].fg, th.sel);
    assert_eq!(buf[(r.x + 5, r.y)].bg, th.sel);
    assert_eq!(buf[(r.x + r.width - 1, r.y)].symbol(), CAP_RIGHT);
}

#[test]
fn every_colour_of_a_soft_frame_is_a_token_or_a_shade_between_two() {
    for name in Theme::NAMES {
        let th = Theme::named(name, ColorDepth::True).unwrap();
        let mut allowed = th.all_colors();
        for w in 1..=200usize {
            for x in 0..w {
                allowed.push(th.row_shade(x, w));
            }
        }
        let mut h = scene(IconSet::Nerd);
        h.app.th = th.clone();
        let _ = h.screen();
        let buf = h.term.backend().buffer().clone();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                for c in [cell.fg, cell.bg] {
                    if c == Color::Reset {
                        continue;
                    }
                    assert!(
                        allowed.contains(&c),
                        "theme {name}: cell ({x},{y}) {:?} uses {c:?}",
                        cell.symbol()
                    );
                }
            }
        }
    }
}

#[test]
fn in_256_colours_the_shade_is_solid() {
    let mut h = scene(IconSet::Nerd);
    h.app.th = Theme::named("rada", ColorDepth::Ansi256).unwrap();
    let th = h.app.th.clone();
    let cells = cursor_cells(&mut h);
    let w = cells.len();
    assert!(
        cells[1..w - 1].iter().all(|c| c.bg == th.cursor),
        "one solid colour"
    );
    // The ends are still round: the font is the same.
    assert_eq!(cells[0].symbol(), CAP_LEFT);
    let buf = h.term.backend().buffer().clone();
    // Nothing on the screen is a true colour.
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            for c in [buf[(x, y)].fg, buf[(x, y)].bg] {
                assert!(!matches!(c, Color::Rgb(..)), "({x},{y}) {c:?}");
            }
        }
    }
}

#[test]
fn without_nerd_glyphs_there_are_no_special_glyphs_and_no_shade() {
    for icons in [IconSet::UnicodeIcons, IconSet::UnicodeStd, IconSet::None] {
        let mut h = scene(icons);
        let th = h.app.th.clone();
        let screen = h.screen();
        assert!(
            !screen.contains(CAP_LEFT) && !screen.contains(CAP_RIGHT),
            "{icons:?}"
        );
        assert!(!screen.contains('▔'), "{icons:?}: no line under the bar");
        let cells = cursor_cells(&mut h);
        let w = cells.len();
        assert!(
            cells[..w - 1].iter().skip(1).all(|c| c.bg == th.cursor),
            "{icons:?}: a solid cursor row"
        );
    }
}

#[test]
fn the_basic_profile_draws_the_plain_look() {
    let mut h = scene(IconSet::None);
    h.app.basic = true;
    h.app.th = Theme::named("rada", ColorDepth::Ansi16).unwrap();
    let screen = h.screen();
    assert!(!screen.contains(CAP_LEFT) && !screen.contains('▔'));
    // The disks are bars of ten segments in brackets once the glyphs are plain.
    let backend = rada_tui::compat::CompatBackend::new(
        ratatui::backend::TestBackend::new(170, 40),
        rada_tui::compat::Rendering::BASIC,
    );
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, &mut h.app)).unwrap();
    let buf = term.backend().inner().buffer();
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("[######----]"), "{text}");
    assert!(
        text.is_ascii(),
        "only ASCII reaches the terminal in the basic profile"
    );
    // The half blocks that pad the bars would turn into rows of `#`: they are plain colour.
    assert!(!text.contains(&"#".repeat(20)), "{text}");
}
