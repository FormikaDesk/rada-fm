//! The process is killed (SIGKILL, no chance to clean up) at different points of a copy or a
//! move. On the next start the journal must be coherent, no temporary file may be left behind,
//! and undoing what had been done must still work.
//!
//! The test runs itself as the victim: `crash_child` does nothing unless the environment says
//! it is the child, and then performs the operation until it parks at the chosen point, where
//! the parent kills it.
#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rada_core::journal::{EntryStatus, Journal};
use rada_core::ops::*;
use rada_core::platform::{self, Dirs};
use rada_core::testutil::*;

const MIB: usize = 1 << 20;

fn park(marker: PathBuf) -> ! {
    std::fs::write(&marker, b"parked").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

/// The victim. Only does something when started by [`crash_at`].
#[test]
fn crash_child() {
    let Ok(point) = std::env::var("RADA_CRASH_POINT") else {
        return;
    };
    let root = PathBuf::from(std::env::var("RADA_CRASH_ROOT").unwrap());
    let src = PathBuf::from(std::env::var("RADA_CRASH_SRC").unwrap());
    let dest = PathBuf::from(std::env::var("RADA_CRASH_DEST").unwrap());
    let marker = PathBuf::from(std::env::var("RADA_CRASH_MARKER").unwrap());
    let mv = std::env::var("RADA_CRASH_MODE").as_deref() == Ok("move");

    let dirs = Dirs::under(&root);
    let journal = Journal::open(dirs.journal_path()).unwrap();
    let ffs = FaultFs::new();
    let (kind, name) = point.split_once(':').unwrap();
    let m = marker.clone();
    match kind {
        "before" => ffs.hook(Op::CopyFile, name.to_string(), 1, move || park(m.clone())),
        "mid" => ffs.on_bytes(
            name.to_string(),
            1,
            ByteAction::Run(Arc::new(move || park(m.clone()))),
        ),
        // The second look at the destination is the one right after it was renamed into place.
        "after_rename" => ffs.hook(Op::Lstat, name.to_string(), 2, move || park(m.clone())),
        "before_unlink" => ffs.hook(Op::RemoveFile, name.to_string(), 1, move || park(m.clone())),
        other => panic!("unknown crash point {other}"),
    }
    let engine = Engine::new(ffs, platform::current(dirs));
    let opts = if mv {
        TransferOptions::mv(ConflictPolicy::Skip)
    } else {
        TransferOptions::copy(ConflictPolicy::Skip)
    };
    let plan = engine.plan_transfer(&scan(&engine, &[src]), &dest, &opts);
    let _ = engine.run_operation(&journal, &plan, &mut SkipErrors, &Cancel::new());
    // The chosen point was never reached: say so, the parent will complain.
    std::fs::write(&marker, b"finished").unwrap();
}

fn build_tree(sb: &Sandbox) {
    sb.write("src/tree/a.txt", "alpha");
    let mut f = std::fs::File::create(sb.path("src/tree/b.bin")).unwrap();
    let block: Vec<u8> = (0..MIB).map(|i| (i % 253) as u8).collect();
    for _ in 0..24 {
        f.write_all(&block).unwrap();
    }
    sb.write("src/tree/c.txt", "gamma");
    sb.write("src/tree/sub/d.txt", "delta");
    sb.write("src/tree/sub/e.txt", "epsilon");
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

struct Outcome {
    adopted: usize,
    temps_removed: usize,
    temps_before: usize,
}

/// Kill a child that copies (or moves) `src/tree` at `point`; settle; undo; check.
fn crash_at(point: &str, mv: bool) -> Outcome {
    let sb = Sandbox::new();
    let other = mv.then(|| other_filesystem_dir(&sb)).flatten();
    if mv && other.is_none() {
        return Outcome {
            adopted: 0,
            temps_removed: 0,
            temps_before: 0,
        };
    }
    build_tree(&sb);
    let original = snapshot(&sb.path("src"));
    let base = other
        .as_ref()
        .map(|t| t.path().to_path_buf())
        .unwrap_or_else(|| sb.root.clone());
    let dest = base.join("OUT");
    std::fs::create_dir_all(&dest).unwrap();
    let marker = sb.root.join("marker");

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash_child",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("RADA_CRASH_POINT", point)
        .env("RADA_CRASH_ROOT", &sb.root)
        .env("RADA_CRASH_SRC", sb.path("src/tree"))
        .env("RADA_CRASH_DEST", &dest)
        .env("RADA_CRASH_MARKER", &marker)
        .env("RADA_CRASH_MODE", if mv { "move" } else { "copy" })
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(60);
    while !marker.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read(&marker).unwrap_or_default(),
        b"parked",
        "{point}: the child never reached the point"
    );
    child.kill().unwrap(); // SIGKILL
    child.wait().unwrap();

    // The next start.
    let e = sb.engine();
    let j = sb.journal();
    assert_eq!(
        j.interrupted().unwrap().len(),
        1,
        "{point}: the journal knows an operation was cut short"
    );
    let temps_before = leftovers(&base).len();
    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec.len(), 1, "{point}: {rec:?}");
    assert!(
        leftovers(&base).is_empty(),
        "{point}: temporary files left: {:?}",
        leftovers(&base)
    );
    assert!(
        e.recover_interrupted(&j).unwrap().is_empty(),
        "{point}: settled once, not twice"
    );
    let entry = j.entries().unwrap().pop().unwrap();
    assert_eq!(entry.status, EntryStatus::Finished(RunStatus::Interrupted));

    // Undo whatever was done: the world is as it was.
    let (up, rep) = undo(&e, &j, &entry.id);
    assert!(
        up.blocked.is_empty() && rep.failed.is_empty(),
        "{point}: {:?} {:?}",
        up.blocked,
        rep.failed
    );
    let after = snapshot(&dest);
    assert!(
        after.is_empty(),
        "{point}: the destination still has {:?}",
        after.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot(&sb.path("src")),
        original,
        "{point}: the source is as it was"
    );
    Outcome {
        adopted: rec[0].adopted.len(),
        temps_removed: rec[0].removed_temps.len(),
        temps_before,
    }
}

#[test]
fn killed_before_a_file_is_copied() {
    let o = crash_at("before:src/tree/c.txt", false);
    assert_eq!(o.adopted, 0);
    assert_eq!(o.temps_before, 0);
}

#[test]
fn killed_in_the_middle_of_a_big_file_the_temporary_file_is_cleaned_up() {
    let o = crash_at("mid:tree/b.bin", false);
    assert!(o.temps_before >= 1, "the kill really left a temporary file");
    assert_eq!(o.temps_removed, o.temps_before);
    assert_eq!(o.adopted, 0, "a half-copied file never counts as copied");
}

#[test]
fn killed_just_after_the_rename_the_finished_file_is_adopted_into_the_history() {
    let o = crash_at("after_rename:OUT/tree/c.txt", false);
    assert_eq!(
        o.adopted, 1,
        "the copy exists but the journal had not yet written its undo"
    );
}

#[test]
fn killed_inside_a_folder_after_some_files_is_coherent_too() {
    crash_at("before:src/tree/sub/e.txt", false);
}

#[test]
fn a_move_across_filesystems_killed_mid_file_loses_nothing() {
    crash_at("mid:tree/b.bin", true);
}

#[test]
fn a_move_killed_between_the_copy_and_the_removal_of_the_source_keeps_both_and_undo_removes_the_copy()
 {
    let o = crash_at("before_unlink:src/tree/c.txt", true);
    assert_eq!(o.adopted, 1, "the copy is in place");
}

#[test]
fn a_running_operation_is_not_touched_by_the_recovery() {
    // An interrupted-looking entry of a process that is still alive (this one) is left alone.
    let sb = Sandbox::new();
    let j = sb.journal();
    let e = sb.engine();
    j.begin(
        OpKind::Copy,
        "still going",
        true,
        Totals::default(),
        None,
        None,
    )
    .unwrap();
    assert!(e.recover_interrupted(&j).unwrap().is_empty());
    assert_eq!(j.interrupted().unwrap().len(), 1, "it is still open");
}

#[test]
fn a_torn_last_line_of_the_journal_is_ignored() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let id = j
        .begin(OpKind::Copy, "x", true, Totals::default(), None, None)
        .unwrap();
    j.end(&id, RunStatus::Completed, 0, 0, 0).unwrap();
    // The process died in the middle of writing the next record.
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(j.path())
        .unwrap();
    f.write_all(br#"{"t":"begin","id":"cut","time":1,"ki"#)
        .unwrap();
    drop(f);
    let entries = j.entries().unwrap();
    assert_eq!(entries.len(), 1, "the half record is not an operation");
    assert_eq!(
        entries[0].status,
        EntryStatus::Finished(RunStatus::Completed)
    );
}

#[test]
fn an_undo_that_was_cut_short_resumes_where_it_stopped() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    for n in ["a", "b", "c", "d"] {
        sb.write(format!("src/{n}.txt"), n);
    }
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(out.report.failed.is_empty());

    // An undo that got through two files and was then killed: those two are gone, the journal
    // has their per-step records but no final account.
    let up = e.plan_undo(&j, &out.id).unwrap();
    let mut done = 0;
    for (k, step) in up.plan.steps.iter().enumerate() {
        if let Step::RemoveFile { path, .. } = step
            && done < 2
        {
            std::fs::remove_file(path).unwrap();
            j.mark_step_undone(&out.id, up.index_map[k]).unwrap();
            done += 1;
        }
    }
    assert_eq!(done, 2);
    let entry = j.entries().unwrap().pop().unwrap();
    assert!(
        matches!(entry.undo_state, rada_core::journal::UndoState::Partial { remaining } if remaining > 0),
        "{:?}",
        entry.undo_state
    );
    assert!(entry.can_retry_undo());

    // Taking it up again does the rest, and does not trip over what is already done.
    let (up2, rep) = undo(&e, &j, &out.id);
    assert!(
        up2.blocked.is_empty() && rep.failed.is_empty(),
        "{:?} {:?}",
        up2.blocked,
        rep.failed
    );
    assert!(snapshot(&dest).is_empty());
    assert_eq!(
        j.entries().unwrap().pop().unwrap().undo_state,
        rada_core::journal::UndoState::Undone
    );
}
