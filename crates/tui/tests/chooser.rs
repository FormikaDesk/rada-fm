//! rada as a file chooser for another program: what Enter and Esc hand over, and the promise
//! that nothing is deleted, moved or renamed on the way.

mod common;

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::testutil::snapshot;
use rada_tui::chooser::Chooser;
use rada_tui::keymap::Action;

fn chooser_at(ch: Chooser) -> (H, std::path::PathBuf, std::path::PathBuf) {
    let (sb, dir) = sandbox_with_files();
    let root = sb.work.clone();
    let h = H::with_config(sb, dir.clone(), 120, 30, move |c| c.chooser = Some(ch));
    (h, dir, root)
}

fn alt_enter(h: &mut H) {
    h.app
        .on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
}

#[test]
fn enter_on_a_file_chooses_it_and_quits() {
    let (mut h, dir, _) = chooser_at(Chooser::file(false));
    assert_eq!(h.names(), ["sub", "a.txt", "b.txt"]);
    h.key(KeyCode::Down); // a.txt: sub is first
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("a.txt")]));
    assert!(h.app.should_quit);
}

#[test]
fn enter_on_a_folder_goes_in_and_chooses_nothing() {
    let (mut h, dir, _) = chooser_at(Chooser::file(false));
    h.key(KeyCode::Enter);
    h.wait("in sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    assert!(h.app.chosen.is_none() && !h.app.should_quit);
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("sub/inner.txt")]));
}

#[test]
fn the_right_arrow_never_opens_a_file() {
    let (mut h, _, _) = chooser_at(Chooser::file(false));
    h.key(KeyCode::Down);
    h.key(KeyCode::Down);
    h.key(KeyCode::Right);
    h.keys("l");
    assert!(h.opened().is_empty(), "{:?}", h.opened());
    assert!(h.app.chosen.is_none() && !h.app.should_quit);
}

#[test]
fn a_double_click_chooses_a_file_and_enters_a_folder() {
    use rada_tui::hits::Target;
    let (mut h, dir, _) = chooser_at(Chooser::file(false));
    h.double_click(&Target::Row(1));
    assert_eq!(h.app.chosen, Some(vec![dir.join("a.txt")]));
    assert!(h.opened().is_empty());
    let (mut h, _, _) = chooser_at(Chooser::file(false));
    h.double_click(&Target::Row(0));
    h.wait("in sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    assert!(h.app.chosen.is_none());
}

#[test]
fn several_files_are_marked_and_chosen_together() {
    let (mut h, dir, _) = chooser_at(Chooser::file(true));
    // A folder cannot be marked in a chooser of files.
    h.key(KeyCode::Char(' '));
    assert!(h.marked_names().is_empty(), "{:?}", h.marked_names());
    assert_eq!(h.cursor_name(), "a.txt", "the cursor moves on");
    h.key(KeyCode::Char(' '));
    h.key(KeyCode::Char(' '));
    assert_eq!(h.marked_names(), ["a.txt", "b.txt"]);
    h.key(KeyCode::Enter);
    assert_eq!(
        h.app.chosen,
        Some(vec![dir.join("a.txt"), dir.join("b.txt")])
    );
    assert!(h.app.should_quit);
}

#[test]
fn enter_with_nothing_marked_chooses_the_file_under_the_cursor() {
    let (mut h, dir, _) = chooser_at(Chooser::file(true));
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("a.txt")]));
}

#[test]
fn a_single_chooser_takes_no_marks() {
    let (mut h, _, _) = chooser_at(Chooser::file(false));
    h.key(KeyCode::Down);
    h.key(KeyCode::Char(' '));
    h.press("ctrl+a");
    assert!(h.marked_names().is_empty());
}

#[test]
fn a_folder_chooser_lists_folders_only_and_chooses_the_one_under_the_cursor() {
    let (mut h, dir, _) = chooser_at(Chooser::folder(false));
    assert_eq!(h.names(), ["sub"]);
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("sub")]));
}

#[test]
fn alt_enter_chooses_the_folder_you_are_in() {
    let (mut h, dir, _) = chooser_at(Chooser::folder(false));
    alt_enter(&mut h);
    assert_eq!(h.app.chosen, Some(vec![dir]));
    assert!(h.app.should_quit);
}

#[test]
fn a_folder_with_no_folders_in_it_is_chosen_by_enter() {
    let (mut h, dir, _) = chooser_at(Chooser::folder(false));
    h.key(KeyCode::Right); // into sub, which has no folders
    h.wait("in sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    assert!(h.names().is_empty());
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("sub")]));
}

#[test]
fn a_double_click_on_a_folder_goes_in_when_a_folder_is_asked_for() {
    use rada_tui::hits::Target;
    let (mut h, _, _) = chooser_at(Chooser::folder(false));
    h.double_click(&Target::Row(0));
    h.wait("in sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    assert!(h.app.chosen.is_none());
}

#[test]
fn several_folders_can_be_chosen() {
    let (sb, dir) = sandbox_with_files();
    sb.mkdir("proj.v1/other");
    let mut h = H::with_config(sb, dir.clone(), 120, 30, |c| {
        c.chooser = Some(Chooser::folder(true))
    });
    h.key(KeyCode::Char(' '));
    h.key(KeyCode::Char(' '));
    h.key(KeyCode::Enter);
    let chosen = h.app.chosen.clone().unwrap();
    assert_eq!(chosen, vec![dir.join("other"), dir.join("sub")]);
}

#[test]
fn saving_asks_for_a_name_with_the_suggested_one_filled_in() {
    let (mut h, dir, _) = chooser_at(Chooser::save("report.pdf"));
    h.press("ctrl+s");
    let Some(rada_tui::app::Modal::Input(iv)) = &h.app.modal else {
        panic!("{}", h.screen());
    };
    assert_eq!(iv.text, "report.pdf");
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("report.pdf")]));
    // Nothing was created: the program that asked saves the file itself.
    assert!(!dir.join("report.pdf").exists());
}

#[test]
fn saving_in_another_folder_and_under_another_name() {
    let (mut h, dir, _) = chooser_at(Chooser::save("report.pdf"));
    h.key(KeyCode::Enter); // into sub
    h.wait("in sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    alt_enter(&mut h);
    h.press("ctrl+u");
    h.keys("notes.md");
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("sub/notes.md")]));
}

#[test]
fn saving_over_a_file_asks_twice() {
    let (mut h, dir, _) = chooser_at(Chooser::save("a.txt"));
    h.press("ctrl+s");
    h.key(KeyCode::Enter);
    assert!(h.app.chosen.is_none(), "the first Enter only warns");
    let Some(rada_tui::app::Modal::Input(iv)) = &h.app.modal else {
        panic!("{}", h.screen());
    };
    assert!(iv.error.as_deref().unwrap().contains("already exists"));
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("a.txt")]));
}

#[test]
fn enter_on_a_file_while_saving_offers_to_replace_it() {
    let (mut h, dir, _) = chooser_at(Chooser::save("zzz.txt"));
    h.key(KeyCode::Down); // a.txt
    h.key(KeyCode::Enter);
    let Some(rada_tui::app::Modal::Input(iv)) = &h.app.modal else {
        panic!("{}", h.screen());
    };
    assert_eq!(iv.text, "a.txt");
    h.key(KeyCode::Enter);
    h.key(KeyCode::Enter);
    assert_eq!(h.app.chosen, Some(vec![dir.join("a.txt")]));
}

#[test]
fn a_name_that_cannot_be_a_name_is_refused() {
    for bad in ["a/b", "..", "sub"] {
        let (mut h, _, _) = chooser_at(Chooser::save("x.txt"));
        h.press("ctrl+s");
        h.press("ctrl+u");
        h.keys(bad);
        h.key(KeyCode::Enter);
        assert!(h.app.chosen.is_none(), "{bad}");
        assert!(
            matches!(&h.app.modal, Some(rada_tui::app::Modal::Input(iv)) if iv.error.is_some()),
            "{bad}: {}",
            h.screen()
        );
    }
}

#[test]
fn esc_cancels_and_chooses_nothing() {
    for ch in [
        Chooser::file(false),
        Chooser::file(true),
        Chooser::folder(false),
        Chooser::save("x"),
    ] {
        let (mut h, _, _) = chooser_at(ch);
        h.key(KeyCode::Down);
        h.key(KeyCode::Esc);
        assert!(h.app.should_quit);
        assert!(h.app.chosen.is_none());
    }
    // So does q.
    let (mut h, _, _) = chooser_at(Chooser::file(false));
    h.keys("q");
    assert!(h.app.should_quit && h.app.chosen.is_none());
}

#[test]
fn esc_in_the_name_window_closes_it_and_stays_in_the_chooser() {
    let (mut h, _, _) = chooser_at(Chooser::save("x.txt"));
    h.press("ctrl+s");
    h.key(KeyCode::Esc);
    assert!(h.app.modal.is_none() && !h.app.should_quit);
}

#[test]
fn nothing_that_changes_a_file_can_be_done() {
    for ch in [
        Chooser::file(false),
        Chooser::file(true),
        Chooser::folder(true),
        Chooser::save("x.txt"),
    ] {
        let (mut h, _, root) = chooser_at(ch);
        let before = snapshot(&root);
        h.key(KeyCode::Down);
        for spec in [
            "delete",
            "shift+delete",
            "F2",
            "F6",
            "ctrl+x",
            "ctrl+v",
            "ctrl+c",
            "ctrl+z",
            "ctrl+y",
            "x",
            "d",
            "r",
            "p",
            "y",
            "u",
            "ctrl+n",
            "F7",
        ] {
            h.press(spec);
            if matches!(h.app.modal, Some(rada_tui::app::Modal::Input(_))) {
                // The window for a name of the file to save is the only one that may open.
                h.key(KeyCode::Esc);
            }
            assert!(h.app.modal.is_none(), "{spec}: {}", h.screen());
        }
        for a in Action::ALL {
            h.app.dispatch(a);
            assert!(!h.modal_is_plan(), "{a:?}");
            // Whatever it opened is closed again.
            h.key(KeyCode::Esc);
            h.key(KeyCode::Esc);
            h.app.chosen = None;
            h.app.should_quit = false;
        }
        h.pump(200);
        assert_eq!(before, snapshot(&root), "files changed");
        assert!(h.opened().is_empty(), "{:?}", h.opened());
    }
}

#[test]
fn the_menu_of_a_right_click_does_not_open() {
    use rada_tui::hits::Target;
    let (mut h, _, _) = chooser_at(Chooser::file(false));
    h.right_click(&Target::Row(1));
    assert!(h.app.modal.is_none());
    h.press("shift+F10");
    assert!(h.app.modal.is_none());
}

#[test]
fn a_new_folder_can_be_made_where_the_answer_is_a_folder_or_a_place_to_save() {
    let (mut h, dir, _) = chooser_at(Chooser::save("x.txt"));
    h.app.dispatch(Action::NewFolder);
    assert!(matches!(h.app.modal, Some(rada_tui::app::Modal::Input(_))));
    h.keys("fresh");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("fresh").is_dir());
    // And in a chooser of files it cannot.
    let (mut h, _, _) = chooser_at(Chooser::file(false));
    h.app.dispatch(Action::NewFolder);
    assert!(h.app.modal.is_none());
}

#[test]
fn a_plan_that_is_not_a_new_folder_is_never_run() {
    // Whatever slipped through to the confirmation window, the chooser only makes folders.
    let (sb, dir) = sandbox_with_files();
    let root = sb.work.clone();
    let before = snapshot(&root);
    let mut h = H::with_config(sb, dir.clone(), 120, 30, |_| {});
    h.key(KeyCode::Down);
    h.key(KeyCode::Down);
    h.press("delete");
    plan_modal(&mut h);
    // Now the same session becomes a chooser: the plan is shown, and refused when confirmed.
    h.app.chooser = Some(Chooser::file(false));
    h.key(KeyCode::Enter);
    h.pump(300);
    assert_eq!(before, snapshot(&root));
    assert!(dir.join("a.txt").exists());
}

#[test]
fn the_bar_says_what_is_asked_and_the_buttons_for_other_things_are_gone() {
    for (ch, title) in [
        (Chooser::file(false), "Choose a file"),
        (Chooser::file(true), "Choose files"),
        (Chooser::folder(false), "Choose a folder"),
        (Chooser::save("x.txt"), "Save as"),
    ] {
        let (mut h, _, _) = chooser_at(ch);
        let screen = h.screen();
        let first = screen.lines().next().unwrap();
        assert!(first.contains(title), "{first:?}");
        assert!(first.contains("Esc"), "{first:?}");
        assert!(!screen.contains("Trash"), "{screen}");
    }
}

#[test]
fn a_chooser_leaves_the_remembered_state_alone() {
    let (sb, dir) = sandbox_with_files();
    let state = sb.dirs.rada_state();
    let mut h = H::with_config(sb, dir, 120, 30, |c| c.chooser = Some(Chooser::file(false)));
    h.press("alt+p");
    h.press("ctrl+t");
    h.pump(300);
    assert!(
        !state.join("ui.json").exists(),
        "{:?}",
        std::fs::read_dir(&state).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
}
