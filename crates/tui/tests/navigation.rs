//! Back and forward through the folders visited, as in a browser.

mod common;

use common::*;
use crossterm::event::KeyCode;

fn at(h: &mut H, name: &str) {
    let n = name.to_string();
    h.wait(&format!("in {name}"), move |a| {
        a.cwd.ends_with(&n) && !a.is_loading()
    });
}

#[test]
fn back_and_forward_walk_through_the_visited_folders() {
    let (sb, dir) = sandbox_with_files();
    let start = dir.file_name().unwrap().to_string_lossy().into_owned();
    let mut h = H::new(sb, dir, 120, 30);
    assert!(!h.app.nav.can_back() && !h.app.nav.can_forward());

    h.key(KeyCode::Enter); // into sub
    at(&mut h, "sub");
    assert!(h.app.nav.can_back() && !h.app.nav.can_forward());

    h.press("alt+left");
    at(&mut h, &start);
    assert!(!h.app.nav.can_back() && h.app.nav.can_forward());
    assert_eq!(
        h.app.current().map(|e| e.display.clone()).as_deref(),
        Some("sub"),
        "the cursor is back on the folder we came out of"
    );

    h.press("alt+right");
    at(&mut h, "sub");
    assert!(h.app.nav.can_back() && !h.app.nav.can_forward());
}

#[test]
fn alt_up_goes_to_the_parent_whatever_the_key_preset() {
    let (sb, dir) = sandbox_with_files();
    let start = dir.file_name().unwrap().to_string_lossy().into_owned();
    let mut h = H::new(sb, dir, 120, 30);
    h.key(KeyCode::Enter);
    at(&mut h, "sub");
    h.press("alt+up");
    at(&mut h, &start);
}

#[test]
fn going_somewhere_new_after_back_drops_the_forward_history() {
    let (sb, dir) = sandbox_with_files();
    sb.mkdir("proj.v1/zother");
    let start = dir.file_name().unwrap().to_string_lossy().into_owned();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.key(KeyCode::Enter); // sub
    at(&mut h, "sub");
    h.press("alt+left");
    at(&mut h, &start);
    assert!(h.app.nav.can_forward());
    h.app.open_dir(dir.join("zother"));
    at(&mut h, "zother");
    assert!(!h.app.nav.can_forward(), "forward is gone");
    h.press("alt+left");
    at(&mut h, &start);
}

#[test]
fn a_folder_that_vanished_is_reported_and_the_history_stays_as_it_was() {
    let (sb, dir) = sandbox_with_files();
    let start = dir.file_name().unwrap().to_string_lossy().into_owned();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.key(KeyCode::Enter);
    at(&mut h, "sub");
    h.press("alt+left");
    at(&mut h, &start);
    std::fs::remove_dir_all(dir.join("sub")).unwrap();
    h.press("alt+right");
    h.wait("the error", |a| a.toast.is_some());
    assert!(h.app.cwd.ends_with(&start), "still where we were");
    assert!(h.app.nav.can_forward(), "the entry is still there");
}

#[test]
fn back_and_forward_can_be_rebound_and_are_listed_in_the_help() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 120, 56);
    h.press("F1");
    let s = h.screen();
    assert!(s.contains("Back to the previous folder"), "{s}");
    assert!(s.contains("Alt+←"), "{s}");
}
