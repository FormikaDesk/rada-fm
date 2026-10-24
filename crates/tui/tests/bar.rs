//! The bar at the top as one field: a path, words that filter the files and offer commands,
//! Enter and Esc, and what Ctrl+L, Ctrl+P and Ctrl+F start.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_tui::app::{BarItem, BarMode, Modal};
use rada_tui::hits::Target;
use rada_tui::keymap::Action;

fn files() -> H {
    let sb = rada_core::testutil::Sandbox::new();
    for n in ["alpha.txt", "beta.txt", "gamma.md", "notes.txt"] {
        sb.write(format!("w/{n}"), n);
    }
    sb.mkdir("w/docs");
    let dir = sb.path("w");
    H::new(sb, dir, 140, 36)
}

fn row(h: &mut H) -> String {
    h.screen().lines().nth(2).unwrap_or_default().to_string()
}

#[test]
fn ctrl_l_selects_the_path_and_the_first_key_replaces_it() {
    let mut h = files();
    h.press("ctrl+l");
    let a = h.app.windows.address.as_ref().expect("the field is open");
    assert!(a.fresh && a.text.contains("w/"), "{:?}", a.text);
    assert_eq!(a.mode(), BarMode::Path);
    // Typing a word replaces the path: it is a search now.
    h.keys("be");
    let a = h.app.windows.address.as_ref().unwrap();
    assert_eq!(a.text, "be");
    assert_eq!(a.mode(), BarMode::Search);
    assert_eq!(
        h.names(),
        ["beta.txt"],
        "the files are filtered as it is typed"
    );
    // Moving the cursor first leaves the path to be edited.
    h.key(KeyCode::Esc);
    h.press("ctrl+l");
    h.key(KeyCode::End);
    assert!(!h.app.windows.address.as_ref().unwrap().fresh);
}

#[test]
fn ctrl_p_and_ctrl_f_start_from_nothing() {
    for key in ["ctrl+p", "ctrl+f"] {
        let mut h = files();
        h.press(key);
        let a = h.app.windows.address.as_ref().expect(key);
        assert!(a.text.is_empty() && !a.fresh, "{key}");
        assert_eq!(a.mode(), BarMode::Empty);
        assert!(h.app.modal.is_none(), "{key}: not a window");
        assert!(
            row(&mut h).contains("▏Search files or type a command"),
            "{}",
            row(&mut h)
        );
    }
}

#[test]
fn words_filter_the_files_and_offer_the_commands_that_match() {
    let mut h = files();
    h.press("ctrl+p");
    h.keys("rena");
    let a = h.app.windows.address.as_ref().unwrap();
    let first = a.hits.first().expect("a command matches");
    assert!(matches!(first.item, BarItem::Command(Action::Rename)));
    // The command can be asked for in the other language too.
    let s = h.screen();
    assert!(s.contains("Rename") && s.contains("command"), "{s}");
    // The files are filtered by the same words (none has "rena" in its name).
    assert!(h.names().is_empty(), "{:?}", h.names());
}

#[test]
fn enter_runs_the_command_picked_with_the_arrows() {
    let mut h = files();
    h.press("ctrl+p");
    h.keys("rena");
    h.key(KeyCode::Down); // pick the first
    assert_eq!(
        h.app.windows.address.as_ref().unwrap().hit_selected,
        Some(0)
    );
    h.key(KeyCode::Enter);
    assert!(h.app.windows.address.is_none(), "the field closed");
    assert!(h.app.filter.is_none(), "and took its filter off");
    // Rename needs a file under the cursor: with none it does nothing, but the command ran
    // (the bar is not left open holding words).
    let mut h = files();
    h.press("ctrl+p");
    h.keys("new f");
    let hit = h
        .app
        .windows
        .address
        .as_ref()
        .unwrap()
        .hits
        .iter()
        .position(|x| matches!(x.item, BarItem::Command(Action::NewFolder)))
        .expect("New folder is offered");
    for _ in 0..=hit {
        h.key(KeyCode::Down);
    }
    h.key(KeyCode::Enter);
    assert!(
        matches!(h.app.modal, Some(Modal::Input(_))),
        "{}",
        h.screen()
    );
}

#[test]
fn enter_keeps_the_filter_when_files_match_and_esc_takes_it_off() {
    let mut h = files();
    h.press("ctrl+f");
    h.keys("txt");
    assert_eq!(h.names(), ["alpha.txt", "beta.txt", "notes.txt"]);
    h.key(KeyCode::Enter);
    assert!(h.app.windows.address.is_none(), "the field is done");
    assert_eq!(h.names().len(), 3, "the filter stays on the folder");
    assert!(
        row(&mut h).contains("txt"),
        "and the field shows it: {}",
        row(&mut h)
    );
    // Esc, with the field closed, clears the filter (as before).
    h.key(KeyCode::Esc);
    assert_eq!(h.names().len(), 5);
    // Esc inside the field takes the filter off and returns to the path.
    h.press("ctrl+f");
    h.keys("gam");
    assert_eq!(h.names(), ["gamma.md"]);
    h.key(KeyCode::Esc);
    assert!(h.app.windows.address.is_none() && h.app.filter.is_none());
    assert_eq!(h.names().len(), 5);
    assert!(row(&mut h).contains("Search files"));
}

#[test]
fn enter_with_no_file_matching_runs_the_best_command() {
    let mut h = files();
    h.press("ctrl+p");
    h.keys("compress");
    assert!(h.names().is_empty());
    h.key(KeyCode::Enter);
    // Nothing is selected and no file is under a filter: Compress asks for the files (or
    // does nothing), but the field is closed and the filter gone.
    assert!(h.app.windows.address.is_none() && h.app.filter.is_none());
}

#[test]
fn a_path_is_completed_and_gone_to_not_searched() {
    let mut h = files();
    h.press("ctrl+l");
    h.keys("~/");
    assert_eq!(
        h.app.windows.address.as_ref().unwrap().mode(),
        BarMode::Path
    );
    assert!(h.app.filter.is_none(), "a path filters nothing");
    h.key(KeyCode::Esc);
    h.press("ctrl+l");
    h.keys("..");
    h.key(KeyCode::Enter);
    h.wait("the parent", |a| a.cwd.ends_with("work") && !a.is_loading());
}

#[test]
fn backspace_on_an_empty_field_leaves_it() {
    let mut h = files();
    h.press("ctrl+p");
    h.keys("ab");
    h.key(KeyCode::Backspace);
    h.key(KeyCode::Backspace);
    assert!(
        h.app.windows.address.is_some(),
        "the field is empty but still open"
    );
    assert!(h.app.filter.is_none(), "an empty field filters nothing");
    h.key(KeyCode::Backspace);
    assert!(h.app.windows.address.is_none(), "one more leaves it");
}

#[test]
fn the_arrows_move_through_the_files_when_there_is_no_command_to_pick() {
    let mut h = files();
    h.press("ctrl+f");
    h.keys("txt");
    let before = h.cursor_name();
    h.key(KeyCode::Down);
    let after = h.cursor_name();
    // Either a command was picked (when one matched "txt") or the cursor moved; the field
    // never swallows the key.
    let a = h.app.windows.address.as_ref().unwrap();
    assert!(a.hit_selected.is_some() || before != after);
}

#[test]
fn clicking_the_field_and_its_key_start_the_typing() {
    let mut h = files();
    h.click(&Target::Act(Action::Palette));
    assert!(h.app.windows.address.is_some());
    h.key(KeyCode::Esc);
    h.click(&Target::Act(Action::Filter));
    assert!(h.app.windows.address.is_some());
}
