//! A file that is replaced between the plan and the removal, or in the instant before the
//! name is taken off, is not deleted: the removal looks at the file again as late as it can.

use std::path::Path;

use rada_core::ops::*;
use rada_core::testutil::*;

/// Copy `src/f.txt` to `dest/f.txt` through the journal; the undo of that copy removes the copy.
fn copied(
    sb: &Sandbox,
    e: &Engine,
    j: &rada_core::journal::Journal,
) -> (String, std::path::PathBuf) {
    let src = sb.write("src/f.txt", "the original contents");
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(e, &[src]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(e, j, &plan);
    (out.id, dest.join("f.txt"))
}

fn replace(path: &Path, with: &str) {
    std::fs::remove_file(path).unwrap();
    std::fs::write(path, with).unwrap();
}

#[test]
fn a_file_replaced_between_the_plan_and_the_run_is_kept() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let (id, copy) = copied(&sb, &e, &j);
    let up = e.plan_undo(&j, &id).unwrap();
    assert!(up.blocked.is_empty(), "the plan was clean");

    replace(&copy, "something else, written after the plan was shown");
    let rep = e
        .run_undo(&j, &up, &mut SkipErrors, &Cancel::new())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&copy).unwrap(),
        "something else, written after the plan was shown"
    );
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

#[test]
fn a_file_replaced_in_the_instant_before_it_is_removed_is_kept() {
    let sb = Sandbox::new();
    let ffs = FaultFs::new();
    let (e, j) = (sb.engine_with(ffs.clone()), sb.journal());
    let (id, copy) = copied(&sb, &e, &j);
    let up = e.plan_undo(&j, &id).unwrap();
    assert!(up.blocked.is_empty());

    // Runs after the executor has decided to remove the file and before the name goes.
    let target = copy.clone();
    ffs.hook(Op::RemoveFile, "dest/f.txt", 1, move || {
        replace(&target, "a newer file that took the name");
    });
    let rep = e
        .run_undo(&j, &up, &mut SkipErrors, &Cancel::new())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&copy).unwrap(),
        "a newer file that took the name",
        "the file that took the name was deleted"
    );
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
}

#[cfg(unix)]
#[test]
fn a_link_put_in_place_of_the_file_is_removed_never_followed() {
    let sb = Sandbox::new();
    let ffs = FaultFs::new();
    let (e, j) = (sb.engine_with(ffs.clone()), sb.journal());
    let (id, copy) = copied(&sb, &e, &j);
    let up = e.plan_undo(&j, &id).unwrap();
    let precious = sb.write("precious.txt", "do not touch");
    let (target, to) = (copy.clone(), precious.clone());
    ffs.hook(Op::RemoveFile, "dest/f.txt", 1, move || {
        std::fs::remove_file(&target).unwrap();
        std::os::unix::fs::symlink(&to, &target).unwrap();
    });
    e.run_undo(&j, &up, &mut SkipErrors, &Cancel::new())
        .unwrap();
    assert_eq!(std::fs::read_to_string(&precious).unwrap(), "do not touch");
    assert!(
        std::fs::symlink_metadata(&copy).is_ok(),
        "the link that took the name is not what the plan was about"
    );
}
