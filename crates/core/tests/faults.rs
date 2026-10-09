//! Things going wrong in the middle of an operation: the disk fills up halfway through a
//! file, a right is revoked, the source changes or vanishes while it is copied, the
//! destination disappears (a drive unplugged). None of it may leave a half-written file under
//! a real name, a temporary file behind, or a journal that cannot undo what did get done.
#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rada_core::ops::*;
use rada_core::testutil::*;

const MIB: usize = 1 << 20;

fn big(path: &Path, mib: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(path).unwrap();
    let block: Vec<u8> = (0..MIB).map(|i| (i % 251) as u8).collect();
    for _ in 0..mib {
        f.write_all(&block).unwrap();
    }
}

fn leftovers(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with(".rada-part-") {
                out.push(p.clone());
            }
            if p.is_dir() && !p.is_symlink() {
                walk(&p, out);
            }
        }
    }
    let mut v = Vec::new();
    walk(root, &mut v);
    v
}

fn copy_plan(e: &Engine, src: &Path, dest: &Path, verify: bool) -> Plan {
    let mut opts = TransferOptions::copy(ConflictPolicy::Skip);
    opts.verify = verify;
    e.plan_transfer(&scan(e, &[src.to_path_buf()]), dest, &opts)
}

#[test]
fn a_full_disk_halfway_through_a_file_leaves_no_partial_file_and_the_rest_is_undoable() {
    let sb = Sandbox::new();
    big(&sb.path("src/big.bin"), 24);
    sb.write("src/a.txt", "a");
    sb.write("src/z.txt", "z");
    let ffs = FaultFs::new();
    ffs.on_bytes("big.bin", 1, ByteAction::Fail(libc::ENOSPC));
    let e = sb.engine_with(ffs);
    let j = sb.journal();
    let dest = sb.mkdir("dest");
    let plan = copy_plan(&e, &sb.path("src"), &dest, false);
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(out.report.failed.len(), 1, "{:?}", out.report.failed);
    assert!(
        out.report.failed[0]
            .error
            .to_lowercase()
            .contains("no space"),
        "the real reason is shown: {}",
        out.report.failed[0].error
    );
    assert!(
        !dest.join("src/big.bin").exists(),
        "no half file under the real name"
    );
    assert!(leftovers(&dest).is_empty(), "{:?}", leftovers(&dest));
    assert!(
        dest.join("src/a.txt").exists() && dest.join("src/z.txt").exists(),
        "the others went through"
    );
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(
        up.blocked.is_empty() && rep.failed.is_empty(),
        "{:?} {:?}",
        up.blocked,
        rep.failed
    );
    assert!(
        snapshot(&dest).is_empty(),
        "undo leaves the destination as it was"
    );
    assert!(sb.path("src/big.bin").exists());
}

#[test]
fn a_right_revoked_during_the_operation_fails_that_file_and_nothing_else() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    let secret = sb.write("src/secret.txt", "hidden");
    sb.write("src/a.txt", "a");
    sb.write("src/z.txt", "z");
    let ffs = FaultFs::new();
    let s = secret.clone();
    // Just before that file is copied, someone takes the read right away.
    ffs.hook(Op::CopyFile, "secret.txt", 1, move || chmod(&s, 0o000));
    let e = sb.engine_with(ffs);
    let dest = sb.mkdir("dest");
    let plan = copy_plan(&e, &sb.path("src"), &dest, false);
    let rep = run(&e, &plan);
    chmod(&secret, 0o644);
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert!(
        rep.failed[0].error.contains("secret.txt"),
        "the path is named: {}",
        rep.failed[0].error
    );
    assert!(!dest.join("src/secret.txt").exists());
    assert!(dest.join("src/a.txt").exists() && dest.join("src/z.txt").exists());
    assert!(leftovers(&dest).is_empty());
}

#[test]
fn a_source_that_changes_while_it_is_copied_is_caught_when_verifying() {
    let sb = Sandbox::new();
    let src = sb.path("src/log.bin");
    big(&src, 8);
    let ffs = FaultFs::new();
    let s = src.clone();
    ffs.on_bytes(
        "log.bin",
        1,
        ByteAction::Run(Arc::new(move || {
            let mut f = std::fs::OpenOptions::new().append(true).open(&s).unwrap();
            f.write_all(b"appended while copying").unwrap();
        })),
    );
    let e = sb.engine_with(ffs);
    let dest = sb.mkdir("dest");
    let plan = copy_plan(&e, &sb.path("src"), &dest, true);
    let rep = run(&e, &plan);
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert!(
        rep.failed[0].error.contains("changed"),
        "it says the source changed: {}",
        rep.failed[0].error
    );
    assert!(
        !dest.join("src/log.bin").exists(),
        "a copy of a moving target is not kept"
    );
    assert!(leftovers(&dest).is_empty());
}

#[test]
fn a_source_changing_without_verification_still_ends_cleanly() {
    let sb = Sandbox::new();
    let src = sb.path("src/log.bin");
    big(&src, 8);
    let ffs = FaultFs::new();
    let s = src.clone();
    ffs.on_bytes(
        "log.bin",
        1,
        ByteAction::Run(Arc::new(move || {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&s)
                .unwrap()
                .set_len(3 * MIB as u64)
                .unwrap();
        })),
    );
    let e = sb.engine_with(ffs);
    let dest = sb.mkdir("dest");
    let plan = copy_plan(&e, &sb.path("src"), &dest, false);
    let rep = run(&e, &plan);
    // No claim about what was copied from a file that shrank under it, only that nothing is
    // half-made or left behind.
    assert!(leftovers(&dest).is_empty(), "{:?}", leftovers(&dest));
    if rep.failed.is_empty() {
        assert!(dest.join("src/log.bin").exists());
    } else {
        assert!(!dest.join("src/log.bin").exists());
    }
}

#[test]
fn a_source_deleted_while_it_is_copied_does_not_lose_the_copy_of_a_move() {
    let sb = Sandbox::new();
    let Some(other) = other_filesystem_dir(&sb) else {
        return;
    };
    let src = sb.path("src/gone.bin");
    big(&src, 8);
    let ffs = FaultFs::new();
    let s = src.clone();
    ffs.on_bytes(
        "gone.bin",
        1,
        ByteAction::Run(Arc::new(move || std::fs::remove_file(&s).unwrap())),
    );
    let e = sb.engine_with(ffs);
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        other.path(),
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    let rep = run(&e, &plan);
    assert!(
        rep.failed.is_empty(),
        "the move wanted it gone: {:?}",
        rep.failed
    );
    let moved = other.path().join("src/gone.bin");
    assert_eq!(
        std::fs::metadata(&moved).unwrap().len(),
        8 * MIB as u64,
        "the copy is whole and kept"
    );
    assert!(leftovers(other.path()).is_empty());
}

#[test]
fn a_destination_that_vanishes_mid_way_fails_the_rest_without_panics_and_undo_still_runs() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "a");
    sb.write("src/b.txt", "b");
    sb.write("src/c.txt", "c");
    let ffs = FaultFs::new();
    let dest = sb.mkdir("dest");
    let away = sb.root.join("unplugged");
    let (d, a) = (dest.clone(), away.clone());
    // As the second file is about to be copied, the drive is pulled out.
    ffs.hook(Op::CopyFile, "b.txt", 1, move || {
        std::fs::rename(&d, &a).unwrap()
    });
    let e = sb.engine_with(ffs);
    let j = sb.journal();
    let plan = copy_plan(&e, &sb.path("src"), &dest, false);
    let out = run_journaled(&e, &j, &plan);
    assert!(
        !out.report.failed.is_empty(),
        "the steps after the loss fail, and say so"
    );
    assert!(!dest.exists(), "nothing is recreated where the drive was");
    assert!(
        leftovers(&sb.root).is_empty() || leftovers(&sb.root).iter().all(|p| p.starts_with(&away))
    );
    // The journal still undoes what it believes it did: what is gone is simply gone.
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(rep.failed.is_empty(), "{:?} {:?}", up.blocked, rep.failed);
    assert_eq!(
        snapshot(&sb.path("src")).len(),
        3,
        "the sources are untouched"
    );
}

#[test]
fn a_copy_of_a_read_only_file_into_a_read_only_destination_is_refused_in_the_plan() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.write("src/a.txt", "a");
    let dest = sb.mkdir("dest");
    chmod(&dest, 0o555);
    let e = sb.engine();
    let plan = copy_plan(&e, &sb.path("src"), &dest, false);
    chmod(&dest, 0o755);
    assert!(plan.blocking().next().is_some(), "{:?}", plan.warnings);
}

#[test]
fn a_file_created_in_the_destination_meanwhile_is_never_overwritten() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "from the source");
    let ffs = FaultFs::new();
    let dest = sb.mkdir("dest");
    let d = dest.clone();
    // After planning, before the copy: someone else makes a file of the same name.
    ffs.hook(Op::CopyFile, "a.txt", 1, move || {
        let _ = std::fs::create_dir_all(d.join("src"));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o644)
            .open(d.join("src/a.txt"))
            .unwrap();
        f.write_all(b"someone else's").unwrap();
    });
    let e = sb.engine_with(ffs);
    let plan = copy_plan(&e, &sb.path("src"), &dest, false);
    let rep = run(&e, &plan);
    assert_eq!(
        std::fs::read(dest.join("src/a.txt")).unwrap(),
        b"someone else's"
    );
    assert!(!rep.failed.is_empty(), "the clash is reported");
    assert!(leftovers(&dest).is_empty());
}
