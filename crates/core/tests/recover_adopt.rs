//! A destination that is there after a crash is taken as made by the operation that was cut
//! short only when it is what that operation was writing: the size the step declared and, when
//! the copy keeps times, the modification time. Anything else was put there by somebody else,
//! and undoing the operation must not remove it.
#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rada_core::journal::Journal;
use rada_core::ops::{Cancel, SkipErrors};
use rada_core::testutil::*;

const MTIME: i64 = 1_700_000_000;

fn append(j: &Journal, line: serde_json::Value) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(j.path())
        .unwrap();
    writeln!(f, "{line}").unwrap();
}

/// An operation of a rada that is gone, cut short in a copy to `dst` that was to be `intent`.
fn cut_short(j: &Journal, intent: serde_json::Value) -> String {
    let id = format!("1-{:x}-0", std::process::id());
    let old = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 2 * 24 * 3600;
    append(
        j,
        serde_json::json!({"t":"begin","id":id,"time":old,"kind":"copy","title":"cut",
            "reversible":true,"totals":{"items":0,"files":0,"dirs":0,"symlinks":0,"bytes":0}}),
    );
    append(
        j,
        serde_json::json!({"t":"pending","id":id,"intent":intent}),
    );
    id
}

fn set_mtime(p: &Path, secs: i64) {
    let t = UNIX_EPOCH + Duration::from_secs(secs as u64);
    std::fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn create_intent(dst: &Path, size: u64, mtime: Option<i64>) -> serde_json::Value {
    let mut v = serde_json::json!({"k":"create","dst":dst,"size":size});
    if let Some(secs) = mtime {
        v["mtime"] = serde_json::json!({"secs":secs,"nanos":0});
    }
    v
}

#[test]
fn a_file_of_somebody_else_is_not_taken_for_the_copy_and_survives_the_undo() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let dst = sb.write("d/a.txt", "put here by someone else");
    let id = cut_short(&j, create_intent(&dst, 5, None));

    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec.len(), 1);
    assert!(rec[0].adopted.is_empty(), "{:?}", rec[0].adopted);
    assert!(!rec[0].notes.is_empty(), "the file left alone is mentioned");
    if let Ok(up) = e.plan_undo(&j, &id) {
        assert!(up.plan.steps.is_empty(), "{:?}", up.plan.steps);
    }
    assert!(dst.exists());
}

#[test]
fn a_file_of_the_right_size_and_the_wrong_time_is_left_when_the_copy_keeps_times() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let dst = sb.write("d/a.txt", "12345");
    set_mtime(&dst, MTIME + 3600);
    cut_short(&j, create_intent(&dst, 5, Some(MTIME)));

    let rec = e.recover_interrupted(&j).unwrap();
    assert!(rec[0].adopted.is_empty(), "{:?}", rec[0].adopted);
    assert!(dst.exists());
}

#[test]
fn the_file_the_step_was_writing_is_taken_and_undone() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let dst = sb.write("d/a.txt", "12345");
    set_mtime(&dst, MTIME);
    let id = cut_short(&j, create_intent(&dst, 5, Some(MTIME)));

    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec[0].adopted, vec![dst.clone()]);
    let (_, report) = undo(&e, &j, &id);
    assert!(report.failed.is_empty());
    assert!(!dst.exists());
}

#[test]
fn a_record_without_size_or_time_is_taken_as_before() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let dst = sb.write("d/a.txt", "whatever");
    cut_short(&j, serde_json::json!({"k":"create","dst":dst}));

    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec[0].adopted, vec![dst]);
}

fn link_intent(k: &str, dst: &Path, target: &str) -> serde_json::Value {
    let mut v = serde_json::json!({"k":k,"dst":dst,"target":target});
    if k == "move" {
        v["src"] = serde_json::json!(dst.with_file_name("old-place"));
        v["symlink"] = serde_json::json!(true);
    }
    v
}

/// Recover a cut-short operation, then undo it: `(adopted, notes)`.
fn recover_and_undo(sb: &Sandbox, intent: serde_json::Value) -> (usize, usize) {
    let (e, j) = (sb.engine(), sb.journal());
    let id = cut_short(&j, intent);
    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec.len(), 1);
    if let Ok(up) = e.plan_undo(&j, &id) {
        e.run_undo(&j, &up, &mut SkipErrors, &Cancel::new())
            .unwrap();
    }
    (rec[0].adopted.len(), rec[0].notes.len())
}

#[test]
fn a_file_where_a_link_was_to_be_made_is_not_taken_and_survives_the_undo() {
    for k in ["create", "move"] {
        let sb = Sandbox::new();
        let dst = sb.write("d/l", "a file of somebody else");
        let (adopted, notes) = recover_and_undo(&sb, link_intent(k, &dst, "wanted"));
        assert_eq!((adopted, notes), (0, 1), "{k}");
        assert_eq!(
            std::fs::read_to_string(&dst).unwrap(),
            "a file of somebody else"
        );
    }
}

#[test]
fn a_link_to_somewhere_else_is_not_taken_and_survives_the_undo() {
    for k in ["create", "move"] {
        let sb = Sandbox::new();
        let dst = sb.path("d/l");
        std::fs::create_dir_all(sb.path("d")).unwrap();
        std::os::unix::fs::symlink("elsewhere", &dst).unwrap();
        let (adopted, notes) = recover_and_undo(&sb, link_intent(k, &dst, "wanted"));
        assert_eq!((adopted, notes), (0, 1), "{k}");
        assert_eq!(std::fs::read_link(&dst).unwrap(), Path::new("elsewhere"));
    }
}

#[test]
fn the_link_the_step_was_making_is_taken_and_undone() {
    let sb = Sandbox::new();
    let dst = sb.path("d/l");
    std::fs::create_dir_all(sb.path("d")).unwrap();
    std::os::unix::fs::symlink("wanted", &dst).unwrap();
    let (adopted, _) = recover_and_undo(&sb, link_intent("create", &dst, "wanted"));
    assert_eq!(adopted, 1);
    assert!(
        std::fs::symlink_metadata(&dst).is_err(),
        "the undo removed the link"
    );
}

#[test]
fn an_archive_nothing_can_vouch_for_is_left_alone_with_a_note() {
    let sb = Sandbox::new();
    let dst = sb.write("d/new.zip", "already here");
    let (adopted, notes) = recover_and_undo(
        &sb,
        serde_json::json!({"k":"create","dst":dst,"unverifiable":true}),
    );
    assert_eq!((adopted, notes), (0, 1));
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "already here");
}

#[test]
fn the_records_of_the_steps_carry_what_recovery_needs() {
    use rada_core::journal::Intent;
    use rada_core::ops::Step;
    let link = Step::CopySymlink {
        src: "/a/l".into(),
        dst: "/b/l".into(),
        target: "t".into(),
        remove_source: false,
    };
    let v = serde_json::to_value(Intent::of(&link).unwrap()).unwrap();
    assert_eq!(v["target"], "t");
    let archive = Step::Compress {
        dst: "/b/x.zip".into(),
        format: rada_core::archive::ArchiveKind::Zip,
        items: Vec::new(),
        size: 0,
    };
    let v = serde_json::to_value(Intent::of(&archive).unwrap()).unwrap();
    assert_eq!(v["unverifiable"], true);
}
