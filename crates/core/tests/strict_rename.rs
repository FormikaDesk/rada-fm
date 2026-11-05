//! The rename that never looks first and renames after, through the filesystem the engine
//! uses, and the way the test filesystem can make it fail.

#![cfg(unix)]

use std::io;

use rada_core::fs::FsEngine;
use rada_core::testutil::*;

#[test]
fn a_free_name_is_taken_and_a_taken_one_is_never_replaced() {
    let sb = Sandbox::new();
    let ffs = FaultFs::new();
    let from = sb.write("a.txt", "new");
    let to = sb.path("b.txt");

    ffs.rename_noreplace_strict(&from, &to).unwrap();
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "new");

    let again = sb.write("c.txt", "other");
    let e = ffs.rename_noreplace_strict(&again, &to).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "new");
    assert!(again.exists());
}

#[test]
fn a_fault_on_the_strict_rename_reaches_it_and_nothing_else() {
    let sb = Sandbox::new();
    let ffs = FaultFs::new();
    ffs.fail(Op::RenameStrict, "a.txt", libc::ENOSYS);
    let a = sb.write("a.txt", "a");

    let e = ffs
        .rename_noreplace_strict(&a, &sb.path("b.txt"))
        .unwrap_err();

    assert_eq!(e.kind(), io::ErrorKind::Unsupported);
    assert!(a.exists() && !sb.path("b.txt").exists());
    ffs.rename_noreplace(&a, &sb.path("c.txt")).unwrap();
    assert!(sb.path("c.txt").exists());
}

#[test]
fn a_fault_on_every_rename_reaches_the_strict_one_too() {
    let sb = Sandbox::new();
    let ffs = FaultFs::new();
    ffs.fail(Op::Rename, "a.txt", libc::EIO);
    let a = sb.write("a.txt", "a");

    assert!(ffs.rename_noreplace_strict(&a, &sb.path("b.txt")).is_err());
    assert!(a.exists());
}
