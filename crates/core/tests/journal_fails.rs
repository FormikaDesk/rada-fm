//! The journal cannot be written in the middle of an operation that can be undone: the
//! operation stops before the next step, or goes on and says that it cannot be undone.

use std::path::PathBuf;

use rada_core::journal::{Journal, JournalEntry};
use rada_core::ops::*;
use rada_core::testutil::*;

/// Answers the question about the journal and remembers what it was asked.
struct Answering {
    choice: Option<JournalChoice>,
    asked: Vec<(usize, String)>,
}

impl Answering {
    fn new(choice: Option<JournalChoice>) -> Self {
        Answering {
            choice,
            asked: Vec::new(),
        }
    }
}

impl ExecHandler for Answering {
    fn on_journal_failure(&mut self, f: &Failure<'_>) -> JournalChoice {
        self.asked.push((f.index, f.error.to_string()));
        self.choice.unwrap_or(JournalChoice::Stop)
    }
}

fn four_files(sb: &Sandbox) -> (Engine, Journal, Plan) {
    let e = sb.engine();
    let srcs: Vec<PathBuf> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| sb.write(format!("src/{n}"), n.as_bytes()))
        .collect();
    sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &srcs),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    (e, sb.journal(), plan)
}

fn copied(sb: &Sandbox) -> Vec<&'static str> {
    ["a", "b", "c", "d"]
        .into_iter()
        .filter(|n| sb.path(format!("dest/{n}")).exists())
        .collect()
}

fn entry(j: &Journal) -> JournalEntry {
    j.entries().unwrap().remove(0)
}

// The records of a run: the start, then the intent and the way back of each copy.

#[test]
fn when_the_intent_cannot_be_written_the_step_does_not_run() {
    let sb = Sandbox::new();
    let (e, j, plan) = four_files(&sb);
    j.fail_write_after(3); // start, a (intent, way back); the intent of b fails
    let mut h = Answering::new(None);
    let out = e.run_operation(&j, &plan, &mut h, &Cancel::new()).unwrap();

    assert_eq!(copied(&sb), ["a"], "stopped before the next step");
    assert_eq!(h.asked.len(), 1);
    assert!(h.asked[0].1.contains("journal"), "{:?}", h.asked);
    assert_eq!(out.report.status(), RunStatus::Aborted);
    assert!(out.report.journal_gap.as_ref().unwrap().stopped);
    let en = entry(&j);
    assert_eq!((en.undo_steps, en.unrecorded_from), (1, None));
}

#[test]
fn when_the_way_back_cannot_be_written_the_run_stops_before_the_next_step() {
    let sb = Sandbox::new();
    let (e, j, plan) = four_files(&sb);
    j.fail_write_after(4); // ... the intent of b is written; its way back fails
    let mut h = Answering::new(Some(JournalChoice::Stop));
    let out = e.run_operation(&j, &plan, &mut h, &Cancel::new()).unwrap();

    assert_eq!(copied(&sb), ["a", "b"]);
    assert_eq!(out.report.status(), RunStatus::Aborted);
    let gap = out.report.journal_gap.as_ref().unwrap();
    assert!(gap.stopped && gap.path.ends_with("src/b"), "{gap:?}");
    let en = entry(&j);
    assert_eq!((en.undo_steps, en.unrecorded_from), (1, Some(1)));
}

#[test]
fn a_handler_with_nobody_to_ask_stops() {
    let sb = Sandbox::new();
    let (e, j, plan) = four_files(&sb);
    j.fail_write_after(4);
    let out = e
        .run_operation(&j, &plan, &mut SkipErrors, &Cancel::new())
        .unwrap();
    assert_eq!(copied(&sb), ["a", "b"]);
    assert_eq!(out.report.status(), RunStatus::Aborted);
}

#[test]
fn going_on_marks_the_operation_as_not_undoable_from_that_step() {
    let sb = Sandbox::new();
    let (e, j, plan) = four_files(&sb);
    j.fail_write_after(4);
    let mut h = Answering::new(Some(JournalChoice::ContinueUnrecorded));
    let out = e.run_operation(&j, &plan, &mut h, &Cancel::new()).unwrap();

    assert_eq!(copied(&sb), ["a", "b", "c", "d"]);
    assert_eq!(h.asked.len(), 1, "asked once, not at every later step");
    assert_eq!(out.report.status(), RunStatus::Completed);
    let gap = out.report.journal_gap.as_ref().unwrap();
    assert!(!gap.stopped && gap.path.ends_with("src/b"), "{gap:?}");
    let en = entry(&j);
    assert_eq!((en.undo_steps, en.unrecorded_from), (1, Some(1)));

    let (_, rep) = undo(&e, &j, &out.id);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(
        copied(&sb),
        ["b", "c", "d"],
        "undo covers only what was recorded"
    );
}

#[test]
fn a_journal_that_works_is_never_asked_about() {
    let sb = Sandbox::new();
    let (e, j, plan) = four_files(&sb);
    let mut h = Answering::new(None);
    let out = e.run_operation(&j, &plan, &mut h, &Cancel::new()).unwrap();
    assert!(h.asked.is_empty() && out.report.journal_gap.is_none());
    assert_eq!(copied(&sb), ["a", "b", "c", "d"]);
    assert_eq!(entry(&j).unrecorded_from, None);
}
