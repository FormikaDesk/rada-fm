//! Move, trash and delete: same filesystem, across filesystems (tmpfs <-> disk),
//! the freedesktop trash layout, and failure modes seen in other file managers.

use std::path::PathBuf;

use rada_core::ops::*;
use rada_core::testutil::*;

fn tree(sb: &Sandbox, root: &str) -> PathBuf {
    sb.write(format!("{root}/top.txt"), "top");
    sb.write(format!("{root}/sub.d/inner.bin"), vec![9u8; 200_000]);
    sb.write(format!("{root}/sub.d/deep/er/file with space.txt"), "deep");
    sb.mkdir(format!("{root}/empty"));
    #[cfg(unix)]
    {
        sb.symlink("top.txt", format!("{root}/link_file"));
        sb.symlink("sub.d", format!("{root}/link_dir"));
        sb.symlink("nowhere", format!("{root}/link_broken"));
        sb.symlink("link_loop2", format!("{root}/link_loop1"));
        sb.symlink("link_loop1", format!("{root}/link_loop2"));
    }
    sb.path(root)
}

// ---------------------------------------------------------------------------- move

#[test]
fn same_filesystem_move_is_a_rename_per_item_and_keeps_everything() {
    let sb = Sandbox::new();
    let src = tree(&sb, "src");
    let before = snapshot(&src);
    let dest = sb.mkdir("dest");
    let e = sb.engine();
    let (plan, rep) = do_move(&e, std::slice::from_ref(&src), &dest, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(plan.steps.len(), 1, "one atomic rename for the whole tree");
    assert!(!src.exists());
    assert_eq!(before, snapshot(&dest.join("src")));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "needs a tmpfs next to the disk (two filesystems), which only the Linux test machines have"
)]
fn moving_between_filesystems_copies_verifies_then_removes_the_source() {
    let sb = Sandbox::new();
    let Some(other) = other_filesystem_dir(&sb) else {
        eprintln!("skipped: no second filesystem available");
        return;
    };
    let e = sb.engine();

    // disk -> tmpfs
    let src = tree(&sb, "src");
    let before = snapshot(&src);
    let (plan, rep) = do_move(
        &e,
        std::slice::from_ref(&src),
        other.path(),
        ConflictPolicy::Skip,
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.kind == WarningKind::CrossDevice),
        "{:?}",
        plan.warnings
    );
    assert!(
        plan.steps.len() > 1,
        "expanded to copy + verify + remove steps"
    );
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(!src.exists(), "source must be gone");
    assert_eq!(before, snapshot(&other.path().join("src")));
    // Symlinks stayed symlinks across the boundary.
    assert!(
        std::fs::symlink_metadata(other.path().join("src/link_broken"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // tmpfs -> disk
    let back = sb.mkdir("back");
    let (_, rep) = do_move(&e, &[other.path().join("src")], &back, ConflictPolicy::Skip);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(before, snapshot(&back.join("src")));
    assert!(!other.path().join("src").exists());
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "needs a tmpfs next to the disk (two filesystems), which only the Linux test machines have"
)]
fn cross_filesystem_move_never_deletes_what_it_could_not_copy() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    let Some(other) = other_filesystem_dir(&sb) else {
        return;
    };
    let e = sb.engine();
    sb.write("src/good.txt", "good");
    let bad = sb.write("src/bad.txt", "bad");
    sb.write("src/sub/also_good.txt", "also");
    chmod(&bad, 0o000);
    let (_, rep) = do_move(&e, &[sb.path("src")], other.path(), ConflictPolicy::Skip);
    chmod(&bad, 0o644);
    assert_eq!(rep.failed.len(), 1);
    assert!(other.path().join("src/good.txt").exists());
    assert!(other.path().join("src/sub/also_good.txt").exists());
    // The unreadable file is still at the source, and so is its folder.
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), "bad");
    assert!(
        !sb.path("src/good.txt").exists(),
        "files that did move are gone from the source"
    );
    assert!(
        rep.kept.iter().any(|(p, _)| p.ends_with("src")),
        "{:?}",
        rep.kept
    );
}

#[test]
fn a_rename_that_hits_exdev_reports_cross_device_instead_of_losing_data() {
    let sb = Sandbox::new();
    sb.write("a.txt", "x");
    let fault = FaultFs::new();
    let e = sb.engine_with(fault.clone());
    // EXDEV; Windows says ERROR_NOT_SAME_DEVICE.
    fault.fail(Op::Rename, "a.txt", if cfg!(windows) { 17 } else { 18 });
    let dest = sb.mkdir("dest");
    let (_, rep) = do_move(&e, &[sb.path("a.txt")], &dest, ConflictPolicy::Skip);
    assert_eq!(rep.failed.len(), 1);
    assert!(sb.path("a.txt").exists(), "source untouched");
    assert!(rep.failed[0].error.contains("a.txt"));
}

#[test]
fn moving_onto_an_existing_folder_merges_and_removes_the_emptied_source() {
    let sb = Sandbox::new();
    sb.write("src/box/a.txt", "a");
    sb.write("src/box/b.txt", "src-b");
    sb.write("dest/box/b.txt", "dest-b");
    sb.write("dest/box/c.txt", "c");
    let e = sb.engine();
    let (_, rep) = do_move(
        &e,
        &[sb.path("src/box")],
        &sb.path("dest"),
        ConflictPolicy::Skip,
    );
    assert_eq!(rep.status(), RunStatus::Completed);
    assert!(sb.path("dest/box/a.txt").exists());
    assert_eq!(
        std::fs::read_to_string(sb.path("dest/box/b.txt")).unwrap(),
        "dest-b"
    );
    // b.txt was skipped, so the source folder must NOT have been deleted with it inside.
    assert!(sb.path("src/box/b.txt").exists());
    assert!(!sb.path("src/box/a.txt").exists());
}

// ---------------------------------------------------------------------------- trash (portable API)

#[test]
fn trashing_a_symlink_trashes_the_link_not_its_target() {
    let sb = Sandbox::new();
    let target = sb.write("real/keep.txt", "keep me");
    #[cfg(unix)]
    {
        let link = sb.symlink("real", "linkdir");
        let e = sb.engine();
        let rep = run(&e, &e.plan_trash(&scan(&e, std::slice::from_ref(&link))));
        assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep me");
    }
    #[cfg(not(unix))]
    let _ = target;
}

#[test]
fn the_trash_refuses_the_root_and_its_own_contents() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let plan = e.plan_trash(&scan(&e, &[PathBuf::from("/")]));
    assert!(!plan.is_executable());
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message.contains("cannot be read")
                || w.examples.contains(&PathBuf::from("/")))
    );
    let f = sb.write("t.txt", "x");
    run(&e, &e.plan_trash(&scan(&e, &[f])));
    let stored = sb.trashed_dir().join("t.txt");
    let rep = run(&e, &e.plan_trash(&scan(&e, &[stored])));
    assert_eq!(
        rep.failed.len(),
        1,
        "trashing something already in the trash is refused"
    );
}

// ---------------------------------------------------------------------------- delete

#[test]
fn permanent_delete_removes_links_but_never_their_targets() {
    let sb = Sandbox::new();
    let precious = sb.write("outside/precious.txt", "keep");
    sb.write("victim/a.txt", "a");
    sb.write("victim/sub/b.txt", "b");
    #[cfg(unix)]
    {
        sb.symlink(sb.path("outside"), "victim/link_to_outside");
        sb.symlink("nowhere", "victim/broken");
    }
    let e = sb.engine();
    let plan = e.plan_delete(&scan(&e, &[sb.path("victim")]));
    assert!(!plan.reversible);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.kind == WarningKind::Irreversible)
    );
    let rep = run(&e, &plan);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(!sb.path("victim").exists());
    assert_eq!(std::fs::read_to_string(precious).unwrap(), "keep");
}

#[test]
fn deleting_the_root_is_blocked_and_never_scans_it() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let started = std::time::Instant::now();
    let root = std::path::Path::new(if cfg!(windows) { "C:\\" } else { "/" });
    assert!(
        !e.plan_delete(&scan(&e, &[root.to_path_buf()]))
            .is_executable()
    );
    assert!(
        started.elapsed().as_secs() < 2,
        "the root must be refused, not scanned"
    );
}

// ---------------------------------------------------------------------------- bulk rename / mkdir

#[test]
fn bulk_rename_previews_and_applies_a_numbered_pattern() {
    let sb = Sandbox::new();
    let files: Vec<PathBuf> = ["IMG_9.jpg", "IMG_1.jpg", "IMG_5.JPG"]
        .iter()
        .map(|n| sb.write(format!("p.v2/{n}"), *n))
        .collect();
    let e = sb.engine();
    let pat = Pattern::parse("holiday_{n:3}.{ext}").unwrap();
    let preview = pat.preview(&files);
    assert_eq!(
        preview[0].to.as_ref().unwrap().to_str().unwrap(),
        "holiday_001.jpg"
    );
    let plan = e.plan_bulk_rename(&files, &pat);
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(run(&e, &plan).status(), RunStatus::Completed);
    assert!(sb.path("p.v2/holiday_001.jpg").exists() && sb.path("p.v2/holiday_003.JPG").exists());
}

#[test]
fn bulk_rename_swaps_names_through_temporary_names() {
    let sb = Sandbox::new();
    let a = sb.write("d/1", "one");
    let b = sb.write("d/2", "two");
    let e = sb.engine();
    // 1 -> 2 and 2 -> 3: a chain, so every rename goes through a temporary name.
    let pat = Pattern::parse("{n}").unwrap().with_counter(2, 1);
    let plan = e.plan_bulk_rename(&[a, b], &pat);
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(run(&e, &plan).status(), RunStatus::Completed);
    assert_eq!(std::fs::read_to_string(sb.path("d/2")).unwrap(), "one");
    assert_eq!(std::fs::read_to_string(sb.path("d/3")).unwrap(), "two");
    assert!(!sb.path("d/1").exists());
}

#[test]
fn bulk_rename_refuses_collisions_and_invalid_results() {
    let sb = Sandbox::new();
    let a = sb.write("d/a.txt", "a");
    let b = sb.write("d/b.txt", "b");
    sb.write("d/taken.txt", "t");
    let e = sb.engine();
    // Both would be "same.txt".
    let plan = e.plan_bulk_rename(
        &[a.clone(), b.clone()],
        &Pattern::parse("same.txt").unwrap(),
    );
    assert!(!plan.is_executable());
    // Target exists and is not part of the renamed set.
    let plan = e.plan_bulk_rename(
        std::slice::from_ref(&a),
        &Pattern::parse("taken.txt").unwrap(),
    );
    assert!(!plan.is_executable());
    // Result contains a slash.
    let plan = e.plan_bulk_rename(
        std::slice::from_ref(&a),
        &Pattern::parse("x/{name}").unwrap(),
    );
    assert!(!plan.is_executable());
    assert!(b.exists() && sb.path("d/a.txt").exists());
}

#[test]
fn mkdir_validates_and_creates() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let ok = e.plan_mkdir(&sb.work, "new.folder v1.2".as_ref());
    assert!(ok.is_executable());
    run(&e, &ok);
    assert!(sb.path("new.folder v1.2").is_dir());
    assert!(
        !e.plan_mkdir(&sb.work, "new.folder v1.2".as_ref())
            .is_executable(),
        "exists"
    );
    assert!(!e.plan_mkdir(&sb.work, "a/b".as_ref()).is_executable());
    assert!(!e.plan_mkdir(&sb.work, "".as_ref()).is_executable());
}

// ---------------------------------------------------------------------------- Linux: freedesktop trash

#[cfg(target_os = "linux")]
mod linux_only {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use rada_core::fs::LocalFs;
    use rada_core::ops::*;
    use rada_core::platform::freedesktop::{FreedesktopTrash, percent_decode};
    use rada_core::platform::linux::LinuxPlatform;
    use rada_core::platform::{Platform, TrashHandle};
    use rada_core::testutil::*;

    /// Remove `.Trash-<uid>` at a mount top if (and only if) this test created it.
    struct TrashCleanup(PathBuf, bool);
    impl TrashCleanup {
        fn new(top: &Path) -> Self {
            let uid = std::fs::metadata("/proc/self")
                .map(|m| m.uid())
                .unwrap_or(0);
            let p = top.join(format!(".Trash-{uid}"));
            let existed = p.exists();
            TrashCleanup(p, existed)
        }
    }
    impl Drop for TrashCleanup {
        fn drop(&mut self) {
            if !self.1 {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn read_info(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn trashing_writes_a_valid_freedesktop_entry_and_restores_it() {
        let sb = Sandbox::new();
        let f = sb.write("docs/ünï cödé & 100%.txt", "hello");
        let e = sb.engine();
        let plan = e.plan_trash(&scan(&e, std::slice::from_ref(&f)));
        let rep = run(&e, &plan);
        assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
        assert!(!f.exists());

        let trash = sb.dirs.home_trash();
        let info_dir = trash.join("info");
        let infos: Vec<_> = std::fs::read_dir(&info_dir).unwrap().flatten().collect();
        assert_eq!(infos.len(), 1);
        let text = read_info(&infos[0].path());
        assert!(text.starts_with("[Trash Info]\nPath="), "{text}");
        let path_line = text.lines().find(|l| l.starts_with("Path=")).unwrap();
        assert_eq!(percent_decode(&path_line[5..]), f.as_os_str());
        let date = text
            .lines()
            .find(|l| l.starts_with("DeletionDate="))
            .unwrap();
        // YYYY-MM-DDThh:mm:ss
        let d = &date["DeletionDate=".len()..];
        assert_eq!(d.len(), 19, "{date}");
        assert_eq!(&d[10..11], "T");
        assert_eq!(
            std::fs::read_to_string(trash.join("files/ünï cödé & 100%.txt")).unwrap(),
            "hello"
        );

        // Same name again: unique names, no clobbering.
        let g = sb.write("other/ünï cödé & 100%.txt", "second");
        let plan = e.plan_trash(&scan(&e, &[g]));
        run(&e, &plan);
        assert_eq!(std::fs::read_dir(trash.join("files")).unwrap().count(), 2);
        assert_eq!(std::fs::read_dir(&info_dir).unwrap().count(), 2);
    }

    #[test]
    fn trash_across_filesystems_uses_the_per_device_trash_and_restores() {
        let sb = Sandbox::new();
        let Some(other) = other_filesystem_dir(&sb) else {
            eprintln!("skipped: no second filesystem available");
            return;
        };
        let top = {
            // The mount point of `other`: highest ancestor with the same device.
            let dev = std::fs::metadata(other.path()).unwrap().dev();
            let mut t = other.path().to_path_buf();
            for a in other.path().ancestors().skip(1) {
                if std::fs::metadata(a).map(|m| m.dev()).ok() == Some(dev) {
                    t = a.to_path_buf()
                } else {
                    break;
                }
            }
            t
        };
        let _cleanup = TrashCleanup::new(&top);
        let victim = other.path().join("victim dir");
        std::fs::create_dir_all(victim.join("sub")).unwrap();
        std::fs::write(victim.join("sub/f.txt"), "payload").unwrap();
        let e = sb.engine();
        let plan = e.plan_trash(&scan(&e, std::slice::from_ref(&victim)));
        let rep = run(&e, &plan);
        assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
        assert!(!victim.exists());

        // It went to $topdir/.Trash-uid (a rename on the same device), not to the home trash.
        assert_eq!(
            std::fs::read_dir(sb.dirs.home_trash().join("files"))
                .map(|d| d.count())
                .unwrap_or(0),
            0
        );
        let per_dev = std::fs::read_dir(&top)
            .unwrap()
            .flatten()
            .find(|d| d.file_name().to_string_lossy().starts_with(".Trash-"))
            .expect("per-device trash")
            .path();
        let info = std::fs::read_dir(per_dev.join("info"))
            .unwrap()
            .flatten()
            .next()
            .unwrap()
            .path();
        let text = read_info(&info);
        let rel = text.lines().find(|l| l.starts_with("Path=")).unwrap()[5..].to_string();
        assert!(
            !rel.starts_with('/'),
            "Path= must be relative to the topdir for per-device trashes: {rel}"
        );
        assert!(per_dev.join("files/victim dir/sub/f.txt").exists());

        // And restore works from there.
        let item = {
            let n = std::fs::read_dir(per_dev.join("files"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            rada_core::platform::TrashedItem {
                original: victim.clone(),
                handle: TrashHandle::Freedesktop { stored: n, info },
            }
        };
        sb.platform().trash().restore(&item).unwrap();
        assert_eq!(
            std::fs::read_to_string(victim.join("sub/f.txt")).unwrap(),
            "payload"
        );
    }

    #[test]
    fn when_no_per_device_trash_is_possible_the_item_is_copied_to_the_home_trash() {
        let sb = Sandbox::new();
        let Some(other) = other_filesystem_dir(&sb) else {
            return;
        };
        // Force "topdir" to a place where .Trash-uid cannot be created.
        let trash = FreedesktopTrash::new(sb.dirs.home_trash(), Arc::new(LocalFs))
            .with_topdir_finder(|_| Ok(PathBuf::from("/proc")));
        let platform: Arc<dyn Platform> =
            Arc::new(LinuxPlatform::with_trash(sb.dirs.clone(), trash));
        let e = Engine::local(platform);
        let victim = other.path().join("far.txt");
        std::fs::write(&victim, "far away").unwrap();
        let rep = run(&e, &e.plan_trash(&scan(&e, std::slice::from_ref(&victim))));
        assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
        assert!(!victim.exists());
        let stored = sb.dirs.home_trash().join("files/far.txt");
        assert_eq!(std::fs::read_to_string(stored).unwrap(), "far away");
        assert_eq!(
            std::fs::read_dir(sb.dirs.home_trash().join("info"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn a_failed_trash_leaves_no_orphan_trashinfo() {
        // Regression: "invalid cross-device link" left file1.txt.2.trashinfo behind.
        let sb = Sandbox::new();
        let f = sb.write("will_fail.txt", "x");
        let fault = FaultFs::new();
        fault.fail(Op::Rename, "will_fail.txt", 13); // EACCES
        let trash = FreedesktopTrash::new(sb.dirs.home_trash(), fault.clone());
        let platform: Arc<dyn Platform> =
            Arc::new(LinuxPlatform::with_trash(sb.dirs.clone(), trash));
        let e = Engine::local(platform);
        let rep = run(&e, &e.plan_trash(&scan(&e, std::slice::from_ref(&f))));
        assert_eq!(rep.failed.len(), 1);
        assert!(f.exists());
        let infos = std::fs::read_dir(sb.dirs.home_trash().join("info"))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(infos, 0, "no orphan .trashinfo");
    }
}

#[cfg(not(target_os = "linux"))]
mod linux_only {
    #[test]
    #[ignore = "tests the freedesktop Trash layout (.trashinfo files, per-volume .Trash-uid folders), which exists on Linux only"]
    fn freedesktop_trash_suite() {}
}
