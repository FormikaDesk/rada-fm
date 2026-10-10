//! Creating an empty file goes through the same plan, journal and undo as everything else,
//! and the journal can say in a few words what the next undo would undo.

use rada_core::journal::UndoState;
use rada_core::ops::*;
use rada_core::testutil::*;

fn plan_new_file(e: &Engine, sb: &Sandbox, parent: &str, name: &str) -> Plan {
    e.plan_mkfile(&sb.path(parent), std::ffi::OsStr::new(name))
}

#[test]
fn a_new_file_is_planned_created_and_undone() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.mkdir("w");
    let plan = plan_new_file(&e, &sb, "w", "notes.txt");
    assert_eq!(plan.kind, OpKind::MakeFile);
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert!(
        !sb.path("w/notes.txt").exists(),
        "planning writes nothing at all"
    );

    let out = run_journaled(&e, &j, &plan);
    assert_eq!(out.report.status(), RunStatus::Completed);
    let made = sb.path("w/notes.txt");
    assert!(made.is_file());
    assert_eq!(std::fs::metadata(&made).unwrap().len(), 0);

    let entry = j.entries().unwrap().pop().unwrap();
    assert_eq!(entry.describe(), "new file");

    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(!made.exists(), "undo removes the file it made");
    assert_eq!(j.entries().unwrap()[0].undo_state, UndoState::Undone);
}

#[test]
fn undo_leaves_a_new_file_alone_once_it_has_content() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.mkdir("w");
    let out = run_journaled(&e, &j, &plan_new_file(&e, &sb, "w", "draft.txt"));
    std::thread::sleep(std::time::Duration::from_millis(15));
    std::fs::write(sb.path("w/draft.txt"), "I wrote something").unwrap();
    let (up, rep) = undo(&e, &j, &out.id);
    // The file changed after it was made: it is kept, and the undo says why.
    assert_eq!(
        std::fs::read_to_string(sb.path("w/draft.txt")).unwrap(),
        "I wrote something",
        "what the user wrote is never deleted"
    );
    assert!(
        !up.blocked.is_empty() || !rep.failed.is_empty() || rep.status() != RunStatus::Completed
    );
}

#[test]
fn an_existing_name_blocks_the_plan_and_an_odd_one_is_refused() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.write("w/taken.txt", "mine");
    let plan = plan_new_file(&e, &sb, "w", "taken.txt");
    assert!(!plan.is_executable());
    assert!(plan.steps.is_empty());
    assert_eq!(
        std::fs::read_to_string(sb.path("w/taken.txt")).unwrap(),
        "mine"
    );
    let bad = plan_new_file(&e, &sb, "w", "a/b");
    assert!(!bad.is_executable(), "a slash is not part of a name");
}

#[test]
fn the_request_form_validates_and_plans() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.mkdir("w");
    let req = OpRequest::MakeFile {
        parent: sb.path("w"),
        name: "x.md".into(),
    };
    req.validate().unwrap();
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"op\":\"make_file\""), "{json}");
    let back: OpRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back, req);
    assert!(
        OpRequest::MakeFile {
            parent: sb.path("w"),
            name: String::new()
        }
        .validate()
        .is_err()
    );
    let cancel = Cancel::new();
    let planned = e
        .plan_request(
            &req,
            None,
            ScanControl {
                cancel: cancel.flag(),
                progress: &mut |_| {},
            },
        )
        .expect("a request plans");
    assert_eq!(planned.plan.kind, OpKind::MakeFile);
}

#[test]
fn the_journal_describes_operations_briefly() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.write("src/a.txt", "a");
    sb.write("src/b.txt", "b");
    sb.write("src/c.txt", "c");
    sb.mkdir("dest");
    let srcs: Vec<_> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|n| sb.path(format!("src/{n}")))
        .collect();
    let plan = e.plan_transfer(
        &scan(&e, &srcs),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    run_journaled(&e, &j, &plan);
    let last = j.last_undoable().unwrap().expect("a copy can be undone");
    assert_eq!(last.describe(), "copy of 3 items");
}
