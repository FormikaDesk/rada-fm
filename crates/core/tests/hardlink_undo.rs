//! The fingerprint of a file with several names has no status-change time (making or removing
//! a link changes it for every name), so a change that keeps the size and the modification time
//! is not seen by it. When an undo would remove the last name of such a file, the contents are
//! checked against a hash taken when the file was made.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rada_core::ops::*;
use rada_core::testutil::*;

/// Copy `a/x` and its second name `a/y` to `dest`, through the journal.
fn copied_group(sb: &Sandbox, e: &Engine, j: &rada_core::journal::Journal) -> (String, PathBuf) {
    let x = sb.write("a/x", "0123456789");
    std::fs::hard_link(&x, sb.path("a/y")).unwrap();
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(e, &[sb.path("a")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(e, j, &plan);
    assert!(out.report.failed.is_empty(), "{:?}", out.report);
    (out.id, dest.join("a"))
}

/// Other contents, same length, same modification time: what `touch -r` or `rsync -t` leaves.
fn rewrite_keeping_times(p: &Path, with: &str) {
    let mtime: SystemTime = std::fs::metadata(p).unwrap().modified().unwrap();
    let mut f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    std::io::Write::write_all(&mut f, with.as_bytes()).unwrap();
    f.set_modified(mtime).unwrap();
}

#[test]
fn the_last_name_of_a_file_changed_without_touching_its_times_is_not_removed() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let (id, copy) = copied_group(&sb, &e, &j);
    rewrite_keeping_times(&copy.join("x"), "9876543210");

    let (up, _) = undo(&e, &j, &id);
    assert_eq!(up.blocked.len(), 1, "{:?}", up.blocked);
    assert!(
        up.blocked[0].reason.contains("modified"),
        "{:?}",
        up.blocked
    );
    let left: Vec<_> = ["x", "y"]
        .iter()
        .filter(|n| copy.join(n).exists())
        .collect();
    assert_eq!(left.len(), 1, "one name goes, the last one stays: {left:?}");
    assert_eq!(
        std::fs::read_to_string(copy.join(left[0])).unwrap(),
        "9876543210",
        "the changed contents are still there"
    );
}

#[test]
fn a_group_nobody_touched_is_removed_whole() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let (id, copy) = copied_group(&sb, &e, &j);
    let (up, rep) = undo(&e, &j, &id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert!(rep.failed.is_empty());
    assert!(!copy.join("x").exists() && !copy.join("y").exists());
}

#[test]
fn a_changed_file_that_has_another_name_elsewhere_loses_only_its_names_here() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let (id, copy) = copied_group(&sb, &e, &j);
    let elsewhere = sb.path("kept-name");
    std::fs::hard_link(copy.join("x"), &elsewhere).unwrap();
    rewrite_keeping_times(&copy.join("x"), "9876543210");

    let (up, _) = undo(&e, &j, &id);
    assert!(
        up.blocked.is_empty(),
        "the contents survive: {:?}",
        up.blocked
    );
    assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "9876543210");
}

#[test]
fn the_last_name_of_a_file_that_cannot_be_read_is_not_removed() {
    use std::os::unix::fs::PermissionsExt;
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let (id, copy) = copied_group(&sb, &e, &j);
    std::fs::set_permissions(copy.join("x"), std::fs::Permissions::from_mode(0o000)).unwrap();

    let (up, _) = undo(&e, &j, &id);
    assert_eq!(up.blocked.len(), 1, "{:?}", up.blocked);
    assert!(
        up.blocked[0].reason.contains("whether") && up.blocked[0].reason.contains("modified"),
        "{:?}",
        up.blocked
    );
    let left = ["x", "y"].iter().filter(|n| copy.join(n).exists()).count();
    assert_eq!(left, 1, "one name goes, the last one stays");
}
