//! What the journal does with a line it cannot read: a write cut short by a crash and a record
//! of a newer rada are skipped; damage in the middle of the file stops recovery and undo for
//! the operations after it, and is reported.

use std::io::Write;
use std::path::Path;

use rada_core::journal::{EntryStatus, Journal};
use rada_core::ops::*;
use rada_core::testutil::*;

fn append(path: &Path, bytes: &[u8]) {
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    f.write_all(bytes).unwrap();
}

/// A finished, undoable operation whose undo step removes `file`.
fn finished(j: &Journal, title: &str, file: &Path) -> String {
    let id = j
        .begin(OpKind::Copy, title, true, Totals::default(), None, None)
        .unwrap();
    j.record_undo(
        &id,
        Step::RemoveFile {
            path: file.to_path_buf(),
            expect: None,
        },
    )
    .unwrap();
    j.end(&id, RunStatus::Completed, 1, 0, 0).unwrap();
    id
}

/// An operation of a process that no longer exists, begun by a rada without locks.
fn dead_operation(path: &Path, id: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let line = format!(
        r#"{{"t":"begin","id":"{id}","time":{now},"kind":"copy","title":"cut","reversible":true,"totals":{{"items":0,"files":0,"dirs":0,"symlinks":0,"bytes":0}}}}"#
    );
    append(path, format!("{line}\n").as_bytes());
}

#[test]
fn a_last_line_cut_by_a_crash_is_ignored_and_does_not_swallow_the_next_record() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let a = finished(&j, "first", &sb.path("a"));
    // The process died in the middle of writing a record: no newline.
    append(j.path(), br#"{"t":"begin","id":"cut","time":1,"ki"#);
    assert_eq!(j.entries().unwrap().len(), 1);
    assert!(j.damage().unwrap().is_none(), "a torn tail is not damage");

    // The next start writes on: its records must be readable and nothing may be flagged.
    let j2 = sb.journal();
    let b = finished(&j2, "second", &sb.path("b"));
    let entries = j2.entries().unwrap();
    assert_eq!(
        entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        [a.as_str(), b.as_str()],
        "the record written after the crash was lost"
    );
    assert!(j2.damage().unwrap().is_none());
    assert_eq!(j2.last_undoable().unwrap().unwrap().id, b);
}

#[test]
fn a_record_of_a_newer_rada_is_ignored_wherever_it_is() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let a = finished(&j, "first", &sb.path("a"));
    append(
        j.path(),
        b"{\"t\":\"from_the_future\",\"id\":\"x\",\"n\":3}\n",
    );
    let b = finished(&j, "second", &sb.path("b"));
    assert_eq!(j.entries().unwrap().len(), 2);
    assert!(
        j.damage().unwrap().is_none(),
        "a newer record is not damage"
    );
    assert_eq!(j.last_undoable().unwrap().unwrap().id, b);
    assert!(sb.engine().plan_undo(&j, &a).is_ok());
}

#[test]
fn damage_in_the_middle_stops_recovery_and_undo_after_it() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let e = sb.engine();
    let a = finished(&j, "before", &sb.path("a"));
    append(j.path(), b"this is not a record at all\n");
    let b = finished(&j, "after", &sb.path("b"));
    dead_operation(j.path(), "ffff-ffffff7-0");

    assert_eq!(j.damage().unwrap(), Some(4), "the fourth line of the file");

    // Recovery leaves the operation after the damage as it is.
    assert!(e.recover_interrupted(&j).unwrap().is_empty());
    let last = j.entries().unwrap().pop().unwrap();
    assert_eq!(last.status, EntryStatus::Interrupted);

    // Undo is refused after the damage, with the file named, and still works before it.
    assert_eq!(j.last_undoable().unwrap().unwrap().id, a);
    let err = e.plan_undo(&j, &b).unwrap_err().to_string();
    assert!(err.contains("journal.jsonl"), "{err}");
    assert!(err.contains("4"), "{err}");
    assert!(e.plan_undo(&j, &a).is_ok());
}

#[test]
fn a_complete_record_missing_only_its_newline_is_kept() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let a = finished(&j, "first", &sb.path("a"));
    // Killed between the record and its newline.
    append(
        j.path(),
        br#"{"t":"begin","id":"1-1-0","time":1,"kind":"copy","title":"t","reversible":true,"totals":{"items":0,"files":0,"dirs":0,"symlinks":0,"bytes":0}}"#,
    );
    assert_eq!(j.entries().unwrap().len(), 2);
    let b = finished(&sb.journal(), "second", &sb.path("b"));
    let ids: Vec<String> = j.entries().unwrap().into_iter().map(|e| e.id).collect();
    assert_eq!(ids, [a, "1-1-0".to_string(), b]);
    assert!(j.damage().unwrap().is_none());
}

#[test]
fn damage_is_reported_at_start_with_the_line_and_the_file() {
    use rada_core::events::{CoreEvent, JobEvent};
    use std::sync::Arc;
    let sb = Sandbox::new();
    let j = Arc::new(sb.journal());
    finished(&j, "before", &sb.path("a"));
    append(j.path(), b"{{{\n");
    finished(&j, "after", &sb.path("b"));
    let (tx, rx) = crossbeam_channel::unbounded();
    rada_core::jobs::Jobs::new(sb.engine(), Some(j.clone()), tx).recover_on_start();
    let ev = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    match ev {
        CoreEvent::Job(JobEvent::JournalDamaged { path, line }) => {
            assert_eq!(line, 4);
            assert_eq!(path, j.path());
        }
        _ => panic!("the damage was not reported"),
    }
}
