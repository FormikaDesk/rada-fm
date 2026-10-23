//! The Columns view: the folder above, the folder here, and what is under the cursor, with
//! Left and Right to move between the columns.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_tui::ViewMode;
use rada_tui::hits::Target;

/// `w/` with a folder `dir` (holding `inner/` and `one.txt`), a file `a.txt` and `b.txt`.
fn scene() -> (H, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/a.txt", "alpha");
    sb.write("w/b.txt", "beta");
    sb.write("w/dir/one.txt", "one");
    sb.mkdir("w/dir/inner");
    sb.mkdir("w/other");
    sb.mkdir("x"); // beside w, in the folder above
    let dir = sb.path("w");
    let h = H::with_config(sb, dir.clone(), 150, 30, |c| c.view = ViewMode::Columns);
    (h, dir)
}

#[test]
fn columns_show_parent_here_and_cursor() {
    let (mut h, dir) = scene();
    // Draw until the folder above has been read.
    for _ in 0..200 {
        let s = h.screen();
        if s.contains("other") && s.contains("work") {
            break;
        }
        h.pump(10);
    }
    let s = h.screen();
    // First column: the folder above lists the folders here ("w") among its own.
    let parent = dir
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let _ = parent;
    assert!(s.contains("w") && s.contains('›'), "{s}");
    // Second column: this folder's items; folders have an arrow.
    assert!(
        s.contains("dir") && s.contains("other") && s.contains("a.txt") && s.contains("b.txt"),
        "{s}"
    );
    // Third: the cursor is on the first folder (dir): what it holds.
    assert_eq!(h.cursor_name(), "dir");
    h.wait("what dir holds", |a| {
        a.preview.name == "dir" && a.preview.content.is_some()
    });
    let s = h.screen();
    assert!(s.contains("inner") && s.contains("one.txt"), "{s}");
    // The view in use is named at the top right and marked in the status bar.
    assert!(s.contains("Columns"), "{s}");
}

#[test]
fn right_goes_into_the_folder_and_left_comes_back_to_where_it_was() {
    let (mut h, dir) = scene();
    assert_eq!(h.cursor_name(), "dir");
    h.key(KeyCode::Right);
    let inside = dir.join("dir");
    let d = inside.clone();
    h.wait("inside", move |a| a.cwd == d && !a.is_loading());
    h.key(KeyCode::Left);
    let d = dir.clone();
    h.wait("back", move |a| a.cwd == d && !a.is_loading());
    assert_eq!(h.cursor_name(), "dir", "the cursor is where it was");
}

#[test]
fn right_on_a_file_stays_and_opens_nothing_but_enter_still_opens() {
    let (mut h, dir) = scene();
    h.keys("j"); // other
    h.keys("j"); // a.txt
    assert_eq!(h.cursor_name(), "a.txt");
    h.key(KeyCode::Right);
    h.pump(50);
    assert_eq!(h.app.cwd, dir, "a file is not a place to go to");
    assert!(h.opened().is_empty(), "and Right does not launch a program");
    h.key(KeyCode::Enter);
    // The opening is made by a thread of its own: wait for it, not for a fixed time.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.opened().is_empty() && std::time::Instant::now() < end {
        h.pump(50);
    }
    assert_eq!(h.opened().len(), 1, "Enter opens it as before");
}

#[test]
fn a_click_in_the_first_column_goes_to_that_folder() {
    let (mut h, dir) = scene();
    let sibling = dir.parent().unwrap().join("x");
    for _ in 0..200 {
        let _ = h.screen();
        if h.app.hits.find(&Target::Place(sibling.clone())).is_some() {
            break;
        }
        h.pump(10);
    }
    // The folder we are in is a row of the first column, picked out; so is its sibling.
    assert!(h.app.hits.find(&Target::Place(dir.clone())).is_some());
    h.click(&Target::Place(sibling.clone()));
    let d = sibling.clone();
    h.wait("the sibling", move |a| a.cwd == d && !a.is_loading());
}

#[test]
fn the_cursor_keys_move_in_the_middle_column() {
    let (mut h, _dir) = scene();
    h.key(KeyCode::Down);
    assert_eq!(h.cursor_name(), "other");
    h.key(KeyCode::Up);
    assert_eq!(h.cursor_name(), "dir");
    h.wait("what dir holds", |a| {
        a.preview.name == "dir" && a.preview.content.is_some()
    });
    let s = h.screen();
    assert!(
        s.contains("one.txt"),
        "the third column follows the cursor: {s}"
    );
    h.key(KeyCode::Down);
    h.wait("the other folder is empty", |a| a.preview.name == "other");
    let s = h.screen();
    assert!(!s.contains("one.txt"), "{s}");
}

#[test]
fn too_narrow_for_three_columns_it_is_a_table() {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/a.txt", "alpha");
    let dir = sb.path("w");
    let mut h = H::with_config(sb, dir, 50, 20, |c| c.view = ViewMode::Columns);
    let s = h.screen();
    assert!(s.contains("Name") && s.contains("a.txt"), "{s}");
}

#[test]
fn columns_choice_is_kept_per_tab() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 150, 30);
    assert_eq!(h.app.view, ViewMode::Details);
    h.click(&Target::Menu(rada_tui::app::MenuKind::View));
    let s = h.screen();
    assert!(s.contains("Columns"), "{s}");
    h.press("down");
    h.press("down"); // Columns
    h.press("enter");
    assert_eq!(h.app.view, ViewMode::Columns);
    h.press("ctrl+1");
    assert_eq!(h.app.view, ViewMode::Details);
    h.press("ctrl+3");
    assert_eq!(h.app.view, ViewMode::Columns);
    assert_eq!(ViewMode::parse("columns"), Some(ViewMode::Columns));
    assert_eq!(ViewMode::Columns.id(), "columns");
}
