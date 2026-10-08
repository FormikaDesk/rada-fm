//! Undo: turn a journal entry back into a plan, after checking that it is still safe.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::engine::Engine;
use super::exec::{Cancel, ExecHandler, ExecReport, Failure, Progress};
use super::plan::*;
use crate::journal::{Journal, JournalEntry};
use crate::{Error, Result, display};

#[derive(Clone, Debug)]
pub struct Blocked {
    /// Index into the entry's undo steps.
    pub index: usize,
    pub step: Step,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct UndoPlan {
    pub entry: JournalEntry,
    pub plan: Plan,
    /// Journal undo-step index for each step of `plan`.
    pub index_map: Vec<usize>,
    pub blocked: Vec<Blocked>,
}

/// What the filesystem will look like while the undo plan runs, so that a plan made of
/// several dependent steps (bulk-rename swaps, restore after remove) is judged as a
/// whole instead of step by step against the *current* state.
struct Sim<'a> {
    engine: &'a Engine,
    overlay: HashMap<PathBuf, bool>,
}

impl Sim<'_> {
    fn exists(&self, p: &Path) -> bool {
        self.overlay
            .get(p)
            .copied()
            .unwrap_or_else(|| self.engine.fs.lstat(p).is_ok())
    }

    fn touched(&self, p: &Path) -> bool {
        self.overlay.contains_key(p)
    }

    fn apply(&mut self, step: &Step) {
        match step {
            Step::MakeDir { path, .. } => {
                self.overlay.insert(path.clone(), true);
            }
            Step::RemoveFile { path, .. } | Step::RemoveDir { path } | Step::TrashItem { path } => {
                self.overlay.insert(path.clone(), false);
            }
            Step::Rename { from, to } => {
                self.overlay.insert(from.clone(), false);
                self.overlay.insert(to.clone(), true);
            }
            Step::CopyFile {
                src,
                dst,
                remove_source,
                ..
            }
            | Step::CopySymlink {
                src,
                dst,
                remove_source,
                ..
            } => {
                self.overlay.insert(dst.clone(), true);
                if *remove_source {
                    self.overlay.insert(src.clone(), false);
                }
            }
            Step::Restore { item } => {
                self.overlay.insert(item.original.clone(), true);
            }
            Step::FinishDir { .. } => {}
        }
    }

    /// `Err(reason)` if the step must not run.
    fn check(&self, step: &Step) -> std::result::Result<(), String> {
        match step {
            Step::RemoveFile { path, expect } => {
                if !self.exists(path) {
                    return Ok(()); // already gone: nothing to do
                }
                if !self.touched(path) {
                    if let (Some(exp), Ok(now)) = (expect, self.engine.fs.lstat(path)) {
                        if let Some(why) = exp.diff(&now.fingerprint()) {
                            return Err(format!(
                                "{} was modified after the operation ({why})",
                                display::path(path)
                            ));
                        }
                    }
                }
                Ok(())
            }
            Step::RemoveDir { .. } | Step::FinishDir { .. } => Ok(()),
            Step::MakeDir { path, .. } => {
                let parent_ok = path.parent().is_none_or(|p| self.exists(p));
                if parent_ok {
                    Ok(())
                } else {
                    Err(format!("the parent of {} is gone", display::path(path)))
                }
            }
            Step::Rename { from, to } => {
                if !self.exists(from) {
                    Err(format!("{} is no longer there", display::path(from)))
                } else if self.exists(to) {
                    Err(format!(
                        "{} is occupied; refusing to overwrite it",
                        display::path(to)
                    ))
                } else {
                    Ok(())
                }
            }
            Step::CopyFile { src, dst, .. } | Step::CopySymlink { src, dst, .. } => {
                if !self.exists(src) {
                    Err(format!("{} is no longer there", display::path(src)))
                } else if self.exists(dst) {
                    Err(format!(
                        "{} is occupied; refusing to overwrite it",
                        display::path(dst)
                    ))
                } else {
                    Ok(())
                }
            }
            Step::Restore { item } => {
                if !self.engine.platform.trash().contains(item) {
                    Err(format!(
                        "{} is no longer in the trash",
                        display::path(&item.original)
                    ))
                } else if self.exists(&item.original) {
                    Err(format!(
                        "{} already exists; refusing to overwrite it",
                        display::path(&item.original)
                    ))
                } else {
                    Ok(())
                }
            }
            Step::TrashItem { .. } => Ok(()),
        }
    }
}

impl Engine {
    /// Build the plan that reverses `entry_id`. Nothing is modified.
    pub fn plan_undo(&self, journal: &Journal, entry_id: &str) -> Result<UndoPlan> {
        let entry = journal
            .entries()?
            .into_iter()
            .find(|e| e.id == entry_id)
            .ok_or_else(|| Error::Journal(format!("no such operation: {entry_id}")))?;
        if !entry.reversible {
            return Err(Error::Invalid(format!(
                "\"{}\" is permanent and cannot be undone",
                entry.title
            )));
        }
        let (steps, remaining) = journal.load_undo(entry_id)?;
        let mut sim = Sim {
            engine: self,
            overlay: HashMap::new(),
        };
        let mut plan = Plan::empty(OpKind::Undo, format!("Undo: {}", entry.title));
        plan.reversible = false;
        let mut index_map = Vec::new();
        let mut blocked = Vec::new();
        let mut ws = WarningSet::default();

        // Undo runs the recorded inverses newest first.
        let mut todo: Vec<usize> = remaining.into_iter().filter(|&i| i < steps.len()).collect();
        todo.sort_unstable();
        for &i in todo.iter().rev() {
            let step = &steps[i];
            match sim.check(step) {
                Ok(()) => {
                    sim.apply(step);
                    plan.steps.push(step.clone());
                    index_map.push(i);
                    match step {
                        Step::RemoveFile { .. } => plan.totals.files += 1,
                        Step::RemoveDir { .. } => plan.totals.dirs += 1,
                        _ => {}
                    }
                    plan.totals.bytes += step.bytes();
                }
                Err(reason) => {
                    let kind = if reason.contains("modified") {
                        WarningKind::ModifiedSince
                    } else {
                        WarningKind::CannotUndo
                    };
                    ws.add_with(
                        kind,
                        Severity::Warning,
                        Some(step.path()),
                        Some(reason.clone()),
                    );
                    blocked.push(Blocked {
                        index: i,
                        step: step.clone(),
                        reason,
                    });
                }
            }
        }
        plan.totals.items = plan.steps.len() as u64;
        plan.warnings = ws.finish();
        Ok(UndoPlan {
            entry,
            plan,
            index_map,
            blocked,
        })
    }

    /// Execute an undo plan and record the outcome in the journal.
    pub fn run_undo(
        &self,
        journal: &Journal,
        up: &UndoPlan,
        handler: &mut dyn ExecHandler,
        cancel: &Cancel,
    ) -> Result<ExecReport> {
        struct Tracker<'a> {
            inner: &'a mut dyn ExecHandler,
            index_map: &'a [usize],
            finished: Vec<usize>,
            kept: Vec<usize>,
        }
        impl ExecHandler for Tracker<'_> {
            fn progress(&mut self, p: &Progress) {
                self.inner.progress(p);
            }
            fn on_failure(&mut self, f: &Failure<'_>) -> super::exec::ErrorChoice {
                self.inner.on_failure(f)
            }
            fn step_finished(&mut self, index: usize, step: &Step, result: &StepResult) {
                if let Some(&orig) = self.index_map.get(index) {
                    if result.status == StepStatus::Kept {
                        self.kept.push(orig);
                    } else {
                        self.finished.push(orig);
                    }
                }
                self.inner.step_finished(index, step, result);
            }
        }

        let mut tr = Tracker {
            inner: handler,
            index_map: &up.index_map,
            finished: Vec::new(),
            kept: Vec::new(),
        };
        let report = self.execute(&up.plan, &mut tr, cancel);
        let finished = tr.finished;
        let (all, previous_remaining) = journal.load_undo(&up.entry.id)?;
        let remaining: Vec<usize> = (0..all.len())
            .filter(|i| previous_remaining.contains(i) && !finished.contains(i))
            .collect();
        journal.mark_undone(&up.entry.id, remaining)?;
        Ok(report)
    }
}
