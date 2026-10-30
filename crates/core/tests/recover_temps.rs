//! Settling an operation that was cut short removes the temporary files of that operation and
//! of no other, also when another rada that is running has the same process id.
#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rada_core::journal::Journal;
use rada_core::ops::*;
use rada_core::platform::{self, Dirs};
use rada_core::testutil::*;

fn append(j: &Journal, line: serde_json::Value) {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(j.path())
        .unwrap();
    writeln!(f, "{line}").unwrap();
}

/// An operation of a rada that is gone: begun two days ago, with the step it was in.
fn cut_short(j: &Journal, id: &str, intent: serde_json::Value) {
    let old = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 2 * 24 * 3600;
    append(
        j,
        serde_json::json!({"t":"begin","id":id,"time":old,"kind":"copy","title":"cut",
            "reversible":true,"totals":{"items":0,"files":0,"dirs":0,"symlinks":0,"bytes":0}}),
    );
    append(
        j,
        serde_json::json!({"t":"pending","id":id,"intent":intent}),
    );
}

fn parts(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rada-part-"))
        .collect();
    v.sort();
    v
}

fn pid_id(n: u32) -> String {
    format!("1-{:x}-{n}", std::process::id())
}

/// What another rada, running a copy right now, has in the folder: caught in the middle of a
/// copy of this process, which has the same process id as the operation that was cut short.
fn temps_while_copying(sb: &Sandbox, dest: &Path) -> (Vec<String>, Vec<String>) {
    sb.write("src/big.bin", "x".repeat(2 << 20));
    let seen: Arc<Mutex<(Vec<String>, Vec<String>)>> = Arc::default();
    let (root, dir, out) = (sb.root.clone(), dest.to_path_buf(), seen.clone());
    let ffs = FaultFs::new();
    ffs.on_bytes(
        "big.bin",
        1,
        ByteAction::Run(Arc::new(move || {
            let dirs = Dirs::under(&root);
            let engine = Engine::local(platform::current(dirs.clone()));
            let journal = Journal::open(dirs.journal_path()).unwrap();
            let settled = engine.recover_interrupted(&journal).unwrap();
            let mut seen = out.lock().unwrap();
            seen.0 = settled.iter().map(|r| r.id.clone()).collect();
            seen.1 = parts(&dir);
        })),
    );
    let engine = sb.engine_with(ffs);
    let plan = engine.plan_transfer(
        &scan(&engine, &[sb.path("src/big.bin")]),
        dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let _ = engine.run_operation(&sb.journal(), &plan, &mut SkipErrors, &Cancel::new());
    seen.lock().unwrap().clone()
}

#[test]
fn settling_an_old_record_leaves_the_copy_of_another_rada_with_the_same_pid() {
    let sb = Sandbox::new();
    let dest = sb.mkdir("d");
    let a = pid_id(0);
    cut_short(
        &sb.journal(),
        &a,
        serde_json::json!({"k":"create","dst":dest.join("a.txt")}),
    );

    let (settled, during) = temps_while_copying(&sb, &dest);
    assert_eq!(settled, vec![a], "the old operation is settled");
    assert_eq!(
        during.len(),
        1,
        "the copy in progress keeps its file: {during:?}"
    );
}

#[test]
fn an_operation_that_names_its_temporary_file_loses_that_one_only() {
    let sb = Sandbox::new();
    let dest = sb.mkdir("d");
    let (mine, other): (PathBuf, PathBuf) = (
        dest.join(".rada-part-op7-3-1"),
        dest.join(format!(".rada-part-{}-1", std::process::id())),
    );
    std::fs::write(&mine, b"half").unwrap();
    std::fs::write(&other, b"half").unwrap();
    let a = pid_id(0);
    cut_short(
        &sb.journal(),
        &a,
        serde_json::json!({"k":"create","dst":dest.join("a.txt"),"temp":"op7-3"}),
    );

    let rec = sb.engine().recover_interrupted(&sb.journal()).unwrap();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].removed_temps, vec![mine.clone()]);
    assert!(!mine.exists() && other.exists());
}

#[test]
fn only_the_counter_of_the_step_follows_its_name() {
    let sb = Sandbox::new();
    let dest = sb.mkdir("d");
    let mine = dest.join(".rada-part-op7-3-1");
    let kept = [
        // More after the counter.
        ".rada-part-op7-3-0-extra",
        ".rada-part-op7-3-1.bak",
        // No counter at all.
        ".rada-part-op7-3-",
        // A step whose tag goes on where this one's counter would be.
        ".rada-part-op7-3-1-2",
    ];
    std::fs::write(&mine, b"half").unwrap();
    for n in kept {
        std::fs::write(dest.join(n), b"theirs").unwrap();
    }
    cut_short(
        &sb.journal(),
        &pid_id(0),
        serde_json::json!({"k":"create","dst":dest.join("a.txt"),"temp":"op7-3"}),
    );

    let rec = sb.engine().recover_interrupted(&sb.journal()).unwrap();
    assert_eq!(rec[0].removed_temps, vec![mine.clone()]);
    assert!(!mine.exists());
    for n in kept {
        assert!(dest.join(n).exists(), "{n} is not this step's");
    }
}
