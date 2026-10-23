//! Executing a plan: byte progress, per-step error handling, no silent partial states.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::engine::Engine;
use super::plan::*;
use crate::archive::{self, ArchiveError, ListControl, Session};
use crate::fs::local::{CopyFault, copy_fault};
use crate::fs::{CopyControl, CopyRequest, Fingerprint, FsMeta, Stamp};
use crate::platform::{Preserve, Tag, TrashedItem};
use crate::{Error, Result, display};

/// Cooperative cancellation shared between the UI and a worker.
#[derive(Clone, Default, Debug)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn flag(&self) -> &AtomicBool {
        &self.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub steps_done: u64,
    pub steps_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub current: PathBuf,
    pub current_done: u64,
    pub current_total: u64,
}

impl Progress {
    /// 0.0..=1.0, by bytes when there are bytes to move, by steps otherwise.
    pub fn fraction(&self) -> f64 {
        if self.bytes_total > 0 {
            (self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0)
        } else if self.steps_total > 0 {
            (self.steps_done as f64 / self.steps_total as f64).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorChoice {
    /// Skip this step and carry on with the rest.
    Skip,
    /// Skip this and every later failure without asking again.
    SkipAll,
    Retry,
    /// Stop here. What already happened stays in the journal and can be undone.
    Abort,
}

pub struct Failure<'a> {
    pub index: usize,
    pub step: &'a Step,
    pub error: &'a Error,
    pub attempt: u32,
}

pub trait ExecHandler {
    fn progress(&mut self, _p: &Progress) {}
    /// Decide what to do about a failed step. The default is to skip it.
    fn on_failure(&mut self, _f: &Failure<'_>) -> ErrorChoice {
        ErrorChoice::Skip
    }
    /// Called before a step starts (once, not on retries): the journal writes its intent here.
    fn step_starting(&mut self, _index: usize, _step: &Step) {}
    /// Called after every successful step (the journal hooks in here).
    fn step_finished(&mut self, _index: usize, _step: &Step, _result: &StepResult) {}
}

/// A handler that never asks: skip every failure.
pub struct SkipErrors;
impl ExecHandler for SkipErrors {}

#[derive(Clone, Debug)]
pub struct FailedStep {
    pub index: usize,
    pub description: String,
    pub error: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Completed,
    CompletedWithProblems,
    Aborted,
    Cancelled,
    /// The process died mid-operation; the next start settled what it could.
    Interrupted,
}

#[derive(Clone, Debug, Default)]
pub struct ExecReport {
    pub done: u64,
    pub noop: u64,
    pub kept: Vec<(PathBuf, String)>,
    pub skipped_dependent: u64,
    pub failed: Vec<FailedStep>,
    pub bytes: u64,
    pub aborted: bool,
    pub cancelled: bool,
    /// What could not be carried over to a copy (owner, extended attributes, ACLs, a hard
    /// link the destination cannot make), with the item it concerns.
    pub not_preserved: Vec<(PathBuf, String)>,
}

impl ExecReport {
    pub fn status(&self) -> RunStatus {
        if self.cancelled {
            RunStatus::Cancelled
        } else if self.aborted {
            RunStatus::Aborted
        } else if self.failed.is_empty() && self.skipped_dependent == 0 {
            RunStatus::Completed
        } else {
            RunStatus::CompletedWithProblems
        }
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_sibling(dst: &Path) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    // Short and independent of the target name, so a 255-byte name still fits.
    dst.with_file_name(format!(".rada-part-{}-{n}", std::process::id()))
}

struct RunState {
    created_dirs: HashSet<PathBuf>,
    blocked: HashSet<PathBuf>,
    skip_all: bool,
    /// This run is an undo: it may unlock a read-only folder to take out what the operation
    /// had put in it.
    undo: bool,
    /// Permissions to put back when the run is over, oldest first (applied newest first, so
    /// that a folder is locked again only after what is inside it is done).
    deferred_modes: Vec<(PathBuf, u32)>,
    /// Folders that got new names (extracted files): written to the disk once, when the run
    /// is over, and not after each file.
    unsynced_dirs: std::collections::BTreeSet<PathBuf>,
    /// For each archive the plan reads: the members it will ask for, so that one pass over
    /// the archive serves them all.
    archive_wanted: std::collections::HashMap<PathBuf, Vec<usize>>,
    archives: std::collections::HashMap<PathBuf, ArchiveRun>,
}

/// An archive being extracted from: its list, and the one pass over its data.
struct ArchiveRun {
    index: Arc<archive::Index>,
    session: Session,
}

impl RunState {
    /// A step is pointless when its destination lies under something that failed.
    fn blocked_by(&self, step: &Step) -> bool {
        if self.blocked.is_empty() {
            return false;
        }
        let paths = [step.destination(), Some(step.path())];
        paths
            .into_iter()
            .flatten()
            .any(|p| p.ancestors().any(|a| self.blocked.contains(a)))
    }
}

/// What a copied file is given besides its data.
struct FileAttrs {
    mode: Option<u32>,
    mtime: Option<Stamp>,
    atime: Option<Stamp>,
    owner: Option<(u32, u32)>,
}

/// How a copy goes: checked afterwards, ending a move, one name of several hard links.
#[derive(Clone, Copy)]
struct CopyOptions {
    verify: bool,
    remove_source: bool,
    link_primary: bool,
}

/// The archive member an extraction reads, with the size the archive declares for it.
struct ArchiveMember<'a> {
    src: &'a Path,
    archive: &'a Path,
    entry: usize,
    size: u64,
}

/// Progress/cancel adapter handed to the filesystem copy loop.
struct Ctl<'a> {
    handler: &'a mut dyn ExecHandler,
    cancel: &'a Cancel,
    prog: &'a mut Progress,
    base_bytes: u64,
    last_emit: &'a mut Instant,
}

impl CopyControl for Ctl<'_> {
    fn advance(&mut self, n: u64) -> bool {
        if self.cancel.is_cancelled() {
            return false;
        }
        self.prog.current_done += n;
        let shown = if self.prog.current_total > 0 {
            self.prog.current_done.min(self.prog.current_total)
        } else {
            self.prog.current_done
        };
        self.prog.bytes_done = self.base_bytes + shown;
        if self.last_emit.elapsed() >= Duration::from_millis(40) {
            self.handler.progress(self.prog);
            *self.last_emit = Instant::now();
        }
        true
    }
}

impl Engine {
    /// Run every step of `plan` in order.
    pub fn execute(
        &self,
        plan: &Plan,
        handler: &mut dyn ExecHandler,
        cancel: &Cancel,
    ) -> ExecReport {
        let mut report = ExecReport::default();
        let mut st = RunState {
            created_dirs: HashSet::new(),
            blocked: HashSet::new(),
            skip_all: false,
            undo: plan.kind == OpKind::Undo,
            deferred_modes: Vec::new(),
            unsynced_dirs: Default::default(),
            archive_wanted: archive_members(plan),
            archives: Default::default(),
        };
        let mut prog = Progress {
            steps_total: plan.steps.len() as u64,
            bytes_total: plan.total_bytes(),
            ..Default::default()
        };
        let mut base_bytes = 0u64;
        let mut last_emit = Instant::now();
        handler.progress(&prog);

        'steps: for (index, step) in plan.steps.iter().enumerate() {
            if cancel.is_cancelled() {
                report.cancelled = true;
                break;
            }
            if st.blocked_by(step) {
                report.skipped_dependent += 1;
                if let Step::MakeDir { path, .. } = step {
                    st.blocked.insert(path.clone());
                }
                base_bytes += step.bytes();
                prog.steps_done += 1;
                prog.bytes_done = base_bytes;
                continue;
            }
            prog.current = step.path().to_path_buf();
            prog.current_done = 0;
            prog.current_total = step.bytes();
            prog.bytes_done = base_bytes;
            handler.progress(&prog);

            handler.step_starting(index, step);
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                prog.current_done = 0;
                let outcome = {
                    let mut ctl = Ctl {
                        handler: &mut *handler,
                        cancel,
                        prog: &mut prog,
                        base_bytes,
                        last_emit: &mut last_emit,
                    };
                    self.run_step(step, &mut st, &mut ctl)
                };
                match outcome {
                    Ok(res) => {
                        match res.status {
                            StepStatus::Done => {
                                report.done += 1;
                                report.bytes += step.bytes();
                                if matches!(step, Step::CopyFile { .. } | Step::ExtractFile { .. })
                                {
                                    prog.files_done += 1;
                                }
                            }
                            StepStatus::NoOp => report.noop += 1,
                            StepStatus::Kept => {
                                report.kept.push((
                                    step.path().to_path_buf(),
                                    res.note.clone().unwrap_or_default(),
                                ));
                            }
                        }
                        if !res.not_preserved.is_empty() {
                            let at = step
                                .destination()
                                .unwrap_or_else(|| step.path())
                                .to_path_buf();
                            for what in &res.not_preserved {
                                tracing::warn!("{}: {what} was not preserved", at.display());
                                report.not_preserved.push((at.clone(), what.clone()));
                            }
                        }
                        handler.step_finished(index, step, &res);
                        break;
                    }
                    Err(Error::Cancelled) => {
                        report.cancelled = true;
                        break 'steps;
                    }
                    Err(error) => {
                        let choice = if st.skip_all {
                            ErrorChoice::Skip
                        } else {
                            handler.on_failure(&Failure {
                                index,
                                step,
                                error: &error,
                                attempt,
                            })
                        };
                        match choice {
                            ErrorChoice::Retry => continue,
                            ErrorChoice::Abort => {
                                report.failed.push(failed(index, step, &error));
                                report.aborted = true;
                                break 'steps;
                            }
                            ErrorChoice::Skip | ErrorChoice::SkipAll => {
                                if choice == ErrorChoice::SkipAll {
                                    st.skip_all = true;
                                }
                                report.failed.push(failed(index, step, &error));
                                match step {
                                    Step::MakeDir { path, .. } | Step::TrashItem { path } => {
                                        st.blocked.insert(path.clone());
                                    }
                                    _ => {}
                                }
                                break;
                            }
                        }
                    }
                }
            }
            base_bytes += step.bytes();
            prog.steps_done += 1;
            prog.bytes_done = base_bytes;
        }
        for dir in &st.unsynced_dirs {
            if let Err(e) = self.fs.sync_dir(dir) {
                tracing::debug!("sync {}: {e}", dir.display());
            }
        }
        // Folders that were opened up for the run get their own permissions back.
        for (path, mode) in st.deferred_modes.iter().rev() {
            if let Err(e) = self.fs.set_mode(path, *mode) {
                tracing::debug!("restore permissions of {}: {e}", path.display());
            }
        }
        handler.progress(&prog);
        report
    }

    /// Run `op` on `path`; if an undo is refused because the folder holding it is read-only,
    /// open the folder up (and put its permissions back when the run is over) and try again.
    fn retry_unlocked<T>(
        &self,
        st: &mut RunState,
        path: &Path,
        op: impl Fn() -> io::Result<T>,
    ) -> io::Result<T> {
        match op() {
            Err(e) if st.undo && crate::error::is_denied(&e) => {
                let Some(parent) = path.parent() else {
                    return Err(e);
                };
                let Some(mode) = self.fs.lstat(parent).ok().and_then(|m| m.mode) else {
                    return Err(e);
                };
                if self.fs.set_mode(parent, (mode & 0o7777) | 0o300).is_err() {
                    return Err(e);
                }
                st.deferred_modes
                    .push((parent.to_path_buf(), mode & 0o7777));
                op()
            }
            r => r,
        }
    }

    /// The pass over `archive` that serves this plan, started on first use.
    fn archive_run<'a>(&self, st: &'a mut RunState, archive: &Path) -> Result<&'a mut ArchiveRun> {
        if !st.archives.contains_key(archive) {
            let cancel = never;
            let index = archive::index::open_cached(archive, &mut ListControl::new(&cancel))
                .map_err(|e| e.into_error(archive))?;
            let wanted = st.archive_wanted.get(archive).cloned().unwrap_or_default();
            let session = Session::open(index.clone(), &wanted);
            st.archives
                .insert(archive.to_path_buf(), ArchiveRun { index, session });
        }
        Ok(st.archives.get_mut(archive).expect("just inserted"))
    }

    fn extract_file(
        &self,
        st: &mut RunState,
        ctl: &mut Ctl<'_>,
        member: ArchiveMember<'_>,
        dst: &Path,
        mode: Option<u32>,
        mtime: Option<Stamp>,
    ) -> Result<StepResult> {
        let ArchiveMember {
            src,
            archive,
            entry,
            size,
        } = member;
        let fs = &*self.fs;
        self.require_free(dst)?;
        let inner = src.strip_prefix(archive).unwrap_or(src);
        let run = self.archive_run(st, archive)?;
        // The list the plan was made from must still be the archive's list.
        if run.index.entries.get(entry).is_none_or(|e| e.path != inner) {
            return Err(Error::Archive {
                path: archive.to_path_buf(),
                message: "the archive changed since the plan was made".into(),
            });
        }
        // A device takes the data when the file is closed and cannot rename it: see CopyFile.
        let device = self.platform.on_device(dst);
        let tmp = if device {
            dst.to_path_buf()
        } else {
            temp_sibling(dst)
        };
        let mut sink = fs
            .create_file(&tmp, 0o600)
            .map_err(|e| Error::io("create", dst, e))?;
        enum Stop {
            No,
            Write(io::Error),
            TooBig,
            Cancelled,
        }
        let mut stop = Stop::No;
        let mut written = 0u64;
        let r = run.session.read(entry, &mut |chunk| {
            written += chunk.len() as u64;
            if written > size {
                stop = Stop::TooBig;
                return Ok(false);
            }
            if let Err(e) = io::Write::write_all(&mut sink, chunk) {
                stop = Stop::Write(e);
                return Ok(false);
            }
            if !ctl.advance(chunk.len() as u64) {
                stop = Stop::Cancelled;
                return Ok(false);
            }
            Ok(true)
        });
        drop(sink);
        let fail = |e: Error| {
            let _ = fs.remove_file(&tmp);
            Err(e)
        };
        match (stop, r) {
            (Stop::Write(e), _) => return fail(Error::io("write", dst, e)),
            (Stop::TooBig, _) => {
                return fail(Error::Archive {
                    path: src.to_path_buf(),
                    message: format!(
                        "it holds more than the {size} bytes the archive declares for it (an archive bomb?)"
                    ),
                });
            }
            (Stop::Cancelled, _) | (_, Err(ArchiveError::Cancelled)) => {
                return fail(Error::Cancelled);
            }
            (Stop::No, Err(e)) => return fail(e.into_error(src)),
            (Stop::No, Ok(n)) if n != size => {
                return fail(Error::Archive {
                    path: src.to_path_buf(),
                    message: format!("it holds {n} bytes, but the archive declares {size}"),
                });
            }
            (Stop::No, Ok(_)) => {}
        }
        if let Some(t) = mtime
            && let Err(e) = fs.set_mtime(&tmp, t.into(), None)
        {
            tracing::debug!("set time of {}: {e}", tmp.display());
        }
        if let Some(m) = mode
            && !device
            && let Err(e) = fs.set_mode(&tmp, m)
        {
            return fail(Error::io("set permissions of", dst, e));
        }
        if device {
            // What the device kept is what was sent, or this is not an extraction.
            match fs.lstat(&tmp) {
                Ok(m) if m.size == size => {}
                Ok(m) => {
                    return fail(Error::io(
                        "write",
                        dst,
                        io::Error::other(format!("the device kept {} of {size} bytes", m.size)),
                    ));
                }
                Err(e) => return fail(Error::io("write", dst, e)),
            }
        } else {
            // The bytes reach the disk before the name does: the journal will say "done" for
            // this file, and after a power cut the name must not stand on an empty file.
            if let Err(e) = fs.sync_file(&tmp)
                && e.kind() != io::ErrorKind::Unsupported
            {
                return fail(Error::io("write", dst, e));
            }
        }
        if tmp != dst
            && let Err(e) = fs.rename_noreplace(&tmp, dst)
        {
            let _ = fs.remove_file(&tmp);
            return Err(if e.kind() == io::ErrorKind::AlreadyExists {
                Error::AlreadyExists(dst.to_path_buf())
            } else {
                Error::io2("finish extraction", &tmp, dst, e)
            });
        }
        if let Some(parent) = dst.parent()
            && !device
        {
            st.unsynced_dirs.insert(parent.to_path_buf());
        }
        let after = fs.lstat(dst).ok().map(|m| m.fingerprint());
        Ok(StepResult {
            after,
            ..StepResult::done()
        })
    }

    fn compress(
        &self,
        ctl: &mut Ctl<'_>,
        dst: &Path,
        format: archive::ArchiveKind,
        items: &[archive::writer::CompressItem],
    ) -> Result<StepResult> {
        let fs = &*self.fs;
        self.require_free(dst)?;
        let tmp = temp_sibling(dst);
        let sink = fs
            .create_file(&tmp, 0o666)
            .map_err(|e| Error::io("create", dst, e))?;
        let mut cancelled = false;
        let r = archive::writer::write_archive(format, sink, items, &mut |n| {
            let go = ctl.advance(n);
            cancelled |= !go;
            go
        });
        match r {
            Ok(()) => {}
            Err(e) => {
                let _ = fs.remove_file(&tmp);
                return Err(match e {
                    ArchiveError::Cancelled => Error::Cancelled,
                    _ if cancelled => Error::Cancelled,
                    ArchiveError::Io(e) => Error::io("write", dst, e),
                    other => other.into_error(dst),
                });
            }
        }
        if let Err(e) = fs.rename_noreplace(&tmp, dst) {
            let _ = fs.remove_file(&tmp);
            return Err(if e.kind() == io::ErrorKind::AlreadyExists {
                Error::AlreadyExists(dst.to_path_buf())
            } else {
                Error::io2("finish archive", &tmp, dst, e)
            });
        }
        let after = fs.lstat(dst).ok().map(|m| m.fingerprint());
        Ok(StepResult {
            after,
            ..StepResult::done()
        })
    }

    fn run_step(&self, step: &Step, st: &mut RunState, ctl: &mut Ctl<'_>) -> Result<StepResult> {
        match step {
            Step::MakeDir {
                path,
                mode,
                restore_mode,
            } => self.make_dir(st, path, *mode, *restore_mode),
            Step::MakeFile { path, mode } => self.make_file(path, *mode),
            Step::FinishDir {
                path,
                mode,
                mtime,
                src,
                uid,
                gid,
            } => self.finish_dir(st, path, *mode, *mtime, src.as_deref(), uid.zip(*gid)),
            Step::CopyFile {
                src,
                dst,
                mode,
                mtime,
                atime,
                verify,
                remove_source,
                uid,
                gid,
                link_primary,
                ..
            } => {
                let attrs = FileAttrs {
                    mode: *mode,
                    mtime: *mtime,
                    atime: *atime,
                    owner: uid.zip(*gid),
                };
                let options = CopyOptions {
                    verify: *verify,
                    remove_source: *remove_source,
                    link_primary: *link_primary,
                };
                self.copy_file(ctl, src, dst, attrs, options)
            }
            Step::HardLink {
                existing,
                link,
                src_link,
                ..
            } => self.hard_link(ctl, existing, link, src_link.as_deref()),
            Step::CopySymlink {
                src,
                dst,
                target,
                remove_source,
            } => self.copy_symlink(src, dst, target, *remove_source),
            Step::ExtractFile {
                src,
                archive,
                entry,
                dst,
                size,
                mode,
                mtime,
            } => {
                let member = ArchiveMember {
                    src,
                    archive,
                    entry: *entry,
                    size: *size,
                };
                self.extract_file(st, ctl, member, dst, *mode, *mtime)
            }
            Step::ExtractSymlink { dst, target, .. } => self.make_symlink(dst, target),
            Step::Compress {
                dst, format, items, ..
            } => self.compress(ctl, dst, *format, items),
            Step::Rename { from, to } => self.rename(from, to),
            Step::TrashItem { path } => self.trash_item(path),
            Step::Restore { item } => self.restore_item(item),
            Step::RemoveFile { path, expect } => self.remove_file(st, path, expect.as_ref()),
            Step::RemoveDir { path } => self.remove_dir(st, path),
            Step::SetTags {
                path,
                before,
                after,
            } => self.set_tags(path, before, after),
        }
    }

    fn trash_item(&self, path: &Path) -> Result<StepResult> {
        let item = self.platform.trash().trash(path)?;
        Ok(StepResult {
            trashed: Some(item),
            ..StepResult::done()
        })
    }

    fn restore_item(&self, item: &TrashedItem) -> Result<StepResult> {
        self.platform.trash().restore(item)?;
        Ok(StepResult::done())
    }

    /// `None` when nothing is there; any other failure to look is an error.
    fn inspect(&self, path: &Path) -> Result<Option<FsMeta>> {
        match self.fs.lstat(path) {
            Ok(meta) => Ok(Some(meta)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io("inspect", path, e)),
        }
    }

    fn require_free(&self, path: &Path) -> Result<()> {
        match self.inspect(path)? {
            None => Ok(()),
            Some(_) => Err(Error::AlreadyExists(path.to_path_buf())),
        }
    }

    fn fingerprint_after(&self, path: &Path) -> Option<Fingerprint> {
        self.fs.lstat(path).ok().map(|m| m.fingerprint())
    }

    fn make_dir(
        &self,
        st: &mut RunState,
        path: &Path,
        mode: Option<u32>,
        restore_mode: Option<u32>,
    ) -> Result<StepResult> {
        match self.fs.create_dir(path, mode) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(Error::AlreadyExists(path.to_path_buf()));
            }
            Err(e) => return Err(Error::io("create folder", path, e)),
        }
        st.created_dirs.insert(path.to_path_buf());
        if let Some(m) = restore_mode {
            st.deferred_modes.push((path.to_path_buf(), m));
        }
        Ok(StepResult {
            created: true,
            ..StepResult::done()
        })
    }

    fn make_file(&self, path: &Path, mode: Option<u32>) -> Result<StepResult> {
        match self.fs.create_file(path, mode.unwrap_or(0o644)) {
            Ok(_sink) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(Error::AlreadyExists(path.to_path_buf()));
            }
            Err(e) => return Err(Error::io("create file", path, e)),
        }
        Ok(StepResult {
            after: self.fingerprint_after(path),
            ..StepResult::done()
        })
    }

    fn finish_dir(
        &self,
        st: &RunState,
        path: &Path,
        mode: Option<u32>,
        mtime: Option<Stamp>,
        src: Option<&Path>,
        owner: Option<(u32, u32)>,
    ) -> Result<StepResult> {
        if !st.created_dirs.contains(path) {
            return Ok(StepResult::noop("folder was not created by this run"));
        }
        // Owner and extended attributes first (the folder is still writable), then mode, then time.
        let device = self.platform.on_device(path);
        let not_preserved = self.copy_folder_attributes(path, src, owner, device);
        if let Some(m) = mode {
            // A folder on a device has no permissions to set: the plan said so.
            if let Err(e) = self.fs.set_mode(path, m)
                && !device
            {
                return Err(Error::io("set permissions of", path, e));
            }
        }
        if let Some(t) = mtime {
            // Best effort: a wrong folder time must not fail the operation.
            if let Err(e) = self.fs.set_mtime(path, t.into(), None) {
                tracing::debug!("set time of {}: {e}", path.display());
            }
        }
        Ok(StepResult {
            not_preserved,
            ..StepResult::done()
        })
    }

    fn copy_folder_attributes(
        &self,
        path: &Path,
        src: Option<&Path>,
        owner: Option<(u32, u32)>,
        device: bool,
    ) -> Vec<String> {
        match src {
            Some(src) if !device && !self.platform.on_device(src) => {
                self.platform
                    .fidelity()
                    .copy_attributes(src, path, Preserve::default(), owner)
                    .not_preserved
            }
            _ => Vec::new(),
        }
    }

    fn copy_file(
        &self,
        ctl: &mut Ctl<'_>,
        src: &Path,
        dst: &Path,
        attrs: FileAttrs,
        options: CopyOptions,
    ) -> Result<StepResult> {
        self.require_free(dst)?;
        let to_device = self.platform.on_device(dst);
        let device = to_device || self.platform.on_device(src);
        // A device has no half-written file to hide and no rename to trust: it gets the real name.
        let tmp = if to_device {
            dst.to_path_buf()
        } else {
            temp_sibling(dst)
        };
        let req = CopyRequest {
            src: src.to_path_buf(),
            dst: tmp,
            mode: attrs.mode.map(|m| m | 0o200), // writable until the attributes are set
            mtime: attrs.mtime.map(Into::into),
            atime: attrs.atime.map(Into::into),
            verify: options.verify,
            sync: options.remove_source,
            device,
        };
        self.copy_data(ctl, &req, dst)?;
        let not_preserved = self.finish_copy(&req, dst, to_device, attrs.mode, attrs.owner)?;
        Ok(StepResult {
            after: self.settle_source(src, dst, options)?,
            not_preserved,
            ..StepResult::done()
        })
    }

    fn copy_data(&self, ctl: &mut Ctl<'_>, req: &CopyRequest, dst: &Path) -> Result<()> {
        let Err(e) = self.fs.copy_file(req, ctl) else {
            return Ok(());
        };
        Err(match copy_fault(&e) {
            Some(CopyFault::Cancelled) => Error::Cancelled,
            Some(CopyFault::SourceChanged) => Error::SourceChanged(req.src.clone()),
            Some(CopyFault::VerifyMismatch(why)) => Error::VerifyFailed {
                path: dst.to_path_buf(),
                reason: why.clone(),
            },
            None => Error::io2("copy", &req.src, dst, e),
        })
    }

    /// What is left after the data is in: attributes, permissions, the final name.
    fn finish_copy(
        &self,
        req: &CopyRequest,
        dst: &Path,
        to_device: bool,
        mode: Option<u32>,
        owner: Option<(u32, u32)>,
    ) -> Result<Vec<String>> {
        let (fs, tmp) = (&*self.fs, &req.dst);
        // Whatever cannot be carried over is reported, never an error.
        let not_preserved = if req.device {
            Vec::new()
        } else {
            self.platform
                .fidelity()
                .copy_attributes(&req.src, tmp, Preserve::default(), owner)
                .not_preserved
        };
        if let Some(m) = mode
            && !to_device
            && let Err(e) = fs.set_mode(tmp, m)
        {
            let _ = fs.remove_file(tmp);
            return Err(Error::io("set permissions of", dst, e));
        }
        if tmp != dst
            && let Err(e) = fs.rename_noreplace(tmp, dst)
        {
            let _ = fs.remove_file(tmp);
            return Err(if e.kind() == io::ErrorKind::AlreadyExists {
                Error::AlreadyExists(dst.to_path_buf())
            } else {
                Error::io2("finish copy", tmp, dst, e)
            });
        }
        Ok(not_preserved)
    }

    /// The fingerprint of the copy, and then the end of the source when the copy was a move.
    fn settle_source(
        &self,
        src: &Path,
        dst: &Path,
        options: CopyOptions,
    ) -> Result<Option<Fingerprint>> {
        let fs = &*self.fs;
        let after = fs.lstat(dst).ok().map(|m| {
            if options.link_primary {
                m.fingerprint_without_ctime()
            } else {
                m.fingerprint()
            }
        });
        if options.remove_source
            && let Err(e) = fs.remove_file(src)
            // Someone else already removed the source: the move is what they wanted.
            && e.kind() != io::ErrorKind::NotFound
        {
            // Never leave the item in two places: undo the copy.
            let _ = fs.remove_file(dst);
            return Err(Error::io("remove source after copy", src, e));
        }
        Ok(after)
    }

    fn hard_link(
        &self,
        ctl: &mut Ctl<'_>,
        existing: &Path,
        link: &Path,
        src_link: Option<&Path>,
    ) -> Result<StepResult> {
        let fs = &*self.fs;
        self.require_free(link)?;
        let mut not_preserved = Vec::new();
        match fs.hard_link(existing, link) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(Error::AlreadyExists(link.to_path_buf()));
            }
            // No hard links here: the name gets its own copy, and the report says so.
            Err(e) if crate::error::is_link_unsupported(&e) => {
                self.copy_instead_of_link(ctl, existing, link)?;
                not_preserved.push(
                    "hard link (the destination cannot make links: copied as a separate file)"
                        .to_string(),
                );
            }
            Err(e) => return Err(Error::io2("link", existing, link, e)),
        }
        let after = self.fingerprint_after(link);
        if let Some(s) = src_link
            && let Err(e) = fs.remove_file(s)
        {
            let _ = fs.remove_file(link);
            return Err(Error::io("remove source link", s, e));
        }
        Ok(StepResult {
            after,
            not_preserved,
            ..StepResult::done()
        })
    }

    fn copy_instead_of_link(&self, ctl: &mut Ctl<'_>, existing: &Path, link: &Path) -> Result<()> {
        let fs = &*self.fs;
        let meta = fs
            .lstat(existing)
            .map_err(|e| Error::io("inspect", existing, e))?;
        let tmp = temp_sibling(link);
        let req = CopyRequest {
            src: existing.to_path_buf(),
            dst: tmp.clone(),
            mode: meta.mode.map(|m| m | 0o200),
            mtime: meta.mtime,
            atime: meta.atime,
            verify: false,
            sync: false,
            device: self.platform.on_device(existing) || self.platform.on_device(link),
        };
        fs.copy_file(&req, ctl)
            .map_err(|e| Error::io2("copy", existing, link, e))?;
        if let Some(m) = meta.mode {
            let _ = fs.set_mode(&tmp, m);
        }
        if let Err(e) = fs.rename_noreplace(&tmp, link) {
            let _ = fs.remove_file(&tmp);
            return Err(Error::io2("finish copy", &tmp, link, e));
        }
        Ok(())
    }

    fn copy_symlink(
        &self,
        src: &Path,
        dst: &Path,
        target: &Path,
        remove_source: bool,
    ) -> Result<StepResult> {
        let made = self.make_symlink(dst, target)?;
        if remove_source
            && let Err(e) = self.fs.remove_file(src)
            && e.kind() != io::ErrorKind::NotFound
        {
            let _ = self.fs.remove_file(dst);
            return Err(Error::io("remove source link", src, e));
        }
        Ok(made)
    }

    fn make_symlink(&self, dst: &Path, target: &Path) -> Result<StepResult> {
        self.require_free(dst)?;
        self.fs.create_symlink(target, dst).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                Error::AlreadyExists(dst.to_path_buf())
            } else {
                Error::io("create link", dst, e)
            }
        })?;
        Ok(StepResult {
            after: self.fingerprint_after(dst),
            ..StepResult::done()
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<StepResult> {
        match self.fs.rename_noreplace(from, to) {
            Ok(()) => Ok(StepResult::done()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                Err(Error::AlreadyExists(to.to_path_buf()))
            }
            Err(e) if crate::error::is_exdev(&e) => {
                Err(Error::CrossDevice(from.to_path_buf(), to.to_path_buf()))
            }
            Err(e) => Err(Error::io2("rename", from, to, e)),
        }
    }

    fn remove_file(
        &self,
        st: &mut RunState,
        path: &Path,
        expect: Option<&Fingerprint>,
    ) -> Result<StepResult> {
        let Some(meta) = self.inspect(path)? else {
            return Ok(StepResult::noop("already gone"));
        };
        if let Some(exp) = expect
            && let Some(reason) = exp.diff(&meta.fingerprint())
        {
            return Err(Error::Modified {
                path: path.to_path_buf(),
                reason,
            });
        }
        self.retry_unlocked(st, path, || self.fs.remove_file(path))
            .map_err(|e| Error::io("delete", path, e))?;
        Ok(StepResult::done())
    }

    fn remove_dir(&self, st: &mut RunState, path: &Path) -> Result<StepResult> {
        let Some(meta) = self.inspect(path)? else {
            return Ok(StepResult::noop("already gone"));
        };
        match self.retry_unlocked(st, path, || self.fs.remove_dir(path)) {
            Ok(()) => Ok(StepResult {
                removed_dir_mode: meta.mode,
                ..StepResult::done()
            }),
            Err(e) if crate::error::is_not_empty(&e) => Ok(StepResult::kept(format!(
                "{} was kept because it is not empty",
                display::path(path)
            ))),
            Err(e) => Err(Error::io("remove folder", path, e)),
        }
    }

    fn set_tags(&self, path: &Path, before: &[Tag], after: &[Tag]) -> Result<StepResult> {
        let store = self
            .platform
            .tags()
            .ok_or(Error::Unsupported("tags on this system"))?;
        // What the item has now must be what the plan saw: tags changed since are not overwritten.
        if store.read(path)? != before {
            return Err(Error::Modified {
                path: path.to_path_buf(),
                reason: "its tags changed".into(),
            });
        }
        store.write(path, after)?;
        Ok(StepResult::done())
    }
}

/// The archive members a plan asks for, by archive.
fn archive_members(plan: &Plan) -> std::collections::HashMap<PathBuf, Vec<usize>> {
    let mut out: std::collections::HashMap<PathBuf, Vec<usize>> = Default::default();
    for step in &plan.steps {
        if let Step::ExtractFile { archive, entry, .. } = step {
            out.entry(archive.clone()).or_default().push(*entry);
        }
    }
    out
}

fn never() -> bool {
    false
}

fn failed(index: usize, step: &Step, error: &Error) -> FailedStep {
    FailedStep {
        index,
        description: step.describe(),
        error: error.to_string(),
        path: step.path().to_path_buf(),
    }
}
