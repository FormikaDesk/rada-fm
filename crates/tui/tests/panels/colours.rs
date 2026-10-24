//! Every colour of the panels is a token of the theme.

use crate::common::*;
use rada_tui::Theme;
use ratatui::style::Color;

#[test]
fn selected_row_has_the_selection_fill() {
    for name in THEMES {
        let mut h = scene(160, 46, name, true);
        let th = h.app.settings.th.clone();
        let buf = h.redrawn_buffer();
        // The row of photo.jpg: a run of cells with the cursor colour as their background.
        let (x, y) = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .find(|(x, y)| buf[(*x, *y)].bg == th.cursor && buf[(*x, *y)].symbol() == "p")
            .unwrap_or_else(|| panic!("{name}: no selected row"));
        let mut first = x;
        while buf[(first - 1, y)].bg == th.cursor {
            first -= 1;
        }
        let mut cells = Vec::new();
        let mut cx = first;
        while cx < buf.area.width && buf[(cx, y)].bg == th.cursor {
            cells.push(cx);
            cx += 1;
        }
        // Everything written on it is in the text colour of the selection: never white on a
        // light theme, never a dim tone that would not read.
        for c in &cells {
            let cell = &buf[(*c, y)];
            if cell.symbol().trim().is_empty() {
                continue;
            }
            assert_eq!(
                cell.fg,
                th.cursor_text,
                "{name}: {:?} at {c},{y}",
                cell.symbol()
            );
        }
        // Rounded at both ends, in the colour of the selection on the box's inside.
        let left = &buf[(cells[0] - 1, y)];
        assert_eq!(left.symbol(), "\u{e0b6}", "{name}");
        assert_eq!((left.fg, left.bg), (th.cursor, th.inner), "{name}");
        let right = &buf[(cx, y)];
        assert_eq!(right.symbol(), "\u{e0b4}", "{name}");
        assert_eq!((right.fg, right.bg), (th.cursor, th.inner), "{name}");
    }
}

/// Nothing on the screen is drawn in a colour the theme does not have: the boxes, the
/// labels, the icons and the selection all take theirs from it, in every theme.
#[test]
fn every_colour_of_the_panels_is_a_theme_token_in_every_theme() {
    for name in Theme::NAMES {
        let mut h = scene(160, 46, name, true);
        let th = h.app.settings.th.clone();
        let allowed = th.all_colors();
        let buf = h.redrawn_buffer();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                for c in [cell.fg, cell.bg] {
                    assert!(
                        c == Color::Reset || allowed.contains(&c),
                        "theme {name}: cell ({x},{y}) {:?} uses {c:?}, which is not one of its tokens",
                        cell.symbol()
                    );
                }
            }
        }
    }
}

#[test]
fn way_folders_have_the_folder_colour() {
    let (mut h, _) = path_scene();
    let buf = h.redrawn_buffer();
    let nav = h.app.frame.nav_area;
    let mut in_nav = std::collections::BTreeSet::new();
    let mut in_list = std::collections::BTreeSet::new();
    for y in nav.top()..nav.bottom() {
        for x in 0..buf.area.width {
            let c = &buf[(x, y)];
            if c.symbol() == "▸" {
                if x < nav.right() {
                    in_nav.insert(format!("{:?}", c.fg));
                } else {
                    in_list.insert(format!("{:?}", c.fg));
                }
            }
        }
    }
    assert!(!in_nav.is_empty() && !in_list.is_empty());
    // (The list also has the row under the cursor, in the colour of the highlight.)
    assert!(
        in_nav.is_subset(&in_list),
        "{in_nav:?} in NAV, {in_list:?} in the list"
    );
}
