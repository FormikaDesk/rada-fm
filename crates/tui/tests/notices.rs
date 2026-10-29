//! The notification card: what it says after an operation, the way back and the way to see
//! the result, how long it stays, and that it never takes the keyboard. Time is the clock of
//! the cards, moved on by the test: nobody waits six seconds.

mod common;

use std::time::Duration;

use common::*;
use crossterm::event::KeyCode;
use rada_core::ops::ConflictPolicy;
use rada_tui::app::{Modal, NoticeAction, NoticeKind};
use rada_tui::hits::{NoticeHit, Target};

/// A folder with a few files and another to copy them into.
fn files() -> (H, std::path::PathBuf, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    for n in ["a.txt", "b.txt", "c.txt"] {
        sb.write(format!("w/{n}"), n);
    }
    let dest = sb.mkdir("Documenti");
    let dir = sb.path("w");
    let h = H::new(sb, dir.clone(), 150, 36);
    (h, dir, dest)
}

/// Wait for the plan window and say yes to it.
fn confirm(h: &mut H) {
    plan_modal(h);
    run_plan_and_wait(h);
}

fn copy_all_to(h: &mut H, dest: &std::path::Path) {
    h.press("ctrl+a");
    h.press("ctrl+c");
    h.app.open_dir(dest.to_path_buf());
    let d = dest.to_path_buf();
    h.wait("the destination", move |a| {
        a.tab.cwd == d && !a.tab.is_loading()
    });
    h.press("ctrl+v");
    confirm(h);
}

#[test]
fn after_a_copy_the_card_says_what_was_done_and_where_with_undo_and_show() {
    let (mut h, _dir, dest) = files();
    copy_all_to(&mut h, &dest);
    let n = h.app.current_notice().expect("a card").clone();
    assert_eq!((n.kind, n.action), (NoticeKind::Ok, NoticeAction::Undo));
    assert_eq!(n.text, "3 items copied to Documenti");
    assert_eq!(n.show.as_deref(), Some(dest.as_path()));
    let s = h.screen();
    assert!(s.contains("3 items copied to Documenti"), "{s}");
    assert!(
        s.contains("Undo") && s.contains("Ctrl+Z") && s.contains("Show"),
        "{s}"
    );
    // The card is rounded (a corner above and below it) and sits at the bottom right.
    let lines: Vec<&str> = s.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.contains("3 items copied"))
        .unwrap();
    assert!(
        lines[at - 1].contains('╭') && lines[at + 2].contains('╰'),
        "{s}"
    );
    let col = lines[at].find("3 items").unwrap();
    assert!(col > 150 / 2, "at the right: {col}");
}

#[test]
fn the_card_goes_by_itself_after_about_six_seconds() {
    let (mut h, _dir, dest) = files();
    copy_all_to(&mut h, &dest);
    assert!(h.app.current_notice().is_some());
    h.app.advance_notices(Duration::from_millis(5900));
    assert!(h.app.current_notice().is_some(), "still there at 5.9 s");
    assert!(h.screen().contains("items copied"));
    h.app.advance_notices(Duration::from_millis(200));
    assert!(h.app.current_notice().is_none(), "gone after six seconds");
    assert!(!h.screen().contains("items copied"));
}

#[test]
fn the_first_key_closes_it_and_still_does_its_job() {
    let (mut h, _dir, dest) = files();
    copy_all_to(&mut h, &dest);
    h.wait("the copies are listed", |a| !a.tab.visible.is_empty());
    // Typed at once, before the card has been seen: it stays.
    h.key(KeyCode::Down);
    assert!(
        h.app.current_notice().is_some(),
        "a key typed at the very moment"
    );
    // A moment later, a key closes it (and moves the cursor as it always does).
    h.app.advance_notices(Duration::from_millis(800));
    let before = h.cursor_name();
    h.key(KeyCode::Down);
    assert!(h.app.current_notice().is_none());
    assert_ne!(h.cursor_name(), before, "the key was not taken");
}

#[test]
fn show_on_the_card_goes_to_the_result() {
    let (mut h, dir, dest) = files();
    copy_all_to(&mut h, &dest);
    // Go elsewhere, then use the card: it brings us back to where the copies are.
    h.app.open_dir(dir.clone());
    let d = dir.clone();
    h.wait("away", move |a| a.tab.cwd == d && !a.tab.is_loading());
    let id = h.app.current_notice().unwrap().id;
    let _ = h.screen();
    h.click(&Target::Notice(id, NoticeHit::Show));
    let d = dest.clone();
    h.wait("the destination", move |a| {
        a.tab.cwd == d && !a.tab.is_loading()
    });
    assert!(h.app.current_notice().is_none(), "a click closes the card");
}

#[test]
fn undo_on_the_card_takes_the_copy_back() {
    let (mut h, _dir, dest) = files();
    copy_all_to(&mut h, &dest);
    assert!(dest.join("a.txt").exists());
    let _ = h.screen();
    let id = h.app.current_notice().unwrap().id;
    h.click(&Target::Notice(id, NoticeHit::Undo));
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(!dest.join("a.txt").exists(), "the copy was taken back");
}

#[test]
fn an_error_stays_until_it_is_closed_and_has_the_style_of_an_error() {
    let (mut h, _dir, _dest) = files();
    h.app.toast(
        rada_tui::app::ToastKind::Error,
        "cannot copy a.txt: permission denied",
        6,
    );
    let s = h.screen();
    assert!(
        s.contains("permission denied") && s.contains("Close") && s.contains("Esc"),
        "{s}"
    );
    let th = h.app.settings.th.clone();
    let buf = h.term.backend().buffer().clone();
    let (x, y) = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == "✕" && buf[(x, y)].fg == th.error)
        .expect("the mark of an error, in the colour of errors");
    let _ = (x, y);
    h.app.advance_notices(Duration::from_secs(3600));
    assert!(
        h.app.current_notice().is_some(),
        "an hour later it is still there"
    );
    h.key(KeyCode::Down);
    assert!(h.app.current_notice().is_some(), "a key does not close it");
    h.key(KeyCode::Esc);
    assert!(h.app.current_notice().is_none(), "Esc does");
}

#[test]
fn notifications_pile_up_and_each_goes_when_its_time_is_up() {
    use rada_tui::app::Notice;
    let (mut h, _dir, _dest) = files();
    h.app.notify(
        Notice::new(NoticeKind::Ok, "3 items moved to the trash").with_action(NoticeAction::Undo),
    );
    h.app.advance_notices(Duration::from_secs(2));
    h.app
        .notify(Notice::new(NoticeKind::Error, "cannot rename setup.sh"));
    let now = h.app.notice_now();
    assert_eq!(
        h.app.windows.notices.shown(now).len(),
        2,
        "both are on screen"
    );
    let s = h.screen();
    assert!(
        s.contains("moved to the trash") && s.contains("Cannot rename"),
        "{s}"
    );
    // The first was there first and goes first; the other is an error and stays.
    h.app.advance_notices(Duration::from_millis(4100));
    let s = h.screen();
    assert!(!s.contains("moved to the trash"), "{s}");
    assert!(s.contains("Cannot rename"));
    h.app.advance_notices(Duration::from_secs(3600));
    assert!(h.screen().contains("Cannot rename"), "an error stays");
}

#[test]
fn more_than_three_wait_for_room() {
    use rada_tui::app::Notice;
    let (mut h, _dir, _dest) = files();
    for n in ["a", "b", "c", "d"] {
        h.app.notify(
            Notice::new(NoticeKind::Ok, format!("done {n}")).with_action(NoticeAction::Undo),
        );
    }
    let now = h.app.notice_now();
    assert_eq!(h.app.windows.notices.shown(now).len(), 3);
    assert_eq!(h.app.windows.notices.waiting(), 1);
    let s = h.screen();
    assert!(s.contains("Done a") && s.contains("Done c"), "{s}");
    assert!(!s.contains("Done d"), "{s}");
    h.app.advance_notices(Duration::from_secs(7));
    let s = h.screen();
    assert!(s.contains("Done d"), "{s}");
}

#[test]
fn trash_rename_and_new_folder_say_so_too() {
    let (mut h, _dir, _dest) = files();
    h.keys("j"); // a.txt
    h.press("delete");
    confirm(&mut h);
    let t = h.app.current_notice().unwrap().text.clone();
    assert_eq!(t, "1 item moved to the trash");
    h.app.advance_notices(Duration::from_secs(7));
    h.press("ctrl+shift+n");
    h.keys("new");
    h.key(KeyCode::Enter);
    confirm(&mut h);
    assert_eq!(h.app.current_notice().unwrap().text, "Folder created: new");
}

#[test]
fn the_cards_are_in_italian_when_the_language_is() {
    rada_core::i18n::with_lang(rada_core::i18n::Lang::It, || {
        let (mut h, _dir, dest) = files();
        copy_all_to(&mut h, &dest);
        let s = h.screen();
        assert!(s.contains("3 elementi copiati in Documenti"), "{s}");
        assert!(s.contains("Annulla") && s.contains("Mostra"), "{s}");
    });
}

/// The three files again, with one of them already in the destination, copied or moved with
/// the policy chosen in the plan window.
fn clash_then(h: &mut H, dest: &std::path::Path, cut: bool, policy: ConflictPolicy) {
    std::fs::write(dest.join("b.txt"), "already").unwrap();
    h.press("ctrl+a");
    h.press(if cut { "ctrl+x" } else { "ctrl+c" });
    h.app.open_dir(dest.to_path_buf());
    let d = dest.to_path_buf();
    h.wait("the destination", move |a| {
        a.tab.cwd == d && !a.tab.is_loading()
    });
    h.press("ctrl+v");
    plan_modal(h);
    h.click(&Target::Policy(policy));
    h.wait("the plan with that policy", move |a| {
        match &a.windows.modal {
            Some(Modal::Plan(pv)) => pv.plan.policy == policy && pv.replanning.is_none(),
            _ => false,
        }
    });
    run_plan_and_wait(h);
}

fn card_text(h: &H) -> String {
    h.app.current_notice().expect("a card").text.clone()
}

#[test]
fn the_card_counts_only_what_was_copied_and_says_what_was_skipped() {
    let (mut h, _dir, dest) = files();
    clash_then(&mut h, &dest, false, ConflictPolicy::Skip);
    assert_eq!(card_text(&h), "2 items copied to Documenti, 1 skipped");
    assert!(
        h.screen()
            .contains("2 items copied to Documenti, 1 skipped")
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("b.txt")).unwrap(),
        "already"
    );
}

#[test]
fn the_card_says_the_same_of_a_move() {
    let (mut h, dir, dest) = files();
    clash_then(&mut h, &dest, true, ConflictPolicy::Skip);
    assert_eq!(card_text(&h), "2 items moved to Documenti, 1 skipped");
    assert!(dir.join("b.txt").exists(), "the skipped one did not move");
}

#[test]
fn keeping_both_or_replacing_counts_every_item_and_skips_none() {
    for policy in [ConflictPolicy::KeepBoth, ConflictPolicy::Overwrite] {
        let (mut h, _dir, dest) = files();
        clash_then(&mut h, &dest, false, policy);
        assert_eq!(card_text(&h), "3 items copied to Documenti", "{policy:?}");
    }
}

#[test]
fn several_skipped_are_counted() {
    let (mut h, dir, dest) = files();
    std::fs::write(dest.join("a.txt"), "already").unwrap();
    std::fs::write(dir.join("d.txt"), "d").unwrap();
    h.app.open_dir(dir.clone());
    let d = dir.clone();
    h.wait("the source", move |a| a.tab.cwd == d && !a.tab.is_loading());
    clash_then(&mut h, &dest, false, ConflictPolicy::Skip);
    assert_eq!(card_text(&h), "2 items copied to Documenti, 2 skipped");
}

#[test]
fn the_card_says_it_in_italian_too() {
    rada_core::i18n::with_lang(rada_core::i18n::Lang::It, || {
        let (mut h, _dir, dest) = files();
        clash_then(&mut h, &dest, false, ConflictPolicy::Skip);
        assert_eq!(card_text(&h), "2 elementi copiati in Documenti, 1 saltato");
        let (mut h, _dir, dest) = files();
        std::fs::write(dest.join("a.txt"), "already").unwrap();
        clash_then(&mut h, &dest, true, ConflictPolicy::Skip);
        assert_eq!(card_text(&h), "1 elemento spostato in Documenti, 2 saltati");
    });
}
