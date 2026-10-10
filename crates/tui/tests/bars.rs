//! The bars: tabs, address, commands, status and hints. What is a button, what is only text,
//! how they give way when the terminal narrows, and what the hints say.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_tui::app::Modal;
use rada_tui::hits::Target;
use rada_tui::keymap::Action;

fn lines(h: &mut H) -> Vec<String> {
    h.screen().lines().map(str::to_string).collect()
}

/// The address row, the second line of the screen.
fn address_row(h: &mut H) -> String {
    lines(h).get(1).cloned().unwrap_or_default()
}

/// The status bar and the hints row: the last two lines (when the hints are shown).
fn status_row(h: &mut H) -> String {
    let l = lines(h);
    l[l.len() - 2].clone()
}

fn hints_row(h: &mut H) -> String {
    lines(h).last().cloned().unwrap_or_default()
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
    assert!(h.app.hits.find(&Target::Act(Action::Refresh)).is_some());

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
fn the_search_field_is_labelled_clickable_and_shows_what_is_typed() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 30);
    let row = address_row(&mut h);
    assert!(
        row.contains("Search in proj.v1") && row.contains("Ctrl+F"),
        "{row}"
    );
    h.click(&Target::Act(Action::Filter));
    assert!(h.app.filter.as_ref().is_some_and(|f| f.editing));
    h.keys("a");
    let row = address_row(&mut h);
    assert!(
        row.contains("a▏") && !row.contains("Search in"),
        "the field shows what is being typed: {row}"
    );
    h.key(KeyCode::Esc);
    assert!(address_row(&mut h).contains("Search in"));
}

#[test]
fn go_to_is_a_hint_at_the_bottom_and_opens_the_palette() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 170, 30);
    let row = hints_row(&mut h);
    assert!(row.contains("Ctrl+P") && row.contains("go to"), "{row}");
    h.click(&Target::Act(Action::Palette));
    assert!(matches!(h.app.modal, Some(Modal::Palette(_))));
}

#[test]
fn when_it_narrows_the_search_shrinks_and_goes_and_the_path_folds_around_the_current_folder() {
    let mut h = deep(220);
    let row = address_row(&mut h);
    assert!(row.contains("Search in") && row.contains("Ctrl+F"), "{row}");
    for w in (50..=220u16).rev() {
        let mut h = deep(w);
        let row = address_row(&mut h);
        // The field is there from 70 columns, shorter as the terminal narrows.
        assert_eq!(row.contains("Search"), w >= 70, "{w}: {row}");
        assert!(
            row.contains("eeeeee"),
            "the current folder always shows ({w}): {row}"
        );
        assert!(
            unicode_width::UnicodeWidthStr::width(row.as_str()) <= w as usize,
            "{w}: the row is wider than the screen"
        );
    }
    // The sandbox path is long: it folds, whatever the width.
    let mut h = deep(80);
    let row = address_row(&mut h);
    assert!(row.contains('…'), "{row}");
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
    let mut h = H::new(sb, dir, 200, 30);
    let row = hints_row(&mut h);
    for k in [
        "open", "select", "copy", "paste", "trash", "rename", "undo", "new tab", "go to",
    ] {
        assert!(row.contains(k), "{k}: {row}");
    }
    assert!(row.contains("Ctrl+C"), "keys are shown on caps: {row}");

    h.keys(" "); // select an item
    let row = status_row(&mut h);
    assert!(row.contains("1 selected"), "{row}");
    let row = hints_row(&mut h);
    assert!(row.contains("rename all") && row.contains("clear"), "{row}");
    assert!(!row.contains("open"), "{row}");

    h.press("ctrl+c");
    let row = status_row(&mut h);
    assert!(row.contains("1 item ready to paste"), "{row}");

    h.key(KeyCode::Esc); // clears the selection
    h.press("ctrl+f");
    let row = hints_row(&mut h);
    assert!(
        row.contains("keep") && row.contains("clear"),
        "typing a search: {row}"
    );
}

#[test]
fn the_hints_follow_the_keymap_in_use() {
    use rada_tui::keymap::{Keymap, Preset};
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 200, 30);
    h.app.keymap = Keymap::new(Preset::Vim, &Default::default());
    let row = hints_row(&mut h);
    assert!(
        row.contains(" y ") && row.contains("copy") && !row.contains("Ctrl+C"),
        "the vim keys are what the hints show: {row}"
    );
    // A rebound key shows its new name.
    let mut keys = std::collections::HashMap::new();
    keys.insert("copy".to_string(), vec!["f9".to_string()]);
    h.app.keymap = Keymap::new(Preset::VimClassic, &keys);
    let row = hints_row(&mut h);
    assert!(row.contains("F9") && row.contains("copy"), "{row}");
}

#[test]
fn hints_can_be_switched_off_and_the_status_stays() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 160, 30);
    h.app.show_hints = false;
    let last = hints_row(&mut h);
    assert!(
        !last.contains("Ctrl+C") && last.contains("items"),
        "the status bar is now the last line: {last}"
    );
    h.keys(" ");
    assert!(hints_row(&mut h).contains("1 selected"));
}

#[test]
fn a_window_changes_the_hints_to_its_own_keys() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 120, 30);
    h.press("ctrl+p");
    let row = hints_row(&mut h);
    assert!(row.contains("Esc") && row.contains("close"), "{row}");
    assert!(!row.contains("trash"), "{row}");
}
