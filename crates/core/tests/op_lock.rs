//! An operation that is still running is never settled as interrupted, however old it is:
//! the process doing it holds a lock for the whole operation, and the lock goes away by
//! itself when the process dies.
//!
//! The test runs itself as the other rada: `lock_child` does nothing unless the environment
//! says it is the child, and then begins an operation and parks.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rada_core::journal::{EntryStatus, Journal};
use rada_core::ops::*;
use rada_core::platform::Dirs;
use rada_core::testutil::*;

/// The other rada: begins an operation, says so, and goes on "copying" until it is killed.
#[test]
fn lock_child() {
    let Ok(root) = std::env::var("RADA_LOCK_ROOT") else {
        return;
    };
    let journal = Journal::open(Dirs::under(Path::new(&root)).journal_path()).unwrap();
    journal
        .begin(
            OpKind::Copy,
            "a long copy",
            true,
            Totals::default(),
            None,
            None,
        )
        .unwrap();
    std::fs::write(Path::new(&root).join("marker"), b"parked").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

/// The other rada, killed when the test ends however it ends.
struct Other(Child);

impl Drop for Other {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_other_rada(sb: &Sandbox) -> Other {
    let marker = sb.root.join("marker");
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lock_child",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("RADA_LOCK_ROOT", &sb.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(60);
    while !marker.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    let child = Other(child);
    assert!(
        marker.exists(),
        "the other process never began its operation"
    );
    child
}

/// Rewrite the `begin` records of the journal as if they had been written `days` ago.
fn age_journal(path: &Path, days: i64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = String::new();
    for line in text.lines() {
        let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
        if v["t"] == "begin" {
            v["time"] = (now - days * 24 * 3600).into();
        }
        out.push_str(&v.to_string());
        out.push('\n');
    }
    std::fs::write(path, out).unwrap();
}

/// A `begin` record as an older rada wrote it: no lock file goes with it.
fn append_begin(path: &Path, id: &str, time: i64) {
    use std::io::Write;
    let line = format!(
        r#"{{"t":"begin","id":"{id}","time":{time},"kind":"copy","title":"old","reversible":true,"totals":{{"items":0,"files":0,"dirs":0,"symlinks":0,"bytes":0}}}}"#
    );
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(f, "{line}").unwrap();
}

fn settled(j: &Journal) -> bool {
    j.entries()
        .unwrap()
        .iter()
        .all(|e| e.status == EntryStatus::Finished(RunStatus::Interrupted))
}

#[test]
fn an_old_operation_held_by_a_live_process_is_not_touched_until_it_dies() {
    let sb = Sandbox::new();
    let mut child = start_other_rada(&sb);
    let j = sb.journal();
    let e = sb.engine();
    // The copy to the phone has been going on for two days.
    age_journal(j.path(), 2);
    assert_eq!(j.interrupted().unwrap().len(), 1);

    assert!(
        e.recover_interrupted(&j).unwrap().is_empty(),
        "an operation whose process is alive was settled"
    );
    assert!(!settled(&j));

    child.0.kill().unwrap(); // SIGKILL / TerminateProcess: no chance to clean up
    child.0.wait().unwrap();
    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec.len(), 1, "the lock should have gone with the process");
    assert!(settled(&j));
}

#[test]
fn an_operation_of_an_older_rada_is_judged_by_its_process_and_its_age() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let e = sb.engine();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mine = format!("{:x}-{:x}-0", 1, std::process::id());
    append_begin(j.path(), &mine, now);
    assert!(
        e.recover_interrupted(&j).unwrap().is_empty(),
        "fresh and alive: left alone"
    );
    // A process number that nothing has: the operation is settled.
    append_begin(j.path(), "2-ffffff7-0", now - 3600);
    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].id, "2-ffffff7-0");
}

#[test]
fn a_finished_operation_leaves_no_lock_behind() {
    let sb = Sandbox::new();
    let j = sb.journal();
    let id = j
        .begin(OpKind::Copy, "x", true, Totals::default(), None, None)
        .unwrap();
    let locks = locks_dir(&j);
    assert_eq!(std::fs::read_dir(&locks).unwrap().count(), 1);
    j.end(&id, RunStatus::Completed, 0, 0, 0).unwrap();
    assert_eq!(std::fs::read_dir(&locks).unwrap().count(), 0);
}

fn locks_dir(j: &Journal) -> PathBuf {
    j.path().parent().unwrap().join("locks")
}
