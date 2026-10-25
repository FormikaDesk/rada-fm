//! What the worker says about operations: scanning, planned, failed, finished, recovered, and
//! the summary of how one ended.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::Sender;
use rada_core::events::{FailureInfo, JobEvent, JobId};
use rada_core::i18n::{Key, tf};
use rada_core::journal::JournalEntry;
use rada_core::ops::{
    ErrorChoice, ExecReport, OpKind, Plan, Recovered, RunStatus, Scan, TransferMode, UndoPlan,
};

use super::{App, Modal, Notice, NoticeKind, PlanView, Replan, ResultView, Running, ToastKind};
use crate::fmt;

impl App {
    pub(super) fn on_job(&mut self, ev: JobEvent) {
        match ev {
            JobEvent::Scanning {
                job,
                files,
                dirs,
                bytes,
                current,
            } => self.on_scanning(job, (files, dirs, bytes, current)),
            JobEvent::Planned {
                job,
                plan,
                scan,
                undo,
            } => self.on_planned(job, *plan, scan, undo),
            JobEvent::PlanFailed { job, message } | JobEvent::Error { job, message } => {
                self.on_job_failed(job, message)
            }
            JobEvent::UndoLabel { label, .. } => self.ops.undo_label = label,
            JobEvent::NothingToUndo { job } => self.on_nothing_to(job, Key::TNothingUndo, 3),
            JobEvent::NothingToRedo { job } => self.on_nothing_to(job, Key::TNothingRedo, 4),
            JobEvent::Progress { job, progress } => {
                self.ops.progress(job, progress);
            }
            JobEvent::AskFailure { job, info, reply } => self.on_ask_failure(job, info, reply),
            JobEvent::Finished {
                job,
                title,
                report,
                journal_errors,
                ..
            } => self.on_finished(job, title, report, journal_errors),
            JobEvent::Recovered { items } => self.on_recovered(items),
            JobEvent::History { entries, .. } => self.on_history_loaded(entries),
        }
    }

    fn on_ask_failure(&mut self, job: JobId, info: FailureInfo, reply: Sender<ErrorChoice>) {
        if self.ops.is_running(job) {
            self.windows.modal = Some(Modal::Failure { job, info, reply });
        } else {
            let _ = reply.send(ErrorChoice::Abort);
        }
    }

    fn on_history_loaded(&mut self, entries: Vec<JournalEntry>) {
        if let Some(Modal::History(h)) = &mut self.windows.modal {
            h.entries = entries;
            h.loading = false;
            h.selected = 0;
        }
    }

    /// The count of what the scan found so far, in the window that waits for it.
    fn on_scanning(&mut self, job: JobId, found: (u64, u64, u64, PathBuf)) {
        let (files, dirs, bytes, current) = found;
        if let Some(Modal::Scanning {
            job: j,
            files: f,
            dirs: d,
            bytes: b,
            current: c,
            ..
        }) = &mut self.windows.modal
            && *j == job
        {
            (*f, *d, *b, *c) = (files, dirs, bytes, current);
        }
    }

    fn on_job_failed(&mut self, job: JobId, message: String) {
        if self.ops.fail(job) {
            self.windows.modal = None;
            // A sentence that does not fit a one-line message is read in a window.
            if message.chars().count() > 70 {
                self.windows.modal = Some(Modal::Result(ResultView {
                    title: Key::ResultCouldNot.tr().to_string(),
                    lines: vec![(ToastKind::Error, message)],
                    scroll: 0,
                }));
            } else {
                self.toast(ToastKind::Error, message, 6);
            }
        }
    }

    /// An undo or a redo found nothing to do: `what` is the word for it.
    fn on_nothing_to(&mut self, job: JobId, what: Key, secs: u64) {
        if self.ops.end_plan(job) {
            self.windows.modal = None;
            self.toast(ToastKind::Info, what.tr(), secs);
        }
    }

    fn on_finished(&mut self, job: JobId, title: String, report: ExecReport, journal_errors: u64) {
        if let Some(done) = self.ops.finish(job) {
            self.finish(title, report, journal_errors, Some(done));
            // Refresh even if live updates are unavailable.
            let generation = self.tabs.next_generation();
            self.svc.loader.load(self.tab.cwd.clone(), generation);
            self.svc.jobs.load_undo_label();
        }
    }

    fn on_recovered(&mut self, items: Vec<Recovered>) {
        let mut lines: Vec<(ToastKind, String)> = Vec::new();
        for r in &items {
            lines.push((
                ToastKind::Warn,
                tf(
                    Key::ResultRecovered,
                    &[&r.title, &r.removed_temps.len(), &r.adopted.len()],
                ),
            ));
            for n in &r.notes {
                lines.push((ToastKind::Warn, n.clone()));
            }
        }
        lines.push((ToastKind::Info, Key::ResultOpenHistory.tr().to_string()));
        self.windows.modal = Some(Modal::Result(ResultView {
            title: Key::ResultInterruptedTitle.tr().to_string(),
            lines,
            scroll: 0,
        }));
    }

    fn finish(
        &mut self,
        title: String,
        report: ExecReport,
        journal_errors: u64,
        done: Option<Running>,
    ) {
        let lines = self.result_lines(&report, journal_errors);
        let clean = report.status() == RunStatus::Completed
            && report.kept.is_empty()
            && report.not_preserved.is_empty();
        // The clean ending: a card with what was done, the way back and the way to see it.
        if clean && lines.is_empty() {
            let notice = match done.as_ref() {
                Some(r) => self.done_notice(r),
                None => Notice::new(NoticeKind::Ok, tf(Key::NDoneDefault, &[&title])),
            };
            self.notify(notice);
            return;
        }
        let (kind, headline) = headline(&report, &title);
        if lines.is_empty() {
            self.toast(kind, headline, 4);
        } else {
            self.windows.modal = Some(Modal::Result(ResultView {
                title: headline,
                lines,
                scroll: 0,
            }));
        }
    }

    /// What went wrong or was left undone, one line each, for the window that ends an operation.
    fn result_lines(&self, report: &ExecReport, journal_errors: u64) -> Vec<(ToastKind, String)> {
        let mut lines: Vec<(ToastKind, String)> = Vec::new();
        for f in &report.failed {
            lines.push((ToastKind::Error, f.error.clone()));
        }
        if report.skipped_dependent > 0 {
            lines.push((
                ToastKind::Warn,
                if report.skipped_dependent == 1 {
                    Key::NSkippedFolderOne.tr().to_string()
                } else {
                    tf(Key::NSkippedFolderMany, &[&report.skipped_dependent])
                },
            ));
        }
        for (_, note) in &report.kept {
            lines.push((ToastKind::Warn, note.clone()));
        }
        lines.extend(self.not_preserved_lines(report));
        if journal_errors > 0 {
            lines.push((
                ToastKind::Warn,
                if journal_errors == 1 {
                    Key::NJournalErrorsOne.tr().to_string()
                } else {
                    tf(Key::NJournalErrorsMany, &[&journal_errors])
                },
            ));
        }
        lines
    }

    /// What a copy could not carry over (owner, extended attributes, ACLs...), one line per
    /// kind with how many items and an example.
    fn not_preserved_lines(&self, report: &ExecReport) -> Vec<(ToastKind, String)> {
        let mut lost: std::collections::BTreeMap<&str, Vec<&Path>> = Default::default();
        for (path, what) in &report.not_preserved {
            lost.entry(what.as_str()).or_default().push(path);
        }
        lost.into_iter()
            .map(|(what, paths)| {
                (
                    ToastKind::Warn,
                    tf(
                        Key::NNotPreserved,
                        &[
                            &what,
                            &fmt::count(paths.len() as u64, Key::ItemOne, Key::ItemMany),
                            &fmt::short_path(paths[0], self.home()),
                        ],
                    ),
                )
            })
            .collect()
    }

    fn on_planned(
        &mut self,
        job: JobId,
        plan: Plan,
        scan: Option<Arc<Scan>>,
        undo: Option<Box<UndoPlan>>,
    ) {
        // Re-plan of an open plan window?
        if matches!(&self.windows.modal, Some(Modal::Plan(pv)) if pv.replanning == Some(job)) {
            self.replace_plan(plan, scan);
            return;
        }
        if !self.ops.end_plan(job) {
            return; // the user cancelled while it was being planned
        }
        let replan = replan_for(&plan);
        let pv = Box::new(PlanView {
            plan,
            scan,
            undo,
            replan,
            scroll: 0,
            typed: String::new(),
            replanning: None,
            details: false,
            cancel_focus: false,
        });
        if self.hooks.demo_auto_run {
            self.hooks.demo_auto_run = false;
            self.windows.modal = None;
            self.run_plan(pv);
            return;
        }
        self.windows.modal = Some(Modal::Plan(pv));
    }

    /// The plan window that asked for another plan gets it, and shows it from the top.
    fn replace_plan(&mut self, plan: Plan, scan: Option<Arc<Scan>>) {
        if let Some(Modal::Plan(pv)) = &mut self.windows.modal {
            pv.plan = plan;
            pv.scan = scan.or(pv.scan.take());
            pv.replanning = None;
            pv.scroll = 0;
        }
    }
}

/// How a plan can be asked for again with another way of settling name clashes.
fn replan_for(plan: &Plan) -> Replan {
    match (&plan.kind, &plan.destination) {
        (rada_core::ops::OpKind::Copy, Some(d)) => Replan::Transfer {
            dest: d.clone(),
            mode: TransferMode::Copy,
        },
        (rada_core::ops::OpKind::Move, Some(d)) => Replan::Transfer {
            dest: d.clone(),
            mode: TransferMode::Move,
        },
        (OpKind::Extract | OpKind::Compress, _) if plan.request.is_some() => Replan::Request,
        (OpKind::Rename, _) if plan.has_conflicts() && plan.blocking().next().is_none() => {
            Replan::Request
        }
        _ => Replan::None,
    }
}

/// The kind of card and the first line for how an operation ended.
fn headline(report: &ExecReport, title: &str) -> (ToastKind, String) {
    use RunStatus::*;
    match report.status() {
        Completed if report.kept.is_empty() && report.not_preserved.is_empty() => {
            (ToastKind::Ok, tf(Key::NDoneDefault, &[&title]))
        }
        Completed if report.not_preserved.is_empty() => {
            (ToastKind::Warn, tf(Key::NDoneKept, &[&title]))
        }
        Completed => (ToastKind::Warn, tf(Key::NDoneNotKept, &[&title])),
        CompletedWithProblems => (
            ToastKind::Warn,
            tf(
                Key::NProblems,
                &[
                    &title,
                    &(report.failed.len() + report.skipped_dependent as usize),
                ],
            ),
        ),
        Aborted => (ToastKind::Warn, tf(Key::NStopped, &[&title])),
        Cancelled => (ToastKind::Warn, tf(Key::NCancelled, &[&title])),
        Interrupted => (ToastKind::Warn, tf(Key::NInterrupted, &[&title])),
    }
}
