//! Running an operation with journaling.

use std::collections::HashMap;

use super::engine::Engine;
use super::exec::{Cancel, ErrorChoice, ExecHandler, ExecReport, Failure, Progress};
use super::plan::*;
use crate::Result;
use crate::fs::local::content_hash;
use crate::fs::{FileKind, FsEngine, LocalFs};
use crate::journal::Journal;

pub struct RunOutcome {
    pub id: String,
    pub report: ExecReport,
    /// Journal writes that failed during the run (those steps may not be undoable).
    pub journal_errors: u64,
}

struct Recording<'a> {
    inner: &'a mut dyn ExecHandler,
    journal: &'a Journal,
    id: &'a str,
    reversible: bool,
    errors: u64,
    /// Contents already hashed in this run, by (device, file): the names of one file.
    hashed: HashMap<(u64, u64), String>,
    /// The name the running step gives its temporary file, as its pending record says.
    tag: Option<String>,
}

impl Recording<'_> {
    /// A file made with several names has a fingerprint without a status-change time, which
    /// cannot see contents rewritten with the same size and modification time: its contents
    /// are hashed now, so that undo can tell before it removes the last name.
    fn record_digest(&mut self, inverse: &Step) {
        let Step::RemoveFile {
            path,
            expect: Some(fp),
        } = inverse
        else {
            return;
        };
        if fp.ctime.is_some() || fp.kind != FileKind::File {
            return;
        }
        let key = LocalFs
            .lstat(path)
            .ok()
            .and_then(|m| Some((m.dev?, m.ino?)));
        let hash = match key.and_then(|k| self.hashed.get(&k)) {
            Some(h) => h.clone(),
            None => match content_hash(path) {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("journal: cannot hash {}: {e}", path.display());
                    return;
                }
            },
        };
        if let Some(k) = key {
            self.hashed.insert(k, hash.clone());
        }
        if let Err(e) = self.journal.record_digest(self.id, path, &hash) {
            tracing::error!("journal: {e}");
            self.errors += 1;
        }
    }
}

impl ExecHandler for Recording<'_> {
    fn progress(&mut self, p: &Progress) {
        self.inner.progress(p);
    }

    fn on_failure(&mut self, f: &Failure<'_>) -> ErrorChoice {
        self.inner.on_failure(f)
    }

    fn step_starting(&mut self, index: usize, step: &Step) {
        // Written before the step runs: if the process dies in it, the next start can see
        // from the disk whether it happened.
        self.tag = None;
        if self.reversible
            && let Some(intent) = crate::journal::Intent::of(step)
        {
            let tag = format!("{}-{index}", self.id);
            match self
                .journal
                .pending(self.id, intent.with_temp(Some(tag.clone())))
            {
                Ok(()) => self.tag = Some(tag),
                Err(e) => {
                    tracing::error!("journal: {e}");
                    self.errors += 1;
                }
            }
        }
        self.inner.step_starting(index, step);
    }

    fn temp_tag(&self) -> Option<String> {
        self.tag.clone()
    }

    fn step_finished(&mut self, index: usize, step: &Step, result: &StepResult) {
        if self.reversible
            && let Some(inv) = step.inverse(result)
        {
            if let Err(e) = self.journal.record_undo(self.id, inv.clone()) {
                tracing::error!("journal: {e}");
                self.errors += 1;
            }
            self.record_digest(&inv);
        }
        self.inner.step_finished(index, step, result);
    }
}

impl Engine {
    /// Execute `plan`, recording in `journal` how to undo every step as it completes.
    /// Refuses to start if the journal cannot be written: an operation that cannot be
    /// undone must be a decision, not an accident.
    pub fn run_operation(
        &self,
        journal: &Journal,
        plan: &Plan,
        handler: &mut dyn ExecHandler,
        cancel: &Cancel,
    ) -> Result<RunOutcome> {
        let id = journal.begin(
            plan.kind,
            &plan.title,
            plan.reversible,
            plan.totals,
            plan.request.as_ref(),
            plan.redo_of.as_deref(),
        )?;
        let mut rec = Recording {
            inner: handler,
            journal,
            id: &id,
            reversible: plan.reversible,
            errors: 0,
            hashed: HashMap::new(),
            tag: None,
        };
        let report = self.execute(plan, &mut rec, cancel);
        let mut errors = rec.errors;
        if let Err(e) = journal.end(
            &id,
            report.status(),
            report.done,
            report.failed.len() as u64,
            report.bytes,
        ) {
            tracing::error!("journal: {e}");
            errors += 1;
        }
        Ok(RunOutcome {
            id,
            report,
            journal_errors: errors,
        })
    }
}
