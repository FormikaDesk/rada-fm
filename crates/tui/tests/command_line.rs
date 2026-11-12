//! The command line: the bar opened with `:`. Each command, what is wrong with a line and
//! where, completion, the lines run before, and the keys that open it.

mod common;

use std::collections::HashMap;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::ViewKind;
use rada_core::model::SortKey;
use rada_tui::app::{BarItem, BarMode, Modal};
use rada_tui::keymap::{Keymap, Preset};

fn scene() -> (rada_core::testutil::Sandbox, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/alpha.txt", "a");
    sb.write("w/beta.txt", "bb");
    sb.write("w/sub/img_001.jpg", "n");
    sb.write("w/sub/img_x.jpg", "n");
    let dir = sb.path("w");
    (sb, dir)
}

fn harness() -> H {
    let (sb, dir) = scene();
    H::new(sb, dir, 120, 32)
}

/// Open the command line and type `line` after the colon.
fn type_line(h: &mut H, line: &str) {
    h.keys(":");
    assert_eq!(bar_text(h), ":", "the colon opens the command line");
    for c in line.chars() {
        h.key(KeyCode::Char(c));
    }
}

/// Type `line` on the command line and press Enter.
fn run(h: &mut H, line: &str) {
    type_line(h, line);
    h.key(KeyCode::Enter);
}

fn bar_text(h: &H) -> String {
    h.app
        .windows
        .address
        .as_ref()
        .map(|a| a.text.clone())
        .unwrap_or_default()
}

/// What the command line says is wrong, if it says it.
fn problem(h: &H) -> Option<String> {
    let a = h.app.windows.address.as_ref()?;
    a.hits.iter().find_map(|hit| match &hit.item {
        BarItem::Problem(text) => Some(text.clone()),
        _ => None,
    })
}

fn ready(h: &mut H) {
    h.wait("the folder", |a| {
        !a.tab.is_loading() && !a.tab.visible.is_empty()
    });
}

/// Run the plan the line opened and wait for it to end.
fn confirm(h: &mut H) {
    plan_modal(h);
    run_plan_and_wait(h);
}

#[test]
fn the_colon_opens_the_command_line_with_the_colon_written() {
    let mut h = harness();
    h.keys(":");
    let a = h.app.windows.address.as_ref().expect("the bar is open");
    assert_eq!((a.text.as_str(), a.cursor), (":", 1));
    assert_eq!(a.mode(), BarMode::Command);
    let screen = h.screen();
    assert!(screen.contains(":"), "{screen}");
    h.key(KeyCode::Esc);
    assert!(h.app.windows.address.is_none());
}

#[test]
fn on_an_italian_keyboard_the_colon_is_shift_and_the_dot_alone_still_shows_hidden() {
    let mut h = harness();
    // Shift and the full stop: the terminal sends a colon, with Shift held.
    h.app
        .on_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::SHIFT));
    assert_eq!(bar_text(&h), ":");
    h.key(KeyCode::Esc);
    let hidden = h.app.settings.show_hidden;
    h.keys(".");
    assert_eq!(
        h.app.settings.show_hidden, !hidden,
        "the dot shows the hidden files"
    );
    assert!(h.app.windows.address.is_none(), "and opens no bar");
}

#[test]
fn the_classic_keys_have_no_colon() {
    let mut h = harness();
    h.app.settings.keymap = Keymap::new(Preset::Classic, &HashMap::new());
    h.keys(":");
    assert!(h.app.windows.address.is_none(), "{}", h.screen());
    // The palette is still where the classic keys have it.
    h.press("ctrl+p");
    assert!(h.app.windows.address.is_some());
}

#[test]
fn mkdir_makes_a_plan_and_undo_and_redo_take_it_back_and_again() {
    let (sb, dir) = scene();
    let made = dir.join("notes");
    let mut h = H::new(sb, dir, 120, 32);
    ready(&mut h);
    run(&mut h, "mkdir notes");
    assert!(h.app.windows.address.is_none(), "the bar closes");
    plan_modal(&mut h);
    assert!(!made.exists(), "nothing is made before the plan is run");
    run_plan_and_wait(&mut h);
    assert!(made.is_dir());
    run(&mut h, "undo");
    confirm(&mut h);
    assert!(!made.exists(), "undone");
    run(&mut h, "redo");
    confirm(&mut h);
    assert!(made.is_dir(), "done again");
}

#[test]
fn a_name_with_spaces_is_written_between_quotes_or_with_backslashes() {
    let (sb, dir) = scene();
    let mut h = H::new(sb, dir.clone(), 120, 32);
    ready(&mut h);
    run(&mut h, r#"mkdir "my notes""#);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    run(&mut h, r"mkdir old\ notes");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("my notes").is_dir() && dir.join("old notes").is_dir());
}

#[test]
fn rename_with_a_pattern_renames_what_is_selected_through_a_plan() {
    let (sb, dir) = scene();
    let mut h = H::new(sb, dir.clone(), 120, 32);
    ready(&mut h);
    h.cursor_to("alpha.txt");
    run(&mut h, "rename {name}_{n:2}.{ext}");
    plan_modal(&mut h);
    assert!(dir.join("alpha.txt").exists(), "not before the plan runs");
    run_plan_and_wait(&mut h);
    assert!(dir.join("alpha_01.txt").exists());
    assert!(!dir.join("alpha.txt").exists());
}

#[test]
fn sort_takes_a_key_and_an_option() {
    let mut h = harness();
    ready(&mut h);
    run(&mut h, "sort size --reverse");
    assert_eq!(
        (h.app.tab.sort.key, h.app.tab.sort.reverse),
        (SortKey::Size, true)
    );
    run(&mut h, "sort by=name");
    assert_eq!(
        (h.app.tab.sort.key, h.app.tab.sort.reverse),
        (SortKey::Name, false)
    );
}

#[test]
fn cd_goes_to_a_folder_by_its_path() {
    let (sb, dir) = scene();
    let home = sb.dirs.home.clone();
    let mut h = H::new(sb, dir.clone(), 120, 32);
    ready(&mut h);
    run(&mut h, "cd sub");
    h.wait("the folder below", |a| a.tab.cwd == dir.join("sub"));
    run(&mut h, "cd ..");
    h.wait("the folder above", |a| a.tab.cwd == dir);
    run(&mut h, "cd ~");
    h.wait("the home folder", |a| a.tab.cwd == home);
}

fn search_view(a: &rada_tui::app::App) -> bool {
    matches!(a.tab.virtual_view(), Some(ViewKind::Search { .. })) && !a.tab.is_loading()
}

#[test]
fn find_searches_the_folders_below_by_words_or_by_a_pattern() {
    let mut h = harness();
    ready(&mut h);
    run(&mut h, "find img");
    h.wait("the search", search_view);
    let mut names = h.names();
    names.sort();
    assert_eq!(names, ["img_001.jpg", "img_x.jpg"]);
    h.press("backspace");
    h.wait("the folder again", |a| a.tab.virtual_view().is_none());
    // A pattern between slashes: in the bar it would be a path, here it is what to find.
    run(&mut h, r"find /^img_\d+\./");
    h.wait("the search", search_view);
    assert_eq!(h.names(), ["img_001.jpg"]);
}

#[test]
fn history_and_help_open_their_windows() {
    let mut h = harness();
    ready(&mut h);
    run(&mut h, "history");
    assert!(matches!(h.app.windows.modal, Some(Modal::History(_))));
    h.key(KeyCode::Esc);
    run(&mut h, "help");
    assert!(matches!(h.app.windows.modal, Some(Modal::Help)));
}

#[test]
fn what_is_wrong_with_a_line_is_said_under_it_with_its_column() {
    let mut h = harness();
    ready(&mut h);
    run(&mut h, "sort colour");
    // The colon is column 1: "colour" starts at column 7.
    let said = problem(&h).expect("a problem under the bar");
    assert!(
        said.starts_with("column 7: colour is not one of: name, size, date, type"),
        "{said}"
    );
    assert_eq!(
        bar_text(&h),
        ":sort colour",
        "the line stays to be put right"
    );
    assert!(h.screen().contains("column 7"));
    // Typing again takes the problem away.
    h.key(KeyCode::Backspace);
    assert_eq!(problem(&h), None);
    h.key(KeyCode::Esc);
    run(&mut h, r#"mkdir "open"#);
    assert!(problem(&h).unwrap().starts_with("column 8: this quote"));
    h.key(KeyCode::Esc);
    run(&mut h, "cd");
    assert!(
        problem(&h)
            .unwrap()
            .starts_with("column 4: folder is missing")
    );
}

#[test]
fn a_command_that_cannot_be_used_here_says_why() {
    let mut h = harness();
    ready(&mut h);
    run(&mut h, "find img");
    h.wait("the search", search_view);
    run(&mut h, "mkdir x");
    let said = problem(&h).expect("why not");
    assert!(said.contains("not a folder"), "{said}");
    assert!(!h.modal_is_plan());
}

#[test]
fn a_line_that_is_not_a_command_runs_the_best_suggestion_as_the_bar_does() {
    let mut h = harness();
    ready(&mut h);
    let tabs = h.app.tabs.count();
    run(&mut h, "new tab");
    assert_eq!(h.app.tabs.count(), tabs + 1);
}

#[test]
fn tab_completes_names_values_options_and_paths() {
    let mut h = harness();
    ready(&mut h);
    type_line(&mut h, "so");
    h.key(KeyCode::Tab);
    assert_eq!(bar_text(&h), ":sort");
    h.keys(" ");
    h.key(KeyCode::Tab);
    assert_eq!(bar_text(&h), ":sort name");
    h.key(KeyCode::Tab);
    assert_eq!(bar_text(&h), ":sort size", "Tab again: the next one");
    h.keys(" --r");
    h.key(KeyCode::Tab);
    assert_eq!(bar_text(&h), ":sort size --reverse");
    h.key(KeyCode::Esc);
    type_line(&mut h, "cd s");
    h.wait("the names of the folder", |a| {
        a.windows
            .address
            .as_ref()
            .is_some_and(|b| b.listed().is_some())
    });
    h.key(KeyCode::Tab);
    assert_eq!(bar_text(&h), ":cd sub/");
    h.keys("img_0");
    h.wait("the names of the folder below", |a| {
        a.windows
            .address
            .as_ref()
            .is_some_and(|b| b.listed().is_some_and(|d| d.ends_with("sub")))
    });
    h.key(KeyCode::Tab);
    assert_eq!(bar_text(&h), ":cd sub/img_001.jpg", "files too");
}

#[test]
fn the_arrows_go_through_the_lines_run_before_each_once() {
    let mut h = harness();
    ready(&mut h);
    for line in ["sort size", "sort name", "sort size"] {
        run(&mut h, line);
    }
    assert_eq!(h.app.command_history(), ["sort name", "sort size"]);
    h.keys(":");
    h.key(KeyCode::Up);
    assert_eq!(bar_text(&h), ":sort size");
    h.key(KeyCode::Up);
    assert_eq!(bar_text(&h), ":sort name");
    h.key(KeyCode::Up);
    assert_eq!(bar_text(&h), ":sort name", "the oldest stays");
    h.key(KeyCode::Down);
    assert_eq!(bar_text(&h), ":sort size");
    h.key(KeyCode::Down);
    assert_eq!(bar_text(&h), ":", "past the latest, the empty line");
}

#[test]
fn the_lines_run_are_kept_for_the_next_start() {
    let (sb, dir) = scene();
    let state = sb.dirs.rada_state();
    let mut h = H::new(sb, dir.clone(), 120, 32);
    ready(&mut h);
    run(&mut h, "sort size");
    run(&mut h, "history");
    h.key(KeyCode::Esc);
    let mut saved = Vec::new();
    for _ in 0..200 {
        saved = rada_core::uistate::load(&state).commands;
        if saved.len() == 2 {
            break;
        }
        h.pump(10);
    }
    assert_eq!(saved, ["sort size", "history"]);
    // A new start with what was saved: the arrows find them.
    let (sb2, dir2) = scene();
    let mut h2 = H::with_config(sb2, dir2, 120, 32, move |c| {
        c.saved_ui.commands = saved;
    });
    h2.keys(":");
    h2.key(KeyCode::Up);
    assert_eq!(bar_text(&h2), ":history");
    drop(h);
}

#[test]
fn a_line_that_was_not_run_is_not_kept() {
    let mut h = harness();
    ready(&mut h);
    run(&mut h, "sort colour");
    h.key(KeyCode::Esc);
    assert!(h.app.command_history().is_empty());
}
