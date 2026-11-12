//! An item the user has no right to take out of its folder (on a Mac, a program of the system
//! in /Applications): the plan says so before anything is done, and the failure says why.
#![cfg(unix)]

use std::path::{Path, PathBuf};

use rada_core::error::Denied;
use rada_core::ops::*;
use rada_core::testutil::*;

/// A folder the user may not change, inside one they may.
fn locked_folder(sb: &Sandbox) -> PathBuf {
    sb.write("w/App.app/Contents/info.txt", "app");
    sb.write("w/plain/a.txt", "a");
    sb.write("w/loose.txt", "l");
    let app = sb.path("w/App.app");
    chmod(&app, 0o555);
    app
}

fn not_movable(plan: &Plan) -> Option<&Warning> {
    plan.warnings
        .iter()
        .find(|w| w.kind == WarningKind::NotMovable)
}

/// The rights given back, so that the sandbox can be removed.
fn unlock(app: &Path) {
    chmod(app, 0o755);
}

#[test]
fn the_plan_of_the_trash_says_which_folder_cannot_be_taken_out() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    let app = locked_folder(&sb);
    let e = sb.engine();
    let plan = e.plan_trash(&scan(&e, std::slice::from_ref(&app)));
    let w = not_movable(&plan).cloned();
    unlock(&app);
    let w = w.expect("a warning before anything is done");
    assert_eq!(w.severity, Severity::Warning, "a warning, not a block");
    assert_eq!(w.examples, [app]);
    assert!(w.message.contains("administrator"), "{}", w.message);
    // A folder and a file the user may change: nothing to say.
    let plan = e.plan_trash(&scan(&e, &[sb.path("w/plain"), sb.path("w/loose.txt")]));
    assert!(not_movable(&plan).is_none(), "{:?}", plan.warnings);
}

#[test]
fn a_file_in_a_folder_that_cannot_be_changed_is_said_too() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    let app = locked_folder(&sb);
    let e = sb.engine();
    let plan = e.plan_trash(&scan(&e, &[app.join("Contents")]));
    let warned = not_movable(&plan).is_some();
    unlock(&app);
    assert!(warned, "its folder cannot lose it: {:?}", plan.warnings);
}

#[test]
fn the_plan_of_a_move_says_it_too() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    let app = locked_folder(&sb);
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let mv = |paths: &[PathBuf]| {
        e.plan_transfer(
            &scan(&e, paths),
            &dest,
            &TransferOptions::mv(ConflictPolicy::Skip),
        )
    };
    let plan = mv(std::slice::from_ref(&app));
    let copy = e.plan_transfer(
        &scan(&e, std::slice::from_ref(&app)),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let (moved, copied) = (not_movable(&plan).is_some(), not_movable(&copy).is_some());
    unlock(&app);
    assert!(moved, "{:?}", plan.warnings);
    assert!(!copied, "a copy takes nothing out: {:?}", copy.warnings);
    assert!(not_movable(&mv(&[sb.path("w/plain")])).is_none());
}

/// Records why each failure happened.
#[derive(Default)]
struct Why(Vec<Option<Denied>>);

impl ExecHandler for Why {
    fn on_failure(&mut self, f: &Failure<'_>) -> ErrorChoice {
        self.0.push(f.error.denied());
        ErrorChoice::Skip
    }
}

#[test]
fn running_it_anyway_fails_for_want_of_a_right_and_leaves_the_folder() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    let app = locked_folder(&sb);
    let e = sb.engine();
    let plan = e.plan_trash(&scan(&e, std::slice::from_ref(&app)));
    let mut why = Why::default();
    let rep = e.execute(&plan, &mut why, &Cancel::new());
    let still = app.join("Contents/info.txt").exists();
    unlock(&app);
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert_eq!(why.0, [Some(Denied::NoRight)]);
    assert!(still, "the folder is where it was");
}

#[test]
fn the_two_refusals_of_the_system_are_told_apart() {
    use std::io::Error as E;
    let denied = |n| rada_core::error::denied_by(&E::from_raw_os_error(n));
    assert_eq!(denied(libc::EACCES), Some(Denied::NoRight));
    assert_eq!(denied(libc::EPERM), Some(Denied::NotPermitted));
    assert_eq!(denied(libc::ENOENT), None);
    let e = rada_core::Error::io2("rename", "/a", "/b", E::from_raw_os_error(libc::EACCES));
    assert_eq!(e.denied(), Some(Denied::NoRight));
    assert_eq!(rada_core::Error::Cancelled.denied(), None);
}
