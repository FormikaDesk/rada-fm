//! Trashing and renaming look at the item again at the last moment: one that changed between
//! the plan and the run is left where it is, the same refusal as a permanent delete.

use std::path::Path;
use std::time::Duration;

use rada_core::ops::*;
use rada_core::testutil::*;

/// A pause so that a rewrite of the same size still moves the modification time.
fn tick() {
    std::thread::sleep(Duration::from_millis(30));
}

fn trash_plan(e: &Engine, paths: &[std::path::PathBuf]) -> Plan {
    e.plan_trash(&scan(e, paths))
}

fn in_trash(sb: &Sandbox, name: &str) -> bool {
    sb.trashed_dir().join(name).exists()
}

#[test]
fn every_trash_and_rename_step_of_the_interface_carries_what_the_plan_saw() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let a = sb.write("a.txt", "a");
    sb.write("b.txt", "old");
    let trash = trash_plan(&e, std::slice::from_ref(&a));
    let rename = e.plan_rename(&a, "c.txt".as_ref(), ConflictPolicy::Skip);
    let over = e.plan_rename(&a, "b.txt".as_ref(), ConflictPolicy::Overwrite);

    assert!(matches!(
        trash.steps[..],
        [Step::TrashItem {
            expect: Some(_),
            ..
        }]
    ));
    assert!(matches!(
        rename.steps[..],
        [Step::Rename {
            expect_from: Some(_),
            ..
        }]
    ));
    assert!(matches!(
        over.steps[..],
        [
            Step::TrashItem {
                expect: Some(_),
                ..
            },
            Step::Rename {
                expect_from: Some(_),
                ..
            }
        ]
    ));
}

#[test]
fn a_file_changed_after_the_plan_is_not_trashed() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("f.txt", "as the plan saw it");
    let plan = trash_plan(&e, std::slice::from_ref(&f));

    std::fs::write(&f, "written after the plan, and longer").unwrap();
    let rep = run(&e, &plan);

    assert!(f.exists() && !in_trash(&sb, "f.txt"));
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert!(rep.failed[0].error.contains("changed"), "{:?}", rep.failed);
}

#[test]
fn a_file_of_the_same_size_rewritten_after_the_plan_is_not_trashed() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("f.txt", "aaaa");
    let plan = trash_plan(&e, std::slice::from_ref(&f));

    tick();
    std::fs::write(&f, "bbbb").unwrap();
    let rep = run(&e, &plan);

    assert_eq!(std::fs::read_to_string(&f).unwrap(), "bbbb");
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

#[test]
fn an_unchanged_file_is_trashed_and_undo_brings_it_back() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("f.txt", "same");
    let plan = trash_plan(&e, std::slice::from_ref(&f));

    let out = run_journaled(&e, &j, &plan);

    assert_eq!(
        out.report.status(),
        RunStatus::Completed,
        "{:?}",
        out.report.failed
    );
    assert!(!f.exists() && in_trash(&sb, "f.txt"));
    let (_, rep) = undo(&e, &j, &out.id);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "same");
}

#[test]
fn only_the_changed_file_of_a_selection_stays() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let same = sb.write("same.txt", "same");
    let edited = sb.write("edited.txt", "before");
    let plan = trash_plan(&e, &[same.clone(), edited.clone()]);

    std::fs::write(&edited, "after, and longer").unwrap();
    let rep = run(&e, &plan);

    assert!(!same.exists() && in_trash(&sb, "same.txt"));
    assert!(edited.exists());
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

#[test]
fn a_folder_that_gained_an_entry_after_the_plan_is_not_trashed() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.write("box/a.txt", "a");
    let plan = trash_plan(&e, &[sb.path("box")]);

    tick();
    sb.write("box/new.txt", "made after the plan");
    let rep = run(&e, &plan);

    assert!(sb.path("box/new.txt").exists());
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

/// A folder and a file inside it, selected together (a list of tagged items can do that):
/// moving the file changes the folder, which must not count as the folder having changed.
#[test]
fn a_folder_and_an_item_inside_it_are_both_trashed() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let inner = sb.write("box/inner.txt", "inner");
    let plan = trash_plan(&e, &[inner, sb.path("box")]);

    let rep = run(&e, &plan);

    assert!(!sb.path("box").exists(), "{:?}", rep.failed);
    assert!(in_trash(&sb, "box") && in_trash(&sb, "inner.txt"));
}

#[test]
fn a_plan_written_before_the_fingerprints_is_still_accepted_and_runs() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let a = sb.write("a.txt", "a");
    let b = sb.write("b.txt", "b");
    let mut json = serde_json::to_value(trash_plan(&e, std::slice::from_ref(&a))).unwrap();
    let mut rename =
        serde_json::to_value(e.plan_rename(&b, "c.txt".as_ref(), ConflictPolicy::Skip)).unwrap();
    for plan in [&mut json, &mut rename] {
        for step in plan["steps"].as_array_mut().unwrap() {
            let step = step.as_object_mut().unwrap();
            step.remove("expect");
            step.remove("expect_from");
        }
    }

    for text in [json.to_string(), rename.to_string()] {
        let old: Plan = serde_json::from_str(&text).expect("a plan without the new fields");
        assert!(old.steps.iter().all(|s| matches!(
            s,
            Step::TrashItem { expect: None, .. }
                | Step::Rename {
                    expect_from: None,
                    ..
                }
        )));
        assert_eq!(run(&e, &old).status(), RunStatus::Completed);
    }
    assert!(!a.exists() && sb.path("c.txt").exists());
}

fn content(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn a_file_changed_after_the_plan_is_not_renamed() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("a.txt", "as the plan saw it");
    let plan = e.plan_rename(&f, "b.txt".as_ref(), ConflictPolicy::Skip);

    std::fs::write(&f, "written after the plan, and longer").unwrap();
    let rep = run(&e, &plan);

    assert!(f.exists() && !sb.path("b.txt").exists());
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert!(rep.failed[0].error.contains("changed"), "{:?}", rep.failed);
}

#[test]
fn an_unchanged_file_is_renamed_and_undo_puts_the_name_back() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("a.txt", "same");
    let plan = e.plan_rename(&f, "b.txt".as_ref(), ConflictPolicy::Skip);

    let out = run_journaled(&e, &j, &plan);

    assert_eq!(out.report.status(), RunStatus::Completed);
    assert_eq!(content(&sb.path("b.txt")), "same");
    let (_, rep) = undo(&e, &j, &out.id);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(f.exists() && !sb.path("b.txt").exists());
}

/// What is written to a renamed file afterwards does not stop the rename from being undone:
/// the undo never replaces a name, so it has no fingerprint to compare.
#[test]
fn a_file_edited_after_its_rename_is_still_renamed_back() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("a.txt", "same");
    let plan = e.plan_rename(&f, "b.txt".as_ref(), ConflictPolicy::Skip);
    let out = run_journaled(&e, &j, &plan);

    std::fs::write(sb.path("b.txt"), "edited later, and longer").unwrap();
    let (_, rep) = undo(&e, &j, &out.id);

    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(content(&f), "edited later, and longer");
}

#[test]
fn replacing_a_name_whose_old_file_changed_trashes_nothing_and_renames_nothing() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let new = sb.write("a.txt", "new");
    let old = sb.write("b.txt", "old");
    let plan = e.plan_rename(&new, "b.txt".as_ref(), ConflictPolicy::Overwrite);

    std::fs::write(&old, "somebody edited it, and longer").unwrap();
    let rep = run(&e, &plan);

    assert_eq!(content(&old), "somebody edited it, and longer");
    assert_eq!(content(&new), "new", "the rename waits for the name");
    assert!(!in_trash(&sb, "b.txt"));
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert_eq!(rep.skipped_dependent, 1);
}

#[test]
fn a_move_on_the_same_filesystem_leaves_a_file_that_changed_since_the_plan() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("src/f.txt", "as the plan saw it");
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, std::slice::from_ref(&f)),
        &dest,
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    assert!(plan.steps.iter().any(|s| matches!(
        s,
        Step::Rename {
            expect_from: Some(_),
            ..
        }
    )));

    std::fs::write(&f, "written after the plan, and longer").unwrap();
    let rep = run(&e, &plan);

    assert!(f.exists() && !dest.join("f.txt").exists());
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

fn chain(sb: &Sandbox) -> (Vec<std::path::PathBuf>, Engine, Plan) {
    let e = sb.engine();
    let items = vec![
        sb.write("d/1", "one"),
        sb.write("d/2", "two"),
        sb.write("d/3", "three"),
    ];
    // 1 -> 2, 2 -> 3, 3 -> 4: a chain, so every name goes through a temporary one.
    let pat = Pattern::parse("{n}").unwrap().with_counter(2, 1);
    let plan = e.plan_bulk_rename(&items, &pat);
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    (items, e, plan)
}

#[test]
fn a_bulk_rename_looks_at_each_file_before_its_first_phase_only() {
    let sb = Sandbox::new();
    let (_, _, plan) = chain(&sb);
    let phase_one: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Rename {
                to, expect_from, ..
            } => Some((
                to.to_string_lossy().contains(".rada-rename-"),
                expect_from.is_some(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(phase_one.len(), 6);
    // Three moves to a temporary name, with a fingerprint; three from it, without.
    assert_eq!(phase_one[..3], [(true, true); 3]);
    assert_eq!(phase_one[3..], [(false, false); 3]);
}

/// The changed file stays, and so does everything else, but the chain is left half done: the
/// file that was to take the changed file's name waits under its temporary name. The history
/// holds what was done, so undo puts every name back.
#[test]
fn a_bulk_rename_with_a_changed_file_loses_nothing_and_can_be_undone() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let (items, _, plan) = chain(&sb);
    std::fs::write(&items[2], "changed after the plan, and longer").unwrap();

    let out = run_journaled(&e, &j, &plan);

    let rep = &out.report;
    assert_eq!(
        content(&sb.path("d/3")),
        "changed after the plan, and longer"
    );
    assert_eq!(content(&sb.path("d/2")), "one");
    assert!(!sb.path("d/4").exists());
    let waiting: Vec<_> = std::fs::read_dir(sb.path("d"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".rada-rename-"))
        .collect();
    assert_eq!(waiting.len(), 1, "{:?}", rep.failed);
    assert_eq!(content(&waiting[0].path()), "two");
    // The changed file's own failure, the name that was taken, and the step with nothing to act on.
    assert_eq!(rep.failed.len(), 2, "{:?}", rep.failed);
    assert_eq!(rep.skipped_dependent, 1);

    let (_, back) = undo(&e, &j, &out.id);
    assert_eq!(back.status(), RunStatus::Completed, "{:?}", back.failed);
    assert_eq!(content(&sb.path("d/1")), "one");
    assert_eq!(content(&sb.path("d/2")), "two");
    assert_eq!(
        content(&sb.path("d/3")),
        "changed after the plan, and longer"
    );
}
