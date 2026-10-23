//! Renaming onto a name that exists offers the choices of a copy: skip, keep both, replace.

use rada_core::ops::*;
use rada_core::testutil::*;

fn plan_over(e: &Engine, sb: &Sandbox, policy: ConflictPolicy) -> Plan {
    e.plan_rename(&sb.path("a.txt"), "b.txt".as_ref(), policy)
}

fn text(sb: &Sandbox, rel: &str) -> String {
    std::fs::read_to_string(sb.path(rel)).unwrap()
}

#[test]
fn without_a_choice_nothing_changes() {
    let sb = Sandbox::new();
    sb.write("a.txt", "new");
    sb.write("b.txt", "old");
    let plan = plan_over(&sb.engine(), &sb, ConflictPolicy::Skip);
    assert!(plan.steps.is_empty() && plan.blocking().next().is_none());
    assert!(plan.has_conflicts() && !plan.is_executable());
    assert_eq!(text(&sb, "a.txt") + &text(&sb, "b.txt"), "newold");
}

#[test]
fn replacing_trashes_the_old_file_and_undo_brings_it_back() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.write("a.txt", "new");
    sb.write("b.txt", "old");
    let plan = plan_over(&e, &sb, ConflictPolicy::Overwrite);
    assert!(matches!(plan.steps[0], Step::TrashItem { .. }));
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(text(&sb, "b.txt"), "new");
    assert!(!sb.path("a.txt").exists());
    let (_, rep) = undo(&e, &j, &out.id);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(text(&sb, "a.txt") + &text(&sb, "b.txt"), "newold");
}

#[test]
fn keeping_both_picks_a_free_name() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.write("a.txt", "new");
    sb.write("b.txt", "old");
    run(&e, &plan_over(&e, &sb, ConflictPolicy::KeepBoth));
    assert_eq!(text(&sb, "b (1).txt"), "new");
    assert_eq!(text(&sb, "b.txt"), "old");
}

#[test]
fn a_file_cannot_replace_a_folder() {
    let sb = Sandbox::new();
    sb.write("a.txt", "new");
    sb.mkdir("b.txt");
    let plan = plan_over(&sb.engine(), &sb, ConflictPolicy::Overwrite);
    assert!(plan.blocking().next().is_some() && !plan.is_executable());
}

#[test]
fn a_folder_replaces_an_empty_folder_only() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.mkdir("a.txt");
    sb.write("b.txt/x", "x");
    assert!(!plan_over(&e, &sb, ConflictPolicy::Overwrite).is_executable());
    std::fs::remove_file(sb.path("b.txt/x")).unwrap();
    assert!(plan_over(&e, &sb, ConflictPolicy::Overwrite).is_executable());
}
