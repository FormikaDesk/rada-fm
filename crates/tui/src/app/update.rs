//! Updates as the interface sees them: a notice in the status bar when a newer version
//! exists, a window with its release notes, and the progress of the update itself. The work
//! is done by the update worker of `rada-core`; nothing here waits for it.

use rada_core::update::{Applied, Mode, Notice, UpdateEvent};

use super::*;

/// What the update worker last told.
pub enum UpdateStatus {
    /// Nothing known (no check yet, or up to date).
    None,
    Available {
        notice: Notice,
        how: Result<Mode, String>,
    },
    /// An update is running; `step` is what it is doing now.
    Working {
        step: String,
        started: Instant,
    },
    Done(Applied),
    Failed {
        message: String,
        log: PathBuf,
    },
}

/// The window of the update.
pub enum UpdateView {
    /// The release notes and the question.
    Offer {
        notice: Notice,
        how: Result<Mode, String>,
        scroll: usize,
    },
    Failed {
        message: String,
        log: PathBuf,
    },
}

impl App {
    /// Start the update worker (the program does this through [`Services`]; tests through here).
    pub fn start_updates(&mut self, ctx: rada_core::update::Ctx) {
        self.version = ctx.current.to_string();
        self.svc.updater = Some(rada_core::update::Updater::spawn(
            ctx,
            self.svc.notes.clone(),
        ));
    }

    pub(super) fn on_update_event(&mut self, ev: UpdateEvent) {
        match ev {
            UpdateEvent::Available { notice, how } => {
                let asked = std::mem::take(&mut self.update_asked);
                // Never replace the news of an update that is running or has just finished.
                if matches!(
                    self.update,
                    UpdateStatus::Working { .. } | UpdateStatus::Done(_)
                ) {
                    return;
                }
                self.update = UpdateStatus::Available { notice, how };
                // Somebody asked: show what was found, not just a pill in a corner.
                if asked && self.modal.is_none() {
                    self.start_update_action();
                }
            }
            UpdateEvent::UpToDate => {
                if std::mem::take(&mut self.update_asked) {
                    self.update = UpdateStatus::None;
                    self.toast(ToastKind::Ok, "rada is up to date", 4);
                }
                if matches!(self.update, UpdateStatus::Working { .. }) {
                    self.update = UpdateStatus::None;
                }
            }
            UpdateEvent::CheckFailed(e) => {
                if std::mem::take(&mut self.update_asked) {
                    self.toast(ToastKind::Warn, format!("cannot look for updates: {e}"), 6);
                }
            }
            UpdateEvent::Progress(step) => {
                let started = match &self.update {
                    UpdateStatus::Working { started, .. } => *started,
                    _ => Instant::now(),
                };
                self.update = UpdateStatus::Working { step, started };
            }
            UpdateEvent::Done(applied) => {
                let text = if applied.restart {
                    format!(
                        "rada {} is installed: start rada again to use it",
                        applied.version
                    )
                } else {
                    applied.note.clone()
                };
                self.toast(ToastKind::Ok, text, 10);
                self.update = UpdateStatus::Done(applied);
            }
            UpdateEvent::Failed { message, log } => {
                self.update = UpdateStatus::Failed {
                    message: message.clone(),
                    log: log.clone(),
                };
                if self.modal.is_none() {
                    self.modal = Some(Modal::Update(UpdateView::Failed { message, log }));
                } else {
                    self.toast(ToastKind::Error, "the update did not work", 6);
                }
            }
        }
    }

    /// The version of the running program.
    pub fn version(&self) -> String {
        self.version.clone()
    }

    /// Ask the network now, whatever the day's check said (the version in the status bar, and
    /// "Check for updates" in the Help menu). The answer is a window with the news, or "rada
    /// is up to date".
    pub(super) fn check_updates_now(&mut self) {
        match &self.svc.updater {
            _ if matches!(self.update, UpdateStatus::Working { .. }) => {
                self.toast(ToastKind::Info, "an update is already running", 3);
            }
            Some(u) => {
                self.update_asked = true;
                u.check_now();
                self.toast(ToastKind::Info, "looking for a newer version…", 3);
            }
            None => self.toast(ToastKind::Info, "updates are not available here", 3),
        }
    }

    /// "About rada": the version, the channel it follows and how it would be updated.
    pub(super) fn open_about(&mut self) {
        let mut rows: Vec<(String, String)> = vec![("Version".into(), self.version())];
        match &self.svc.updater {
            Some(u) => {
                use rada_core::update::Channel;
                let ctx = u.ctx();
                let followed =
                    rada_core::update::resolve_channel(ctx.settings.channel, &ctx.current);
                let name = match followed {
                    Channel::Prerelease => "pre-releases",
                    _ => "stable releases",
                };
                let why = if ctx.settings.channel == Channel::Auto {
                    "chosen from the version"
                } else {
                    "set in the configuration"
                };
                rows.push(("Channel".into(), format!("{name} ({why})")));
                rows.push((
                    "Mode".into(),
                    match ctx.mode() {
                        Ok(Mode::Binary) => {
                            "a program downloaded from the releases — it replaces itself".into()
                        }
                        Ok(Mode::Source { dir }) => format!(
                            "built from the git checkout in {} — it pulls and builds",
                            dir.display()
                        ),
                        Ok(Mode::Package { hint }) => hint,
                        Err(e) => e,
                    },
                ));
                rows.push((
                    "Checks".into(),
                    if ctx.settings.check {
                        "once a day in the background".into()
                    } else {
                        "only when asked (check = false)".into()
                    },
                ));
                rows.push(("Program".into(), rada_core::display::path(&ctx.exe)));
            }
            None => rows.push(("Updates".into(), "not available here".into())),
        }
        rows.push(("Source".into(), "github.com/FormikaDesk/rada-fm".into()));
        self.modal = Some(Modal::Properties(PropsView {
            title: "About rada".into(),
            rows,
            scroll: 0,
        }));
    }

    /// The Update action: show the news, retry after a failure, or look for a new version.
    pub(super) fn start_update_action(&mut self) {
        match &self.update {
            UpdateStatus::Available { notice, how } => {
                self.modal = Some(Modal::Update(UpdateView::Offer {
                    notice: notice.clone(),
                    how: how.clone(),
                    scroll: 0,
                }));
            }
            UpdateStatus::Working { .. } => {
                self.toast(ToastKind::Info, "an update is already running", 3);
            }
            UpdateStatus::Done(a) => {
                let text = if a.restart {
                    format!(
                        "rada {} is installed: start rada again to use it",
                        a.version
                    )
                } else {
                    a.note.clone()
                };
                self.toast(ToastKind::Ok, text, 6);
            }
            UpdateStatus::Failed { message, log } => {
                self.modal = Some(Modal::Update(UpdateView::Failed {
                    message: message.clone(),
                    log: log.clone(),
                }));
            }
            UpdateStatus::None => match &self.svc.updater {
                Some(u) => {
                    self.update_asked = true;
                    u.check_now();
                    self.toast(ToastKind::Info, "looking for a newer version…", 3);
                }
                None => self.toast(ToastKind::Info, "updates are not available here", 3),
            },
        }
    }

    pub(super) fn on_update_key(&mut self, mut view: UpdateView, key: KeyEvent) {
        match &mut view {
            UpdateView::Offer {
                notice,
                how,
                scroll,
            } => match key.code {
                KeyCode::Enter | KeyCode::Char('y') => match how {
                    Ok(Mode::Package { .. }) | Err(_) => {}
                    Ok(_) => {
                        if let Some(u) = &self.svc.updater {
                            u.apply();
                            self.update = UpdateStatus::Working {
                                step: format!("starting the update to {}", notice.version),
                                started: Instant::now(),
                            };
                            self.toast(ToastKind::Info, "updating in the background", 3);
                        }
                        return;
                    }
                },
                KeyCode::Esc | KeyCode::Char('q') => return,
                KeyCode::Down | KeyCode::Char('j') => *scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => *scroll += 10,
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                _ => {}
            },
            UpdateView::Failed { log, .. } => match key.code {
                KeyCode::Char('o') => {
                    self.open_in_background(log.clone());
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => return,
                _ => {}
            },
        }
        self.modal = Some(Modal::Update(view));
    }

    /// Open a file with the system's program for it, from a thread of its own.
    fn open_in_background(&mut self, path: PathBuf) {
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        std::thread::spawn(move || {
            if let Err(e) = platform.opener().open(&path) {
                let _ = notes.send(CoreEvent::Note {
                    text: format!("cannot open {}: {e}", path.display()),
                    error: true,
                });
            }
        });
    }

    /// The notice for the status bar, from the longest wording to the shortest, and whether a
    /// click on it does something.
    pub fn update_notice(&self) -> Option<(Vec<String>, bool)> {
        let key = self.key_for(Action::Update);
        let press = |what: &str| match &key {
            Some(k) => format!(" — press {k} to {what}"),
            None => String::new(),
        };
        let dot = |k: &Option<String>| k.as_ref().map(|k| format!(" · {k}")).unwrap_or_default();
        match &self.update {
            UpdateStatus::None => None,
            UpdateStatus::Available { notice, .. } => Some((
                vec![
                    format!(
                        "New version {} available{}",
                        notice.version,
                        press("update")
                    ),
                    format!("{}{}", notice.version, dot(&key)),
                    notice.version.clone(),
                ],
                true,
            )),
            UpdateStatus::Working { step, started } => {
                let t = fmt::duration(started.elapsed());
                Some((
                    vec![
                        format!("Updating: {step} ({t})"),
                        format!("Updating… ({t})"),
                    ],
                    false,
                ))
            }
            UpdateStatus::Done(a) if a.restart => Some((
                vec![
                    format!("rada {} installed — start rada again to use it", a.version),
                    "restart rada".into(),
                ],
                false,
            )),
            UpdateStatus::Done(_) => None,
            UpdateStatus::Failed { .. } => Some((
                vec![
                    format!("The update did not work{}", press("see why")),
                    format!("Update failed{}", dot(&key)),
                ],
                true,
            )),
        }
    }
}
