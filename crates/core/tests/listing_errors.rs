//! A folder that the system lists only in part, or entries it will not describe, must not make
//! a scan or a plan look complete: the plan says so, once for each thing.
#![cfg(unix)]

use std::path::PathBuf;

use rada_core::ops::*;
use rada_core::testutil::*;

const EIO: i32 = libc::EIO;
const EACCES: i32 = libc::EACCES;

fn tree(sb: &Sandbox) -> PathBuf {
    for n in ["a", "b", "c", "d"] {
        sb.write(format!("top/{n}"), n);
    }
    sb.write("top/sub/x", "x");
    sb.mkdir("dest");
    sb.path("top")
}

fn find(plan: &Plan, kind: WarningKind) -> Option<&Warning> {
    plan.warnings.iter().find(|w| w.kind == kind)
}

#[test]
fn a_listing_that_breaks_in_the_middle_is_marked_on_the_folder() {
    let sb = Sandbox::new();
    let top = tree(&sb);
    let ffs = FaultFs::new();
    ffs.break_listing("top", EIO);
    let e = sb.engine_with(ffs);

    let sc = scan(&e, std::slice::from_ref(&top));
    let node = &sc.roots[0];
    assert_eq!(node.incomplete.len(), 1, "{:?}", node.incomplete);
    assert!(
        node.children.iter().all(|c| c.name != "sub"),
        "the last entry is missing"
    );
    assert!(node.unreadable.is_empty());

    let plan = e.plan_transfer(
        &sc,
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let w = find(&plan, WarningKind::IncompleteList).expect("a warning in the plan");
    assert_eq!(w.count, 1);
    assert_eq!(w.examples, [top]);
    assert!(w.message.contains("may be missing items"), "{}", w.message);
}

#[test]
fn a_folder_inside_a_copy_or_a_delete_is_marked_too() {
    let sb = Sandbox::new();
    let top = tree(&sb);
    let ffs = FaultFs::new();
    ffs.break_listing("top/sub", EIO);
    let e = sb.engine_with(ffs);
    let sc = scan(&e, &[top]);
    let copy = e.plan_transfer(
        &sc,
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(find(&copy, WarningKind::IncompleteList).is_some());
    assert!(find(&e.plan_delete(&sc), WarningKind::IncompleteList).is_some());
}

#[test]
fn several_unreadable_entries_in_one_folder_are_all_kept() {
    let sb = Sandbox::new();
    let top = tree(&sb);
    let ffs = FaultFs::new();
    ffs.fail_entry("top/a", EACCES);
    ffs.fail_entry("top/c", EIO);
    let e = sb.engine_with(ffs);

    let sc = scan(&e, std::slice::from_ref(&top));
    let got: Vec<_> = sc.roots[0]
        .unreadable
        .iter()
        .map(|(p, why)| (p.strip_prefix(&top).unwrap().to_owned(), why.clone()))
        .collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].0, PathBuf::from("a"));
    assert!(
        got[0].1.to_lowercase().contains("permission denied"),
        "{got:?}"
    );
    assert_eq!(got[1].0, PathBuf::from("c"));

    let plan = e.plan_transfer(
        &sc,
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let w = find(&plan, WarningKind::Unreadable).unwrap();
    assert_eq!(w.count, 2);
    assert_eq!(w.examples, [top.join("a"), top.join("c")]);
}

#[test]
fn a_folder_that_cannot_be_opened_is_kept_with_its_reason() {
    let sb = Sandbox::new();
    let top = tree(&sb);
    let ffs = FaultFs::new();
    ffs.fail(Op::ReadDir, "top/sub", EACCES);
    let e = sb.engine_with(ffs);
    let sc = scan(&e, std::slice::from_ref(&top));
    let sub = sc.roots[0]
        .children
        .iter()
        .find(|c| c.name == "sub")
        .unwrap();
    assert_eq!(sub.unreadable.len(), 1);
    assert_eq!(sub.unreadable[0].0, top.join("sub"));
    assert!(sub.incomplete.is_empty());
}

#[test]
fn a_complete_listing_has_neither() {
    let sb = Sandbox::new();
    let top = tree(&sb);
    let e = sb.engine();
    let plan = e.plan_transfer(
        &scan(&e, &[top]),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(find(&plan, WarningKind::IncompleteList).is_none());
    assert!(find(&plan, WarningKind::Unreadable).is_none());
}

#[test]
fn a_move_that_cannot_list_a_folder_whole_does_not_remove_its_source() {
    use rada_core::fs::{FsEngine, tree::copy_tree};
    let sb = Sandbox::new();
    let top = tree(&sb);
    let ffs = FaultFs::new();
    ffs.break_listing("top", EIO);
    let err = copy_tree(&*ffs as &dyn FsEngine, &top, &sb.path("dest/top"), false).unwrap_err();
    assert!(err.to_string().contains("top"), "{err}");
}
