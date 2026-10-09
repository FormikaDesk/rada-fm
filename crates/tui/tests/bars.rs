//! The top and bottom bars: what is a button, what is only text, how they give way when the
//! terminal narrows, and what the hints say.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_tui::app::Modal;
use rada_tui::hits::Target;
use rada_tui::keymap::Action;

fn header_row(h: &mut H) -> String {
    h.screen().lines().next().unwrap_or_default().to_string()
}

fn footer_row(h: &mut H) -> String {
    h.screen().lines().last().unwrap_or_default().to_string()
}

fn deep(w: u16) -> H {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("aa/bbbbbb/cccccc/dddddd/eeeeee/x.txt", "x");
    let start = sb.path("aa/bbbbbb/cccccc/dddddd/eeeeee");
    H::new(sb, start, w, 30)
}

#[test]
fn unusable_buttons_are_not_clickable_and_usable_ones_are() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 30);
    let _ = h.screen();
    assert!(
        h.app.hits.find(&Target::Act(Action::Back)).is_none(),
        "no history yet"
    );
    assert!(h.app.hits.find(&Target::Act(Action::Forward)).is_none());
    assert!(h.app.hits.find(&Target::Act(Action::Parent)).is_some());

    h.key(KeyCode::Enter); // into sub
    h.wait("sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    h.click(&Target::Act(Action::Back));
    h.wait("back", |a| a.cwd.ends_with("proj.v1") && !a.is_loading());
    assert!(h.app.hits.find(&Target::Act(Action::Forward)).is_some());
    h.click(&Target::Act(Action::Forward));
    h.wait("forward", |a| a.cwd.ends_with("sub") && !a.is_loading());
    h.click(&Target::Act(Action::Parent));
    h.wait("parent", |a| a.cwd.ends_with("proj.v1") && !a.is_loading());
}

#[test]
fn the_filter_field_and_the_jump_button_are_labelled_and_clickable() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 30);
    let row = header_row(&mut h);
    assert!(row.contains("Filter…") && row.contains("Go to…"), "{row}");
    assert!(row.contains("Ctrl+F") && row.contains("Ctrl+P"), "{row}");
    h.click(&Target::Act(Action::Filter));
    assert!(h.app.filter.as_ref().is_some_and(|f| f.editing));
    h.keys("a");
    let row = header_row(&mut h);
    assert!(row.contains("▽ a"), "the field shows what is typed: {row}");
    h.key(KeyCode::Esc);
    h.click(&Target::Act(Action::Palette));
    assert!(matches!(h.app.modal, Some(Modal::Palette(_))));
}

#[test]
fn when_it_narrows_the_count_goes_first_then_the_path_folds_and_the_words_stay() {
    // Wide: everything, count included.
    let mut h = deep(220);
    let row = header_row(&mut h);
    assert!(row.contains("1 item"), "{row}");
    assert!(
        !row.contains('…') || row.contains("Filter…"),
        "path whole: {row}"
    );

    // Narrower: the count is gone while the path is still whole.
    let full = h.app.cwd.to_string_lossy().len();
    let mut found = false;
    for w in (60..=220u16).rev() {
        let mut h = deep(w);
        let row = header_row(&mut h);
        let folded = row.matches('…').count() > 2; // Filter…, Go to… and the path's own
        let counted = row.contains("1 item");
        if !counted && !folded && w as usize > 60 {
            found = true; // count dropped, path not yet folded
            break;
        }
        assert!(
            !(folded && counted),
            "the path must not fold before the count has gone (width {w}): {row}"
        );
    }
    assert!(
        found,
        "there is a width where only the count is gone (path is {full} chars)"
    );

    // Very narrow: the path is folded, and the two words are still there.
    let mut h = deep(70);
    let row = header_row(&mut h);
    assert!(row.contains("Filter") && row.contains("Go to"), "{row}");
    assert!(
        row.contains("eeeeee"),
        "the current folder always shows: {row}"
    );
}

#[test]
fn the_folded_part_of_a_path_opens_a_list_to_pick_from() {
    let mut h = deep(80);
    let _ = h.screen();
    let more = h
        .app
        .hits
        .all()
        .find_map(|(_, t)| matches!(t, Target::CrumbMore(_)).then(|| t.clone()))
        .expect("a folded path has a clickable …");
    let Target::CrumbMore(hidden) = more.clone() else {
        unreachable!()
    };
    assert!(hidden.len() >= 2, "{hidden:?}");
    h.click(&more);
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
    let s = h.screen();
    assert!(s.contains("bbbbbb") || s.contains("cccccc"), "{s}");
    // Choosing an entry goes there.
    h.key(KeyCode::Enter);
    let first = hidden[0].clone();
    h.wait("the chosen folder", move |a| {
        a.cwd == first && !a.is_loading()
    });
}

#[test]
fn hints_follow_the_situation() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 160, 30);
    let row = footer_row(&mut h);
    for k in ["open", "mark", "copy", "paste", "trash", "rename"] {
        assert!(row.contains(k), "{k}: {row}");
    }
    assert!(row.contains("Ctrl+C"), "keys are shown on caps: {row}");

    h.keys(" "); // select an item
    let row = footer_row(&mut h);
    assert!(row.contains("1 selected"), "{row}");
    assert!(
        row.contains("bulk rename") && row.contains("clear"),
        "{row}"
    );
    assert!(!row.contains("open"), "{row}");

    h.press("ctrl+c");
    let row = footer_row(&mut h);
    assert!(row.contains("1 item ready to paste"), "{row}");

    h.key(KeyCode::Esc); // clears the selection
    h.press("ctrl+f");
    let row = footer_row(&mut h);
    assert!(
        row.contains("keep") && row.contains("clear"),
        "typing a filter: {row}"
    );
}

#[test]
fn hints_can_be_switched_off_and_still_give_way_to_the_state() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 160, 30);
    h.app.show_hints = false;
    let row = footer_row(&mut h);
    assert!(!row.contains("open") && !row.contains("Ctrl+C"), "{row}");
    h.keys(" ");
    assert!(footer_row(&mut h).contains("1 selected"));
}

#[test]
fn a_window_changes_the_hints_to_its_own_keys() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 120, 30);
    h.press("ctrl+p");
    let row = footer_row(&mut h);
    assert!(row.contains("Esc") && row.contains("close"), "{row}");
    assert!(!row.contains("trash"), "{row}");
}
