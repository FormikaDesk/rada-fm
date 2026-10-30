//! The window that asks what to do when the journal cannot be written, and the history that
//! says an operation can be undone only in part.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_core::events::{CoreEvent, FailureInfo, JobEvent};
use rada_core::journal::{EntryStatus, JournalEntry, UndoState};
use rada_core::ops::{ErrorChoice, OpKind, RunStatus};
use rada_tui::app::Modal;

fn harness() -> H {
    let sb = rada_core::testutil::Sandbox::new();
    let dir = sb.mkdir("w");
    H::new(sb, dir, 120, 30)
}

fn ask(h: &mut H) -> crossbeam_channel::Receiver<ErrorChoice> {
    let (reply, rx) = crossbeam_channel::bounded(1);
    h.app.windows.modal = Some(Modal::Failure {
        job: 1,
        info: FailureInfo {
            index: 2,
            description: "copy".into(),
            message: "write journal: no space left on device".into(),
            path: "b".into(),
            attempt: 1,
            journal: true,
        },
        reply,
    });
    rx
}

#[test]
fn the_window_offers_to_stop_or_to_go_on_without_a_way_back() {
    let mut h = harness();
    let _rx = ask(&mut h);
    let s = h.screen();
    assert!(s.contains("The journal cannot be written"), "{s}");
    assert!(s.contains("no space left on device"), "{s}");
    assert!(s.contains("Abort"), "{s}");
    assert!(
        s.contains("Continue without being able to undo the rest"),
        "{s}"
    );
    assert!(!s.contains("Skip all") && !s.contains("Retry"), "{s}");
}

#[test]
fn enter_stops_and_only_c_goes_on() {
    for (key, want) in [
        (KeyCode::Enter, ErrorChoice::Abort),
        (KeyCode::Esc, ErrorChoice::Abort),
        (KeyCode::Char('a'), ErrorChoice::Abort),
        (KeyCode::Char('c'), ErrorChoice::Skip),
    ] {
        let mut h = harness();
        let rx = ask(&mut h);
        h.key(key);
        assert_eq!(rx.try_recv().unwrap(), want, "{key:?}");
    }
}

#[test]
fn the_keys_of_a_failed_step_do_nothing_here() {
    let mut h = harness();
    let rx = ask(&mut h);
    for c in ['s', 'S', 'r'] {
        h.key(KeyCode::Char(c));
    }
    assert!(rx.try_recv().is_err());
    assert!(matches!(h.app.windows.modal, Some(Modal::Failure { .. })));
}

#[test]
fn the_history_marks_an_operation_undoable_only_in_part() {
    let mut h = harness();
    h.keys("U");
    h.wait(
        "the history",
        |a| matches!(&a.windows.modal, Some(Modal::History(v)) if !v.loading),
    );
    let entry = |id: &str, from| JournalEntry {
        id: id.into(),
        time: 0,
        kind: OpKind::Copy,
        title: format!("copy {id}"),
        reversible: true,
        totals: Default::default(),
        status: EntryStatus::Finished(RunStatus::Completed),
        done: 4,
        failed: 0,
        bytes: 0,
        undo_steps: 1,
        undo_state: UndoState::NotUndone,
        unrecorded_from: from,
        request: None,
        redoable: false,
        damaged: None,
    };
    h.app.on_core_event(CoreEvent::Job(JobEvent::History {
        job: 0,
        entries: vec![entry("whole", None), entry("cut", Some(1))],
    }));
    let s = h.screen();
    assert_eq!(s.matches("undoable in part").count(), 1, "{s}");
}
