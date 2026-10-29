//! A destination that is there after a crash is taken as made by the operation that was cut
//! short only when it is what that operation was writing: the size the step declared and, when
//! the copy keeps times, the modification time. Anything else was put there by somebody else,
//! and undoing the operation must not remove it.
#![cfg(unix)]

use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rada_core::journal::Journal;
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
