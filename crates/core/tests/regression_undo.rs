//! Undo of every kind of operation, with the safety checks: nothing modified after the
//! operation is ever overwritten or deleted.

use std::path::PathBuf;

use rada_core::journal::{EntryStatus, UndoState};
use rada_core::ops::*;
use rada_core::testutil::*;

fn modify(p: &std::path::Path, content: &str) {
    // Make sure size or mtime really differs even on coarse clocks.
    std::thread::sleep(std::time::Duration::from_millis(15));
    std::fs::write(p, content).unwrap();
}

fn small_tree(sb: &Sandbox, root: &str) -> PathBuf {
    sb.write(format!("{root}/a.txt"), "aaa");
    sb.write(format!("{root}/d.v1/b.txt"), "bbb");
    sb.write(format!("{root}/d.v1/deeper/c.txt"), "ccc");
    sb.mkdir(format!("{root}/empty"));
    #[cfg(unix)]
    sb.symlink("a.txt", format!("{root}/link"));
    sb.path(root)
}

// ------------------------------------------------------------------------- copy

#[test]
fn undoing_a_copy_removes_exactly_what_it_created() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let src = small_tree(&sb, "src");
    sb.write("dest/pre-existing.txt", "mine");
    sb.write("dest/src/old.txt", "was already here"); // merge target
    let before_dest = snapshot(&sb.path("dest"));
    let plan = e.plan_transfer(
        &scan(&e, &[src]),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(out.report.status(), RunStatus::Completed);
    assert!(sb.path("dest/src/d.v1/deeper/c.txt").exists());

    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(
        snapshot(&sb.path("dest")),
        before_dest,
        "destination is back to its previous state"
    );
    assert!(sb.path("src/a.txt").exists(), "the source is untouched");
    assert_eq!(j.entries().unwrap()[0].undo_state, UndoState::Undone);
}

#[test]
fn undo_of_a_copy_leaves_a_file_modified_afterwards_and_says_so() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let src = small_tree(&sb, "src");
    let plan = e.plan_transfer(
        &scan(&e, &[src]),
        &sb.mkdir("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(plan.is_executable());
    let out = run_journaled(&e, &j, &plan);

    let edited = sb.path("dest/src/d.v1/b.txt");
    modify(&edited, "I EDITED THIS AFTER THE COPY");
    let extra = sb.write("dest/src/d.v1/brand-new.txt", "user created");

    let up = e.plan_undo(&j, &out.id).unwrap();
    assert_eq!(up.blocked.len(), 1, "{:?}", up.blocked);
    assert!(up.blocked[0].reason.contains("modified"));
    assert!(
        up.blocked[0].reason.contains("b.txt"),
        "message names the file: {}",
        up.blocked[0].reason
    );
    assert!(
        up.plan
            .warnings
            .iter()
            .any(|w| w.kind == WarningKind::ModifiedSince)
    );

    let rep = e
        .run_undo(&j, &up, &mut SkipErrors, &Cancel::new())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&edited).unwrap(),
        "I EDITED THIS AFTER THE COPY",
        "never overwritten, never deleted"
    );
    assert!(extra.exists());
    assert!(
        !sb.path("dest/src/a.txt").exists(),
        "unmodified copies are removed"
    );
    assert!(!sb.path("dest/src/d.v1/deeper").exists());
    assert!(
        sb.path("dest/src/d.v1").is_dir(),
        "folder kept: it still holds the user's files"
    );
    assert!(!rep.kept.is_empty());
    assert!(matches!(
        j.entries().unwrap()[0].undo_state,
        UndoState::Partial { .. }
    ));
}

#[test]
fn undo_of_a_copy_never_touches_a_file_replaced_by_the_user() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.write("src/f.txt", "original");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src/f.txt")]),
        &sb.mkdir("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    // Same size, new content, mtime changed.
    modify(&sb.path("dest/f.txt"), "OVERWRITE");
    let up = e.plan_undo(&j, &out.id).unwrap();
    assert_eq!(up.blocked.len(), 1);
    assert!(up.plan.steps.is_empty());
}

#[test]
fn undoing_an_overwriting_copy_brings_the_old_file_back_from_the_trash() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.write("src/f.txt", "NEW");
    sb.write("dest/f.txt", "OLD");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src/f.txt")]),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::Overwrite),
    );
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(
        std::fs::read_to_string(sb.path("dest/f.txt")).unwrap(),
        "NEW"
    );
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(
        std::fs::read_to_string(sb.path("dest/f.txt")).unwrap(),
        "OLD"
    );
}

// ------------------------------------------------------------------------- move / rename

#[test]
fn undoing_a_move_puts_everything_back() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let src = small_tree(&sb, "src");
    let before = snapshot(&src);
    let plan = e.plan_transfer(
        &scan(&e, std::slice::from_ref(&src)),
        &sb.mkdir("dest"),
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(!src.exists());
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty());
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(snapshot(&src), before);
    assert!(snapshot(&sb.path("dest")).is_empty());
}

#[test]
fn undo_of_a_move_refuses_to_overwrite_something_now_at_the_origin() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.write("a/f.txt", "moved");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("a/f.txt")]),
        &sb.mkdir("b"),
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    sb.write("a/f.txt", "someone else's new file");
    let up = e.plan_undo(&j, &out.id).unwrap();
    assert_eq!(up.blocked.len(), 1);
    assert!(up.blocked[0].reason.contains("occupied"));
    e.run_undo(&j, &up, &mut SkipErrors, &Cancel::new())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(sb.path("a/f.txt")).unwrap(),
        "someone else's new file"
    );
    assert_eq!(
        std::fs::read_to_string(sb.path("b/f.txt")).unwrap(),
        "moved"
    );
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "needs a tmpfs next to the disk (two filesystems), which only the Linux test machines have"
)]
fn undoing_a_cross_filesystem_move_goes_back_across_the_boundary() {
    let sb = Sandbox::new();
    let Some(other) = other_filesystem_dir(&sb) else {
        return;
    };
    let (e, j) = (sb.engine(), sb.journal());
    let src = small_tree(&sb, "src");
    let before = snapshot(&src);
    let plan = e.plan_transfer(
        &scan(&e, std::slice::from_ref(&src)),
        other.path(),
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(!src.exists());
    assert!(other.path().join("src/d.v1/deeper/c.txt").exists());
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(snapshot(&src), before);
    assert!(snapshot(other.path()).is_empty());
}

#[test]
fn undoing_a_rename_and_a_bulk_rename_including_swaps() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("d/old.name.txt", "x");
    let out = run_journaled(&e, &j, &e.plan_rename(&f, "new.name.txt".as_ref()));
    assert!(sb.path("d/new.name.txt").exists());
    let (_, rep) = undo(&e, &j, &out.id);
    assert_eq!(rep.status(), RunStatus::Completed);
    assert!(f.exists() && !sb.path("d/new.name.txt").exists());

    // Chain 1->2, 2->3 goes through temporary names; the undo must cope with them too.
    let a = sb.write("b/1", "one");
    let b = sb.write("b/2", "two");
    let plan = e.plan_bulk_rename(
        &[a.clone(), b.clone()],
        &Pattern::parse("{n}").unwrap().with_counter(2, 1),
    );
    assert!(plan.steps.len() == 4, "staged: {:?}", plan.steps.len());
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(std::fs::read_to_string(sb.path("b/3")).unwrap(), "two");
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "one");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "two");
    assert!(!sb.path("b/3").exists());
}

// ------------------------------------------------------------------------- mkdir / trash / delete

#[test]
fn undoing_mkdir_removes_it_unless_it_gained_content() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let out = run_journaled(&e, &j, &e.plan_mkdir(&sb.work, "fresh".as_ref()));
    let (_, rep) = undo(&e, &j, &out.id);
    assert_eq!(rep.status(), RunStatus::Completed);
    assert!(!sb.path("fresh").exists());

    let out = run_journaled(&e, &j, &e.plan_mkdir(&sb.work, "fresh2".as_ref()));
    sb.write("fresh2/mine.txt", "x");
    let (_, rep) = undo(&e, &j, &out.id);
    assert!(sb.path("fresh2/mine.txt").exists());
    assert!(
        !rep.kept.is_empty(),
        "reported as kept, not silently ignored"
    );
}

#[test]
fn undoing_trash_restores_files_and_folders_with_their_content() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let src = small_tree(&sb, "v1.2/stuff");
    let single = sb.write("v1.2/single file.txt", "single");
    let before = snapshot(&sb.path("v1.2"));
    let plan = e.plan_trash(&scan(&e, &[src.clone(), single.clone()]));
    let out = run_journaled(&e, &j, &plan);
    assert!(!src.exists() && !single.exists());
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(snapshot(&sb.path("v1.2")), before);
    // The trash no longer holds them, and no orphaned .trashinfo remains.
    assert_eq!(std::fs::read_dir(sb.trashed_dir()).unwrap().count(), 0);
    #[cfg(target_os = "linux")]
    assert_eq!(
        std::fs::read_dir(sb.dirs.home_trash().join("info"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn undoing_trash_refuses_to_overwrite_a_new_file_at_the_original_path() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("f.txt", "old");
    let out = run_journaled(&e, &j, &e.plan_trash(&scan(&e, std::slice::from_ref(&f))));
    sb.write("f.txt", "brand new");
    let up = e.plan_undo(&j, &out.id).unwrap();
    assert_eq!(up.blocked.len(), 1);
    assert!(up.blocked[0].reason.contains("already exists"));
    assert!(up.plan.steps.is_empty());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "brand new");
    // The trashed original is still safe in the trash.
    assert_eq!(std::fs::read_dir(sb.trashed_dir()).unwrap().count(), 1);
}

#[test]
fn undoing_trash_after_the_trash_was_emptied_explains_instead_of_failing() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("gone.txt", "x");
    let out = run_journaled(&e, &j, &e.plan_trash(&scan(&e, &[f])));
    std::fs::remove_dir_all(sb.trashed_dir()).unwrap();
    std::fs::create_dir_all(sb.trashed_dir()).unwrap();
    let up = e.plan_undo(&j, &out.id).unwrap();
    assert_eq!(up.blocked.len(), 1);
    assert!(up.blocked[0].reason.contains("no longer in the trash"));
}

#[test]
fn permanent_delete_is_recorded_but_cannot_be_undone() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("bye.txt", "x");
    let plan = e.plan_delete(&scan(&e, std::slice::from_ref(&f)));
    let out = run_journaled(&e, &j, &plan);
    assert!(!f.exists());
    let entry = j.entries().unwrap().pop().unwrap();
    assert!(!entry.reversible && !entry.is_undoable());
    assert_eq!(entry.undo_steps, 0);
    assert!(e.plan_undo(&j, &out.id).is_err());
    assert!(j.last_undoable().unwrap().is_none());
}

// ------------------------------------------------------------------------- journal behaviour

#[test]
fn last_undoable_follows_history_and_skips_what_is_already_undone() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let a = run_journaled(&e, &j, &e.plan_mkdir(&sb.work, "one".as_ref()));
    let b = run_journaled(&e, &j, &e.plan_mkdir(&sb.work, "two".as_ref()));
    assert_eq!(j.last_undoable().unwrap().unwrap().id, b.id);
    undo(&e, &j, &b.id);
    assert_eq!(j.last_undoable().unwrap().unwrap().id, a.id);
    undo(&e, &j, &a.id);
    assert!(j.last_undoable().unwrap().is_none());
    let entries = j.entries().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "undo does not add its own undoable entries"
    );
    assert!(entries.iter().all(|x| x.undo_state == UndoState::Undone));
}

#[test]
fn the_journal_survives_a_restart_and_non_utf8_paths() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let id;
    let file;
    {
        let j = sb.journal();
        #[cfg(unix)]
        let name = os_from_bytes(b"caf\xe9-\xff.txt");
        #[cfg(not(unix))]
        let name = std::ffi::OsString::from("café.txt");
        file = sb.path("src").join(&name);
        std::fs::create_dir_all(sb.path("src")).unwrap();
        std::fs::write(&file, "data").unwrap();
        let plan = e.plan_transfer(
            &scan(&e, std::slice::from_ref(&file)),
            &sb.mkdir("dest"),
            &TransferOptions::mv(ConflictPolicy::Skip),
        );
        id = run_journaled(&e, &j, &plan).id;
    }
    // New process, new Journal handle on the same file.
    let j2 = sb.journal();
    let entries = j2.entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].status,
        EntryStatus::Finished(RunStatus::Completed)
    );
    let (up, rep) = undo(&e, &j2, &id);
    assert!(up.blocked.is_empty());
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "data");
}

#[test]
fn an_interrupted_operation_can_still_be_undone() {
    // Simulate a crash: the process dies after some steps, before writing `end`.
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    for n in ["a", "b", "c", "d"] {
        sb.write(format!("src/{n}.txt"), n);
    }
    let fault = FaultFs::new();
    let e2 = sb.engine_with(fault.clone());
    let srcs: Vec<PathBuf> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| sb.path(format!("src/{n}.txt")))
        .collect();
    let plan = e2.plan_transfer(
        &scan(&e2, &srcs),
        &sb.mkdir("dest"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    fault.fail(Op::CopyFile, "src/c.txt", 5);
    let mut h = Scripted::new(ErrorChoice::Abort);
    let out = e2.run_operation(&j, &plan, &mut h, &Cancel::new()).unwrap();
    assert_eq!(out.report.status(), RunStatus::Aborted);
    let entry = j.entries().unwrap().pop().unwrap();
    assert!(entry.is_undoable(), "aborted operations are undoable");
    assert_eq!(entry.undo_steps, 2);
    undo(&e, &j, &out.id);
    assert!(snapshot(&sb.path("dest")).is_empty());

    // And a journal whose last record never reached the disk is still readable.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(j.path())
        .unwrap();
    f.write_all(b"{\"t\":\"begin\",\"id\":\"zzz\",\"ti")
        .unwrap();
    assert!(j.entries().is_ok());
}

#[test]
#[cfg_attr(
    windows,
    ignore = "needs POSIX permissions or file names that Windows rejects"
)]
fn journal_files_are_private_to_the_user() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let sb = Sandbox::new();
        let j = sb.journal();
        let mode = std::fs::metadata(j.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "the journal lists the user's file names");
    }
}
