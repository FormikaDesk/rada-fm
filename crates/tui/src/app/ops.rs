//! What is being done to files: what waits on the clipboard, the plan being prepared, the
//! operation that is running and the label of the undo.

use super::notice;
use rada_core::events::JobId;
use rada_core::jobs::JobHandle;
use rada_core::ops::{OpKind, Progress, Totals, TransferMode};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Clip {
    pub mode: TransferMode,
    pub paths: Vec<PathBuf>,
}

pub struct Running {
    pub job: JobId,
    pub title: String,
    pub progress: Progress,
    pub started: Instant,
    pub handle: JobHandle,
    /// (time, bytes) samples for a smoothed transfer rate.
    pub(super) samples: Vec<(Instant, u64)>,
    /// What it is doing, for the message when it ends.
    pub kind: OpKind,
    pub totals: Totals,
    pub reversible: bool,
    /// Where the result will be, for the card that says it is done.
    pub outcome: notice::Outcome,
}

impl Running {
    /// Take what the worker reported, keeping the last four seconds of it for the rate.
    fn note(&mut self, progress: Progress) {
        let now = Instant::now();
        self.samples.push((now, progress.bytes_done));
        self.samples
            .retain(|(t, _)| now.duration_since(*t) < Duration::from_secs(4));
        self.progress = progress;
    }

    pub fn rate(&self) -> f64 {
        match (self.samples.first(), self.samples.last()) {
            (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => {
                (*b1 as f64 - *b0 as f64) / t1.duration_since(*t0).as_secs_f64()
            }
            _ => 0.0,
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        let rate = self.rate();
        let left = self
            .progress
            .bytes_total
            .saturating_sub(self.progress.bytes_done);
        (rate > 1.0 && left > 0).then(|| Duration::from_secs_f64(left as f64 / rate))
    }
}

/// The clipboard, the plan in preparation, the operation running and the undo.
#[derive(Default)]
pub struct Operations {
    pub clipboard: Option<Clip>,
    pub running: Option<Running>,
    /// The job that is preparing a plan (a window "Scanning" is open for it).
    pub(super) plan_job: Option<JobId>,
    /// What the next undo would undo, in a few words (for the command bar).
    pub undo_label: Option<String>,
    /// Text for the system clipboard (the "copy path" command); the event loop takes it.
    pub copied_text: Option<String>,
}

impl Operations {
    pub fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    /// Whether `job` is the one running.
    pub(super) fn is_running(&self, job: JobId) -> bool {
        self.running.as_ref().is_some_and(|r| r.job == job)
    }

    /// Stop waiting for the plan of `job`; false when it is not the one being prepared (the
    /// person gave up on it, or it was replaced).
    pub(super) fn end_plan(&mut self, job: JobId) -> bool {
        let mine = self.plan_job == Some(job);
        if mine {
            self.plan_job = None;
        }
        mine
    }

    /// `job` failed: forget it, planned or running. False when it is neither.
    pub(super) fn fail(&mut self, job: JobId) -> bool {
        if self.plan_job != Some(job) && !self.is_running(job) {
            return false;
        }
        self.plan_job = None;
        self.running = None;
        true
    }

    /// The progress of `job`, if it is the one running.
    pub(super) fn progress(&mut self, job: JobId, progress: Progress) {
        if let Some(r) = self.running.as_mut().filter(|r| r.job == job) {
            r.note(progress);
        }
    }

    /// `job` ended: take it out of `running` (None when it was not the one running).
    pub(super) fn finish(&mut self, job: JobId) -> Option<Running> {
        if self.is_running(job) {
            self.running.take()
        } else {
            None
        }
    }

    /// Ask the running operation to stop; false when nothing is running.
    pub fn cancel(&self) -> bool {
        self.running.iter().for_each(|r| r.handle.cancel.cancel());
        self.running.is_some()
    }

    /// Take the text waiting for the system clipboard, if any.
    pub fn take_copied_text(&mut self) -> Option<String> {
        self.copied_text.take()
    }
}
