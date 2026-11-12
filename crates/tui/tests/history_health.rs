//! The history says, for each operation, what its undo would find: ● it can put everything
//! back, ◐ some of what it left changed (or too much to look at), ○ it is not guaranteed.
//! Only an opinion: the plan of the undo checks again.

mod common;

use std::path::PathBuf;

use common::*;
use crossterm::event::KeyCode;
use rada_core::fs::Fingerprint;
use rada_core::health::Health;
use rada_core::journal::Journal;
use rada_core::ops::{OpKind, Plan, RunStatus, Severity, Step, Warning, WarningKind};
use rada_core::testutil::Sandbox;
use rada_tui::app::Modal;

/// An operation in the journal that left `file` behind, with the limits of its undo as a
/// plan of `kind` with `warnings` would write them; `gap`: the steps after the first ran
/// without their way back.
fn written(
    j: &Journal,
    title: &str,
    kind: OpKind,
    warnings: Vec<Warning>,
    file: &PathBuf,
    gap: bool,
) {
    let mut plan = Plan::empty(kind, title);
    plan.warnings = warnings;
    let id = j.begin_plan(&plan).unwrap();
    let meta = std::fs::symlink_metadata(file).unwrap();
    let expect = Fingerprint::new(
        rada_core::fs::FileKind::File,
        meta.len(),
        meta.modified().ok(),
        None,
        1,
    );
    let step = Step::RemoveFile {
        path: file.clone(),
        expect: Some(expect),
    };
    j.record_undo(&id, step).unwrap();
    let gap = gap.then_some(1);
    j.end_unrecorded(&id, RunStatus::Completed, 1, 0, 1, gap)
        .unwrap();
}

fn device_warning() -> Warning {
    Warning {
        kind: WarningKind::DeviceUndo,
        severity: Severity::Warning,
        message: String::new(),
        count: 1,
        examples: Vec::new(),
    }
}

/// Four operations, in a folder `w` of the home of the sandbox: a plain one, one that ran a
/// program, one on a phone, one with a gap. The folder is returned.
fn journal_of_four(sb: &Sandbox) -> PathBuf {
    let j = sb.journal();
    let dir = sb.dirs.home.join("w");
    std::fs::create_dir_all(&dir).unwrap();
    let f = |n: &str| {
        let p = dir.join(n);
        std::fs::write(&p, n).unwrap();
        p
    };
    written(
        &j,
        "plain",
        OpKind::MakeFile,
        Vec::new(),
        &f("p.txt"),
        false,
    );
    written(&j, "action", OpKind::Action, Vec::new(), &f("a.txt"), false);
    let device = vec![device_warning()];
    written(&j, "phone", OpKind::Copy, device, &f("d.txt"), false);
    written(&j, "gap", OpKind::Copy, Vec::new(), &f("g.txt"), true);
    dir
}

/// The interface on the folder of the four, without the details pane: the pictures do not
/// show where the sandbox is.
fn shown(sb: Sandbox, dir: PathBuf, w: u16, hh: u16) -> H {
    let mut h = H::with_config(sb, dir, w, hh, |c| c.details_pane = Some(false));
    // The places of the navigation pane come from a worker: the picture waits for them.
    h.wait("the places", |a| !a.places.paths.places.is_empty());
    h
}

fn history(h: &mut H) {
    h.keys("U");
    h.wait(
        "the history",
        |a| matches!(&a.windows.modal, Some(Modal::History(v)) if !v.loading),
    );
    h.wait("an opinion on each", |a| match &a.windows.modal {
        Some(Modal::History(v)) => v.entries.iter().all(|e| v.health.contains_key(&e.id)),
        _ => false,
    });
}

/// The history drawn with the time of every entry hidden: pictures that do not depend on
/// the clock.
fn timeless(h: &mut H) -> String {
    if let Some(Modal::History(v)) = &mut h.app.windows.modal {
        for e in &mut v.entries {
            e.time = 0;
        }
    }
    h.screen()
}

fn select(h: &mut H, title: &str) {
    for _ in 0..10 {
        h.key(KeyCode::Up);
    }
    for _ in 0..10 {
        let at = match &h.app.windows.modal {
            Some(Modal::History(v)) => v.entries[v.selected].title.clone(),
            _ => panic!("no history"),
        };
        if at == title {
            return;
        }
        h.key(KeyCode::Down);
    }
    panic!("{title} is not in the history");
}

fn opinion(h: &H, title: &str) -> Health {
    let Some(Modal::History(v)) = &h.app.windows.modal else {
        panic!("no history")
    };
    let e = v.entries.iter().find(|e| e.title == title).unwrap();
    v.health[&e.id]
}

#[test]
fn each_operation_has_its_mark_and_the_one_chosen_says_why() {
    let sb = Sandbox::new();
    let dir = journal_of_four(&sb);
    let mut h = H::new(sb, dir, 110, 30);
    history(&mut h);
    let s = h.screen();
    assert!(s.contains("● can undo"), "{s}");
    assert_eq!(s.matches("○ not guaranteed").count(), 3, "{s}");
    select(&mut h, "action");
    let s = h.screen();
    assert!(s.contains("rada can remove the file it wrote"), "{s}");
    assert!(s.contains("it does not see anything else"), "{s}");
    select(&mut h, "phone");
    assert!(h.screen().contains("a phone or a camera"));
    select(&mut h, "plain");
    assert!(h.screen().contains("Every step was recorded"));
}

#[test]
fn a_file_changed_since_turns_the_mark_and_the_undo_still_decides() {
    let sb = Sandbox::new();
    let dir = journal_of_four(&sb);
    let changed = dir.join("p.txt");
    let mut h = H::new(sb, dir, 110, 30);
    std::fs::write(&changed, "longer than it was").unwrap();
    history(&mut h);
    assert_eq!(opinion(&h, "plain"), Health::ToCheck { changed: 1 });
    select(&mut h, "plain");
    let s = h.screen();
    assert!(s.contains("◐ to check (1 file changed)"), "{s}");
    assert!(s.contains("undo leaves those alone"), "{s}");
    // Enter still asks the plan of the undo, which says the file changed and leaves it.
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(changed.exists());
}

#[test]
fn an_opinion_is_kept_until_an_operation_ends() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 110, 30);
    h.keys("n");
    h.keys("fresh");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    history(&mut h);
    assert_eq!(opinion(&h, "Create folder fresh"), Health::Undoable);
    h.key(KeyCode::Esc);
    assert_eq!(h.app.ops.health.len(), 1, "kept for the next time");
    // The folder goes: an opinion that looked there does not hold, and is made again.
    std::fs::remove_dir(dir.join("fresh")).unwrap();
    history(&mut h);
    assert_eq!(
        opinion(&h, "Create folder fresh"),
        Health::ToCheck { changed: 1 }
    );
    h.key(KeyCode::Esc);
    // An operation that ends forgets them all.
    h.keys("n");
    h.keys("again");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(h.app.ops.health.is_empty());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "a system with no tags has no Tags button, which the Linux snapshot has"
)]
fn the_history_with_its_marks() {
    let sb = Sandbox::new();
    let dir = journal_of_four(&sb);
    std::fs::write(dir.join("p.txt"), "changed").unwrap();
    let mut h = shown(sb, dir, 100, 30);
    history(&mut h);
    select(&mut h, "action");
    insta::assert_snapshot!("history_marks_100x30", timeless(&mut h));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "a system with no tags has no Tags button, which the Linux snapshot has"
)]
fn the_history_with_its_marks_on_a_small_terminal() {
    let sb = Sandbox::new();
    let dir = journal_of_four(&sb);
    let mut h = shown(sb, dir, 60, 20);
    history(&mut h);
    insta::assert_snapshot!("history_marks_60x20", timeless(&mut h));
}
