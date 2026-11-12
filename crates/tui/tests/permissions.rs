//! A folder the user has no right to take out of its folder (on a Mac, a program of the system
//! in /Applications): the plan says so first, and the failure says in words what to do.
#![cfg(unix)]

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_core::error::Denied;
use rada_core::events::FailureInfo;
use rada_core::ops::ErrorChoice;
use rada_tui::app::Modal;
use rada_tui::hits::Target;

fn locked() -> (H, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/App.app/Contents/info.txt", "app");
    sb.write("w/notes.txt", "n");
    let app = sb.path("w/App.app");
    rada_core::testutil::chmod(&app, 0o555);
    let dir = sb.path("w");
    let mut h = H::new(sb, dir, 120, 32);
    h.wait("the folder", |a| {
        !a.tab.is_loading() && !a.tab.visible.is_empty()
    });
    (h, app)
}

#[test]
fn the_plan_warns_and_the_failure_says_what_to_do() {
    if rada_core::testutil::is_root() {
        return;
    }
    let (mut h, app) = locked();
    h.cursor_to("App.app");
    h.keys("d");
    plan_modal(&mut h);
    let plan = h.screen();
    h.key(KeyCode::Enter);
    h.wait("the failure", |a| {
        matches!(a.windows.modal, Some(Modal::Failure { .. }))
    });
    let failure = h.screen();
    let (abort, retry) = (
        h.where_is(&Target::Key(KeyCode::Char('a'))),
        h.where_is(&Target::Key(KeyCode::Char('r'))),
    );
    h.keys("s");
    h.wait("the end", |a| a.ops.running.is_none());
    let still = app.join("Contents/info.txt").exists();
    rada_core::testutil::chmod(&app, 0o755);
    assert!(
        plan.contains("cannot be moved with your permissions"),
        "{plan}"
    );
    assert!(plan.contains("Enter"), "the plan can still be run:\n{plan}");
    assert!(failure.contains("administrator's password"), "{failure}");
    assert!(failure.contains("will not help"), "{failure}");
    assert!(
        (abort.1, abort.0) < (retry.1, retry.0),
        "Retry comes last: {abort:?} {retry:?}\n{failure}"
    );
    assert!(still);
}

fn failure_window(h: &mut H, denied: Option<Denied>) -> String {
    let (reply, rx) = crossbeam_channel::bounded::<ErrorChoice>(1);
    std::mem::forget(rx);
    h.app.windows.modal = Some(Modal::Failure {
        job: 1,
        info: FailureInfo {
            index: 0,
            description: "move to trash".into(),
            message: "rename /Applications/App.app: Operation not permitted (os error 1)".into(),
            path: "/Applications/App.app".into(),
            attempt: 1,
            journal: false,
            denied,
        },
        reply,
    });
    h.screen()
}

#[test]
fn a_refusal_whatever_the_permissions_is_told_apart_from_a_missing_right() {
    let (mut h, app) = locked();
    rada_core::testutil::chmod(&app, 0o755);
    let not_permitted = failure_window(&mut h, Some(Denied::NotPermitted));
    assert!(
        not_permitted.contains("whatever the permissions"),
        "{not_permitted}"
    );
    assert!(!not_permitted.contains("administrator's password:"));
    let other = failure_window(&mut h, None);
    assert!(!other.contains("permissions"), "{other}");
    // Without a refusal Retry is where it always was, before Abort.
    let abort = h.where_is(&Target::Key(KeyCode::Char('a')));
    let retry = h.where_is(&Target::Key(KeyCode::Char('r')));
    assert!((retry.1, retry.0) < (abort.1, abort.0));
}
