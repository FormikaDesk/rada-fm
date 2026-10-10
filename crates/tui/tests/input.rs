//! Keyboard schemes and the mouse, end to end: every action reachable both ways, Ctrl+C
//! never quits, clicks land where the screen says they do.

mod common;

use std::collections::HashMap;

use common::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use rada_core::testutil::*;
use rada_tui::app::Modal;
use rada_tui::hits::Target;
use rada_tui::keymap::{Action, Keymap, Preset};

fn files() -> (Sandbox, std::path::PathBuf) {
    let sb = Sandbox::new();
    for n in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        sb.write(format!("w/{n}"), n);
    }
    sb.mkdir("w/dir");
    let dir = sb.path("w");
    (sb, dir)
}

fn harness() -> H {
    let (sb, dir) = files();
    H::new(sb, dir, 120, 30)
}

// ---------------------------------------------------------------------------------- keys

#[test]
fn ctrl_c_copies_and_never_quits() {
    for preset in [Preset::VimClassic, Preset::Vim, Preset::Classic] {
        let mut h = harness();
        h.app.keymap = Keymap::new(preset, &HashMap::new());
        h.press("ctrl+c");
        h.press("ctrl+c");
        h.press("ctrl+c");
        assert!(!h.app.should_quit, "{preset:?}");
        if preset != Preset::Vim {
            assert!(h.app.clipboard.is_some(), "Ctrl+C copies ({preset:?})");
        }
    }
    // Also inside windows and text fields.
    let mut h = harness();
    h.press("F2");
    assert!(matches!(h.app.modal, Some(Modal::Input(_))));
    h.press("ctrl+c");
    h.press("ctrl+c");
    assert!(!h.app.should_quit);
    h.press("esc");
    h.press("ctrl+p");
    h.press("ctrl+c");
    assert!(!h.app.should_quit);
    h.press("esc");
    h.press("/");
    h.press("ctrl+c");
    assert!(!h.app.should_quit);
}

#[test]
fn classic_navigation_and_selection_keys() {
    let mut h = harness();
    assert_eq!(h.cursor_name(), "dir");
    h.press("down");
    h.press("down");
    assert_eq!(h.cursor_name(), "b.txt");
    h.press("end");
    assert_eq!(h.cursor_name(), "e.txt");
    h.press("home");
    assert_eq!(h.cursor_name(), "dir");
    h.press("pagedown");
    assert_eq!(h.cursor_name(), "e.txt");
    h.press("pageup");
    assert_eq!(h.cursor_name(), "dir");
    // Enter opens, Backspace goes up.
    h.press("enter");
    h.wait("dir loaded", |a| a.cwd.ends_with("dir") && !a.is_loading());
    h.press("backspace");
    h.wait("back", |a| a.cwd.ends_with("w") && !a.is_loading());
    assert_eq!(
        h.cursor_name(),
        "dir",
        "the cursor comes back to where it was"
    );

    // Shift+arrows extend, Esc clears, Ctrl+A selects everything.
    h.press("down"); // a.txt
    h.press("shift+down");
    h.press("shift+down");
    assert_eq!(h.marked_names(), ["a.txt", "b.txt", "c.txt"]);
    h.press("shift+up");
    assert_eq!(
        h.marked_names(),
        ["a.txt", "b.txt"],
        "the range shrinks again"
    );
    h.press("esc");
    assert!(h.marked_names().is_empty());
    h.press("ctrl+a");
    assert_eq!(h.marked_names().len(), 6);
    h.press("esc");
    assert!(h.marked_names().is_empty());
    h.press("shift+end");
    assert_eq!(
        h.marked_names().len(),
        4,
        "from the cursor (b.txt) to the end"
    );
}

#[test]
fn vim_navigation_still_works_alongside() {
    let mut h = harness();
    h.keys("jj");
    assert_eq!(h.cursor_name(), "b.txt");
    h.keys("G");
    assert_eq!(h.cursor_name(), "e.txt");
    h.keys("g");
    assert_eq!(h.cursor_name(), "dir");
    h.press("ctrl+d");
    h.press("ctrl+u");
    h.keys(" ");
    assert_eq!(h.marked_names(), ["dir"]);
}

/// The point of the exercise: each operation can be started with either scheme.
#[test]
fn every_operation_is_reachable_with_the_vim_key_and_with_the_classic_key() {
    type Check = fn(&H) -> bool;
    let cases: &[(&str, &str, Check)] = &[
        ("copy", "y", |h| h.app.clipboard.is_some()),
        ("copy", "ctrl+c", |h| h.app.clipboard.is_some()),
        ("cut", "x", |h| h.app.clipboard.is_some()),
        ("cut", "ctrl+x", |h| h.app.clipboard.is_some()),
        ("trash", "d", |h| h.modal_is_plan() || scanning(h)),
        ("trash", "delete", |h| h.modal_is_plan() || scanning(h)),
        ("delete", "D", |h| h.modal_is_plan() || scanning(h)),
        ("delete", "shift+delete", |h| {
            h.modal_is_plan() || scanning(h)
        }),
        ("rename", "r", |h| {
            matches!(h.app.modal, Some(Modal::Input(_)))
        }),
        ("rename", "F2", |h| {
            matches!(h.app.modal, Some(Modal::Input(_)))
        }),
        ("new folder", "n", |h| {
            matches!(h.app.modal, Some(Modal::Input(_)))
        }),
        ("new folder", "ctrl+n", |h| {
            matches!(h.app.modal, Some(Modal::Input(_)))
        }),
        ("palette", "m", |h| {
            matches!(h.app.modal, Some(Modal::Palette(_)))
        }),
        ("address", "ctrl+l", |h| h.app.address.is_some()),
        ("address", "alt+d", |h| h.app.address.is_some()),
        ("new folder", "ctrl+shift+n", |h| {
            matches!(h.app.modal, Some(Modal::Input(_)))
        }),
        ("new file", "N", |h| {
            matches!(h.app.modal, Some(Modal::Input(_)))
        }),
        ("properties", "alt+enter", |h| {
            matches!(h.app.modal, Some(Modal::Properties(_)))
        }),
        ("refresh", "f5", |h| h.app.is_loading()),
        ("new tab", "ctrl+t", |h| h.app.tab_count() == 2),
        ("details", "alt+p", |h| h.app.details == Some(false)),
        ("view", "v", |h| h.app.view == rada_tui::ViewMode::Icons),
        ("palette", "ctrl+p", |h| {
            matches!(h.app.modal, Some(Modal::Palette(_)))
        }),
        ("filter", "/", |h| h.app.filter.is_some()),
        ("filter", "ctrl+f", |h| h.app.filter.is_some()),
        ("help", "?", |h| matches!(h.app.modal, Some(Modal::Help))),
        ("help", "F1", |h| matches!(h.app.modal, Some(Modal::Help))),
        ("history", "U", |h| {
            matches!(h.app.modal, Some(Modal::History(_)))
        }),
        ("history", "F3", |h| {
            matches!(h.app.modal, Some(Modal::History(_)))
        }),
        ("undo", "u", |h| undoing(h)),
        ("undo", "ctrl+z", |h| undoing(h)),
        ("redo", "ctrl+r", |h| redoing(h)),
        ("redo", "ctrl+y", |h| redoing(h)),
        ("quit", "q", |h| h.app.should_quit),
        ("quit", "ctrl+q", |h| h.app.should_quit),
        ("select all", "ctrl+a", |h| h.app.marked.len() == 6),
        ("hidden", ".", |h| h.app.show_hidden),
        ("sort", "s", |h| h.app.sort_label().starts_with("size")),
    ];
    for (what, key, check) in cases {
        let mut h = harness();
        h.press("down"); // a.txt: something to act on
        h.press(key);
        let mut h = h;
        assert!(check(&h), "{what} via {key}:\n{}", h.screen());
    }
}

fn scanning(h: &H) -> bool {
    matches!(h.app.modal, Some(Modal::Scanning { .. }))
}
fn undoing(h: &H) -> bool {
    // "nothing to undo" is a plan job too: either a scanning window or the toast.
    scanning(h) || h.app.toast.is_some()
}
fn redoing(h: &H) -> bool {
    scanning(h) || h.app.toast.is_some()
}

#[test]
fn the_vim_only_preset_ignores_classic_keys_and_the_classic_one_ignores_letters() {
    let mut h = harness();
    h.app.keymap = Keymap::new(Preset::Vim, &HashMap::new());
    h.press("ctrl+a");
    assert!(h.marked_names().is_empty());
    h.press("delete");
    assert!(h.app.modal.is_none());
    h.keys("y");
    assert!(h.app.clipboard.is_some());

    let mut h = harness();
    h.app.keymap = Keymap::new(Preset::Classic, &HashMap::new());
    h.keys("y");
    assert!(h.app.clipboard.is_none());
    h.keys("q");
    assert!(!h.app.should_quit);
    h.press("ctrl+q");
    assert!(h.app.should_quit);
}

#[test]
fn keys_can_be_rebound_from_the_configuration() {
    let mut h = harness();
    let mut o = HashMap::new();
    o.insert("quit".to_string(), vec!["ctrl+w".to_string()]);
    o.insert("copy".to_string(), vec!["c".to_string()]);
    h.app.keymap = Keymap::new(Preset::VimClassic, &o);
    h.keys("q");
    assert!(!h.app.should_quit, "q no longer quits");
    h.press("ctrl+w");
    assert!(h.app.should_quit);
    let mut h = harness();
    h.app.keymap = Keymap::new(Preset::VimClassic, &o);
    h.keys("c");
    assert!(h.app.clipboard.is_some());
    h.app.clipboard = None;
    h.press("ctrl+c");
    assert!(h.app.clipboard.is_none(), "the old copy keys are gone");
}

#[test]
fn the_filter_narrows_the_list_as_you_type_and_esc_restores_it() {
    let mut h = harness();
    h.press("ctrl+f");
    h.keys("b.t");
    assert_eq!(h.names(), ["b.txt"]);
    assert!(
        h.screen().contains("b.t▏"),
        "the search field shows what is typed"
    );
    h.press("enter"); // keep the filter, back to the list
    assert_eq!(h.names(), ["b.txt"]);
    assert!(h.app.filter.as_ref().is_some_and(|f| !f.editing));
    h.press("esc");
    assert_eq!(h.names().len(), 6, "Esc clears the filter");
    // Several words must all match, case-insensitively.
    h.keys("/");
    h.keys("TXT c");
    assert_eq!(h.names(), ["c.txt"]);
    h.press("backspace");
    h.press("backspace");
    h.press("backspace");
    h.press("backspace");
    h.press("backspace");
    h.press("backspace");
    assert!(h.app.filter.is_none(), "an empty filter goes away");
    // Moving to another folder drops it.
    h.keys("/");
    h.keys("di");
    h.press("enter");
    h.press("enter");
    h.wait("dir", |a| a.cwd.ends_with("dir") && !a.is_loading());
    assert!(h.app.filter.is_none());
}

#[test]
fn operations_use_the_filtered_selection_only() {
    let mut h = harness();
    h.press("ctrl+f");
    h.keys("txt");
    h.press("enter");
    h.press("ctrl+a");
    assert_eq!(
        h.marked_names().len(),
        5,
        "select all means all that are shown"
    );
}

#[test]
fn redo_replans_the_undone_operation_and_asks_for_confirmation_again() {
    let (sb, dir) = files();
    let dest = sb.mkdir("dest");
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.press("down"); // a.txt
    h.press("ctrl+c");
    h.app.open_dir(dest.clone());
    h.wait("dest", |a| a.cwd.ends_with("dest") && !a.is_loading());
    h.press("ctrl+v");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dest.join("a.txt").exists());

    h.press("ctrl+z");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(!dest.join("a.txt").exists());
    assert_eq!(
        h.app.toast.as_ref().unwrap().text,
        "Undone — press Ctrl+R to redo"
    );

    h.press("ctrl+y");
    plan_modal(&mut h);
    assert!(!dest.join("a.txt").exists(), "redo shows the plan first");
    run_plan_and_wait(&mut h);
    assert!(dest.join("a.txt").exists(), "…and then does it again");
    // Nothing left to redo.
    h.press("ctrl+y");
    h.wait("toast", |a| {
        a.toast
            .as_ref()
            .is_some_and(|t| t.text.contains("nothing to redo"))
    });
}

#[test]
fn done_messages_say_what_happened_and_how_to_undo() {
    let (sb, dir) = files();
    let mut h = H::new(sb, dir, 120, 30);
    h.keys("jjd");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    let t = h.app.toast.as_ref().unwrap().text.clone();
    assert_eq!(t, "Moved 1 item to the trash — press u to undo");
}

// ---------------------------------------------------------------------------------- mouse

#[test]
fn click_selects_double_click_opens() {
    let mut h = harness();
    let row = Target::Row(3); // c.txt
    h.click(&row);
    assert_eq!(h.cursor_name(), "c.txt");
    assert!(h.app.modal.is_none());
    // A double click on the folder enters it.
    let dir_row = Target::Row(0);
    h.click(&dir_row);
    h.click(&dir_row);
    h.wait("dir", |a| a.cwd.ends_with("dir") && !a.is_loading());
}

#[test]
fn a_slow_second_click_is_not_a_double_click() {
    let mut h = harness();
    let dir_row = Target::Row(0);
    h.click(&dir_row);
    std::thread::sleep(std::time::Duration::from_millis(500));
    h.click(&dir_row);
    h.settle();
    assert!(h.app.cwd.ends_with("w"));
}

#[test]
fn ctrl_click_adds_shift_click_selects_a_range_plain_click_resets() {
    let mut h = harness();
    h.click(&Target::Row(1)); // a.txt
    h.click_on(&Target::Row(3), KeyModifiers::CONTROL); // + c.txt
    assert_eq!(
        h.marked_names(),
        ["c.txt"],
        "ctrl+click toggles the clicked item"
    );
    h.click_on(&Target::Row(5), KeyModifiers::SHIFT);
    assert_eq!(
        h.marked_names(),
        ["c.txt", "d.txt", "e.txt"],
        "range from the ctrl+clicked anchor"
    );
    h.click_on(&Target::Row(4), KeyModifiers::CONTROL); // remove d.txt
    assert_eq!(h.marked_names(), ["c.txt", "e.txt"]);
    h.click(&Target::Row(2));
    assert!(
        h.marked_names().is_empty(),
        "a plain click clears the selection"
    );
    assert_eq!(h.cursor_name(), "b.txt");
}

#[test]
fn the_wheel_scrolls_the_list_and_the_preview() {
    let mut h = harness();
    let (x, y) = h.where_is(&Target::Row(0));
    h.mouse(MouseEventKind::ScrollDown, x, y, KeyModifiers::NONE);
    assert_eq!(h.cursor_name(), "c.txt", "three rows per notch");
    h.mouse(MouseEventKind::ScrollUp, x, y, KeyModifiers::NONE);
    assert_eq!(h.cursor_name(), "dir");

    // The preview scrolls on its own.
    let (sb, dir) = files();
    let long: String = (0..80).map(|i| format!("line {i}\n")).collect();
    sb.write("w/long.txt", long);
    let mut h = H::new(sb, dir, 140, 30);
    h.keys("G");
    h.wait("preview", |a| a.preview.name == "long.txt");
    let (px, py) = h.where_is(&Target::Preview);
    let before = h.app.preview.scroll;
    h.mouse(MouseEventKind::ScrollDown, px, py + 3, KeyModifiers::NONE);
    assert!(h.app.preview.scroll > before);
    assert_eq!(h.cursor_name(), "long.txt", "the cursor stays");
}

#[test]
fn breadcrumb_segments_and_hint_bar_items_are_clickable() {
    let (sb, _dir) = files();
    sb.write("w/dir/deep/x.txt", "x");
    let start = sb.path("w/dir/deep");
    let mut h = H::new(sb, start, 200, 30);
    let screen = h.screen();
    assert!(screen.contains("deep"), "{screen}");
    // Click the "dir" segment: go there.
    let target = Target::Crumb(h.app.cwd.parent().unwrap().to_path_buf());
    h.click(&target);
    h.wait("parent", |a| a.cwd.ends_with("dir") && !a.is_loading());
    // The top bar's "Go to" button and the bottom bar's hints run their action.
    h.click(&Target::Act(Action::Palette));
    assert!(
        matches!(h.app.modal, Some(Modal::Palette(_))),
        "{}",
        h.screen()
    );
    h.key(crossterm::event::KeyCode::Esc);
    assert!(h.app.modal.is_none());
    h.click(&Target::Act(Action::Rename));
    assert!(
        matches!(h.app.modal, Some(Modal::Input(_))),
        "{}",
        h.screen()
    );
}

#[test]
fn clicking_a_column_title_sorts_and_clicking_again_reverses() {
    let mut h = harness();
    h.click(&Target::SortBy(rada_core::model::SortKey::Size));
    assert!(h.app.sort_label().starts_with("size"));
    h.click(&Target::SortBy(rada_core::model::SortKey::Size));
    assert!(h.app.sort_label().ends_with('↓'), "{}", h.app.sort_label());
}

#[test]
fn right_click_opens_a_menu_with_shortcut_hints_and_its_entries_work() {
    let mut h = harness();
    h.right_click(&Target::Row(2)); // b.txt
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
    assert_eq!(
        h.cursor_name(),
        "b.txt",
        "the item under the pointer is selected"
    );
    let s = h.screen();
    for needle in [
        "Open",
        "Open with…",
        "Cut",
        "Copy",
        "Ctrl+C",
        "Rename",
        "F2",
        "Compress…",
        "Copy the path",
        "Move to trash",
        "Del",
        "Properties",
        "Alt+Enter",
    ] {
        assert!(s.contains(needle), "{needle} missing from the menu:\n{s}");
    }
    // Paste is greyed out until something is copied (it must not do anything).
    h.click(&Target::MenuItem(4));
    assert!(h.app.modal.is_none(), "the menu closes after a click");
    assert!(h.app.clipboard.is_none());

    h.right_click(&Target::Row(2));
    h.click(&Target::MenuItem(3)); // Copy
    assert!(h.app.clipboard.is_some());

    // Keyboard works in the menu too.
    h.right_click(&Target::Row(2));
    h.press("down");
    h.press("esc");
    assert!(h.app.modal.is_none());
    // Right click on empty space: a shorter menu.
    h.right_click(&Target::List);
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
}

#[test]
fn right_click_keeps_a_multi_selection_when_clicking_inside_it() {
    let mut h = harness();
    h.click(&Target::Row(1));
    h.click_on(&Target::Row(2), KeyModifiers::SHIFT);
    assert_eq!(h.marked_names().len(), 2);
    h.right_click(&Target::Row(2));
    assert_eq!(h.marked_names().len(), 2, "the selection survives");
    h.press("esc");
    h.right_click(&Target::Row(4));
    assert!(h.marked_names().is_empty());
}

#[test]
fn plan_window_buttons_and_conflict_control_are_clickable() {
    let (sb, dir) = files();
    let dest = sb.mkdir("dest");
    sb.write("dest/a.txt", "already");
    let mut h = H::new(sb, dir, 120, 36);
    h.press("down");
    h.press("ctrl+c");
    h.app.open_dir(dest.clone());
    h.wait("dest", |a| a.cwd.ends_with("dest") && !a.is_loading());
    h.press("ctrl+v");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(s.contains("keep both"), "{s}");
    // Pick "keep both" with the mouse: the plan is recomputed.
    h.click(&Target::Policy(rada_core::ops::ConflictPolicy::KeepBoth));
    h.wait("replanned", |a| match &a.modal {
        Some(Modal::Plan(pv)) => {
            pv.plan.policy == rada_core::ops::ConflictPolicy::KeepBoth && pv.replanning.is_none()
        }
        _ => false,
    });
    // Cancel with the mouse.
    h.click(&Target::Key(crossterm::event::KeyCode::Esc));
    assert!(h.app.modal.is_none());
    assert!(!dest.join("a (2).txt").exists() && !dest.join("a copy.txt").exists());

    // And confirm with the mouse.
    h.press("ctrl+v");
    plan_modal(&mut h);
    h.click(&Target::Policy(rada_core::ops::ConflictPolicy::KeepBoth));
    h.wait("replanned", |a| match &a.modal {
        Some(Modal::Plan(pv)) => {
            pv.plan.policy == rada_core::ops::ConflictPolicy::KeepBoth && pv.replanning.is_none()
        }
        _ => false,
    });
    h.click(&Target::Key(crossterm::event::KeyCode::Enter));
    h.wait("done", |a| {
        a.running.is_none() && !matches!(a.modal, Some(Modal::Scanning { .. }))
    });
    let names: Vec<_> = std::fs::read_dir(&dest)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn palette_rows_are_clickable() {
    let (sb, dir) = files();
    let mut h = H::new(sb, dir, 120, 30);
    h.press("ctrl+p");
    h.keys("dir");
    let s = h.screen();
    assert!(s.contains("dir"), "{s}");
    h.click(&Target::PaletteRow(0));
    h.wait("moved", |a| a.cwd.ends_with("dir") && !a.is_loading());
    assert!(h.app.modal.is_none());
}

#[test]
fn with_the_mouse_off_nothing_reacts() {
    let mut h = harness();
    h.app.mouse = false;
    let (x, y) = h.where_is(&Target::Row(3));
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        x,
        y,
        KeyModifiers::NONE,
    );
    assert_eq!(h.cursor_name(), "dir");
    h.mouse(MouseEventKind::ScrollDown, x, y, KeyModifiers::NONE);
    assert_eq!(h.cursor_name(), "dir");
}

#[test]
fn mouse_motion_never_triggers_a_redraw() {
    let mut h = harness();
    let _ = h.screen();
    h.app.dirty = false;
    h.mouse(MouseEventKind::Moved, 10, 10, KeyModifiers::NONE);
    h.mouse(
        MouseEventKind::Drag(MouseButton::Left),
        11,
        10,
        KeyModifiers::NONE,
    );
    h.mouse(
        MouseEventKind::Up(MouseButton::Left),
        11,
        10,
        KeyModifiers::NONE,
    );
    assert!(!h.app.dirty);
}

#[test]
fn a_click_outside_a_window_does_not_reach_the_list_behind_it() {
    let mut h = harness();
    h.press("F2");
    assert!(matches!(h.app.modal, Some(Modal::Input(_))));
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        2,
        8,
        KeyModifiers::NONE,
    );
    assert!(matches!(h.app.modal, Some(Modal::Input(_))), "still there");
    assert_eq!(h.cursor_name(), "dir");
}

#[test]
fn help_shows_both_schemes_side_by_side_and_scrolls() {
    let (sb, dir) = files();
    let mut h = H::new(sb, dir, 120, 60);
    h.press("F1");
    let s = h.screen();
    assert!(s.contains("Vim") && s.contains("Classic"), "{s}");
    let line = s
        .lines()
        .find(|l| l.trim_start_matches(['│', ' ']).starts_with("Copy  ") && l.contains("Ctrl+C"))
        .unwrap_or_else(|| panic!("{s}"));
    assert!(
        line.contains('y') && line.contains("Ctrl+C"),
        "both schemes on the Copy line: {line}"
    );
    assert!(
        s.contains("Shift+drag") || s.contains("Moving around"),
        "{s}"
    );
    let first = s.clone();
    h.press("pagedown");
    assert_ne!(h.screen(), first, "the help scrolls");
    h.press("esc");
    assert!(h.app.modal.is_none());
    // Mouse notes mention how to select text, and the file explains the way out.
    h.press("?");
    for _ in 0..8 {
        h.press("pagedown");
    }
    let end = h.screen();
    assert!(end.contains("Shift+drag selects text"), "{end}");
    assert!(end.contains("never quits"), "{end}");
}

// ---------------------------------------------------------------------------------- requests as data

#[test]
fn a_request_given_as_data_goes_through_the_same_plan_window_and_journal() {
    use rada_core::ops::OpRequest;
    let (sb, dir) = files();
    let dest = sb.mkdir("dest");
    let req: OpRequest = serde_json::from_str(&format!(
        r#"{{"op":"copy","sources":[{:?}],"destination":{:?}}}"#,
        dir.join("a.txt").to_string_lossy(),
        dest.to_string_lossy()
    ))
    .unwrap();
    let mut h = H::with_request(sb, dir, 120, 30, req);
    plan_modal(&mut h);
    let s = h.screen();
    assert!(s.contains("Copy 1 item"), "{s}");
    assert!(
        !dest.join("a.txt").exists(),
        "the plan is shown first, nothing happened yet"
    );
    run_plan_and_wait(&mut h);
    assert!(dest.join("a.txt").exists());
    // It is in the journal like any other operation, so it can be undone.
    h.press("u");
    plan_modal(&mut h);
    assert!(h.screen().contains("Undo: Copy"));
    run_plan_and_wait(&mut h);
    assert!(!dest.join("a.txt").exists());
}

#[test]
fn a_destructive_request_still_needs_the_typed_confirmation() {
    use rada_core::ops::OpRequest;
    let (sb, dir) = files();
    let victim = dir.join("a.txt");
    let mut h = H::with_request(
        sb,
        dir,
        120,
        30,
        OpRequest::Delete {
            sources: vec![victim.clone()],
        },
    );
    plan_modal(&mut h);
    h.press("enter");
    assert!(victim.exists(), "Enter alone does not delete");
    h.keys("yes");
    h.press("enter");
    h.wait("done", |a| {
        a.running.is_none() && !matches!(a.modal, Some(Modal::Scanning { .. }))
    });
    assert!(!victim.exists());
}

#[test]
fn a_bad_request_is_reported_and_the_browser_still_works() {
    use rada_core::ops::OpRequest;
    let (sb, dir) = files();
    let mut h = H::with_request(
        sb,
        dir,
        120,
        30,
        OpRequest::Trash {
            sources: vec!["relative/path".into()],
        },
    );
    h.wait("an error", |a| a.toast.is_some());
    assert!(h.app.toast.as_ref().unwrap().text.contains("absolute"));
    assert!(h.app.modal.is_none());
    h.keys("j");
    assert_eq!(h.cursor_name(), "a.txt");
}
