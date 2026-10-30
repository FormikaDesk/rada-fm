//! Renaming a file so that only the case of its name changes (`Report.txt` -> `report.txt`)
//! where the filesystem ignores case (macOS, Windows): the new name is the same file, not a
//! conflict, in a rename, in a bulk rename and in the undo of both.
//!
//! A stand-in filesystem ignores case on top of the real one, so that this runs everywhere;
//! `on_the_real_filesystem` is the same on whatever the machine has (a Mac's and Windows'
//! really do ignore case).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use rada_core::fs::{
    CopyControl, CopyOutcome, CopyRequest, FileSink, FsEngine, FsMeta, Listing, LocalFs,
};
use rada_core::ops::*;
use rada_core::testplatform::WithPathRules;
use rada_core::testutil::*;

/// The local filesystem as a volume that ignores case: a name finds the entry whose name is
/// the same but for case, and a name that is taken that way is taken.
struct FoldFs(LocalFs);

impl FoldFs {
    /// `p` with its last name as the folder spells it, if the folder has one like it.
    fn folded(&self, p: &Path) -> PathBuf {
        let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else {
            return p.to_path_buf();
        };
        let want = name.to_string_lossy().to_lowercase();
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().to_lowercase() == want)
            .map_or_else(|| p.to_path_buf(), |e| e.path())
    }
}

impl FsEngine for FoldFs {
    fn lstat(&self, p: &Path) -> io::Result<FsMeta> {
        self.0.lstat(&self.folded(p))
    }
    fn stat(&self, p: &Path) -> io::Result<FsMeta> {
        self.0.stat(&self.folded(p))
    }
    fn read_dir(&self, p: &Path) -> io::Result<Listing> {
        self.0.read_dir(p)
    }
    fn read_link(&self, p: &Path) -> io::Result<PathBuf> {
        self.0.read_link(&self.folded(p))
    }
    fn create_dir(&self, p: &Path, mode: Option<u32>) -> io::Result<()> {
        self.0.create_dir(p, mode)
    }
    fn create_symlink(&self, t: &Path, l: &Path) -> io::Result<()> {
        self.0.create_symlink(t, l)
    }
    fn create_file(&self, p: &Path, mode: u32) -> io::Result<Box<dyn FileSink>> {
        self.0.create_file(p, mode)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.rename(&self.folded(from), to)
    }
    fn rename_noreplace(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.folded(to).exists() {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        self.0.rename_noreplace(&self.folded(from), to)
    }
    fn remove_file(&self, p: &Path) -> io::Result<()> {
        self.0.remove_file(&self.folded(p))
    }
    fn remove_dir(&self, p: &Path) -> io::Result<()> {
        self.0.remove_dir(&self.folded(p))
    }
    fn set_mode(&self, p: &Path, mode: u32) -> io::Result<()> {
        self.0.set_mode(&self.folded(p), mode)
    }
    fn set_mtime(&self, p: &Path, m: SystemTime, a: Option<SystemTime>) -> io::Result<()> {
        self.0.set_mtime(&self.folded(p), m, a)
    }
    fn hard_link(&self, existing: &Path, link: &Path) -> io::Result<()> {
        self.0.hard_link(&self.folded(existing), link)
    }
    fn copy_file(&self, req: &CopyRequest, ctl: &mut dyn CopyControl) -> io::Result<CopyOutcome> {
        self.0.copy_file(req, ctl)
    }
    fn available_space(&self, p: &Path) -> io::Result<u64> {
        self.0.available_space(p)
    }
    fn can_read(&self, p: &Path) -> bool {
        self.0.can_read(&self.folded(p))
    }
    fn can_write(&self, p: &Path) -> bool {
        self.0.can_write(p)
    }
}

fn names_in(sb: &Sandbox, dir: &str) -> Vec<OsString> {
    let mut v: Vec<_> = std::fs::read_dir(sb.path(dir))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    v.sort();
    v
}

fn folding_engine(sb: &Sandbox) -> Engine {
    Engine::new(
        Arc::new(FoldFs(LocalFs)),
        WithPathRules::case_insensitive(sb.platform()),
    )
}

#[test]
fn a_rename_that_changes_only_the_case_is_not_a_conflict_and_is_undone() {
    let sb = Sandbox::new();
    let (e, j) = (folding_engine(&sb), sb.journal());
    sb.write("d/Report.txt", "contents");

    let plan = e.plan_rename(
        &sb.path("d/Report.txt"),
        "report.txt".as_ref(),
        ConflictPolicy::Skip,
    );
    assert!(
        plan.is_executable() && !plan.has_conflicts(),
        "{:?}",
        plan.warnings
    );
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(
        out.report.status(),
        RunStatus::Completed,
        "{:?}",
        out.report.failed
    );
    assert_eq!(names_in(&sb, "d"), ["report.txt"]);

    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(names_in(&sb, "d"), ["Report.txt"]);
}

#[test]
fn a_bulk_rename_to_lower_case_works_and_is_undone() {
    let sb = Sandbox::new();
    let (e, j) = (folding_engine(&sb), sb.journal());
    let files = [sb.write("d/A.TXT", "1"), sb.write("d/Bee.Txt", "2")];
    let plan = e.plan_bulk_rename(&files, &Pattern::parse("{name:lower}.{ext}").unwrap());
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(
        out.report.status(),
        RunStatus::Completed,
        "{:?}",
        out.report.failed
    );
    assert_eq!(names_in(&sb, "d"), ["a.TXT", "bee.Txt"]);

    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(up.plan.steps.len(), 4, "{:#?}", up.plan.steps);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(names_in(&sb, "d"), ["A.TXT", "Bee.Txt"]);
}

#[test]
fn a_name_that_really_is_another_file_is_still_a_conflict() {
    let sb = Sandbox::new();
    let e = folding_engine(&sb);
    sb.write("d/Report.txt", "one");
    sb.write("d/other.txt", "two");
    let plan = e.plan_rename(
        &sb.path("d/Report.txt"),
        "OTHER.txt".as_ref(),
        ConflictPolicy::Skip,
    );
    assert!(plan.has_conflicts() && plan.steps.is_empty());
}

#[test]
fn on_the_real_filesystem() {
    // Whatever this machine's filesystem does about case, the rename works and goes back.
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    sb.write("d/Report.txt", "contents");
    let plan = e.plan_rename(
        &sb.path("d/Report.txt"),
        "report.txt".as_ref(),
        ConflictPolicy::Skip,
    );
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(
        out.report.status(),
        RunStatus::Completed,
        "{:?}",
        out.report.failed
    );
    assert_eq!(names_in(&sb, "d"), ["report.txt"]);
    let (_, rep) = undo(&e, &j, &out.id);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(names_in(&sb, "d"), ["Report.txt"]);
}
