//! The panels view: the screen as rounded boxes, drawn on the demo home folder at the sizes
//! that matter, and what each part does (the focus, the tree, the preview).
//!
//! Screens are committed as text (`tests/snapshots/`), with a token map of the colours for
//! each theme: a change to the look shows up as a diff to review.

mod common;

use common::*;
use rada_tui::app::App;
use rada_tui::panels::{NavMode, TWO_SPACE_FROM_ROWS};
use rada_tui::theme::ColorDepth;
use rada_tui::{IconSet, LayoutPref, Theme};
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

/// The themes the panels are drawn in for the snapshots.
const THEMES: [&str; 3] = ["neon", "rada", "rada-light"];

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

/// The same with `nav = "albero"`: the NAV box lists the whole tree of Home.
fn tree_scene(w: u16, hh: u16, theme: &str, nerd: bool) -> H {
    let mut h = scene(w, hh, theme, nerd);
    h.app.nav_mode = NavMode::Tree;
    h.app.tree = Default::default();
    h.wait("the way down in the tree", |a| {
        a.cwd
            .ancestors()
            .skip(1)
            .take_while(|p| p.starts_with(a.home()))
            .all(|p| a.tree.kids(p).is_some())
    });
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
        assert!(
            h.app.panels_shown && !h.app.panels_two,
            "{name}: three columns at 160x46"
        );
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
    ignore = "the sidebar of a system with no tags has no TAGS section, which the Linux snapshot has"
)]
fn a_window_below_70_by_24_gets_the_classic_layout_and_the_advice_to_enlarge_it() {
    let mut h = scene(69, 30, "rada", false);
    assert!(!h.app.panels_shown, "69x30 is classic");
    let s = shot(&mut h);
    assert!(
        s.contains("Enlarge the window for the full layout (now 69×30)"),
        "{s}"
    );
    insta::assert_snapshot!("panels_classic_69x30", s);
    for (w, hh) in [(69, 46), (100, 23), (160, 23)] {
        h.term.backend_mut().resize(w, hh);
        let _ = h.screen();
        assert!(!h.app.panels_shown, "{w}x{hh} is classic");
    }
}

// ---------------------------------------------------------------------------- two columns

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn the_two_columns_at_78_by_45_and_at_100_by_30() {
    for (w, hh) in [(78, 45), (100, 30)] {
        let mut h = scene(w, hh, "rada", false);
        assert!(h.app.panels_shown && h.app.panels_two, "{w}x{hh}");
        let s = shot(&mut h);
        assert!(
            !s.contains("Enlarge"),
            "{w}x{hh}: no advice to enlarge\n{s}"
        );
        assert!(
            s.contains("[ NAV ]") && s.contains("[ ~/projects/demo ]"),
            "{s}"
        );
        assert!(
            !s.contains("[ PREVIEW ]"),
            "{w}x{hh}: the preview is not there\n{s}"
        );
        // SPAZIO only where there are rows for it.
        assert_eq!(
            s.contains("[ SPACE ]"),
            hh >= TWO_SPACE_FROM_ROWS,
            "{w}x{hh}\n{s}"
        );
        insta::assert_snapshot!(format!("panels_{w}x{hh}"), s);
        assert_fits(&mut h, w, hh);
    }
}

#[test]
fn the_preview_command_puts_the_preview_in_the_place_of_nav_and_back() {
    let mut h = scene(78, 45, "rada", false);
    let on_screen = |h: &mut H| {
        let s = h.screen();
        (s.contains("[ NAV ]"), s.contains("[ PREVIEW ]"))
    };
    assert_eq!(on_screen(&mut h), (true, false));
    h.press("alt+p");
    assert_eq!(
        on_screen(&mut h),
        (false, true),
        "the preview is where NAV was"
    );
    assert!(
        h.screen().contains("photo.jpg"),
        "it shows the file under the cursor"
    );
    h.press("alt+p");
    assert_eq!(
        on_screen(&mut h),
        (true, false),
        "the same command goes back"
    );
    // So does the item of the View menu, "Details pane".
    choose_in_view_menu(&mut h, "Details pane");
    assert_eq!(on_screen(&mut h), (false, true));
    // Tab takes the keyboard to the box on the left, whichever it is, and back.
    h.press("tab");
    assert!(h.app.preview_focus && !h.app.side_focus);
    h.press("alt+p");
    assert!(
        h.app.side_focus && !h.app.preview_focus,
        "the keyboard follows the box"
    );
    h.press("tab");
    assert!(!h.app.side_focus && !h.app.preview_focus);
    // Where there are three columns the command is left as it was: both boxes are there.
    h.term.backend_mut().resize(160, 46);
    assert!(h.screen().contains("[ NAV ]") && h.screen().contains("[ PREVIEW ]"));
}

#[test]
fn ctrl_b_takes_the_left_column_away_in_two_columns_too() {
    let mut h = scene(78, 45, "rada", false);
    assert!(h.screen().contains("[ NAV ]"));
    h.press("ctrl+b");
    let s = h.screen();
    assert!(!s.contains("[ NAV ]") && !s.contains("[ PREVIEW ]"), "{s}");
    assert!(
        s.contains("[ ~/projects/demo ]"),
        "the folder takes the room\n{s}"
    );
    h.press("ctrl+b");
    assert!(h.screen().contains("[ NAV ]"));
}

#[test]
fn the_pills_are_whole_in_two_columns_down_to_70() {
    for (w, hh) in [(70, 30), (78, 45), (100, 30)] {
        let mut h = scene(w, hh, "rada", true);
        let what = format!("{w}x{hh}");
        let behind = [h.app.th.inner, h.app.th.canvas];
        let pills = assert_pills(&h.redrawn_buffer(), &what, &behind);
        assert!(pills > 5, "{what}: only {pills} pills");
        h.press("alt+p");
        assert_pills(
            &h.redrawn_buffer(),
            &format!("{what}, the preview"),
            &behind,
        );
        h.press("tab");
        assert_pills(
            &h.redrawn_buffer(),
            &format!("{what}, keyboard in it"),
            &behind,
        );
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
    // Rounded corners, and the tree and the disks inside; the favourites only when the person
    // added some.
    assert!(s.contains("╭─[ NAV ]") && s.contains('╯'), "{s}");
    assert!(
        !s.contains("PINNED") && s.contains("System") && s.contains("Data"),
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
    let mut h = tree_scene(160, 46, "rada", false);
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
    let mut h = tree_scene(160, 46, "rada", false);
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

// ---------------------------------------------------------------------------- pills

const CAP_LEFT: &str = "\u{e0b6}";
const CAP_RIGHT: &str = "\u{e0b4}";

/// Every pill on the screen (a button, a label, a key, the selected row, a tab) has both its
/// round ends, on the same row as its text, and one background colour from end to end: the
/// ends are in that colour on whatever is behind the pill. Returns how many pills there were.
fn assert_pills(buf: &ratatui::buffer::Buffer, what: &str, behind: &[Color]) -> usize {
    let mut pills = 0;
    for y in 0..buf.area.height {
        // The colours the pills around are filled with, innermost last.
        let mut open: Vec<(Color, u16)> = Vec::new();
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            match cell.symbol() {
                CAP_LEFT => {
                    // The end is the pill's colour; behind it is what the pill stands on.
                    match open.last() {
                        Some((outer, _)) => assert_eq!(
                            cell.bg, *outer,
                            "{what}: the left end at {x},{y} stands on the pill around it"
                        ),
                        None => assert!(
                            behind.contains(&cell.bg),
                            "{what}: the left end at {x},{y} stands on {:?}, which is not what is behind it",
                            cell.bg
                        ),
                    }
                    open.push((cell.fg, x));
                }
                CAP_RIGHT => {
                    let (fill, at) = open.pop().unwrap_or_else(|| {
                        panic!("{what}: a right end at {x},{y} with no left end before it")
                    });
                    assert_eq!(
                        cell.fg, fill,
                        "{what}: the pill from {at} to {x} on row {y} has a right end of another colour"
                    );
                    match open.last() {
                        Some((outer, _)) => assert_eq!(
                            cell.bg, *outer,
                            "{what}: the right end at {x},{y} stands on the pill around it"
                        ),
                        None => assert!(
                            behind.contains(&cell.bg),
                            "{what}: the right end at {x},{y} stands on {:?}, which is not what is behind it",
                            cell.bg
                        ),
                    }
                    // Something between the ends: a pill with no text is not one.
                    assert!(x > at + 1, "{what}: an empty pill at {at},{y}");
                    pills += 1;
                }
                _ => {
                    if let Some((fill, at)) = open.last() {
                        // Wide glyphs leave a cell after them that the terminal skips.
                        if cell.symbol().is_empty() {
                            continue;
                        }
                        assert_eq!(
                            cell.bg,
                            *fill,
                            "{what}: {:?} at {x},{y} is inside the pill that starts at {at} and is not its colour",
                            cell.symbol()
                        );
                    }
                }
            }
        }
        assert!(
            open.is_empty(),
            "{what}: a pill on row {y} starting at {} has no right end",
            open[0].1
        );
    }
    pills
}

#[test]
fn every_pill_has_both_ends_and_one_colour_from_end_to_end() {
    for name in Theme::NAMES {
        for (w, hh) in [(160, 46), (140, 36), (170, 42)] {
            let mut h = scene(w, hh, name, true);
            let what = format!("{name} {w}x{hh}");
            let buf = h.redrawn_buffer();
            let behind = [h.app.th.inner, h.app.th.canvas];
            let pills = assert_pills(&buf, &what, &behind);
            // The selected row, the tab, the "rada" and the count, the keys, the buttons, the
            // labels: a screen has dozens.
            assert!(pills > 20, "{what}: only {pills} pills");
            // With the keyboard in NAV the row it is on is a pill too.
            h.press("tab");
            assert_pills(
                &h.redrawn_buffer(),
                &format!("{what}, keyboard in NAV"),
                &behind,
            );
            h.press("tab");
            assert_pills(
                &h.redrawn_buffer(),
                &format!("{what}, keyboard in the preview"),
                &behind,
            );
        }
    }
}

#[test]
fn without_a_nerd_font_a_pill_is_a_plain_rectangle_of_the_same_width() {
    // No ends at all, and a space at each side where they would be.
    let mut round = scene(160, 46, "rada", true);
    let mut plain = scene(160, 46, "rada", false);
    let (r, p) = (round.redrawn_buffer(), plain.redrawn_buffer());
    let ends = |b: &ratatui::buffer::Buffer| {
        (0..b.area.height)
            .flat_map(|y| (0..b.area.width).map(move |x| (x, y)))
            .filter(|(x, y)| matches!(b[(*x, *y)].symbol(), CAP_LEFT | CAP_RIGHT))
            .count()
    };
    assert!(ends(&r) > 0);
    assert_eq!(ends(&p), 0, "no round ends without a Nerd Font");
}

/// The check above must be able to fail: a pill whose end is of another colour, a pill with
/// an end missing and a cell inside it of another colour are each caught.
#[test]
fn the_pill_check_catches_a_broken_pill() {
    let mut h = scene(160, 46, "rada", true);
    let th = h.app.th.clone();
    let good = h.redrawn_buffer();
    let find = |sym: &str| {
        (0..good.area.height)
            .flat_map(|y| (0..good.area.width).map(move |x| (x, y)))
            .find(|(x, y)| good[(*x, *y)].symbol() == sym)
            .unwrap()
    };
    let caught = |edit: &dyn Fn(&mut ratatui::buffer::Buffer)| {
        let mut b = good.clone();
        edit(&mut b);
        let behind = [th.inner, th.canvas];
        std::panic::catch_unwind(|| assert_pills(&b, "broken", &behind)).is_err()
    };
    let (rx, ry) = find(CAP_RIGHT);
    assert!(
        caught(&|b| {
            b[(rx, ry)].set_fg(Color::Rgb(1, 2, 3));
        }),
        "an end of another colour"
    );
    assert!(
        caught(&|b| {
            b[(rx, ry)].set_symbol(" ");
        }),
        "an end that is missing"
    );
    assert!(
        caught(&|b| {
            b[(rx - 1, ry)].set_bg(Color::Rgb(1, 2, 3));
        }),
        "a cell of another colour inside"
    );
    let (lx, ly) = find(CAP_LEFT);
    assert!(
        caught(&|b| {
            b[(lx, ly)].set_symbol(" ");
        }),
        "a left end that is missing"
    );
}

// ---------------------------------------------------------------------------- git

/// Run git the way a person would, in the demo folder, to make the repository the test looks at.
fn git(dir: &std::path::Path, home: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The demo folder as a repository on `main`: everything committed but `setup.sh`, then
/// `main.rs` changed. Returns the harness, with git asked for in the way the tests do it.
fn git_scene() -> Option<H> {
    if !std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("git is not installed: test skipped");
        return None;
    }
    let mut h = scene(160, 46, "rada", false);
    let dir = h.app.cwd.clone();
    let home = h.app.home().to_path_buf();
    git(&dir, &home, &["init", "-q", "-b", "main"]);
    git(&dir, &home, &["add", "-A", "--", ".", ":!setup.sh"]);
    git(&dir, &home, &["commit", "-q", "-m", "first"]);
    std::fs::write(
        dir.join("main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .unwrap();
    h.app.use_git(
        rada_core::git::Git::new(true, None)
            .with_env("GIT_CEILING_DIRECTORIES", &home.to_string_lossy())
            .with_env("GIT_CONFIG_GLOBAL", "/dev/null")
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("HOME", &home.to_string_lossy()),
    );
    h.wait("git's answer", |a| a.git_here().is_some());
    Some(h)
}

#[test]
fn a_folder_in_a_git_repository_shows_its_branch_its_counts_and_which_files_changed() {
    let Some(mut h) = git_scene() else { return };
    let s = h.screen();
    // On the top edge of the folder's box: the branch, one changed file, one new one.
    let title = s
        .lines()
        .find(|l| l.contains("[ ~/projects/demo ]"))
        .unwrap();
    assert!(
        title.contains("main") && title.contains("●1") && title.contains("?1"),
        "{title}"
    );
    // After the name of a file: M for a change, ? for what is new; nothing for the rest.
    let row = |name: &str| s.lines().find(|l| l.contains(name)).unwrap().to_string();
    assert!(row("main.rs").contains("main.rs M"), "{}", row("main.rs"));
    assert!(
        row("setup.sh").contains("setup.sh ?"),
        "{}",
        row("setup.sh")
    );
    assert!(
        !row("Cargo.toml").contains("Cargo.toml M") && !row("Cargo.toml").contains("Cargo.toml ?")
    );
    // The status bar names the branch too.
    let status = s.lines().find(|l| l.contains("19 items")).unwrap();
    assert!(status.contains("main"), "{status}");
    // Under the cursor: what git says of that file, in the box of details.
    put_cursor(&mut h, "main.rs");
    let s = h.screen();
    let git_row = s.lines().find(|l| l.contains(" Git ")).expect("a Git row");
    assert!(git_row.contains("M modified"), "{git_row}");
    put_cursor(&mut h, "setup.sh");
    assert!(h.screen().contains("? new"), "{}", h.screen());
    put_cursor(&mut h, "Cargo.toml");
    assert!(h.screen().contains("no changes"));
}

#[test]
fn leaving_the_repository_takes_the_branch_away() {
    let Some(mut h) = git_scene() else { return };
    let docs = h.app.home().join("Documents");
    h.app.open_dir(docs.clone());
    h.wait("Documents", |a| a.cwd == docs && !a.is_loading());
    // The answer for the new folder is that it is in no repository.
    h.wait("no git here", |a| a.git.is_none());
    let s = h.screen();
    assert!(!s.contains('●') || !s.contains("main"), "{s}");
    assert!(
        !s.lines()
            .any(|l| l.contains("[ ~/Documents ]") && l.contains("main"))
    );
}

#[test]
fn git_is_never_called_when_it_is_off() {
    // The harness gives the program no git at all, as the configuration `git = false` does.
    let mut h = scene(160, 46, "rada", false);
    h.pump(300);
    assert!(h.app.git.is_none());
    let s = h.screen();
    let title = s
        .lines()
        .find(|l| l.contains("[ ~/projects/demo ]"))
        .unwrap();
    assert!(!title.contains('●') && !title.contains('?'), "{title}");
}

#[test]
fn the_git_labels_are_pills_and_every_colour_of_them_is_the_themes() {
    let Some(mut h) = git_scene() else { return };
    h.app.icons = IconSet::Nerd;
    for name in ["main.rs", "setup.sh"] {
        put_cursor(&mut h, name);
        let th = h.app.th.clone();
        let buf = h.redrawn_buffer();
        let behind = [th.inner, th.canvas];
        assert_pills(&buf, &format!("git, on {name}"), &behind);
        let allowed = th.all_colors();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                for c in [buf[(x, y)].fg, buf[(x, y)].bg] {
                    assert!(
                        c == Color::Reset || allowed.contains(&c),
                        "git, on {name}: {x},{y} {:?} uses {c:?}",
                        buf[(x, y)].symbol()
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------- the choice of layout

/// Open the View menu and walk to the entry whose label contains `label`, then choose it.
fn choose_in_view_menu(h: &mut H, label: &str) {
    use rada_tui::app::{MenuKind, Modal};
    h.app.open_menu(MenuKind::View, (60, 4));
    let selected = |h: &H| match &h.app.modal {
        Some(Modal::Menu(m)) => m
            .items
            .get(m.selected)
            .map(|i| i.label.clone())
            .unwrap_or_default(),
        _ => String::new(),
    };
    for _ in 0..30 {
        if selected(h).contains(label) {
            break;
        }
        h.press("down");
    }
    assert!(selected(h).contains(label), "{label}: {}", h.screen());
    h.press("enter");
}

#[test]
fn the_view_menu_chooses_the_layout_and_the_window_follows() {
    use rada_tui::app::{MenuKind, Modal};
    let mut h = scene(120, 40, "rada", false);
    assert!(
        h.app.panels_shown && h.app.panels_two,
        "auto at 120x40 is two columns"
    );
    // The three layouts are in the menu, the one in use ticked.
    h.app.open_menu(MenuKind::View, (60, 4));
    let Some(Modal::Menu(m)) = &h.app.modal else {
        panic!("a menu");
    };
    let ticked: Vec<&str> = m
        .items
        .iter()
        .filter(|i| i.label.starts_with("Layout") && i.checked == Some(true))
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(ticked, ["Layout: automatic"]);
    let s = h.screen();
    for l in ["Layout: automatic", "Layout: panels", "Layout: classic"] {
        assert!(s.contains(l), "{l}:\n{s}");
    }
    h.press("esc");

    // Panels asked for: drawn in a window that auto would give the classic layout.
    choose_in_view_menu(&mut h, "panels");
    assert_eq!(h.app.layout_pref, LayoutPref::Panels);
    let _ = h.screen();
    assert!(
        h.app.panels_shown && !h.app.panels_two,
        "three columns at 120x40 when asked for"
    );
    // Two in a window with room for two only, and none where there is no room.
    h.term.backend_mut().resize(90, 40);
    let _ = h.screen();
    assert!(h.app.panels_shown && h.app.panels_two);
    h.term.backend_mut().resize(65, 40);
    let _ = h.screen();
    assert!(!h.app.panels_shown);
    h.term.backend_mut().resize(160, 46);
    // Classic asked for: never panels.
    choose_in_view_menu(&mut h, "classic");
    let _ = h.screen();
    assert!(!h.app.panels_shown, "classic at 160x46 when asked for");
    // Automatic again: the size decides, by itself, as the window is made larger and smaller.
    choose_in_view_menu(&mut h, "automatic");
    for (w, hh, panels, two) in [
        (160, 46, true, false),
        (139, 46, true, true),
        (140, 36, true, false),
        (140, 35, true, true),
        (70, 24, true, true),
        (69, 24, false, false),
        (200, 60, true, false),
    ] {
        h.term.backend_mut().resize(w, hh);
        let _ = h.screen();
        assert_eq!(h.app.panels_shown, panels, "{w}x{hh}");
        assert_eq!(h.app.panels_two, two, "{w}x{hh}");
    }
}

#[test]
fn a_file_chooser_always_draws_the_classic_layout() {
    for pref in [LayoutPref::Auto, LayoutPref::Panels] {
        let sb = rada_core::testutil::Sandbox::new();
        sb.write("w/a.txt", "a");
        let dir = sb.path("w");
        let mut h = H::with_config(sb, dir, 200, 60, move |c| {
            c.chooser = Some(rada_tui::chooser::Chooser::file(false));
            c.layout_pref = pref;
        });
        let s = h.screen();
        assert!(!h.app.panels_shown, "{pref:?}: a chooser is classic");
        assert!(!s.contains("[ NAV ]") && s.contains("a.txt"), "{s}");
    }
}

#[test]
fn the_compact_arrangement_never_has_panels() {
    let sb = rada_core::testutil::Sandbox::new();
    let dir = sb.mkdir("w");
    let mut h = H::with_config(sb, dir, 200, 60, |c| {
        c.layout = rada_tui::LayoutKind::Compact;
        c.layout_pref = LayoutPref::Panels;
    });
    let _ = h.screen();
    assert!(!h.app.panels_shown);
}

/// Open `dir` and wait until the folder is shown and the tree has been told about it.
fn go_and_wait(h: &mut H, dir: &std::path::Path) {
    h.app.open_dir(dir.to_path_buf());
    let d = dir.to_path_buf();
    h.wait("the folder", move |a| a.cwd == d && !a.is_loading());
    h.settle();
}

#[test]
fn a_click_on_a_folder_of_the_tree_shows_it_in_the_middle_box() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    go_and_wait(&mut h, &home);
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    let docs = home.join("Documents");
    h.click(&rada_tui::hits::Target::Place(docs.clone()));
    h.wait("Documents", {
        let d = docs.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    h.settle();
    let s = h.screen();
    assert!(s.contains("[ ~/Documents ]"), "{s}");
    let rows = h.app.nav_rows();
    assert_eq!(
        App::nav_current(&rows, &h.app.cwd).map(|i| &rows[i].path),
        Some(&docs)
    );
    // The way back is the history's: Alt+Left returns to Home.
    h.press("alt+left");
    h.wait("home again", {
        let d = home.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
}

#[test]
fn enter_on_a_folder_of_the_tree_shows_it_and_the_arrow_only_opens_the_branch() {
    let mut h = tree_scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    go_and_wait(&mut h, &home);
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    h.press("tab");
    let docs = home.join("Documents");
    let at = h
        .app
        .nav_rows()
        .iter()
        .position(|r| r.path == docs)
        .unwrap();
    h.app.nav_cursor = at;
    h.press("enter");
    h.wait("Documents", {
        let d = docs.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    // A click on the arrow of a folder opens its branch and goes nowhere.
    let projects = home.join("projects");
    go_and_wait(&mut h, &home);
    let was_open = h.app.tree.is_open(&projects);
    h.click(&rada_tui::hits::Target::TreeToggle(projects.clone()));
    assert_ne!(h.app.tree.is_open(&projects), was_open);
    assert_eq!(h.app.cwd, home);
}

/// The demo home with sixty more folders in it, so that the tree is taller than its box, and a
/// chain of folders far down the list.
fn long_tree_scene() -> (H, std::path::PathBuf) {
    let mut h = tree_scene(160, 40, "rada", false);
    let home = h.app.home().to_path_buf();
    for n in 0..60 {
        std::fs::create_dir_all(home.join(format!("folder-{n:02}"))).unwrap();
    }
    let deep = home.join("folder-57").join("one").join("two");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir_all(home.join("folder-02/inner")).unwrap();
    go_and_wait(&mut h, &home);
    h.wait("the new folders in the tree", |a| {
        a.tree
            .kids(a.home())
            .is_some_and(|k| k.iter().any(|k| k.name == "folder-59"))
    });
    h.settle();
    (h, deep)
}

fn nav_text(h: &mut H) -> String {
    let a = h.app.nav_area;
    let s = h.screen();
    s.lines()
        .skip(a.y as usize)
        .take(a.height as usize)
        .map(|l| {
            l.chars()
                .skip(a.x as usize)
                .take(a.width as usize)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_deep_folder_is_opened_highlighted_and_scrolled_into_view_in_a_long_tree() {
    let (mut h, deep) = long_tree_scene();
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    assert!(
        !nav_text(&mut h).contains("folder-57"),
        "off the box at first"
    );
    h.app.open_dir(deep.clone());
    h.wait("the folder", {
        let d = deep.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    // Every folder on the way is listed, whatever the order the answers come in.
    h.wait("the way down", {
        let d = deep.clone();
        move |a| {
            d.ancestors()
                .skip(1)
                .take_while(|p| p.starts_with(a.home()))
                .all(|p| a.tree.kids(p).is_some())
        }
    });
    h.settle();
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &deep).expect("the folder is a row of the tree");
    assert!(h.app.tree.is_open(deep.parent().unwrap()));
    let a = h.app.nav_area;
    assert!(
        at >= h.app.nav_scroll && at < h.app.nav_scroll + a.height as usize,
        "row {at} is outside {}..{}",
        h.app.nav_scroll,
        h.app.nav_scroll + a.height as usize
    );
    let text = nav_text(&mut h);
    assert!(text.contains("two"), "{text}");
    assert!(
        text.contains('┃'),
        "a thumb shows that there is more:\n{text}"
    );
}

#[test]
fn the_wheel_and_the_arrows_scroll_a_long_tree() {
    let (mut h, _) = long_tree_scene();
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    h.settle();
    let a = h.app.nav_area;
    let (x, y) = (a.x + 2, a.y + 2);
    assert_eq!(h.app.nav_scroll, 0);
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::ScrollDown,
        x,
        y,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.settle();
    assert_eq!(h.app.nav_scroll, 3, "three rows to a turn");
    let t = nav_text(&mut h);
    assert!(!t.lines().next().unwrap().contains("Home"), "{t}");
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::ScrollUp,
        x,
        y,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.settle();
    assert_eq!(h.app.nav_scroll, 0);
    // The wheel does not move the folder shown.
    assert_eq!(h.app.cwd, h.app.home());
    // Down from the keyboard in NAV follows the cursor past the bottom of the box.
    h.press("tab");
    let height = a.height as usize;
    for _ in 0..height + 5 {
        h.press("down");
    }
    let _ = h.screen();
    assert!(h.app.nav_cursor >= height);
    assert!(
        h.app.nav_cursor >= h.app.nav_scroll && h.app.nav_cursor < h.app.nav_scroll + height,
        "cursor {} scroll {}",
        h.app.nav_cursor,
        h.app.nav_scroll
    );
}

#[test]
fn a_click_on_a_folder_far_down_a_long_tree_shows_it() {
    let (mut h, _) = long_tree_scene();
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    let a = h.app.nav_area;
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::ScrollDown,
        a.x + 2,
        a.y + 2,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.settle();
    // The first folder-NN that is on the box, by the screen: click on its name.
    let text = nav_text(&mut h);
    let (dy, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("folder-"))
        .unwrap();
    let name: String = line
        .split_whitespace()
        .find(|w| w.starts_with("folder-"))
        .unwrap()
        .into();
    let dx = line.find(&name).unwrap();
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::Down(
            ratatui::crossterm::event::MouseButton::Left,
        ),
        a.x + dx as u16,
        a.y + dy as u16,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    let want = h.app.home().join(&name);
    h.wait("the folder", move |a| a.cwd == want && !a.is_loading());
}

#[test]
fn the_favourites_are_only_the_folders_the_person_added() {
    let mut h = tree_scene(160, 46, "rada", false);
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    let is_favorite = |a: &App| {
        a.nav_rows()
            .iter()
            .any(|r| matches!(r.kind, rada_tui::panels::NavKind::Favorite(_)))
    };
    assert!(!is_favorite(&h.app), "the standard folders are in the tree");
    assert!(
        !h.screen().contains("PINNED"),
        "no title without favourites"
    );
    let added = h.app.home().join("projects");
    // B in NAV, on the folder.
    h.press("tab");
    h.app.nav_cursor = h
        .app
        .nav_rows()
        .iter()
        .position(|r| r.path == added)
        .unwrap();
    h.press("B");
    h.wait("the bookmark", |a| a.paths.bookmarks.contains(&added));
    let rows = h.app.nav_rows();
    let favs: Vec<_> = rows
        .iter()
        .filter(|r| matches!(r.kind, rada_tui::panels::NavKind::Favorite(_)))
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(favs, vec![added]);
    assert!(h.screen().contains("PINNED"));
}

/// The scene on a terminal that draws pictures (Kitty), with a cell of 10 × 20 pixels.
fn ring_scene(theme: &str) -> H {
    let mut h = scene(160, 46, theme, false);
    h.app.image_ui = Some(rada_tui::ImageUi::sized(
        ratatui_image::picker::ProtocolType::Kitty,
        (10, 20),
    ));
    h.app.th = Theme::named(theme, ColorDepth::True).unwrap();
    h
}

#[test]
fn where_the_terminal_draws_pictures_the_main_disk_is_a_ring_with_its_figures_beside_it() {
    let mut h = ring_scene("neon");
    let s = h.screen();
    let ring = h.app.ring.as_ref().expect("the ring was made");
    assert_eq!(ring.pieces.len(), 4, "around the hole for the text");
    // In the hole, the percentage and the word; beside it the name, used of total, free. The
    // cells that carry a picture are left out of the rows.
    let buf = h.redrawn_buffer();
    let plain: Vec<String> = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .filter(|c| c.len() <= 4)
                .collect()
        })
        .collect();
    let at = plain
        .iter()
        .position(|l| l.contains("usato") || l.contains("used"))
        .expect("the word under the percentage");
    assert!(
        plain[at - 1].contains("62%"),
        "the percentage above the word"
    );
    assert!(
        plain.iter().any(|l| l.contains("293 / 477 GB")),
        "used of total"
    );
    assert!(plain.iter().any(|l| l.contains("183 GB free")), "free");
    // The main disk has no bar across the box (the others have twelve cells).
    assert!(
        !s.lines().any(|l| l.contains("■■■■■■■■■■■■■■")),
        "a bar for the main disk"
    );
    // The other volumes keep their bars of twelve cells.
    assert!(
        s.contains("■■■■") || s.contains("□□□□"),
        "bars of the others"
    );
}

#[test]
fn the_ring_is_made_again_only_when_the_percentage_changes() {
    let mut h = ring_scene("neon");
    let _ = h.screen();
    let _ = h.screen();
    h.app.dirty = true;
    let _ = h.screen();
    assert_eq!(
        h.app.ring.as_ref().unwrap().made,
        1,
        "three frames, one picture"
    );
    // Another percentage: another picture.
    let total = 512_000_000_000u64;
    // The first volume of the demo is the main disk (its mount point is the root of the
    // system, which is not `/` everywhere).
    h.app.volumes[0].available = Some(total / 10);
    let _ = h.screen();
    assert_eq!(h.app.ring.as_ref().unwrap().made, 2);
}

#[test]
fn without_pictures_or_true_colours_the_main_disk_stays_a_bar() {
    // Half blocks: the terminal draws no pictures.
    let mut h = scene(160, 46, "neon", false);
    let s = h.screen();
    assert!(h.app.ring.is_none());
    assert!(s.contains("■"), "a bar");
    // Pictures, but a theme in 256 colours.
    let mut h = ring_scene("neon");
    h.app.th = Theme::named("neon", ColorDepth::Ansi256).unwrap();
    let s = h.screen();
    assert!(h.app.ring.is_none());
    assert!(s.contains("■"), "a bar");
}

#[test]
fn a_click_and_enter_on_scaricati_in_the_tree_show_its_files_in_the_middle_box() {
    let mut h = tree_scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let dl = home.join("Scaricati");
    std::fs::create_dir_all(&dl).unwrap();
    std::fs::write(dl.join("fattura.pdf"), "x").unwrap();
    go_and_wait(&mut h, &home);
    h.wait("Scaricati in the tree", |a| {
        a.tree
            .kids(a.home())
            .is_some_and(|k| k.iter().any(|k| k.name == "Scaricati"))
    });
    // A click on the name, found on the screen and not by what the hit map says.
    let text = nav_text(&mut h);
    let (dy, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("Scaricati"))
        .expect("the row is in NAV");
    let a = h.app.nav_area;
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::Down(
            ratatui::crossterm::event::MouseButton::Left,
        ),
        a.x + line.find("Scaricati").unwrap() as u16 + 2,
        a.y + dy as u16,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.wait("Scaricati", {
        let d = dl.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    assert!(
        h.screen().contains("fattura.pdf"),
        "its files are in the middle box"
    );
    let rows = h.app.nav_rows();
    assert_eq!(
        App::nav_current(&rows, &dl).map(|i| &rows[i].path),
        Some(&dl)
    );
    // The same with Enter, from NAV.
    go_and_wait(&mut h, &home);
    h.press("tab");
    h.app.nav_cursor = h.app.nav_rows().iter().position(|r| r.path == dl).unwrap();
    h.press("enter");
    h.wait("Scaricati again", move |a| a.cwd == dl && !a.is_loading());
    assert!(h.screen().contains("fattura.pdf"));
}

/// The demo home with a chain of folders `a/b/c` that has a sibling at every step, and two
/// folders inside `c` besides `d`; the folder shown is `c`, and the NAV box has answered.
fn path_scene() -> (H, std::path::PathBuf) {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let c = home.join("a/b/c");
    for d in [
        "a/sib-a",
        "a/b/sib-b",
        "a/b/c/d",
        "a/b/c/kid-1",
        "a/b/c/kid-2",
        "sib-home",
    ] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    go_and_wait(&mut h, &c);
    h.wait("the subfolders of c", |a| {
        a.tree.kids(&a.cwd).is_some_and(|k| k.len() == 3)
    });
    h.settle();
    (h, c)
}

#[test]
fn nav_lists_the_places_on_top_as_the_classic_sidebar_does() {
    let (mut h, _) = path_scene();
    let rows = h.app.nav_rows();
    let at = |name: &str| {
        rows.iter()
            .position(|r| r.name == name)
            .unwrap_or(usize::MAX)
    };
    let title = rows
        .iter()
        .position(|r| r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")
        .expect("the title of the way");
    let places = [
        "Home",
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Music",
        "Videos",
        "DEVICES",
    ];
    let mut last = 0;
    for p in places {
        assert!(at(p) < title, "{p} is above the way: {rows:?}");
        assert!(at(p) >= last, "{p} is in the order of the sidebar");
        last = at(p);
    }
    assert!(at("USB drive") < title, "{rows:?}");
    // The tags are not on every system (Windows has none): then there is no row, and no
    // second blank row in its place.
    if h.app.tags_supported() {
        assert!(
            at("USB drive") < at("TAGS") && at("TAGS") < title,
            "{rows:?}"
        );
    } else {
        assert_eq!(at("TAGS"), usize::MAX);
        assert!(
            !rows
                .windows(2)
                .any(|w| { w[0].kind == rada_tui::panels::NavKind::Gap && w[1].kind == w[0].kind })
        );
    }
    let text = nav_text(&mut h);
    assert!(
        text.contains("PATH") && text.contains("Documents"),
        "{text}"
    );
}

#[test]
fn nav_lists_only_the_ancestors_and_the_subfolders_of_the_folder_shown() {
    let (mut h, c) = path_scene();
    let home = h.app.home().to_path_buf();
    let rows = h.app.nav_rows();
    let title = rows
        .iter()
        .position(|r| r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")
        .unwrap();
    let way: Vec<_> = rows[title + 1..]
        .iter()
        .map(|r| (r.path.clone(), r.depth))
        .collect();
    // (Three levels of indent at most: the subfolders of c, the fourth, stay at the third.)
    let want: Vec<_> = [
        (home.clone(), 0),
        (home.join("a"), 1),
        (home.join("a/b"), 2),
        (c.clone(), 3),
        (c.join("d"), 3),
        (c.join("kid-1"), 3),
        (c.join("kid-2"), 3),
    ]
    .into();
    assert_eq!(
        way, want,
        "no sibling of an ancestor, nothing of Home's own folders"
    );
    // The folder shown carries its number of items and is the highlighted row.
    let here = &rows[title + 4];
    assert_eq!(here.path, c);
    assert_eq!(here.count, Some(h.app.listing.all().len()));
    assert_eq!(App::nav_current(&rows, &c), Some(title + 4));
    let text = nav_text(&mut h);
    assert!(
        !text.contains("sib-") && !text.contains("Desktop ("),
        "{text}"
    );
    // Moving to a folder in the middle box moves the way by itself.
    go_and_wait(&mut h, &home.join("a"));
    h.wait("the subfolders of a", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let text = nav_text(&mut h);
    assert!(text.contains("sib-a") && !text.contains("kid-1"), "{text}");
}

#[test]
fn a_click_on_an_ancestor_of_the_way_shows_that_folder_in_the_middle_box() {
    let (mut h, c) = path_scene();
    let b = c.parent().unwrap().to_path_buf();
    h.click(&rada_tui::hits::Target::Place(b.clone()));
    h.wait("b", {
        let b = b.clone();
        move |a| a.cwd == b && !a.is_loading()
    });
    h.settle();
    assert!(h.screen().contains("[ ~/a/b ]"));
    // Enter does the same from the keyboard, on a subfolder.
    h.wait("the subfolders of b", |a| a.tree.kids(&a.cwd).is_some());
    h.press("tab");
    let at = h
        .app
        .nav_rows()
        .iter()
        .position(|r| r.path == c)
        .expect("c is listed under b");
    h.app.nav_cursor = at;
    h.press("enter");
    h.wait("c", move |a| a.cwd == c && !a.is_loading());
}

#[test]
fn what_does_not_fit_under_the_way_is_counted_in_a_row_of_more() {
    let (mut h, c) = path_scene();
    for n in 0..40 {
        std::fs::create_dir_all(c.join(format!("many-{n:02}"))).unwrap();
    }
    go_and_wait(&mut h, &c);
    h.app.refresh();
    h.wait("the subfolders of c", |a| {
        a.tree.kids(&a.cwd).is_some_and(|k| k.len() == 43)
    });
    h.settle();
    let rows = h.app.nav_rows();
    assert!(
        rows.len() <= h.app.nav_area.height as usize,
        "{}",
        rows.len()
    );
    let more = rows.last().unwrap();
    assert_eq!(more.kind, rada_tui::panels::NavKind::More);
    let shown = rows
        .iter()
        .filter(|r| {
            r.path.parent() == Some(c.as_path()) && r.kind != rada_tui::panels::NavKind::More
        })
        .count();
    assert_eq!(more.name, format!("… {} more", 43 - shown));
    assert!(!more.selectable());
}

#[test]
fn with_nav_set_to_albero_the_box_keeps_the_whole_tree() {
    let h = tree_scene(160, 46, "rada", false);
    let rows = h.app.nav_rows();
    assert!(
        rows.iter().all(|r| r.name != "PATH"),
        "no title of the way in the tree"
    );
    let home = h.app.home().to_path_buf();
    // Home's own folders are all there, with their arrows, as before.
    for d in ["Documents", "projects"] {
        let r = rows.iter().find(|r| r.path == home.join(d)).unwrap();
        assert_eq!(r.depth, 1);
    }
    assert!(rows.iter().any(|r| r.path == home.join("projects/demo")));
    assert!(matches!(
        rows[0].kind,
        rada_tui::panels::NavKind::Folder { open: Some(true) }
    ));
    assert_eq!(NavMode::parse("albero"), Some(NavMode::Tree));
    assert_eq!(NavMode::parse("percorso"), Some(NavMode::Path));
    assert_eq!(NavMode::default(), NavMode::Path);
}

#[test]
fn the_way_stays_in_view_when_the_box_gets_shorter_after_the_first_frames() {
    // The disks arrive after the first frames: SPAZIO appears and NAV loses rows. Here the
    // window gets shorter instead, with the same effect on the box.
    let mut h = scene(160, 56, "rada", false);
    let home = h.app.home().to_path_buf();
    // How far down the rows the first folder under Home is depends on the system (tags, disks),
    // and so does the height of the box: the chain is made as deep as the last row of the tall
    // box needs.
    std::fs::create_dir_all(home.join("d0")).unwrap();
    go_and_wait(&mut h, &home.join("d0"));
    h.wait("the way", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let tall = h.app.nav_area.height as usize;
    let rows = h.app.nav_rows();
    let first = App::nav_current(&rows, &home.join("d0")).unwrap();
    let levels = tall - first;
    let mut deep = home.join("d0");
    for n in 1..levels {
        deep.push(format!("d{n}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    go_and_wait(&mut h, &deep);
    h.wait("the way", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    // (Indented as deep as that the name does not fit, so the row is looked for by its place.)
    let in_view = |h: &H| {
        let rows = h.app.nav_rows();
        let at = App::nav_current(&rows, &h.app.cwd).unwrap();
        let (from, height) = (h.app.nav_scroll, h.app.nav_area.height as usize);
        (from..from + height).contains(&at)
    };
    let tall_box = tall;
    assert!(in_view(&h), "in view at first");
    h.term.backend_mut().resize(160, 36);
    let _ = h.screen();
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &deep).unwrap();
    let shown = h.app.nav_area.height as usize;
    assert!(shown < tall_box, "the box is shorter: {shown} < {tall_box}");
    assert!(
        at >= shown,
        "the folder is below the box's rows: {at} of {shown}"
    );
    assert!(in_view(&h), "the folder shown is in view");
}

/// A trash in the places of the demo, which has none.
fn with_trash(h: &mut H) {
    let t = h.app.home().join(".trash");
    let kind = rada_core::platform::PlaceKind::Trash;
    h.app.paths.places.retain(|p| p.kind != kind);
    h.app
        .paths
        .places
        .push(rada_core::places::Place { kind, path: t });
    h.app.dirty = true;
}

/// What is listed under the title of the way, as paths.
fn way_paths(h: &H) -> Vec<std::path::PathBuf> {
    let rows = h.app.nav_rows();
    match rows
        .iter()
        .position(|r| r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")
    {
        Some(t) => rows[t + 1..]
            .iter()
            .filter(|r| r.kind != rada_tui::panels::NavKind::Trash)
            .map(|r| r.path.clone())
            .collect(),
        None => Vec::new(),
    }
}

#[test]
fn in_a_place_there_is_no_way_and_the_place_is_the_highlighted_row() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    for place in [
        home.clone(),
        home.join("Documents"),
        h.app.volumes[0].mount_point.clone(),
    ] {
        go_and_wait(&mut h, &place);
        h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
        h.settle();
        let rows = h.app.nav_rows();
        assert!(
            rows.iter()
                .all(|r| !(r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")),
            "no title of the way in {place:?}"
        );
        let at = App::nav_current(&rows, &place).expect("the place is highlighted");
        assert!(matches!(
            rows[at].kind,
            rada_tui::panels::NavKind::Favorite(_)
        ));
        assert!(!nav_text(&mut h).contains("PATH"));
    }
}

#[test]
fn the_way_starts_at_the_nearest_place_that_holds_the_folder() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let deep = home.join("Documents/x/y");
    std::fs::create_dir_all(&deep).unwrap();
    go_and_wait(&mut h, &deep);
    h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let docs = home.join("Documents");
    assert_eq!(
        way_paths(&h),
        vec![docs.clone(), docs.join("x"), deep.clone()],
        "Documents, x, y"
    );
    // The first row is the place, with its icon.
    let rows = h.app.nav_rows();
    let t = rows.iter().position(|r| r.name == "PATH").unwrap();
    assert!(matches!(
        rows[t + 1].kind,
        rada_tui::panels::NavKind::Favorite(_)
    ));
    // Under Home, and not in a place of its own, it starts at Home.
    let abc = home.join("a/b/c");
    std::fs::create_dir_all(&abc).unwrap();
    go_and_wait(&mut h, &abc);
    h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
    let way = way_paths(&h);
    assert_eq!(way[0], home);
    assert_eq!(&way[1..], [home.join("a"), home.join("a/b"), abc.clone()]);
}

#[test]
fn the_trash_is_the_last_row_of_nav_in_every_view() {
    let mut h = scene(160, 46, "rada", false);
    with_trash(&mut h);
    let home = h.app.home().to_path_buf();
    let deep =
        home.join("t1/t2/t3/t4/t5/t6/t7/t8/t9/t10/t11/t12/t13/t14/t15/t16/t17/t18/t19/t20/t21");
    std::fs::create_dir_all(&deep).unwrap();
    for (w, hh, dir) in [
        (160, 46, home.clone()),
        (160, 46, deep.clone()),
        (160, 36, deep.clone()),
        (160, 36, home.join("Documents")),
    ] {
        h.term.backend_mut().resize(w, hh);
        go_and_wait(&mut h, &dir);
        h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
        h.settle();
        // The places are read again now and then: the demo's trash with them.
        with_trash(&mut h);
        let rows = h.app.nav_rows();
        assert_eq!(
            rows.last().map(|r| &r.kind),
            Some(&rada_tui::panels::NavKind::Trash),
            "{dir:?}"
        );
        let text = nav_text(&mut h);
        let last = text.lines().last().unwrap();
        assert!(last.contains("Trash"), "{dir:?}: {text}");
    }
    // A tag's list is a view too, with no way of folders.
    let view = rada_core::tagsearch::view_path("Red");
    h.app.open_dir(view.clone());
    h.wait("the view", |a| a.cwd == view);
    h.settle();
    with_trash(&mut h);
    let text = nav_text(&mut h);
    assert!(text.lines().last().unwrap().contains("Trash"), "{text}");
}

#[test]
fn the_folders_of_the_way_have_the_colour_of_the_folders_of_the_middle_box() {
    let (mut h, _) = path_scene();
    let buf = h.redrawn_buffer();
    let nav = h.app.nav_area;
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

/// The row of a screen that has `label` in it, and the background of the cell at `x` there.
fn row_bg(h: &mut H, label: &str, x: u16) -> ratatui::style::Color {
    let buf = h.redrawn_buffer();
    let y = (0..buf.area.height)
        .find(|y| {
            let line: String = (0..buf.area.width).map(|x| buf[(x, *y)].symbol()).collect();
            line.contains(label)
        })
        .unwrap_or_else(|| panic!("no row with {label}"));
    buf[(x, y)].bg
}

#[test]
fn in_the_trash_folder_only_the_trash_is_the_current_place() {
    // The classic sidebar and the NAV box of the panels view: the trash is highlighted,
    // and no other place (Downloads, say) is.
    let mut h = demo_scene(120, 44);
    with_trash(&mut h);
    let trash = h.app.paths.places.last().unwrap().path.clone();
    std::fs::create_dir_all(&trash).unwrap();
    go_and_wait(&mut h, &trash);
    // (The places are read again now and then: the demo's trash with them.)
    with_trash(&mut h);
    let items = h.app.side_items();
    let at = rada_tui::sidebar::current_index(&items, &h.app.cwd).expect("a current place");
    assert_eq!(items[at].name, "Trash", "{:?}", items[at]);
    let _ = h.screen();
    let (t, d) = (row_bg(&mut h, "Trash", 4), row_bg(&mut h, "Downloads", 4));
    assert_ne!(t, d, "the trash row is highlighted, Downloads is not");
    let quiet = row_bg(&mut h, "Pictures", 4);
    assert_eq!(
        d, quiet,
        "Downloads is as quiet as the places that are not here"
    );

    let mut h = scene(160, 46, "rada", false);
    with_trash(&mut h);
    let trash = h.app.paths.places.last().unwrap().path.clone();
    std::fs::create_dir_all(&trash).unwrap();
    go_and_wait(&mut h, &trash);
    with_trash(&mut h);
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &trash).expect("a current row");
    assert_eq!(rows[at].kind, rada_tui::panels::NavKind::Trash);
}

#[test]
fn a_deep_way_keeps_the_name_of_the_folder_shown_whole() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let deep = home.join("uno/due/tre/quattro/cinque/sei/sette/cartella8");
    std::fs::create_dir_all(&deep).unwrap();
    go_and_wait(&mut h, &deep);
    h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &deep).unwrap();
    assert_eq!(rows[at].name, "cartella8");
    // Every row of the way is indented by three levels at most.
    let t = rows.iter().position(|r| r.name == "PATH").unwrap();
    let most = rows[t + 1..].iter().map(|r| r.depth).max().unwrap();
    assert_eq!(most, rada_tui::panels::NAV_INDENT_MAX);
    let text = nav_text(&mut h);
    assert!(
        text.contains("cartella8"),
        "the name is there whole:\n{text}"
    );
}
