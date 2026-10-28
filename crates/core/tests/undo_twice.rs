//! Two instances of rada undoing the same history entry: the second is refused with a message, whether it
//! comes while the first is running or after it has finished.

use rada_core::journal::{Journal, UndoState};
use rada_core::ops::*;
use rada_core::testutil::*;

fn copied(sb: &Sandbox, e: &Engine, j: &Journal) -> String {
    sb.write("src/a.txt", "alpha");
    sb.write("src/b.txt", "beta");
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(e, j, &plan);
    assert!(out.report.failed.is_empty());
    out.id
}

/// Runs its closure once, after the first step of the undo it is part of.
struct Meanwhile<F: FnMut()>(Option<F>);

impl<F: FnMut()> ExecHandler for Meanwhile<F> {
    fn step_finished(&mut self, _index: usize, _step: &Step, _result: &StepResult) {
        if let Some(mut f) = self.0.take() {
            f();
        }
    }
}

#[test]
fn a_second_rada_undoing_the_same_entry_while_the_first_runs_is_refused() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let id = copied(&sb, &e, &j);
    let up = e.plan_undo(&j, &id).unwrap();
    // Another instance: its own handle on the same journal file.
    let (e2, j2) = (sb.engine(), sb.journal());
    let up2 = e2.plan_undo(&j2, &id).unwrap();

    let mut refused = None;
    let mut h = Meanwhile(Some(|| {
        refused = Some(e2.run_undo(&j2, &up2, &mut SkipErrors, &Cancel::new()));
    }));
    e.run_undo(&j, &up, &mut h, &Cancel::new()).unwrap();
    let err = refused.expect("the second undo was tried").unwrap_err();
    assert!(
        err.to_string().contains("being undone by another rada"),
        "{err}"
    );
    assert!(snapshot(&sb.path("dest")).is_empty());
    assert_eq!(j.entries().unwrap()[0].undo_state, UndoState::Undone);
}

#[test]
fn a_plan_made_before_another_rada_undid_the_entry_is_refused() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let id = copied(&sb, &e, &j);
    let (e2, j2) = (sb.engine(), sb.journal());
    let stale = e2.plan_undo(&j2, &id).unwrap();

    undo(&e, &j, &id);
    let err = e2
        .run_undo(&j2, &stale, &mut SkipErrors, &Cancel::new())
        .unwrap_err();
    assert!(err.to_string().contains("already undone"), "{err}");
    let undone_records = std::fs::read_to_string(j.path())
        .unwrap()
        .matches(r#""t":"undone""#)
        .count();
    assert_eq!(undone_records, 1, "the entry is not marked a second time");
}

#[test]
fn the_lock_of_an_undo_is_let_go_when_it_ends() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let id = copied(&sb, &e, &j);
    undo(&e, &j, &id);
    let left = std::fs::read_dir(j.path().parent().unwrap().join("locks"))
        .unwrap()
        .count();
    assert_eq!(left, 0);
}
