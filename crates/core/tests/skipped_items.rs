//! How many of the selected items a plan leaves alone: what the card after an operation says
//! it did not do.

use rada_core::archive::Compression;
use rada_core::archive::testkit::{Member, write_tar};
use rada_core::ops::*;
use rada_core::testutil::*;

/// Three files to move or copy, one of which already has its name in the destination.
fn clash() -> (Sandbox, Vec<std::path::PathBuf>, std::path::PathBuf) {
    let sb = Sandbox::new();
    let srcs: Vec<_> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|n| sb.write(format!("src/{n}"), *n))
        .collect();
    let dest = sb.mkdir("dest");
    sb.write("dest/b.txt", "old");
    (sb, srcs, dest)
}

fn plan(mode: TransferMode, policy: ConflictPolicy) -> (Plan, ExecReport) {
    let (sb, srcs, dest) = clash();
    let e = sb.engine();
    let opts = match mode {
        TransferMode::Copy => TransferOptions::copy(policy),
        TransferMode::Move => TransferOptions::mv(policy),
    };
    let plan = e.plan_transfer(&scan(&e, &srcs), &dest, &opts);
    let rep = run(&e, &plan);
    (plan, rep)
}

#[test]
fn skip_leaves_the_clashing_item_alone_and_says_so() {
    for mode in [TransferMode::Copy, TransferMode::Move] {
        let (plan, rep) = plan(mode, ConflictPolicy::Skip);
        assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
        assert_eq!(plan.totals.items, 3, "three were selected");
        assert_eq!(plan.skipped_items(), 1, "{mode:?}");
    }
}

#[test]
fn keep_both_and_overwrite_leave_nothing_alone() {
    for mode in [TransferMode::Copy, TransferMode::Move] {
        for policy in [ConflictPolicy::KeepBoth, ConflictPolicy::Overwrite] {
            let (plan, rep) = plan(mode, policy);
            assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
            assert_eq!(plan.skipped_items(), 0, "{mode:?} {policy:?}");
        }
    }
}

#[test]
fn an_operation_without_conflicts_skips_nothing() {
    let sb = Sandbox::new();
    let a = sb.write("src/a.txt", "a");
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let plan = e.plan_transfer(
        &scan(&e, &[a]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert_eq!(plan.skipped_items(), 0);
    let trash = e.plan_trash(&scan(&e, &[sb.path("src/a.txt")]));
    assert_eq!(trash.skipped_items(), 0);
}

/// Taking two members out of an archive when one of them is already where it would go.
#[test]
fn an_extraction_counts_the_member_it_left_alone() {
    for (policy, left_alone) in [
        (ConflictPolicy::Skip, 1),
        (ConflictPolicy::KeepBoth, 0),
        (ConflictPolicy::Overwrite, 0),
    ] {
        let sb = Sandbox::new();
        let e = sb.engine();
        let out = sb.mkdir("out");
        sb.write("out/top.txt", "old");
        let a = sb.path("a.tar.xz");
        let members = [
            Member::file("top.txt", "top"),
            Member::file("other.txt", "other"),
        ];
        write_tar(&a, &members, Compression::Xz);
        let req = OpRequest::Copy {
            sources: vec![a.join("top.txt"), a.join("other.txt")],
            destination: out,
            conflict: policy,
            verify: false,
        };
        let cancel = Cancel::new();
        let plan = e
            .plan_request(
                &req,
                None,
                ScanControl {
                    cancel: cancel.flag(),
                    progress: &mut |_| {},
                },
            )
            .unwrap()
            .plan;
        assert_eq!(plan.kind, OpKind::Extract);
        assert_eq!(plan.totals.items, 2);
        assert_eq!(plan.skipped_items(), left_alone, "{policy:?}");
    }
}
