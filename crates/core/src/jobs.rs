//! Planning and executing operations on worker threads. The UI submits a request and
//! later receives [`JobEvent`]s; it never scans, plans, copies or reads the journal
//! itself.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::Sender;

use crate::events::{CoreEvent, FailureInfo, JobEvent, JobId};
use crate::journal::Journal;
use crate::ops::{
    Cancel, Engine, ErrorChoice, ExecHandler, Failure, Pattern, Plan, Progress, Scan, ScanControl,
    TransferOptions, UndoPlan,
};

#[derive(Clone)]
pub struct JobHandle {
    pub id: JobId,
    pub cancel: Cancel,
}

#[derive(Clone)]
pub struct Jobs {
    engine: Engine,
    journal: Option<Arc<Journal>>,
    out: Sender<CoreEvent>,
    next: Arc<AtomicU64>,
}

impl Jobs {
    pub fn new(engine: Engine, journal: Option<Arc<Journal>>, out: Sender<CoreEvent>) -> Jobs {
        Jobs {
            engine,
            journal,
            out,
            next: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn has_journal(&self) -> bool {
        self.journal.is_some()
    }

    fn spawn(
        &self,
        f: impl FnOnce(JobId, Cancel, &Engine, &Sender<CoreEvent>) + Send + 'static,
    ) -> JobHandle {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let cancel = Cancel::new();
        let handle = JobHandle {
            id,
            cancel: cancel.clone(),
        };
        let engine = self.engine.clone();
        let out = self.out.clone();
        std::thread::Builder::new()
            .name(format!("vela-job-{id}"))
            // Deep trees recurse in the scanner.
            .stack_size(16 << 20)
            .spawn(move || f(id, cancel, &engine, &out))
            .expect("spawn job thread");
        handle
    }

    fn scan_with_progress(
        engine: &Engine,
        sources: &[PathBuf],
        id: JobId,
        cancel: &Cancel,
        out: &Sender<CoreEvent>,
    ) -> Scan {
        engine.scan(
            sources,
            ScanControl {
                cancel: cancel.flag(),
                progress: &mut |p| {
                    let _ = out.send(CoreEvent::Job(JobEvent::Scanning {
                        job: id,
                        files: p.files,
                        dirs: p.dirs,
                        bytes: p.bytes,
                        current: p.current.clone(),
                    }));
                },
            },
        )
    }

    fn planned(out: &Sender<CoreEvent>, job: JobId, plan: Plan, scan: Option<Arc<Scan>>) {
        let _ = out.send(CoreEvent::Job(JobEvent::Planned {
            job,
            plan: Box::new(plan),
            scan,
            undo: None,
        }));
    }

    // ------------------------------------------------------------------ planning

    pub fn plan_transfer(
        &self,
        sources: Vec<PathBuf>,
        dest: PathBuf,
        opts: TransferOptions,
    ) -> JobHandle {
        self.spawn(move |id, cancel, engine, out| {
            let scan = Arc::new(Self::scan_with_progress(engine, &sources, id, &cancel, out));
            if cancel.is_cancelled() {
                return;
            }
            let plan = engine.plan_transfer(&scan, &dest, &opts);
            Self::planned(out, id, plan, Some(scan));
        })
    }

    /// Re-plan from an existing scan (e.g. after the user picked another conflict policy).
    pub fn replan_transfer(
        &self,
        scan: Arc<Scan>,
        dest: PathBuf,
        opts: TransferOptions,
    ) -> JobHandle {
        self.spawn(move |id, _cancel, engine, out| {
            let plan = engine.plan_transfer(&scan, &dest, &opts);
            Self::planned(out, id, plan, Some(scan));
        })
    }

    pub fn plan_trash(&self, sources: Vec<PathBuf>) -> JobHandle {
        self.spawn(move |id, cancel, engine, out| {
            let scan = Arc::new(Self::scan_with_progress(engine, &sources, id, &cancel, out));
            if cancel.is_cancelled() {
                return;
            }
            Self::planned(out, id, engine.plan_trash(&scan), Some(scan));
        })
    }

    pub fn plan_delete(&self, sources: Vec<PathBuf>) -> JobHandle {
        self.spawn(move |id, cancel, engine, out| {
            let scan = Arc::new(Self::scan_with_progress(engine, &sources, id, &cancel, out));
            if cancel.is_cancelled() {
                return;
            }
            Self::planned(out, id, engine.plan_delete(&scan), Some(scan));
        })
    }

    pub fn plan_rename(&self, from: PathBuf, new_name: std::ffi::OsString) -> JobHandle {
        self.spawn(move |id, _c, engine, out| {
            Self::planned(out, id, engine.plan_rename(&from, &new_name), None)
        })
    }

    pub fn plan_mkdir(&self, parent: PathBuf, name: std::ffi::OsString) -> JobHandle {
        self.spawn(move |id, _c, engine, out| {
            Self::planned(out, id, engine.plan_mkdir(&parent, &name), None)
        })
    }

    pub fn plan_bulk_rename(&self, items: Vec<PathBuf>, pattern: Pattern) -> JobHandle {
        self.spawn(move |id, _c, engine, out| {
            Self::planned(out, id, engine.plan_bulk_rename(&items, &pattern), None)
        })
    }

    /// Plan the undo of `entry_id`, or of the latest undoable operation when `None`.
    pub fn plan_undo(&self, entry_id: Option<String>) -> JobHandle {
        let journal = self.journal.clone();
        self.spawn(move |id, _c, engine, out| {
            let Some(journal) = journal else {
                let _ = out.send(CoreEvent::Job(JobEvent::Error {
                    job: id,
                    message: "the journal is unavailable; nothing can be undone".into(),
                }));
                return;
            };
            let target = match entry_id {
                Some(i) => i,
                None => match journal.last_undoable() {
                    Ok(Some(e)) => e.id,
                    Ok(None) => {
                        let _ = out.send(CoreEvent::Job(JobEvent::NothingToUndo { job: id }));
                        return;
                    }
                    Err(e) => {
                        let _ = out.send(CoreEvent::Job(JobEvent::Error {
                            job: id,
                            message: e.to_string(),
                        }));
                        return;
                    }
                },
            };
            match engine.plan_undo(&journal, &target) {
                Ok(up) => {
                    let plan = up.plan.clone();
                    let _ = out.send(CoreEvent::Job(JobEvent::Planned {
                        job: id,
                        plan: Box::new(plan),
                        scan: None,
                        undo: Some(Box::new(up)),
                    }));
                }
                Err(e) => {
                    let _ = out.send(CoreEvent::Job(JobEvent::PlanFailed {
                        job: id,
                        message: e.to_string(),
                    }));
                }
            }
        })
    }

    pub fn load_history(&self) -> JobHandle {
        let journal = self.journal.clone();
        self.spawn(move |id, _c, _e, out| {
            let ev = match journal.as_ref().map(|j| j.entries()) {
                Some(Ok(mut entries)) => {
                    entries.reverse(); // newest first
                    JobEvent::History { job: id, entries }
                }
                Some(Err(e)) => JobEvent::Error {
                    job: id,
                    message: e.to_string(),
                },
                None => JobEvent::Error {
                    job: id,
                    message: "the journal is unavailable".into(),
                },
            };
            let _ = out.send(CoreEvent::Job(ev));
        })
    }

    // ------------------------------------------------------------------ execution

    pub fn execute(&self, plan: Plan) -> JobHandle {
        let journal = self.journal.clone();
        self.spawn(move |id, cancel, engine, out| {
            let mut h = Forward {
                job: id,
                out: out.clone(),
            };
            let title = plan.title.clone();
            let (report, journal_id, journal_errors) = match &journal {
                Some(j) => match engine.run_operation(j, &plan, &mut h, &cancel) {
                    Ok(o) => (o.report, Some(o.id), o.journal_errors),
                    Err(e) => {
                        let _ = out.send(CoreEvent::Job(JobEvent::Error {
                            job: id,
                            message: format!("not started: {e}"),
                        }));
                        return;
                    }
                },
                None => (engine.execute(&plan, &mut h, &cancel), None, 0),
            };
            let _ = out.send(CoreEvent::Job(JobEvent::Finished {
                job: id,
                title,
                report,
                journal_id,
                journal_errors,
            }));
        })
    }

    pub fn execute_undo(&self, up: UndoPlan) -> JobHandle {
        let journal = self.journal.clone();
        self.spawn(move |id, cancel, engine, out| {
            let Some(journal) = journal else { return };
            let mut h = Forward {
                job: id,
                out: out.clone(),
            };
            let title = up.plan.title.clone();
            match engine.run_undo(&journal, &up, &mut h, &cancel) {
                Ok(report) => {
                    let _ = out.send(CoreEvent::Job(JobEvent::Finished {
                        job: id,
                        title,
                        report,
                        journal_id: None,
                        journal_errors: 0,
                    }));
                }
                Err(e) => {
                    let _ = out.send(CoreEvent::Job(JobEvent::Error {
                        job: id,
                        message: e.to_string(),
                    }));
                }
            }
        })
    }
}

/// Forwards executor callbacks to the UI channel; a failure blocks only this worker.
struct Forward {
    job: JobId,
    out: Sender<CoreEvent>,
}

impl ExecHandler for Forward {
    fn progress(&mut self, p: &Progress) {
        let _ = self.out.send(CoreEvent::Job(JobEvent::Progress {
            job: self.job,
            progress: p.clone(),
        }));
    }

    fn on_failure(&mut self, f: &Failure<'_>) -> ErrorChoice {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let info = FailureInfo {
            index: f.index,
            description: f.step.describe(),
            message: f.error.to_string(),
            path: f.step.path().to_path_buf(),
            attempt: f.attempt,
        };
        if self
            .out
            .send(CoreEvent::Job(JobEvent::AskFailure {
                job: self.job,
                info,
                reply: tx,
            }))
            .is_err()
        {
            return ErrorChoice::Abort;
        }
        rx.recv().unwrap_or(ErrorChoice::Abort)
    }
}
