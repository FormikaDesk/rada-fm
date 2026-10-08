//! Regression tests for the bugs found in superfile (B1, B2, B3, B10) plus the
//! behaviours the plan/execute engine promises.

use std::path::{Path, PathBuf};

use rada_core::ops::*;
use rada_core::testutil::*;

const UNIX_ONLY: &str = "needs POSIX symlinks/permissions; Windows support is in development";

// ------------------------------------------------------------------------------ B1

#[test]
fn folder_names_with_dots_do_not_misplace_the_destination() {
    let sb = Sandbox::new();
    // v1.2/sub/ : the bug cut the *path* at the last dot, so output went to "v1/".
    sb.write("v1.2/sub/data.txt", "x");
    sb.write("v1.2/sub/archive.tar.gz", "y");
    sb.mkdir(".hidden.dir/inner.v2");
    let e = sb.engine();

    let dest = sb.mkdir("out.d/with.dots");
    let (plan, rep) = do_copy(&e, &[sb.path("v1.2/sub")], &dest, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", plan.warnings);
    assert_eq!(snapshot(&sb.path("v1.2/sub")), snapshot(&dest.join("sub")));
    assert!(!sb.path("v1").exists() && !sb.path("out").exists());

    // Rename keeps the dotted parents and only touches the final component.
    let plan = e.plan_rename(&dest.join("sub/data.txt"), "renamed.v3.txt".as_ref());
    assert!(plan.is_executable());
    run(&e, &plan);
    assert!(dest.join("sub/renamed.v3.txt").exists());

    // Keep-both numbering works on names, not on paths.
    let again = e.plan_transfer(
        &scan(&e, &[sb.path("v1.2/sub/archive.tar.gz")]),
        &sb.path("v1.2/sub"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    run(&e, &again);
    assert!(sb.path("v1.2/sub/archive (1).tar.gz").exists());
}

// ------------------------------------------------------------------------------ B2

fn links_fixture(sb: &Sandbox) -> PathBuf {
    sb.write("box/real.txt", "data");
    sb.write("box/realdir/x.txt", "inside");
    sb.symlink("real.txt", "box/a_link_to_file");
    sb.symlink("realdir", "box/b_link_to_dir");
    sb.symlink("does-not-exist", "box/c_broken");
    sb.symlink("loop_b", "box/d_loop_a");
    sb.symlink("d_loop_a", "box/loop_b");
    sb.symlink("e_self", "box/e_self");
    sb.symlink(".", "box/f_dot");
    sb.symlink("..", "box/g_dotdot");
    sb.symlink("/etc/hostname", "box/h_absolute");
    sb.symlink("../../outside", "box/i_escaping");
    sb.path("box")
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "needs POSIX symlinks; Windows support is in development"
)]
fn symlinks_are_preserved_as_symlinks_including_broken_and_circular() {
    let sb = Sandbox::new();
    let src = links_fixture(&sb);
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let (plan, rep) = do_copy(&e, std::slice::from_ref(&src), &dest, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(plan.is_executable());

    let snap_src = snapshot(&src);
    let snap_dst = snapshot(&dest.join("box"));
    assert_eq!(
        snap_src, snap_dst,
        "tree and link targets must be identical"
    );

    for (name, target) in [
        ("a_link_to_file", "real.txt"),
        ("b_link_to_dir", "realdir"),
        ("c_broken", "does-not-exist"),
        ("d_loop_a", "loop_b"),
        ("loop_b", "d_loop_a"),
        ("e_self", "e_self"),
        ("f_dot", "."),
        ("g_dotdot", ".."),
    ] {
        let p = dest.join("box").join(name);
        let md = std::fs::symlink_metadata(&p).unwrap();
        assert!(md.file_type().is_symlink(), "{name} must stay a symlink");
        assert_eq!(std::fs::read_link(&p).unwrap(), Path::new(target), "{name}");
    }
    // The link to a file must NOT have become an executable regular file (superfile: 755).
    assert!(
        std::fs::symlink_metadata(dest.join("box/a_link_to_file"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // ...and the real data was copied as well (superfile aborted before it).
    assert_eq!(
        std::fs::read_to_string(dest.join("box/real.txt")).unwrap(),
        "data"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("box/realdir/x.txt")).unwrap(),
        "inside"
    );

    // The plan said so up front.
    let kinds: Vec<_> = plan.warnings.iter().map(|w| w.kind).collect();
    assert!(kinds.contains(&WarningKind::Symlink));
    assert!(kinds.contains(&WarningKind::BrokenSymlink));
    assert!(kinds.contains(&WarningKind::CircularLink));
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "needs POSIX symlinks; Windows support is in development"
)]
fn a_selected_symlink_to_a_folder_is_copied_as_a_link_not_expanded() {
    let sb = Sandbox::new();
    sb.write("realdir/x.txt", "1");
    let link = sb.symlink("realdir", "link_to_dir");
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let (_plan, rep) = do_copy(&e, &[link], &dest, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed);
    let md = std::fs::symlink_metadata(dest.join("link_to_dir")).unwrap();
    assert!(md.file_type().is_symlink());
    assert!(!dest.join("realdir").exists());
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "needs POSIX symlinks; Windows support is in development"
)]
fn copying_a_folder_into_itself_or_a_descendant_is_blocked() {
    let sb = Sandbox::new();
    sb.write("a/b/c.txt", "x");
    let e = sb.engine();
    for dest in [sb.path("a"), sb.path("a/b")] {
        let plan = e.plan_transfer(
            &scan(&e, &[sb.path("a")]),
            &dest,
            &TransferOptions::copy(ConflictPolicy::KeepBoth),
        );
        assert!(!plan.is_executable(), "dest {dest:?}");
        assert!(plan.blocking().any(|w| w.kind == WarningKind::InsideItself));
    }
    // Also through a symlinked destination.
    let via = sb.symlink("a/b", "viaLink");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("a")]),
        &via,
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    assert!(!plan.is_executable());
}

// ------------------------------------------------------------------------------ B3

#[test]
#[cfg_attr(
    not(unix),
    ignore = "needs POSIX permissions; Windows support is in development"
)]
fn an_unreadable_file_does_not_stop_the_rest_and_the_error_has_the_full_path() {
    if is_root() {
        eprintln!("skipped: running as root, chmod 000 does not block reads");
        return;
    }
    let sb = Sandbox::new();
    sb.write("mixed/a_ok.txt", "a");
    let bad = sb.write("mixed/b_nope.txt", "secret");
    sb.write("mixed/c_ok.txt", "c");
    sb.write("mixed/sub/d_ok.txt", "d");
    chmod(&bad, 0o000);
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("mixed")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    // Warned in advance.
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.kind == WarningKind::Unreadable)
    );

    let mut h = Scripted::new(ErrorChoice::Skip);
    let rep = e.execute(&plan, &mut h, &Cancel::new());
    chmod(&bad, 0o644);

    assert_eq!(rep.status(), RunStatus::CompletedWithProblems);
    assert_eq!(rep.failed.len(), 1);
    assert!(
        rep.failed[0].error.contains(&bad.display().to_string()),
        "full path in message: {}",
        rep.failed[0].error
    );
    assert!(
        rep.failed[0]
            .error
            .to_lowercase()
            .contains("permission denied")
    );
    for ok in ["mixed/a_ok.txt", "mixed/c_ok.txt", "mixed/sub/d_ok.txt"] {
        assert!(dest.join(ok).exists(), "{ok} must be copied");
    }
    assert!(!dest.join("mixed/b_nope.txt").exists());
    // No temp leftovers.
    assert!(
        snapshot(&dest)
            .keys()
            .all(|k| !k.to_string_lossy().contains("rada-part"))
    );
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "needs POSIX permissions; Windows support is in development"
)]
fn an_unreadable_folder_is_reported_and_siblings_still_copy() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.write("top/ok.txt", "ok");
    sb.write("top/locked/inner.txt", "no");
    sb.write("top/zz.txt", "zz");
    chmod(&sb.path("top/locked"), 0o000);
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("top")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let rep = run(&e, &plan);
    chmod(&sb.path("top/locked"), 0o755);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.kind == WarningKind::Unreadable && w.message.contains("cannot be read"))
    );
    assert!(dest.join("top/ok.txt").exists() && dest.join("top/zz.txt").exists());
    assert!(
        dest.join("top/locked").is_dir(),
        "the folder itself is recreated, empty"
    );
    let _ = rep;
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "needs POSIX permissions; Windows support is in development"
)]
fn permissions_and_times_are_preserved_and_readonly_folders_can_be_filled() {
    let sb = Sandbox::new();
    sb.write("ro/f.txt", "x");
    chmod(&sb.path("ro/f.txt"), 0o640);
    chmod(&sb.path("ro"), 0o555); // cannot write into it, yet it must copy
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let (_, rep) = do_copy(&e, &[sb.path("ro")], &dest, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(m(dest.join("ro")), 0o555);
        assert_eq!(m(dest.join("ro/f.txt")), 0o640);
    }
    let mt = |p: PathBuf| std::fs::metadata(p).unwrap().modified().unwrap();
    assert_eq!(mt(sb.path("ro/f.txt")), mt(dest.join("ro/f.txt")));
    chmod(&sb.path("ro"), 0o755);
    chmod(&dest.join("ro"), 0o755);
}

// ------------------------------------------------------------------------------ conflicts

#[test]
fn conflict_policies_skip_and_keep_both() {
    let sb = Sandbox::new();
    sb.write("src/same.txt", "NEW");
    sb.write("src/other.txt", "o");
    sb.write("dest/same.txt", "OLD");
    let e = sb.engine();
    let dest = sb.path("dest");

    let skip = e.plan_transfer(
        &scan(&e, &[sb.path("src/same.txt"), sb.path("src/other.txt")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(skip.has_conflicts());
    run(&e, &skip);
    assert_eq!(
        std::fs::read_to_string(dest.join("same.txt")).unwrap(),
        "OLD"
    );
    assert!(dest.join("other.txt").exists());

    let both = e.plan_transfer(
        &scan(&e, &[sb.path("src/same.txt")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::KeepBoth),
    );
    run(&e, &both);
    assert_eq!(
        std::fs::read_to_string(dest.join("same (1).txt")).unwrap(),
        "NEW"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("same.txt")).unwrap(),
        "OLD"
    );
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "overwriting moves the old file to the system Trash, implemented for Linux only; Windows and macOS are in development"
)]
fn the_overwrite_policy_replaces_and_warns() {
    let sb = Sandbox::new();
    sb.write("src/same.txt", "NEW");
    sb.write("dest/same.txt", "OLD");
    let e = sb.engine();
    let dest = sb.path("dest");
    let over = e.plan_transfer(
        &scan(&e, &[sb.path("src/same.txt")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Overwrite),
    );
    assert!(
        over.warnings
            .iter()
            .any(|w| w.kind == WarningKind::Overwrite)
    );
    let rep = run(&e, &over);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(
        std::fs::read_to_string(dest.join("same.txt")).unwrap(),
        "NEW"
    );
}

#[test]
fn pasting_into_the_same_folder_makes_a_numbered_copy() {
    let sb = Sandbox::new();
    sb.write("d/report.final.pdf", "pdf");
    let e = sb.engine();
    let (plan, rep) = do_copy(
        &e,
        &[sb.path("d/report.final.pdf")],
        &sb.path("d"),
        ConflictPolicy::Skip,
    );
    assert_eq!(rep.status(), RunStatus::Completed);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.kind == WarningKind::SameLocation)
    );
    assert!(sb.path("d/report.final (1).pdf").exists());
}

#[test]
fn merging_into_an_existing_folder_keeps_unrelated_files() {
    let sb = Sandbox::new();
    sb.write("src/box/new.txt", "n");
    sb.write("src/box/both.txt", "src-version");
    sb.write("dest/box/keep.txt", "k");
    sb.write("dest/box/both.txt", "dest-version");
    let e = sb.engine();
    let (plan, rep) = do_copy(
        &e,
        &[sb.path("src/box")],
        &sb.path("dest"),
        ConflictPolicy::Skip,
    );
    assert!(plan.warnings.iter().any(|w| w.kind == WarningKind::Merge));
    assert_eq!(rep.status(), RunStatus::Completed);
    assert_eq!(
        std::fs::read_to_string(sb.path("dest/box/keep.txt")).unwrap(),
        "k"
    );
    assert_eq!(
        std::fs::read_to_string(sb.path("dest/box/new.txt")).unwrap(),
        "n"
    );
    assert_eq!(
        std::fs::read_to_string(sb.path("dest/box/both.txt")).unwrap(),
        "dest-version"
    );
}

// ------------------------------------------------------------------------------ names

fn nasty_names() -> Vec<std::ffi::OsString> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut v: Vec<std::ffi::OsString> = [
        "plain.txt",
        "with space.txt",
        " leading and trailing ",
        "emoji 🎉🚀.txt",
        "日本語のファイル.txt",
        "العربية.txt",
        "e\u{301}.txt", // NFD é
        "\u{e9}.txt",   // NFC é
        "zwj 👨\u{200d}👩\u{200d}👧.txt",
        "-leading-dash",
        "--help",
        "$(echo pwned).txt",
        "semi;colon&and|pipe`tick`.txt",
        "quote'single\"double.txt",
        "back\\slash.txt",
        "percent%20name%.txt",
        "tab\there.txt",
        "new\nline.txt",
        "esc\u{1b}[31mred.txt",
        "bidi\u{202e}txt.exe",
        "*star?question[bracket].txt",
        "....",
        "..hidden..",
        "a.b.c.d.e",
    ]
    .iter()
    .map(Into::into)
    .collect();
    #[cfg(unix)]
    {
        v.push(os_from_bytes(b"not-utf8-\xff\xfe-name.bin"));
        v.push(os_from_bytes(b"\x80\x81\x82"));
        v.push(os_from_bytes("long-".repeat(48).as_bytes())); // 240 bytes
    }
    v
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "macOS and Windows normalise Unicode / reject invalid names; their name rules are in development"
)]
fn awkward_file_names_copy_move_and_round_trip_exactly() {
    let sb = Sandbox::new();
    let names = nasty_names();
    let e = sb.engine();
    let mut srcs = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let p = sb.path("src").join(n);
        std::fs::create_dir_all(sb.path("src")).unwrap();
        if i % 3 == 0 {
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("inner"), format!("{i}")).unwrap();
        } else {
            std::fs::write(&p, format!("{i}")).unwrap();
        }
        srcs.push(p);
    }
    let dest = sb.mkdir("dest");
    let (plan, rep) = do_copy(&e, &srcs, &dest, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(plan.is_executable());
    assert_eq!(
        snapshot(&sb.path("src")),
        snapshot(&dest),
        "every name must survive byte-for-byte"
    );

    // Move them on and trash them again.
    let moved = sb.mkdir("moved");
    let srcs2: Vec<PathBuf> = names.iter().map(|n| dest.join(n)).collect();
    let (_, rep) = do_move(&e, &srcs2, &moved, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(snapshot(&sb.path("src")), snapshot(&moved));
    assert!(snapshot(&dest).is_empty());
}

#[test]
fn rename_rejects_invalid_names_without_touching_anything() {
    let sb = Sandbox::new();
    let f = sb.write("f.txt", "x");
    sb.write("exists.txt", "y");
    let e = sb.engine();
    for bad in ["", ".", "..", "a/b", "exists.txt", "f.txt"] {
        let plan = e.plan_rename(&f, bad.as_ref());
        assert!(!plan.is_executable(), "{bad:?} must be refused");
    }
    let too_long = "x".repeat(300);
    assert!(!e.plan_rename(&f, too_long.as_ref()).is_executable());
    assert!(f.exists());
}

// ------------------------------------------------------------------------------ progress & errors

#[test]
fn progress_is_reported_in_bytes_with_intermediate_values() {
    let sb = Sandbox::new();
    sb.write("big.bin", vec![1u8; 40 << 20]);
    sb.write("small.txt", "x");
    let e = sb.engine();
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("big.bin"), sb.path("small.txt")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert_eq!(plan.total_bytes(), (40 << 20) + 1);
    let mut h = Scripted::new(ErrorChoice::Skip);
    e.execute(&plan, &mut h, &Cancel::new());
    let last = h.progress.last().unwrap();
    assert_eq!(last.bytes_done, last.bytes_total);
    assert_eq!(last.bytes_total, (40 << 20) + 1);
    let mid = h
        .progress
        .iter()
        .filter(|p| p.bytes_done > 0 && p.bytes_done < p.bytes_total)
        .count();
    assert!(
        mid > 0,
        "bar must move before the end: {} events",
        h.progress.len()
    );
    assert!(
        h.progress
            .windows(2)
            .all(|w| w[0].bytes_done <= w[1].bytes_done),
        "monotonic"
    );
}

#[test]
fn retry_then_success_and_skip_all_and_abort() {
    let sb = Sandbox::new();
    for n in ["a", "b", "c", "d"] {
        sb.write(format!("src/{n}.txt"), n);
    }
    let fault = FaultFs::new();
    let e = sb.engine_with(fault.clone());
    let dest = sb.mkdir("dest");
    let srcs: Vec<PathBuf> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| sb.path(format!("src/{n}.txt")))
        .collect();
    let plan = e.plan_transfer(
        &scan(&e, &srcs),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );

    // Retry: b.txt fails twice (EIO), the third attempt works.
    fault.fail_times(Op::CopyFile, "src/b.txt", 5, 2);
    let mut h = Scripted::new(ErrorChoice::Retry);
    let rep = e.execute(&plan, &mut h, &Cancel::new());
    assert_eq!(rep.status(), RunStatus::Completed);
    assert_eq!(h.failures.len(), 2);
    assert!(h.failures[0].contains("b.txt"));
    assert!(dest.join("b.txt").exists());

    // SkipAll: everything that fails is skipped, handler asked only once.
    std::fs::remove_dir_all(&dest).unwrap();
    std::fs::create_dir(&dest).unwrap();
    fault.clear();
    fault.fail(Op::CopyFile, "src/a.txt", 5);
    fault.fail(Op::CopyFile, "src/c.txt", 5);
    let mut h = Scripted::new(ErrorChoice::SkipAll);
    let rep = e.execute(&plan, &mut h, &Cancel::new());
    assert_eq!(rep.failed.len(), 2);
    assert_eq!(h.failures.len(), 1, "SkipAll must stop the questions");
    assert!(dest.join("b.txt").exists() && dest.join("d.txt").exists());
    assert!(!dest.join("a.txt").exists() && !dest.join("c.txt").exists());

    // Abort: nothing after the failing step is touched.
    std::fs::remove_dir_all(&dest).unwrap();
    std::fs::create_dir(&dest).unwrap();
    fault.clear();
    fault.fail(Op::CopyFile, "src/b.txt", 5);
    let mut h = Scripted::new(ErrorChoice::Abort);
    let rep = e.execute(&plan, &mut h, &Cancel::new());
    assert_eq!(rep.status(), RunStatus::Aborted);
    assert!(dest.join("a.txt").exists());
    assert!(!dest.join("c.txt").exists() && !dest.join("d.txt").exists());
}

#[test]
fn a_failed_folder_skips_its_children_without_asking_again() {
    let sb = Sandbox::new();
    sb.write("src/dir/one.txt", "1");
    sb.write("src/dir/two.txt", "2");
    sb.write("src/after.txt", "3");
    let fault = FaultFs::new();
    let e = sb.engine_with(fault.clone());
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    fault.fail(Op::CreateDir, "dest/src/dir", 28); // ENOSPC
    let mut h = Scripted::new(ErrorChoice::Skip);
    let rep = e.execute(&plan, &mut h, &Cancel::new());
    assert_eq!(
        h.failures.len(),
        1,
        "children must not generate their own prompts"
    );
    assert!(rep.skipped_dependent >= 2);
    assert!(dest.join("src/after.txt").exists());
}

#[test]
fn cancelling_stops_cleanly_and_leaves_no_partial_files() {
    let sb = Sandbox::new();
    sb.write("big/a.bin", vec![1u8; 64 << 20]);
    sb.write("big/b.bin", vec![2u8; 64 << 20]);
    let e = sb.engine();
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("big")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let cancel = Cancel::new();
    struct CancelAfter(Cancel, u64);
    impl ExecHandler for CancelAfter {
        fn progress(&mut self, p: &Progress) {
            if p.bytes_done > self.1 {
                self.0.cancel();
            }
        }
    }
    let mut h = CancelAfter(cancel.clone(), 8 << 20);
    let rep = e.execute(&plan, &mut h, &cancel);
    assert_eq!(rep.status(), RunStatus::Cancelled);
    let names: Vec<_> = snapshot(&dest)
        .keys()
        .map(|k| k.to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.contains("rada-part")), "{names:?}");
    assert!(!names.contains(&"big/b.bin".to_string()));
}

#[test]
fn a_destination_that_appears_after_planning_is_never_overwritten() {
    let sb = Sandbox::new();
    sb.write("src/f.txt", "NEW");
    let e = sb.engine();
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src/f.txt")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    std::fs::write(dest.join("f.txt"), "APPEARED").unwrap(); // race: created after the plan
    let rep = run(&e, &plan);
    assert_eq!(rep.failed.len(), 1);
    assert!(rep.failed[0].error.contains("already exists"));
    assert_eq!(
        std::fs::read_to_string(dest.join("f.txt")).unwrap(),
        "APPEARED"
    );
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "uses POSIX permissions or file names that Windows rejects; Windows support is in development"
)]
fn error_messages_use_display_escapes_for_hostile_names() {
    let sb = Sandbox::new();
    let weird = sb.write("a\nb\u{1b}[31m.txt", "x");
    let e = sb.engine();
    let plan = e.plan_rename(&weird, "ok.txt".as_ref());
    assert!(
        plan.title.contains("\\n") && plan.title.contains("\\e"),
        "{}",
        plan.title
    );
    assert!(!plan.title.contains('\n') && !plan.title.contains('\u{1b}'));
    let _ = UNIX_ONLY;
}
