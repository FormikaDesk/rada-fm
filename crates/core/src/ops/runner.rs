//! Running an operation with journaling.

use std::collections::HashMap;

use super::engine::Engine;
use super::exec::{
    Cancel, ErrorChoice, ExecHandler, ExecReport, Failure, JournalChoice, JournalGap, Progress,
};
use super::plan::*;
use crate::Result;
use crate::fs::local::content_hash;
use crate::fs::{FileKind, FsEngine};
use crate::journal::Journal;

pub struct RunOutcome {
    pub id: String,
    pub report: ExecReport,
    /// Journal writes that failed during the run (those steps may not be undoable).
    pub journal_errors: u64,
}

/// Whether the journal is being written, and if not, why.
enum JournalState {
    Writing,
    /// The person (or the lack of one) chose to stop at the first write that failed.
    Stopped,
    /// The person chose to go on; nothing more is recorded.
    Off,
}

struct Recording<'a> {
    inner: &'a mut dyn ExecHandler,
    journal: &'a Journal,
    /// Where the identity of a file is looked up (the engine's, not the real disk's).
    fs: &'a dyn FsEngine,
    id: &'a str,
    reversible: bool,
    errors: u64,
    state: JournalState,
    /// The first step that ran without its way back written.
    unrecorded_from: Option<usize>,
    gap: Option<JournalGap>,
    /// Contents already hashed in this run, by (device, file): the names of one file.
    hashed: HashMap<(u64, u64), String>,
    /// The name the running step gives its temporary file, as its pending record says.
    tag: Option<String>,
}

impl Recording<'_> {
    fn writing(&self) -> bool {
        self.reversible && matches!(self.state, JournalState::Writing)
    }

    /// The first journal write that fails ends the recording: the handler is asked whether to
    /// stop before the next step or to go on without a way back. `ran` tells that the step
    /// the record was for has already happened, so it is the first one not recorded.
    fn journal_failed(&mut self, index: usize, step: &Step, e: &crate::Error, ran: bool) {
        tracing::error!("journal: {e}");
        self.errors += 1;
        let choice = self.inner.on_journal_failure(&Failure {
            index,
            step,
            error: e,
            attempt: 1,
        });
        let stopped = choice == JournalChoice::Stop;
        self.state = if stopped {
            JournalState::Stopped
        } else {
            JournalState::Off
        };
        if ran || !stopped {
            self.unrecorded_from = Some(index);
        }
        self.gap = Some(JournalGap {
            path: step.path().to_path_buf(),
            error: e.to_string(),
            stopped,
        });
    }

    /// A file made with several names has a fingerprint without a status-change time, which
    /// cannot see contents rewritten with the same size and modification time: its contents
    /// are hashed now, so that undo can tell before it removes the last name.
    ///
    /// Undo finds the file by its device and number, so where the system does not give them
    /// (Windows) a digest could never be used: nothing is hashed or written.
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
        let Some(key) = self
            .fs
            .lstat(path)
            .ok()
            .and_then(|m| Some((m.dev?, m.ino?)))
        else {
            return;
        };
        let hash = match self.hashed.get(&key) {
            Some(h) => h.clone(),
            None => match content_hash(path) {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("journal: cannot hash {}: {e}", path.display());
                    return;
                }
            },
        };
        self.hashed.insert(key, hash.clone());
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
        if self.writing()
            && let Some(intent) = crate::journal::Intent::of(step)
        {
            let tag = format!("{}-{index}", self.id);
            match self
                .journal
                .pending(self.id, intent.with_temp(Some(tag.clone())))
            {
                Ok(()) => self.tag = Some(tag),
                Err(e) => self.journal_failed(index, step, &e, false),
            }
        }
        if !self.halted() {
            self.inner.step_starting(index, step);
        }
    }

    fn temp_tag(&self) -> Option<String> {
        self.tag.clone()
    }

    fn step_finished(&mut self, index: usize, step: &Step, result: &StepResult) {
        if let Some(inv) = step.inverse(result) {
            if self.writing() {
                match self.journal.record_undo(self.id, inv.clone()) {
                    Ok(()) => self.record_digest(&inv),
                    Err(e) => self.journal_failed(index, step, &e, true),
                }
            } else if self.reversible {
                self.unrecorded_from.get_or_insert(index);
            }
        }
        self.inner.step_finished(index, step, result);
    }

    fn on_journal_failure(&mut self, f: &Failure<'_>) -> JournalChoice {
        self.inner.on_journal_failure(f)
    }

    fn halted(&self) -> bool {
        matches!(self.state, JournalState::Stopped) || self.inner.halted()
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
            fs: &*self.fs,
            id: &id,
            reversible: plan.reversible,
            errors: 0,
            state: JournalState::Writing,
            unrecorded_from: None,
            gap: None,
            hashed: HashMap::new(),
            tag: None,
        };
        let mut report = self.execute(plan, &mut rec, cancel);
        let mut errors = rec.errors;
        report.journal_gap = rec.gap.take();
        if let Err(e) = journal.end_unrecorded(
            &id,
            report.status(),
            report.done,
            report.failed.len() as u64,
            report.bytes,
            rec.unrecorded_from,
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
