//! Executing a plan: byte progress, per-step error handling, no silent partial states.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::engine::Engine;
use super::plan::*;
use crate::fs::local::{CopyFault, copy_fault};
use crate::fs::{CopyControl, CopyRequest};
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
    dst.with_file_name(format!(".vela-part-{}-{n}", std::process::id()))
}

struct RunState {
    created_dirs: HashSet<PathBuf>,
    blocked: HashSet<PathBuf>,
    skip_all: bool,
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
                                if matches!(step, Step::CopyFile { .. }) {
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
        handler.progress(&prog);
        report
    }

    fn run_step(&self, step: &Step, st: &mut RunState, ctl: &mut Ctl<'_>) -> Result<StepResult> {
        let fs = &*self.fs;
        match step {
            Step::MakeDir { path, mode } => {
                match fs.create_dir(path, *mode) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        return Err(Error::AlreadyExists(path.clone()));
                    }
                    Err(e) => return Err(Error::io("create folder", path, e)),
                }
                st.created_dirs.insert(path.clone());
                Ok(StepResult {
                    created: true,
                    ..StepResult::done()
                })
            }

            Step::FinishDir { path, mode, mtime } => {
                if !st.created_dirs.contains(path) {
                    return Ok(StepResult::noop("folder was not created by this run"));
                }
                if let Some(m) = mode {
                    fs.set_mode(path, *m)
                        .map_err(|e| Error::io("set permissions of", path, e))?;
                }
                if let Some(t) = mtime {
                    // Best effort: a wrong folder time must not fail the operation.
                    if let Err(e) = fs.set_mtime(path, (*t).into(), None) {
                        tracing::debug!("set time of {}: {e}", path.display());
                    }
                }
                Ok(StepResult::done())
            }

            Step::CopyFile {
                src,
                dst,
                mode,
                mtime,
                atime,
                verify,
                remove_source,
                ..
            } => {
                match fs.lstat(dst) {
                    Ok(_) => return Err(Error::AlreadyExists(dst.clone())),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(Error::io("inspect", dst, e)),
                }
                let tmp = temp_sibling(dst);
                let req = CopyRequest {
                    src: src.clone(),
                    dst: tmp.clone(),
                    mode: *mode,
                    mtime: mtime.map(Into::into),
                    atime: atime.map(Into::into),
                    verify: *verify,
                    sync: *remove_source,
                };
                if let Err(e) = fs.copy_file(&req, ctl) {
                    return Err(match copy_fault(&e) {
                        Some(CopyFault::Cancelled) => Error::Cancelled,
                        Some(CopyFault::SourceChanged) => Error::SourceChanged(src.clone()),
                        Some(CopyFault::VerifyMismatch(why)) => Error::VerifyFailed {
                            path: dst.clone(),
                            reason: why.clone(),
                        },
                        None => Error::io2("copy", src, dst, e),
                    });
                }
                if let Err(e) = fs.rename_noreplace(&tmp, dst) {
                    let _ = fs.remove_file(&tmp);
                    return Err(if e.kind() == io::ErrorKind::AlreadyExists {
                        Error::AlreadyExists(dst.clone())
                    } else {
                        Error::io2("finish copy", &tmp, dst, e)
                    });
                }
                let after = fs.lstat(dst).ok().map(|m| m.fingerprint());
                if *remove_source {
                    if let Err(e) = fs.remove_file(src) {
                        // Never leave the item in two places: undo the copy.
                        let _ = fs.remove_file(dst);
                        return Err(Error::io("remove source after copy", src, e));
                    }
                }
                Ok(StepResult {
                    after,
                    ..StepResult::done()
                })
            }

            Step::CopySymlink {
                src,
                dst,
                target,
                remove_source,
            } => {
                match fs.lstat(dst) {
                    Ok(_) => return Err(Error::AlreadyExists(dst.clone())),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(Error::io("inspect", dst, e)),
                }
                fs.create_symlink(target, dst).map_err(|e| {
                    if e.kind() == io::ErrorKind::AlreadyExists {
                        Error::AlreadyExists(dst.clone())
                    } else {
                        Error::io("create link", dst, e)
                    }
                })?;
                let after = fs.lstat(dst).ok().map(|m| m.fingerprint());
                if *remove_source {
                    if let Err(e) = fs.remove_file(src) {
                        let _ = fs.remove_file(dst);
                        return Err(Error::io("remove source link", src, e));
                    }
                }
                Ok(StepResult {
                    after,
                    ..StepResult::done()
                })
            }

            Step::Rename { from, to } => match fs.rename_noreplace(from, to) {
                Ok(()) => Ok(StepResult::done()),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    Err(Error::AlreadyExists(to.clone()))
                }
                Err(e) if crate::error::is_exdev(&e) => {
                    Err(Error::CrossDevice(from.clone(), to.clone()))
                }
                Err(e) => Err(Error::io2("rename", from, to, e)),
            },

            Step::TrashItem { path } => {
                let item = self.platform.trash().trash(path)?;
                Ok(StepResult {
                    trashed: Some(item),
                    ..StepResult::done()
                })
            }

            Step::RemoveFile { path, expect } => {
                let meta = match fs.lstat(path) {
                    Ok(m) => m,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        return Ok(StepResult::noop("already gone"));
                    }
                    Err(e) => return Err(Error::io("inspect", path, e)),
                };
                if let Some(exp) = expect {
                    if let Some(reason) = exp.diff(&meta.fingerprint()) {
                        return Err(Error::Modified {
                            path: path.clone(),
                            reason,
                        });
                    }
                }
                fs.remove_file(path)
                    .map_err(|e| Error::io("delete", path, e))?;
                Ok(StepResult::done())
            }

            Step::RemoveDir { path } => {
                let meta = match fs.lstat(path) {
                    Ok(m) => m,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        return Ok(StepResult::noop("already gone"));
                    }
                    Err(e) => return Err(Error::io("inspect", path, e)),
                };
                match fs.remove_dir(path) {
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

            Step::Restore { item } => {
                self.platform.trash().restore(item)?;
                Ok(StepResult::done())
            }
        }
    }
}

fn failed(index: usize, step: &Step, error: &Error) -> FailedStep {
    FailedStep {
        index,
        description: step.describe(),
        error: error.to_string(),
        path: step.path().to_path_buf(),
    }
}
