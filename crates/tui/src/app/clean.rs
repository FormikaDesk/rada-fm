//! What the workers send that did not begin inside rada: the answer of a program, the label of
//! a disk, the notes of a release, the text of a journal. It is cleaned once, here, as the event
//! comes in (control characters and escapes become visible, see `display::text`), so that no
//! window after this has to remember to.
//!
//! File names are not touched: they travel as paths and are escaped where they are drawn.

use rada_core::display::{block, text};
use rada_core::events::{CoreEvent, FailureInfo, JobEvent, PreviewEvent, WatchEvent};
use rada_core::ops::{ExecReport, FailedStep, Recovered};
use rada_core::platform::{OpenApp, Unmounted, Volume};
use rada_core::preview::{ImagePreview, ImageState, Preview};
use rada_core::update::{Applied, Notice, UpdateEvent};

pub(super) fn clean(ev: CoreEvent) -> CoreEvent {
    match ev {
        CoreEvent::Preview(p) => CoreEvent::Preview(PreviewEvent {
            preview: preview(p.preview),
            ..p
        }),
        CoreEvent::Job(j) => CoreEvent::Job(job(j)),
        CoreEvent::Update(u) => CoreEvent::Update(update(u)),
        CoreEvent::Watch(WatchEvent::Failed(m)) => CoreEvent::Watch(WatchEvent::Failed(text(&m))),
        CoreEvent::Watch(WatchEvent::Degraded(m)) => {
            CoreEvent::Watch(WatchEvent::Degraded(text(&m)))
        }
        CoreEvent::Note { text: t, error } => CoreEvent::Note {
            text: text(&t),
            error,
        },
        CoreEvent::FontInstalled(r) => {
            CoreEvent::FontInstalled(r.map(|s| text(&s)).map_err(|s| text(&s)))
        }
        CoreEvent::Git { dir, status } => CoreEvent::Git {
            dir,
            status: status.map(|mut s| {
                s.branch = text(&s.branch);
                s
            }),
        },
        CoreEvent::Paths(mut p) => {
            p.names.values_mut().for_each(|n| *n = text(n));
            CoreEvent::Paths(p)
        }
        other => devices(other),
    }
}

/// The disks, the phones and the programs that open a file.
fn devices(ev: CoreEvent) -> CoreEvent {
    match ev {
        CoreEvent::Volumes(v) => CoreEvent::Volumes(v.into_iter().map(volume).collect()),
        CoreEvent::Unmounted(l) => CoreEvent::Unmounted(l.into_iter().map(unmounted).collect()),
        CoreEvent::Mounted { id, name, result } => CoreEvent::Mounted {
            id,
            name: text(&name),
            result: result.map_err(|e| text(&e)),
        },
        CoreEvent::Ejected {
            mount,
            name,
            result,
        } => CoreEvent::Ejected {
            mount,
            name: text(&name),
            result: result.map_err(|e| text(&e)),
        },
        CoreEvent::OpenChoices {
            path,
            kind,
            apps,
            always,
        } => CoreEvent::OpenChoices {
            path,
            kind: kind.map(|k| text(&k)),
            apps: apps.into_iter().map(app).collect(),
            always,
        },
        other => other,
    }
}

fn volume(mut v: Volume) -> Volume {
    v.label = v.label.map(|l| text(&l));
    v.fs_type = text(&v.fs_type);
    v.device = text(&v.device);
    v
}

fn unmounted(mut u: Unmounted) -> Unmounted {
    u.name = text(&u.name);
    u
}

fn app(mut a: OpenApp) -> OpenApp {
    a.name = text(&a.name);
    a
}

fn job(ev: JobEvent) -> JobEvent {
    match ev {
        JobEvent::PlanFailed { job, message } => JobEvent::PlanFailed {
            job,
            message: text(&message),
        },
        JobEvent::Error { job, message } => JobEvent::Error {
            job,
            message: text(&message),
        },
        JobEvent::AskFailure { job, info, reply } => JobEvent::AskFailure {
            job,
            info: FailureInfo {
                description: text(&info.description),
                message: text(&info.message),
                ..info
            },
            reply,
        },
        JobEvent::UndoLabel { job, label } => JobEvent::UndoLabel {
            job,
            label: label.map(|l| text(&l)),
        },
        other => journal(other),
    }
}

/// What the journal and the report of an operation say.
fn journal(ev: JobEvent) -> JobEvent {
    match ev {
        JobEvent::Finished {
            job,
            title,
            report,
            journal_id,
            journal_errors,
        } => JobEvent::Finished {
            job,
            title: text(&title),
            report: report_of(report),
            journal_id,
            journal_errors,
        },
        JobEvent::History { job, entries } => JobEvent::History {
            job,
            entries: entries
                .into_iter()
                .map(|mut e| {
                    e.title = text(&e.title);
                    e
                })
                .collect(),
        },
        JobEvent::Recovered { items } => JobEvent::Recovered {
            items: items.into_iter().map(recovered).collect(),
        },
        other => other,
    }
}

fn report_of(mut r: ExecReport) -> ExecReport {
    for (_, why) in r.kept.iter_mut().chain(r.not_preserved.iter_mut()) {
        *why = text(why);
    }
    r.failed = r
        .failed
        .into_iter()
        .map(|f| FailedStep {
            description: text(&f.description),
            error: text(&f.error),
            ..f
        })
        .collect();
    r
}

fn recovered(mut r: Recovered) -> Recovered {
    r.title = text(&r.title);
    r.notes.iter_mut().for_each(|n| *n = text(n));
    r
}

fn update(ev: UpdateEvent) -> UpdateEvent {
    match ev {
        UpdateEvent::Available { notice, how } => UpdateEvent::Available {
            notice: Notice {
                version: text(&notice.version),
                tag: text(&notice.tag),
                notes: block(&notice.notes),
                ..notice
            },
            how: how.map_err(|e| text(&e)),
        },
        UpdateEvent::CheckFailed(m) => UpdateEvent::CheckFailed(text(&m)),
        UpdateEvent::Progress(m) => UpdateEvent::Progress(text(&m)),
        UpdateEvent::Failed { message, log } => UpdateEvent::Failed {
            message: text(&message),
            log,
        },
        UpdateEvent::Done(a) => UpdateEvent::Done(Applied {
            version: text(&a.version),
            note: text(&a.note),
            ..a
        }),
        UpdateEvent::UpToDate => UpdateEvent::UpToDate,
    }
}

fn image(mut i: ImagePreview) -> ImagePreview {
    i.info.format = text(&i.info.format);
    if let Some(d) = i.info.doc.as_mut() {
        d.title = d.title.take().map(|t| text(&t));
        d.author = d.author.take().map(|a| text(&a));
    }
    i.note = i.note.take().map(|n| text(&n));
    i.state = match i.state {
        ImageState::TooLarge(m) => ImageState::TooLarge(text(&m)),
        ImageState::Failed(m) => ImageState::Failed(text(&m)),
        other => other,
    };
    i
}

/// What a file says about itself (the interpreter of a program, the title of a document) and
/// what the programs that read it answer. The lines of text are already prepared by the
/// preview itself, as are the names it lists.
fn preview(p: Preview) -> Preview {
    match p {
        Preview::Binary(mut b) => {
            if let Some(e) = b.card.exec.as_mut() {
                e.arch = text(&e.arch);
                e.kind = text(&e.kind);
                e.interpreter = e.interpreter.take().map(|i| text(&i));
            }
            Preview::Binary(b)
        }
        Preview::Image(i) => Preview::Image(image(i)),
        Preview::Archive(mut a) => {
            a.format = text(&a.format);
            a.note = a.note.take().map(|n| text(&n));
            Preview::Archive(a)
        }
        Preview::Symlink {
            target,
            state,
            inner,
        } => Preview::Symlink {
            target,
            state,
            inner: inner.map(|i| Box::new(preview(*i))),
        },
        Preview::Special(m) => Preview::Special(text(&m)),
        Preview::Error(m) => Preview::Error(text(&m)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rada_core::display::is_dangerous;
    use rada_core::git::GitStatus;
    use std::path::PathBuf;

    const NASTY: &str = "a\x1b[31m\x1b]52;c;ZXZpbA==\x07\nb\r\u{85}\u{202e}c";

    fn clean_of(s: &str) -> bool {
        !s.chars().any(is_dangerous) && s.contains("\\e[31m")
    }

    #[test]
    fn what_a_job_reports_is_cleaned() {
        let (reply, _) = crossbeam_channel::unbounded();
        let JobEvent::AskFailure { info, .. } = job(JobEvent::AskFailure {
            job: 1,
            info: FailureInfo {
                index: 0,
                description: NASTY.into(),
                message: NASTY.into(),
                path: PathBuf::from("x"),
                attempt: 1,
            },
            reply,
        }) else {
            unreachable!()
        };
        assert!(clean_of(&info.description) && clean_of(&info.message));

        for ev in [
            JobEvent::PlanFailed {
                job: 1,
                message: NASTY.into(),
            },
            JobEvent::Error {
                job: 1,
                message: NASTY.into(),
            },
        ] {
            let (JobEvent::PlanFailed { message, .. } | JobEvent::Error { message, .. }) = job(ev)
            else {
                unreachable!()
            };
            assert!(clean_of(&message));
        }
        let JobEvent::UndoLabel { label, .. } = job(JobEvent::UndoLabel {
            job: 1,
            label: Some(NASTY.into()),
        }) else {
            unreachable!()
        };
        assert!(clean_of(&label.unwrap()));
    }

    #[test]
    fn the_reasons_in_a_report_are_cleaned() {
        let report = ExecReport {
            kept: vec![(PathBuf::from("k"), NASTY.into())],
            not_preserved: vec![(PathBuf::from("n"), NASTY.into())],
            failed: vec![FailedStep {
                index: 0,
                description: NASTY.into(),
                error: NASTY.into(),
                path: PathBuf::from("f"),
            }],
            ..Default::default()
        };
        let r = report_of(report);
        assert!(clean_of(&r.kept[0].1) && clean_of(&r.not_preserved[0].1));
        assert!(clean_of(&r.failed[0].description) && clean_of(&r.failed[0].error));
    }

    #[test]
    fn the_notes_of_a_release_keep_their_lines() {
        let UpdateEvent::Available { notice, .. } = update(UpdateEvent::Available {
            notice: Notice {
                version: "1".into(),
                tag: "v1".into(),
                prerelease: false,
                notes: format!("one\r\ntwo {NASTY}"),
            },
            how: Ok(rada_core::update::Mode::Binary),
        }) else {
            unreachable!()
        };
        let lines: Vec<&str> = notice.notes.lines().collect();
        assert_eq!(lines[0], "one");
        assert!(lines[1].starts_with("two ") && !lines[1].chars().any(is_dangerous));
    }

    #[test]
    fn a_branch_a_folder_name_and_a_program_name_are_cleaned() {
        let CoreEvent::Git { status, .. } = clean(CoreEvent::Git {
            dir: PathBuf::from("d"),
            status: Some(GitStatus {
                branch: NASTY.into(),
                ..Default::default()
            }),
        }) else {
            unreachable!()
        };
        assert!(clean_of(&status.unwrap().branch));

        let CoreEvent::Watch(WatchEvent::Failed(m)) =
            clean(CoreEvent::Watch(WatchEvent::Failed(NASTY.into())))
        else {
            unreachable!()
        };
        assert!(clean_of(&m));

        let mut lists = rada_core::places::PathLists::default();
        lists.names.insert(PathBuf::from("p"), NASTY.into());
        let CoreEvent::Paths(p) = clean(CoreEvent::Paths(lists)) else {
            unreachable!()
        };
        assert!(clean_of(&p.names[&PathBuf::from("p")]));
    }

    #[test]
    fn the_answers_of_a_preview_are_cleaned() {
        let Preview::Special(m) = preview(Preview::Special(NASTY.into())) else {
            unreachable!()
        };
        assert!(clean_of(&m));
        let Preview::Error(m) = preview(Preview::Error(NASTY.into())) else {
            unreachable!()
        };
        assert!(clean_of(&m));
        let Preview::Symlink { inner, .. } = preview(Preview::Symlink {
            target: PathBuf::from("t"),
            state: rada_core::ops::LinkState::ToFile,
            inner: Some(Box::new(Preview::Error(NASTY.into()))),
        }) else {
            unreachable!()
        };
        assert!(matches!(*inner.unwrap(), Preview::Error(m) if clean_of(&m)));
    }
}
