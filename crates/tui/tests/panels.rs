//! The panels view: the screen as rounded boxes, drawn on the demo home folder at the sizes
//! that matter, and what each part does (the focus, the tree, the preview).
//!
//! Screens are committed as text (`tests/snapshots/`), with a token map of the colours for
//! each theme: a change to the look shows up as a diff to review.

mod common;

use common::*;
use rada_tui::theme::ColorDepth;
use rada_tui::{IconSet, LayoutPref, Theme};
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

/// The themes the panels are drawn in for the snapshots.
const THEMES: [&str; 2] = ["rada", "rada-light"];

fn shot(h: &mut H) -> String {
    h.screen()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn put_cursor(h: &mut H, name: &str) {
    h.keys("g");
    for _ in 0..40 {
        if h.cursor_name() == name {
            break;
        }
        h.keys("j");
    }
    assert_eq!(h.cursor_name(), name);
    h.wait("preview", |a| a.preview.name == name);
}

/// The demo home with the state of the main screen: one item selected, two waiting on the
/// clipboard, the cursor on the picture and the card of the last copy. `nerd` draws with the
/// glyphs of a Nerd Font (the pills have round ends); the text snapshots do without, so that
/// they can be read.
fn scene(w: u16, hh: u16, theme: &str, nerd: bool) -> H {
    let mut h = demo_scene(w, hh);
    h.app.layout_pref = LayoutPref::Auto;
    h.app.th = Theme::named(theme, ColorDepth::True).unwrap();
    if nerd {
        h.app.icons = IconSet::Nerd;
    }
    put_cursor(&mut h, "backup.zip");
    h.press("space");
    put_cursor(&mut h, "photo.jpg");
    let paths: Vec<_> = ["data.json", "main.rs"]
        .iter()
        .filter_map(|n| h.app.listing.all().iter().find(|e| e.display == *n))
        .map(|e| e.path.clone())
        .collect();
    h.app.clipboard = Some(rada_tui::app::Clip {
        mode: rada_core::ops::TransferMode::Copy,
        paths,
    });
    let docs = h.app.home().join("Documents");
    // It stays for an hour, so that the bar along its bottom edge is whole whenever the test runs.
    h.app.notify(
        rada_tui::app::Notice::new(rada_tui::app::NoticeKind::Ok, "3 items copied to Documents")
            .with_action(rada_tui::app::NoticeAction::Undo)
            .lasting(std::time::Duration::from_secs(3600))
            .showing(docs, None),
    );
    h.settle();
    h
}

/// Every cell reduced to the theme token that colours it: the background first (the box, what
/// is around it, the selection, a pill), then the text.
fn token_map(h: &mut H, th: &Theme) -> String {
    let buf = h.redrawn_buffer();
    let hues = [
        (th.hues.cyan, 'n'),
        (th.hues.pink, 'p'),
        (th.hues.yellow, 'y'),
        (th.hues.green, 'g'),
        (th.hues.orange, 'o'),
        (th.hues.blue, 'b'),
    ];
    let letter = |c: Color, background: bool| -> char {
        if c == Color::Reset {
            return '.';
        }
        let mut tokens = vec![
            (th.canvas, 'c'),
            (th.inner, 'i'),
            (th.cursor, 'C'),
            (th.sel, 'S'),
            (th.accent, 'A'),
            (th.selection, 'E'),
            (th.button, 'U'),
            (th.field, 'F'),
            (th.key_bg, 'K'),
            (th.mark, 'M'),
        ];
        if !background {
            tokens = vec![
                (th.text, 't'),
                (th.text_dim, 'd'),
                (th.muted, 'm'),
                (th.accent, 'A'),
                (th.on_accent, 'O'),
                (th.cursor_text, 'W'),
                (th.success, 's'),
                (th.warn, 'w'),
                (th.error, 'e'),
                (th.disk, 'D'),
                (th.check, 'k'),
            ];
        }
        if let Some((_, l)) = tokens.iter().find(|(t, _)| *t == c) {
            return *l;
        }
        if !background && let Some((_, l)) = hues.iter().find(|(t, _)| *t == c) {
            return *l;
        }
        '+'
    };
    let mut out = String::from(
        "background: c canvas, i inner, C cursor, S sel, A accent, E selection, U button,\n\
         F field, K key_bg, M mark, + a tint, . none\n\
         text: t text, d dim, m muted, A accent, O on_accent, W cursor_text, s success, w warn,\n\
         e error, D disk, k check, n cyan, p pink, y yellow, g green, o orange, b blue, + other\n\n\
         background\n",
    );
    for y in 0..buf.area.height {
        out.extend((0..buf.area.width).map(|x| letter(buf[(x, y)].bg, true)));
        out.push('\n');
    }
    out.push_str("\ntext\n");
    for y in 0..buf.area.height {
        out.extend((0..buf.area.width).map(|x| letter(buf[(x, y)].fg, false)));
        out.push('\n');
    }
    out
}

fn assert_fits(h: &mut H, w: u16, hh: u16) {
    for l in h.screen().lines() {
        assert!(
            l.trim_end().width() <= w as usize,
            "{w}x{hh}: a line is wider than the screen: {l:?}"
        );
    }
}

// ---------------------------------------------------------------------------- screens

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn the_panels_at_160_by_46_in_each_theme() {
    for name in THEMES {
        let mut h = scene(160, 46, name, false);
        assert!(h.app.panels_shown, "{name}: panels at 160x46");
        let th = h.app.th.clone();
        let tag = name.replace('-', "_");
        insta::assert_snapshot!(format!("panels_160x46_{tag}"), shot(&mut h));
        insta::assert_snapshot!(
            format!("panels_160x46_{tag}_tokens"),
            token_map(&mut h, &th)
        );
        assert_fits(&mut h, 160, 46);
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn the_panels_at_the_smallest_window_that_gets_them() {
    let mut h = scene(140, 36, "rada", false);
    assert!(h.app.panels_shown);
    insta::assert_snapshot!("panels_140x36", shot(&mut h));
    assert_fits(&mut h, 140, 36);
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAGS section, which the Linux snapshot has"
)]
fn a_window_below_140_by_36_gets_the_classic_layout() {
    let mut h = scene(120, 40, "rada", false);
    assert!(!h.app.panels_shown, "120x40 is classic");
    insta::assert_snapshot!("panels_classic_120x40", shot(&mut h));
    for (w, hh) in [(139, 46), (160, 35)] {
        h.term.backend_mut().resize(w, hh);
        let _ = h.screen();
        assert!(!h.app.panels_shown, "{w}x{hh} is classic");
    }
}

// ---------------------------------------------------------------------------- the parts

#[test]
fn the_boxes_have_their_names_on_their_top_edge() {
    let mut h = scene(160, 46, "rada", false);
    let s = h.screen();
    for title in [
        "[ NAV ]",
        "[ SPACE ]",
        "[ ~/projects/demo ]",
        "[ PREVIEW ]",
        "[ DETAILS ]",
    ] {
        assert!(s.contains(title), "{title}:\n{s}");
    }
    // Rounded corners, and the tree, the favourites and the disks inside.
    assert!(s.contains("╭─[ NAV ]") && s.contains('╯'), "{s}");
    assert!(
        s.contains("PINNED") && s.contains("System") && s.contains("Data"),
        "{s}"
    );
    // The size column is the number alone: no bars of any kind.
    assert!(!s.contains('▏') && !s.contains('▎'), "{s}");
}

#[test]
fn below_42_rows_the_blank_rows_between_the_bars_go() {
    let top_of_boxes = |hh: u16| -> usize {
        let mut h = scene(160, hh, "rada", false);
        h.screen()
            .lines()
            .position(|l| l.contains("[ NAV ]"))
            .unwrap()
    };
    // Tabs, navigation, buttons: each on its own row with a blank one after it.
    assert_eq!(top_of_boxes(46), 6);
    assert_eq!(top_of_boxes(42), 6);
    // Without them from 41 rows: the three rows, and the boxes right under.
    assert_eq!(top_of_boxes(41), 3);
    assert_eq!(top_of_boxes(36), 3);
    // The same at the bottom: the status bar and the hints are one row each.
    let mut h = scene(160, 41, "rada", false);
    let lines: Vec<String> = h.screen().lines().map(str::to_string).collect();
    let last_box = lines.iter().rposition(|l| l.contains('╯')).unwrap();
    assert!(
        lines[last_box + 1].contains("items"),
        "{}",
        lines[last_box + 1]
    );
    assert!(lines[last_box + 2].contains("Ctrl+C") || lines[last_box + 2].contains("Enter"));
}

#[test]
fn tab_takes_the_keyboard_from_the_folder_to_nav_to_the_preview_and_back() {
    let mut h = scene(160, 46, "rada", false);
    let th = h.app.th.clone();
    // The border of the box with the keyboard in it is the accent colour, the others quiet.
    let border_of = |h: &mut H, title: &str| -> Color {
        let buf = h.redrawn_buffer();
        for y in 0..buf.area.height {
            let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(i) = line.find(title) {
                let x = line[..i].width();
                return buf[(x as u16 - 1, y)].fg;
            }
        }
        panic!("{title} is not on screen");
    };
    let folder = "[ ~/projects/demo ]";
    assert_eq!(border_of(&mut h, folder), th.accent);
    assert_eq!(border_of(&mut h, "[ NAV ]"), th.muted);
    h.press("tab");
    assert!(h.app.side_focus && !h.app.preview_focus);
    assert_eq!(border_of(&mut h, "[ NAV ]"), th.accent);
    assert_eq!(border_of(&mut h, folder), th.muted);
    h.press("tab");
    assert!(!h.app.side_focus && h.app.preview_focus);
    assert_eq!(border_of(&mut h, "[ PREVIEW ]"), th.accent);
    h.press("tab");
    assert!(!h.app.side_focus && !h.app.preview_focus);
    assert_eq!(border_of(&mut h, folder), th.accent);
}

#[test]
fn the_tree_follows_the_folder_and_its_arrows_open_and_close() {
    let mut h = scene(160, 46, "rada", false);
    h.wait("the tree", |a| {
        a.tree.kids(a.home()).is_some() && a.tree.kids(&a.home().join("projects")).is_some()
    });
    let s = h.screen();
    // Home is open, projects is open on the way to demo, which is the folder shown.
    for name in ["Desktop", "Documents", "projects", "demo", "Videos"] {
        assert!(s.contains(name), "{name}:\n{s}");
    }
    let row = s.lines().find(|l| l.contains("(19)")).unwrap();
    assert!(row.contains(" demo"), "the number of items of demo: {row}");
    // A click on the arrow of projects closes it, and demo is out of the tree.
    let projects = h.app.home().join("projects");
    h.click(&rada_tui::hits::Target::TreeToggle(projects.clone()));
    let s = h.screen();
    assert!(
        !s.lines()
            .any(|l| l.contains("│     ") && l.contains(" demo") && l.contains("(19)")),
        "{s}"
    );
    h.click(&rada_tui::hits::Target::TreeToggle(projects));
    assert!(h.screen().lines().any(|l| l.contains("(19)")));
}

#[test]
fn nav_is_driven_from_the_keyboard() {
    let mut h = scene(160, 46, "rada", false);
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    h.press("tab");
    assert!(h.app.side_focus);
    let rows = h.app.nav_rows();
    assert_eq!(
        rows[h.app.nav_cursor].path, h.app.cwd,
        "starts on the folder shown"
    );
    // Up to Home's children, and Enter opens the one under the cursor.
    h.press("up");
    h.press("up");
    let target = h.app.nav_rows()[h.app.nav_cursor].path.clone();
    assert_ne!(target, h.app.cwd);
    h.press("enter");
    h.wait("the folder", {
        let t = target.clone();
        move |a| a.cwd == t && !a.is_loading()
    });
    assert!(
        !h.app.side_focus,
        "Enter goes into the folder and takes the keyboard there"
    );
    // Left closes an open folder, Right opens it again.
    h.press("tab");
    let home = h.app.home().to_path_buf();
    h.press("g");
    assert_eq!(h.app.nav_rows()[h.app.nav_cursor].path, home);
    h.press("left");
    assert!(!h.app.tree.is_open(&home));
    h.press("right");
    assert!(h.app.tree.is_open(&home));
}

#[test]
fn the_arrows_scroll_the_preview_when_the_keyboard_is_in_it() {
    let mut h = scene(160, 46, "rada", false);
    put_cursor(&mut h, "notes.txt");
    h.press("tab");
    h.press("tab");
    assert!(h.app.preview_focus);
    assert_eq!(h.app.preview.scroll, 0);
    h.press("down");
    h.press("down");
    assert_eq!(h.app.preview.scroll, 2);
    h.press("up");
    assert_eq!(h.app.preview.scroll, 1);
    // The cursor of the folder did not move.
    assert_eq!(h.cursor_name(), "notes.txt");
    h.press("esc");
    assert!(!h.app.preview_focus);
}

#[test]
fn ctrl_b_takes_the_left_column_away() {
    let mut h = scene(160, 46, "rada", false);
    h.press("ctrl+b");
    let s = h.screen();
    assert!(!s.contains("[ NAV ]") && !s.contains("[ SPACE ]"), "{s}");
    assert!(s.contains("[ ~/projects/demo ]") && s.contains("[ PREVIEW ]"));
    h.press("ctrl+b");
    assert!(h.screen().contains("[ NAV ]"));
}

#[test]
fn the_selected_row_is_filled_with_the_selection_colour_and_its_text_reads_on_it() {
    for name in THEMES {
        let mut h = scene(160, 46, name, true);
        let th = h.app.th.clone();
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
        let th = h.app.th.clone();
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
