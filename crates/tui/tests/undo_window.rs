//! The window of an undo and of a redo: a question that says what is taken back, a line per
//! thing that comes back or goes away with a label, and buttons that say what they do.

mod common;

use common::*;
use crossterm::event::KeyCode;

fn copy_all(h: &mut H, dest: &std::path::Path) {
    h.press("ctrl+a");
    h.press("ctrl+c");
    h.app.open_dir(dest.to_path_buf());
    let d = dest.to_path_buf();
    h.wait("the destination", move |a| {
        a.tab.cwd == d && !a.tab.is_loading()
    });
    h.press("ctrl+v");
    plan_modal(h);
    run_plan_and_wait(h);
}

fn sandbox(n: usize) -> (H, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    for i in 0..n {
        sb.write(format!("w/f{i:02}.txt"), "x".repeat(10 + i));
    }
    let dest = sb.mkdir("Documenti");
    let dir = sb.path("w");
    (H::new(sb, dir, 150, 44), dest)
}

#[test]
fn undoing_a_copy_asks_what_it_takes_back_and_labels_what_goes() {
    let (mut h, dest) = sandbox(3);
    copy_all(&mut h, &dest);
    h.app.advance_notices(std::time::Duration::from_secs(7));
    h.press("ctrl+z");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(s.contains("Undo the copy of 3 items?"), "{s}");
    for name in ["f00.txt", "f01.txt", "f02.txt"] {
        assert!(s.contains(name), "{name}:\n{s}");
    }
    assert_eq!(s.matches("will be removed").count(), 3, "{s}");
    assert!(s.contains("Undo the copy") && s.contains("Cancel"), "{s}");
    // It cannot itself be undone: no note about that.
    assert!(!s.contains("You can undo this"), "{s}");
    run_plan_and_wait(&mut h);
    assert!(!dest.join("f00.txt").exists());
}

#[test]
fn many_items_are_cut_to_six_and_a_count() {
    let (mut h, dest) = sandbox(9);
    copy_all(&mut h, &dest);
    h.app.advance_notices(std::time::Duration::from_secs(7));
    h.press("ctrl+z");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(s.contains("Undo the copy of 9 items?"), "{s}");
    assert_eq!(s.matches("will be removed").count(), 6, "{s}");
    assert!(s.contains("… and 3 more"), "{s}");
    h.key(KeyCode::Esc);
}

#[test]
fn a_redo_asks_to_repeat_and_offers_the_way_back() {
    let (mut h, dest) = sandbox(2);
    copy_all(&mut h, &dest);
    h.app.advance_notices(std::time::Duration::from_secs(7));
    h.press("ctrl+z");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    h.app.advance_notices(std::time::Duration::from_secs(7));
    h.press("ctrl+y");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(s.contains("Repeat the copy of 2 items?"), "{s}");
    assert!(s.contains("Repeat the copy"), "{s}");
    assert!(
        s.contains("You can undo this"),
        "it can be taken back again: {s}"
    );
    h.key(KeyCode::Esc);
}

#[test]
fn what_cannot_be_undone_is_said_in_a_strip() {
    let (mut h, dest) = sandbox(2);
    copy_all(&mut h, &dest);
    h.app.advance_notices(std::time::Duration::from_secs(7));
    // One of the copies is changed after the copy: undo leaves it alone and says so.
    std::fs::write(dest.join("f00.txt"), "changed later and longer").unwrap();
    h.press("ctrl+z");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(
        s.contains("changed since the operation") && s.contains("left untouched"),
        "{s}"
    );
    h.key(KeyCode::Esc);
}
