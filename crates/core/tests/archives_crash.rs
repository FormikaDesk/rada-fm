//! The process is killed (SIGKILL) in the middle of extracting an archive. On the next start no
//! file may be half written under its final name, the journal must be coherent, the leftover
//! temporary file must be cleaned up, and undoing what was done must still work.
#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rada_core::archive::Compression;
use rada_core::archive::testkit::{Member, write_tar};
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

fn big() -> Vec<u8> {
    (0..24 * MIB).map(|i| (i % 253) as u8).collect()
}

#[test]
fn crash_child_archive() {
    let Ok(point) = std::env::var("RADA_CRASH_POINT") else {
        return;
    };
    let root = PathBuf::from(std::env::var("RADA_CRASH_ROOT").unwrap());
    let archive = PathBuf::from(std::env::var("RADA_CRASH_SRC").unwrap());
    let dest = PathBuf::from(std::env::var("RADA_CRASH_DEST").unwrap());
    let marker = PathBuf::from(std::env::var("RADA_CRASH_MARKER").unwrap());
    let dirs = Dirs::under(&root);
    let journal = Journal::open(dirs.journal_path()).unwrap();
    let ffs = FaultFs::new();
    let (kind, name) = point.split_once(':').unwrap();
    let m = marker.clone();
    match kind {
        "before" => ffs.hook(Op::Lstat, name.to_string(), 1, move || park(m.clone())),
        "mid" => ffs.on_writes_after("OUT", MIB as u64, move || park(m.clone())),
        // Looked at while planning, before the step, and right after the rename: the third.
        "after_rename" => ffs.hook(Op::Lstat, name.to_string(), 3, move || park(m.clone())),
        other => panic!("unknown crash point {other}"),
    }
    let engine = Engine::new(ffs, platform::current(dirs));
    let req = OpRequest::Extract {
        archive,
        destination: dest,
        into: ExtractInto::Here,
        only: vec![],
        conflict: ConflictPolicy::Skip,
    };
    let cancel = Cancel::new();
    let planned = engine
        .plan_request(
            &req,
            None,
            ScanControl {
                cancel: cancel.flag(),
                progress: &mut |_| {},
            },
        )
        .unwrap();
    let _ = engine.run_operation(&journal, &planned.plan, &mut SkipErrors, &Cancel::new());
    std::fs::write(&marker, b"finished").unwrap();
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

fn crash_at(point: &str) -> (usize, usize) {
    let sb = Sandbox::new();
    let members = vec![
        Member::file("a.txt", "alpha"),
        Member::file("b.bin", big()),
        Member::file("c.txt", "gamma"),
        Member::dir("sub"),
        Member::file("sub/d.txt", "delta"),
        Member::file("sub/e.txt", "epsilon"),
    ];
    let archive = sb.path("in/data.tar.gz");
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    write_tar(&archive, &members, Compression::Gzip);
    let original = snapshot(&sb.path("in"));
    let dest = sb.path("OUT");
    std::fs::create_dir_all(&dest).unwrap();
    let marker = sb.root.join("marker");

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash_child_archive",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("RADA_CRASH_POINT", point)
        .env("RADA_CRASH_ROOT", &sb.root)
        .env("RADA_CRASH_SRC", &archive)
        .env("RADA_CRASH_DEST", &dest)
        .env("RADA_CRASH_MARKER", &marker)
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

    // What a reader sees right after the kill: only complete files carry final names.
    for (name, want) in [("a.txt", &b"alpha"[..]), ("c.txt", b"gamma")] {
        if let Ok(got) = std::fs::read(dest.join(name)) {
            assert_eq!(got, want, "{point}: {name} is whole or absent");
        }
    }
    if let Ok(got) = std::fs::read(dest.join("b.bin")) {
        assert_eq!(got.len(), 24 * MIB, "{point}: b.bin is whole or absent");
    }
    let temps_before = leftovers(&dest).len();

    // The next start.
    let e = sb.engine();
    let j = sb.journal();
    assert_eq!(j.interrupted().unwrap().len(), 1, "{point}: journal knows");
    let rec = e.recover_interrupted(&j).unwrap();
    assert_eq!(rec.len(), 1, "{point}: {rec:?}");
    assert!(
        leftovers(&dest).is_empty(),
        "{point}: {:?}",
        leftovers(&dest)
    );
    let entry = j.entries().unwrap().pop().unwrap();
    assert_eq!(entry.status, EntryStatus::Finished(RunStatus::Interrupted));

    let (up, rep) = undo(&e, &j, &entry.id);
    assert!(
        up.blocked.is_empty() && rep.failed.is_empty(),
        "{point}: {:?} {:?}",
        up.blocked,
        rep.failed
    );
    assert!(
        snapshot(&dest).is_empty(),
        "{point}: left {:?}",
        snapshot(&dest).keys().collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot(&sb.path("in")),
        original,
        "{point}: the archive is untouched"
    );
    let _ = std::io::stdout().flush();
    (rec[0].adopted.len(), temps_before)
}

#[test]
fn killed_in_the_middle_of_a_big_member_the_temporary_file_goes_and_nothing_is_half_written() {
    let (adopted, temps) = crash_at("mid:OUT");
    assert!(temps >= 1, "the kill really left a temporary file");
    assert_eq!(
        adopted, 0,
        "a half-written member never counts as extracted"
    );
}

#[test]
fn killed_just_after_a_rename_the_finished_member_is_adopted() {
    let (adopted, _) = crash_at("after_rename:OUT/c.txt");
    assert_eq!(adopted, 1);
}

#[test]
fn killed_before_a_member_inside_a_folder_is_coherent() {
    crash_at("before:OUT/sub/e.txt");
}
