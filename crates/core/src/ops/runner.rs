//! Running an operation with journaling.

use super::engine::Engine;
use super::exec::{Cancel, ErrorChoice, ExecHandler, ExecReport, Failure, Progress};
use super::plan::*;
use crate::Result;
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
        if self.reversible
            && let Some(intent) = crate::journal::Intent::of(step)
            && let Err(e) = self.journal.pending(self.id, intent)
        {
            tracing::error!("journal: {e}");
            self.errors += 1;
        }
        self.inner.step_starting(index, step);
    }

    fn step_finished(&mut self, index: usize, step: &Step, result: &StepResult) {
        if self.reversible
            && let Some(inv) = step.inverse(result)
            && let Err(e) = self.journal.record_undo(self.id, inv)
        {
            tracing::error!("journal: {e}");
            self.errors += 1;
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
