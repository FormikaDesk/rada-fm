//! Which box has the keyboard, and what the keys do in each.

use crate::common::*;
use crate::harness::*;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

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
    assert!(h.app.side_focus && !h.app.preview.focus);
    assert_eq!(border_of(&mut h, "[ NAV ]"), th.accent);
    assert_eq!(border_of(&mut h, folder), th.muted);
    h.press("tab");
    assert!(!h.app.side_focus && h.app.preview.focus);
    assert_eq!(border_of(&mut h, "[ PREVIEW ]"), th.accent);
    h.press("tab");
    assert!(!h.app.side_focus && !h.app.preview.focus);
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
    assert!(h.app.preview.focus);
    assert_eq!(h.app.preview.scroll, 0);
    h.press("down");
    h.press("down");
    assert_eq!(h.app.preview.scroll, 2);
    h.press("up");
    assert_eq!(h.app.preview.scroll, 1);
    // The cursor of the folder did not move.
    assert_eq!(h.cursor_name(), "notes.txt");
    h.press("esc");
    assert!(!h.app.preview.focus);
}
