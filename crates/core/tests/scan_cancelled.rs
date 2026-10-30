//! A scan that was cancelled saw only part of what was selected: no plan is made from it.

use std::path::PathBuf;

use rada_core::archive::ArchiveKind;
use rada_core::ops::*;
use rada_core::testutil::*;

fn cancelled_scan(e: &Engine, paths: &[PathBuf]) -> Scan {
    let cancel = Cancel::new();
    cancel.cancel();
    e.scan(
        paths,
        ScanControl {
            cancel: cancel.flag(),
            progress: &mut |_| {},
        },
    )
}

fn refused(plan: &Plan) {
    assert!(!plan.is_executable(), "{plan:?}");
    assert!(plan.steps.is_empty(), "{:?}", plan.steps);
    assert!(
        plan.blocking()
            .any(|w| w.kind == WarningKind::ScanCancelled),
        "{:?}",
        plan.warnings
    );
}

fn tree(sb: &Sandbox) -> (Engine, Vec<PathBuf>) {
    sb.write("top/a", "a");
    sb.write("top/sub/b", "b");
    sb.mkdir("dest");
    (sb.engine(), vec![sb.path("top")])
}

#[test]
fn a_scan_says_whether_it_was_cancelled() {
    let sb = Sandbox::new();
    let (e, srcs) = tree(&sb);
    assert!(cancelled_scan(&e, &srcs).cancelled);
    assert!(!scan(&e, &srcs).cancelled);
}

#[test]
fn every_kind_of_plan_refuses_a_cancelled_scan() {
    let sb = Sandbox::new();
    let (e, srcs) = tree(&sb);
    let sc = cancelled_scan(&e, &srcs);
    let copy = TransferOptions::copy(ConflictPolicy::Skip);
    refused(&e.plan_transfer(&sc, &sb.path("dest"), &copy));
    refused(&e.plan_transfer(
        &sc,
        &sb.path("dest"),
        &TransferOptions::mv(ConflictPolicy::Skip),
    ));
    refused(&e.plan_trash(&sc));
    refused(&e.plan_delete(&sc));
    let zip = sb.path("dest/x.zip");
    refused(&e.plan_compress(&sc, &zip, ArchiveKind::Zip, ConflictPolicy::Skip));
}

#[test]
fn a_scan_that_ran_to_the_end_is_planned_as_before() {
    let sb = Sandbox::new();
    let (e, srcs) = tree(&sb);
    let plan = e.plan_transfer(
        &scan(&e, &srcs),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(plan.is_executable());
    assert!(
        plan.warnings
            .iter()
            .all(|w| w.kind != WarningKind::ScanCancelled)
    );
}
