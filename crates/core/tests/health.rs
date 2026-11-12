//! The opinion on the undo of each operation of the history: one look at each item an
//! operation left, no hash, and nothing the plan of the undo does not check again itself.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use rada_core::events::{CoreEvent, JobEvent};
use rada_core::health::{self, Health, MAX_LOOKS, UndoLimit, Unsure, Verdict};
use rada_core::journal::{Journal, JournalEntry};
use rada_core::ops::*;
use rada_core::testutil::*;

struct Scene {
    sb: Sandbox,
    e: Engine,
    j: Journal,
}

fn scene() -> Scene {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    Scene { sb, e, j }
}

/// Copy `names` (written under `src/`) into `dest/` through the journal.
fn copied(s: &Scene, names: &[&str]) -> (String, PathBuf) {
    let srcs: Vec<PathBuf> = names
        .iter()
        .map(|n| s.sb.write(format!("src/{n}"), n.as_bytes()))
        .collect();
    let dest = s.sb.mkdir("dest");
    let plan = s.e.plan_transfer(
        &scan(&s.e, &srcs),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let ran = run_journaled(&s.e, &s.j, &plan);
    (ran.id, dest)
}

fn entry(j: &Journal, id: &str) -> JournalEntry {
    j.entries()
        .unwrap()
        .into_iter()
        .find(|e| e.id == id)
        .unwrap()
}

fn verdict(s: &Scene, id: &str) -> Verdict {
    let mut undo = s.j.load_undo_many(&[id.to_string()]).unwrap();
    let (steps, remaining) = undo.remove(id).unwrap();
    assert_eq!(health::without_disk(&entry(&s.j, id)), None);
    health::look(s.e.fs.as_ref(), &steps, &remaining, &|| false).unwrap()
}

#[test]
fn a_copy_left_as_it_was_can_be_undone() {
    let s = scene();
    let (id, _) = copied(&s, &["a.txt", "b.txt"]);
    assert_eq!(verdict(&s, &id).health, Health::Undoable);
}

#[test]
fn copies_changed_or_gone_since_are_counted() {
    let s = scene();
    let (id, dest) = copied(&s, &["a.txt", "b.txt", "c.txt"]);
    std::fs::write(dest.join("a.txt"), "longer than before").unwrap();
    std::fs::remove_file(dest.join("b.txt")).unwrap();
    assert_eq!(verdict(&s, &id).health, Health::ToCheck { changed: 2 });
    // Only an opinion: the undo still takes away what is as it was, and only that.
    let (_, rep) = undo(&s.e, &s.j, &id);
    assert!(!dest.join("c.txt").exists());
    assert!(dest.join("a.txt").exists(), "{:?}", rep.failed);
}

#[test]
fn a_move_whose_item_went_away_is_to_check() {
    let s = scene();
    let src = s.sb.write("src/m.txt", "m");
    let dest = s.sb.mkdir("dest");
    let plan = s.e.plan_transfer(
        &scan(&s.e, std::slice::from_ref(&src)),
        &dest,
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    let id = run_journaled(&s.e, &s.j, &plan).id;
    assert_eq!(verdict(&s, &id).health, Health::Undoable);
    std::fs::remove_file(dest.join("m.txt")).unwrap();
    assert_eq!(verdict(&s, &id).health, Health::ToCheck { changed: 1 });
}

#[test]
fn a_trash_whose_item_left_the_trash_is_to_check() {
    let s = scene();
    let item = s.sb.write("w/t.txt", "t");
    let plan = s.e.plan_trash(&scan(&s.e, std::slice::from_ref(&item)));
    let id = run_journaled(&s.e, &s.j, &plan).id;
    assert_eq!(verdict(&s, &id).health, Health::Undoable);
    let (steps, _) = s.j.load_undo(&id).unwrap();
    let Step::Restore { item } = &steps[0] else {
        panic!("{steps:?}")
    };
    let stored = match &item.handle {
        rada_core::platform::trash::TrashHandle::Freedesktop { stored, .. }
        | rada_core::platform::trash::TrashHandle::Directory { stored } => stored.clone(),
        other => panic!("{other:?}"),
    };
    std::fs::remove_file(stored).unwrap();
    assert_eq!(verdict(&s, &id).health, Health::ToCheck { changed: 1 });
}

#[test]
fn past_the_limit_nothing_is_looked_at() {
    let s = scene();
    let dir = s.sb.mkdir("src/many");
    for i in 0..=MAX_LOOKS {
        std::fs::write(dir.join(format!("{i}.txt")), "x").unwrap();
    }
    let dest = s.sb.mkdir("dest");
    let plan = s.e.plan_transfer(
        &scan(&s.e, &[dir]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let id = run_journaled(&s.e, &s.j, &plan).id;
    let v = verdict(&s, &id);
    assert_eq!(v.health, Health::TooMany);
    assert!(v.folders.is_empty());
}

#[test]
fn an_opinion_holds_until_a_folder_it_looked_into_changes() {
    let s = scene();
    let (id, dest) = copied(&s, &["a.txt"]);
    let v = verdict(&s, &id);
    assert_eq!(v.folders.len(), 1, "{:?}", v.folders);
    assert!(health::still_holds(s.e.fs.as_ref(), &v));
    // A folder's time moves with what is added to it; a second later, so that it shows.
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(dest.join("new.txt"), "n").unwrap();
    assert!(!health::still_holds(s.e.fs.as_ref(), &v));
}

#[test]
fn a_look_can_be_stopped() {
    let s = scene();
    let (id, _) = copied(&s, &["a.txt"]);
    let (steps, remaining) = s.j.load_undo(&id).unwrap();
    assert_eq!(
        health::look(s.e.fs.as_ref(), &steps, &remaining, &|| true),
        None
    );
}

#[test]
fn the_plan_writes_down_why_its_undo_cannot_be_promised() {
    let s = scene();
    let mut action = Plan::empty(OpKind::Action, "make a file");
    assert_eq!(action.undo_limit(), Some(UndoLimit::Action));
    action.kind = OpKind::Copy;
    assert_eq!(action.undo_limit(), None);
    let mut device = Plan::empty(OpKind::Copy, "to a phone");
    device.warnings.push(Warning {
        kind: WarningKind::DeviceUndo,
        severity: Severity::Warning,
        message: String::new(),
        count: 1,
        examples: Vec::new(),
    });
    assert_eq!(device.undo_limit(), Some(UndoLimit::Device));
    let id = s.j.begin_plan(&device).unwrap();
    s.j.end(&id, RunStatus::Completed, 0, 0, 0).unwrap();
    assert_eq!(entry(&s.j, &id).undo_limit, Some(UndoLimit::Device));
    // Most operations write nothing new in the record of their start.
    let (id, _) = copied(&s, &["a.txt"]);
    assert_eq!(entry(&s.j, &id).undo_limit, None);
    let journal = std::fs::read_to_string(s.j.path()).unwrap();
    assert_eq!(journal.matches("undo_limit").count(), 1);
}

#[test]
fn what_is_known_without_the_disk_is_said_without_it() {
    let s = scene();
    let (id, _) = copied(&s, &["a.txt"]);
    let plain = entry(&s.j, &id);
    let with = |limit, gap| JournalEntry {
        undo_limit: limit,
        unrecorded_from: gap,
        ..plain.clone()
    };
    let not = |u| Some(Health::NotGuaranteed(u));
    assert_eq!(health::without_disk(&plain), None, "its files say");
    assert_eq!(
        health::without_disk(&with(Some(UndoLimit::Action), None)),
        not(Unsure::Action)
    );
    assert_eq!(
        health::without_disk(&with(Some(UndoLimit::Device), None)),
        not(Unsure::Device)
    );
    assert_eq!(
        health::without_disk(&with(Some(UndoLimit::Action), Some(1))),
        not(Unsure::Gaps)
    );
    // Nothing to say of what cannot be undone at all: it has its own word already.
    let permanent = JournalEntry {
        reversible: false,
        ..with(Some(UndoLimit::Action), None)
    };
    assert_eq!(health::without_disk(&permanent), None);
}

#[test]
fn the_job_gives_one_opinion_for_each_and_keeps_the_ones_that_hold() {
    let s = scene();
    let (a, _) = copied(&s, &["a.txt"]);
    let item = s.sb.write("w/t.txt", "t");
    let trash = s.e.plan_trash(&scan(&s.e, std::slice::from_ref(&item)));
    let b = run_journaled(&s.e, &s.j, &trash).id;
    let entries: Vec<JournalEntry> = s.j.entries().unwrap().into_iter().rev().collect();
    let (tx, rx) = crossbeam_channel::unbounded();
    let jobs =
        rada_core::jobs::Jobs::new(s.e.clone(), Some(std::sync::Arc::new(s.sb.journal())), tx);
    // An opinion of `a` that still holds, made up so that it shows it was not looked again.
    let held = Verdict {
        health: Health::ToCheck { changed: 7 },
        folders: Vec::new(),
    };
    let cached = HashMap::from([(a.clone(), held.clone())]);
    jobs.check_history(entries, cached);
    let mut got = HashMap::new();
    while got.len() < 2 {
        let event = rx.recv_timeout(Duration::from_secs(10));
        if let CoreEvent::Job(JobEvent::Health { id, verdict, .. }) = event.expect("an opinion") {
            got.insert(id, verdict.health);
        }
    }
    assert_eq!(got[&a], held.health);
    assert_eq!(got[&b], Health::Undoable);
}
