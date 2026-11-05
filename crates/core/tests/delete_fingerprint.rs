//! Permanent deletion looks at each file again when it removes it: a file that changed
//! between the plan and the run is not deleted.

use rada_core::ops::*;
use rada_core::testutil::*;

#[test]
fn a_file_changed_after_the_plan_is_kept_and_the_unchanged_ones_go() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.write("box/same.txt", "untouched");
    let edited = sb.write("box/edited.txt", "as the plan saw it");
    let plan = e.plan_delete(&scan(&e, &[sb.path("box")]));

    std::fs::write(&edited, "written after the plan, and longer").unwrap();
    let rep = run(&e, &plan);

    assert!(!sb.path("box/same.txt").exists());
    assert_eq!(
        std::fs::read_to_string(&edited).unwrap(),
        "written after the plan, and longer"
    );
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert!(rep.failed[0].error.contains("changed"), "{:?}", rep.failed);
    assert!(
        rep.kept.iter().any(|(p, _)| p.ends_with("box")),
        "the folder that still holds a file stays: {:?}",
        rep.kept
    );
}

#[test]
fn a_file_of_the_same_size_rewritten_after_the_plan_is_kept() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("f.txt", "aaaa");
    let plan = e.plan_delete(&scan(&e, std::slice::from_ref(&f)));

    // Same size: only the times tell. A pause makes sure the clock moved on.
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(&f, "bbbb").unwrap();
    let rep = run(&e, &plan);

    assert_eq!(std::fs::read_to_string(&f).unwrap(), "bbbb");
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

#[test]
fn files_that_did_not_change_are_deleted() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.write("box/a.txt", "a");
    sb.write("box/sub/b.txt", "b");
    let plan = e.plan_delete(&scan(&e, &[sb.path("box")]));
    assert!(plan.steps.iter().all(|s| match s {
        Step::RemoveFile { expect, .. } => expect.is_some(),
        _ => true,
    }));

    let rep = run(&e, &plan);

    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(!sb.path("box").exists());
}

#[test]
fn a_file_added_to_a_folder_after_the_plan_keeps_the_folder() {
    let sb = Sandbox::new();
    let e = sb.engine();
    sb.write("box/a.txt", "a");
    let plan = e.plan_delete(&scan(&e, &[sb.path("box")]));

    let newcomer = sb.write("box/new.txt", "made after the plan");
    run(&e, &plan);

    assert!(!sb.path("box/a.txt").exists());
    assert_eq!(
        std::fs::read_to_string(newcomer).unwrap(),
        "made after the plan"
    );
}

#[cfg(unix)]
#[test]
fn a_link_put_in_place_of_a_file_after_the_plan_is_kept_and_never_followed() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("f.txt", "planned");
    let precious = sb.write("precious.txt", "do not touch");
    let plan = e.plan_delete(&scan(&e, std::slice::from_ref(&f)));

    std::fs::remove_file(&f).unwrap();
    sb.symlink(&precious, "f.txt");
    run(&e, &plan);

    assert!(
        std::fs::symlink_metadata(&f).is_ok(),
        "the link is not the planned file"
    );
    assert_eq!(std::fs::read_to_string(precious).unwrap(), "do not touch");
}

/// A move across filesystems copies, then removes the source: if another file took the source's
/// name while the data was being copied, that file is not the one that was moved.
#[test]
fn a_move_across_filesystems_does_not_remove_a_source_that_was_replaced() {
    let sb = Sandbox::new();
    let ffs = FaultFs::new();
    let e = sb.engine_with(ffs.clone());
    let src = sb.write("src/f.txt", "the original contents");
    let dest = sb.mkdir("dest");
    let mut plan = e.plan_transfer(
        &scan(&e, std::slice::from_ref(&src)),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    for step in &mut plan.steps {
        if let Step::CopyFile { remove_source, .. } = step {
            *remove_source = true;
        }
    }

    let target = src.clone();
    ffs.hook(Op::RemoveFile, "src/f.txt", 1, move || {
        std::fs::remove_file(&target).unwrap();
        std::fs::write(&target, "a newer file that took the name").unwrap();
    });
    let rep = run(&e, &plan);

    assert_eq!(
        std::fs::read_to_string(&src).unwrap(),
        "a newer file that took the name"
    );
    assert!(
        !dest.join("f.txt").exists(),
        "the item is never left in two places"
    );
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

#[test]
fn a_move_across_filesystems_still_removes_the_source_that_was_copied() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let src = sb.write("src/f.txt", "the original contents");
    let dest = sb.mkdir("dest");
    let mut plan = e.plan_transfer(
        &scan(&e, std::slice::from_ref(&src)),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    for step in &mut plan.steps {
        if let Step::CopyFile { remove_source, .. } = step {
            *remove_source = true;
        }
    }

    let rep = run(&e, &plan);

    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(!src.exists());
    assert_eq!(
        std::fs::read_to_string(dest.join("f.txt")).unwrap(),
        "the original contents"
    );
}
