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

/// A row of ▄ or ▀: the padding the bars draw above and below themselves.
fn is_edge(l: &str) -> bool {
    !l.trim().is_empty() && l.trim().chars().all(|c| c == '▄' || c == '▀')
}

/// The lines of the screen that are not half-block padding.
fn content_lines(h: &mut H) -> Vec<String> {
    lines(h).into_iter().filter(|l| !is_edge(l)).collect()
}

/// The status bar: the line over the hints.
fn status_row(h: &mut H) -> String {
    let l = content_lines(h);
    l[l.len() - 2].clone()
}

/// The hints: the last line that is not padding.
fn hints_row(h: &mut H) -> String {
    content_lines(h).last().cloned().unwrap_or_default()
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
        h.app.frame.hits.find(&Target::Act(Action::Back)).is_none(),
        "no history yet"
    );
    assert!(
        h.app
            .frame
            .hits
            .find(&Target::Act(Action::Forward))
            .is_none()
    );
    assert!(
        h.app
            .frame
            .hits
            .find(&Target::Act(Action::Parent))
            .is_some()
    );
    assert!(
        h.app
            .frame
            .hits
            .find(&Target::Act(Action::Refresh))
            .is_some()
    );

    h.key(KeyCode::Enter); // into sub
    h.wait("sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    h.click(&Target::Act(Action::Back));
    h.wait("back", |a| a.cwd.ends_with("proj.v1") && !a.is_loading());
    assert!(
        h.app
            .frame
            .hits
            .find(&Target::Act(Action::Forward))
            .is_some()
    );
    h.click(&Target::Act(Action::Forward));
    h.wait("forward", |a| a.cwd.ends_with("sub") && !a.is_loading());
    h.click(&Target::Act(Action::Parent));
    h.wait("parent", |a| a.cwd.ends_with("proj.v1") && !a.is_loading());
}

#[test]
fn one_field_shows_path_search_and_typing() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 30);
    let row = address_row(&mut h);
    assert!(
        row.contains("proj.v1")
            && row.contains("Search files or type a command")
            && row.contains("Ctrl+P"),
        "{row}"
    );
    h.click(&Target::Act(Action::Filter));
    assert!(
        h.app.windows.address.is_some(),
        "the field is being typed in"
    );
    h.keys("a");
    let row = address_row(&mut h);
    assert!(
        row.contains("a▏") && !row.contains("Search files"),
        "the field shows what is being typed: {row}"
    );
    h.key(KeyCode::Esc);
    assert!(h.app.windows.address.is_none());
    assert!(address_row(&mut h).contains("Search files"));
}

#[test]
fn commands_is_a_hint_at_the_bottom_and_starts_the_field() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 170, 30);
    let row = hints_row(&mut h);
    assert!(row.contains("Ctrl+P") && row.contains("commands"), "{row}");
    h.click(&Target::Act(Action::Palette));
    assert!(
        h.app.windows.address.is_some(),
        "the field takes what is typed"
    );
    assert!(h.app.modal.is_none(), "it is not a window");
}

#[test]
fn narrow_bar_drops_search_and_folds_path() {
    let mut h = deep(220);
    let row = address_row(&mut h);
    assert!(
        row.contains("Search files") && row.contains("Ctrl+P"),
        "{row}"
    );
    let mut searching = Vec::new();
    for w in (50..=220u16).rev() {
        let mut h = deep(w);
        let row = address_row(&mut h);
        searching.push(row.contains("Search"));
        assert!(
            row.contains("eeeeee"),
            "the current folder always shows ({w}): {row}"
        );
        assert!(
            unicode_width::UnicodeWidthStr::width(row.as_str()) <= w as usize,
            "{w}: the row is wider than the screen"
        );
    }
    // The search part goes as the terminal narrows and does not come back: once it is gone it
    // stays gone at every narrower width.
    let first_gone = searching.iter().position(|s| !s).expect("it goes");
    assert!(first_gone > 0, "there is room for it on a wide terminal");
    assert!(searching[first_gone..].iter().all(|s| !s), "{searching:?}");
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
        .frame
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
        "open", "select", "copy", "paste", "trash", "rename", "undo", "new tab", "commands",
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
    h.keys("a");
    let row = hints_row(&mut h);
    assert!(
        row.contains("go") && row.contains("cancel"),
        "typing in the field: {row}"
    );
}

#[test]
fn the_hints_follow_the_keymap_in_use() {
    use rada_tui::keymap::{Keymap, Preset};
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 200, 30);
    h.app.settings.keymap = Keymap::new(Preset::Vim, &Default::default());
    let row = hints_row(&mut h);
    assert!(
        row.contains(" y ") && row.contains("copy") && !row.contains("Ctrl+C"),
        "the vim keys are what the hints show: {row}"
    );
    // A rebound key shows its new name.
    let mut keys = std::collections::HashMap::new();
    keys.insert("copy".to_string(), vec!["f9".to_string()]);
    h.app.settings.keymap = Keymap::new(Preset::VimClassic, &keys);
    let row = hints_row(&mut h);
    assert!(row.contains("F9") && row.contains("copy"), "{row}");
}

#[test]
fn hints_can_be_switched_off_and_the_status_stays() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 160, 30);
    h.app.settings.show_hints = false;
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
    h.press("f2");
    assert!(matches!(h.app.modal, Some(Modal::Input(_))));
    let row = hints_row(&mut h);
    assert!(row.contains("Esc") && row.contains("cancel"), "{row}");
    assert!(!row.contains("trash"), "{row}");
}
