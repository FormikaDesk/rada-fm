//! The digest of a copied file is for undo to check the last name of a file with several
//! names, and undo finds that file by its device and number. Where the system gives neither
//! (Windows), nothing is hashed or written.

use std::path::PathBuf;

use rada_core::ops::*;
use rada_core::testutil::*;

fn digests(sb: &Sandbox) -> usize {
    std::fs::read_to_string(sb.dirs.journal_path())
        .unwrap()
        .lines()
        .filter(|l| l.contains(r#""t":"digest""#))
        .count()
}

fn copy_three(sb: &Sandbox, ffs: std::sync::Arc<FaultFs>) {
    let srcs: Vec<PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|n| sb.write(format!("src/{n}"), n.as_bytes()))
        .collect();
    sb.mkdir("dest");
    let e = sb.engine_with(ffs);
    let plan = e.plan_transfer(
        &scan(&e, &srcs),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &sb.journal(), &plan);
    assert_eq!(out.report.status(), RunStatus::Completed);
}

#[test]
fn a_file_without_a_device_and_a_number_gets_no_digest() {
    let sb = Sandbox::new();
    let ffs = FaultFs::new();
    ffs.hide_identity();
    copy_three(&sb, ffs);
    assert_eq!(digests(&sb), 0);
    assert!(sb.path("dest/c").exists());
}
