//! Updates as the interface sees them: a notice in the status bar when a newer version
//! exists, a window with its release notes, and the progress of the update itself. The work
//! is done by the update worker of `rada-core`; nothing here waits for it.

use rada_core::update::{Applied, Ctx, Mode, Notice, UpdateEvent};

use super::{App, Modal, PropsView, ToastKind};
use crate::fmt;
use crate::keymap::Action;
use crate::services::Services;
use crossterm::event::{KeyCode, KeyEvent};
use rada_core::events::CoreEvent;
use rada_core::i18n::{Key, tf};
use std::path::PathBuf;
use std::time::Instant;

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

/// What the interface knows about updates.
pub struct Updates {
    /// The version of this program, as the status bar shows it (the update worker's, when there
    /// is one; tests pin it so that a release does not change every screenshot).
    pub version: String,
    /// What the update worker last told.
    pub update: UpdateStatus,
    /// Somebody asked to look for updates and is waiting for the answer.
    pub(super) update_asked: bool,
}

impl Updates {
    pub fn new(svc: &Services) -> Updates {
        Updates {
            version: svc.updater.as_ref().map_or_else(
                || env!("CARGO_PKG_VERSION").to_string(),
                |u| u.ctx().current.to_string(),
            ),
            update: UpdateStatus::None,
            update_asked: false,
        }
    }
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
        self.updates.version = ctx.current.to_string();
        self.svc.updater = Some(rada_core::update::Updater::spawn(
            ctx,
            self.svc.notes.clone(),
        ));
    }

    pub(super) fn on_update_event(&mut self, ev: UpdateEvent) {
        match ev {
            UpdateEvent::Available { notice, how } => self.on_update_available(notice, how),
            UpdateEvent::UpToDate => self.on_up_to_date(),
            UpdateEvent::CheckFailed(e) => {
                if std::mem::take(&mut self.updates.update_asked) {
                    self.toast(ToastKind::Warn, tf(Key::TCannotLookUpdates, &[&e]), 6);
                }
            }
            UpdateEvent::Progress(step) => {
                let started = match &self.updates.update {
                    UpdateStatus::Working { started, .. } => *started,
                    _ => Instant::now(),
                };
                self.updates.update = UpdateStatus::Working { step, started };
            }
            UpdateEvent::Done(applied) => self.on_update_done(applied),
            UpdateEvent::Failed { message, log } => self.on_update_failed(message, log),
        }
    }

    fn on_update_available(&mut self, notice: Notice, how: Result<Mode, String>) {
        let asked = std::mem::take(&mut self.updates.update_asked);
        // Never replace the news of an update that is running or has just finished.
        if matches!(
            self.updates.update,
            UpdateStatus::Working { .. } | UpdateStatus::Done(_)
        ) {
            return;
        }
        self.updates.update = UpdateStatus::Available { notice, how };
        // Somebody asked: show what was found, not just a pill in a corner.
        if asked && self.windows.modal.is_none() {
            self.start_update_action();
        }
    }

    fn on_up_to_date(&mut self) {
        if std::mem::take(&mut self.updates.update_asked) {
            self.updates.update = UpdateStatus::None;
            self.toast(ToastKind::Ok, Key::TUpToDate.tr(), 4);
        }
        if matches!(self.updates.update, UpdateStatus::Working { .. }) {
            self.updates.update = UpdateStatus::None;
        }
    }

    fn on_update_done(&mut self, applied: Applied) {
        let text = if applied.restart {
            tf(Key::UpDoneRestart, &[&applied.version])
        } else {
            applied.note.clone()
        };
        self.toast(ToastKind::Ok, text, 10);
        self.updates.update = UpdateStatus::Done(applied);
    }

    fn on_update_failed(&mut self, message: String, log: PathBuf) {
        self.updates.update = UpdateStatus::Failed {
            message: message.clone(),
            log: log.clone(),
        };
        if self.windows.modal.is_none() {
            self.windows.modal = Some(Modal::Update(UpdateView::Failed { message, log }));
        } else {
            self.toast(ToastKind::Error, Key::TUpdateFailed.tr(), 6);
        }
    }

    /// The version of the running program.
    pub fn version(&self) -> String {
        self.updates.version.clone()
    }

    /// Ask the network now, whatever the day's check said (the version in the status bar, and
    /// "Check for updates" in the Help menu). The answer is a window with the news, or "rada
    /// is up to date".
    pub(super) fn check_updates_now(&mut self) {
        match &self.svc.updater {
            _ if matches!(self.updates.update, UpdateStatus::Working { .. }) => {
                self.toast(ToastKind::Info, Key::TUpdateRunning.tr(), 3);
            }
            Some(u) => {
                self.updates.update_asked = true;
                u.check_now();
                self.toast(ToastKind::Info, Key::TLookingNewer.tr(), 3);
            }
            None => self.toast(ToastKind::Info, Key::TUpdatesNA.tr(), 3),
        }
    }

    /// "About rada": the version, the channel it follows and how it would be updated.
    pub(super) fn open_about(&mut self) {
        let mut rows: Vec<(String, String)> = vec![(Key::AboutVersion.tr().into(), self.version())];
        match &self.svc.updater {
            Some(u) => rows.extend(updater_rows(u.ctx())),
            None => rows.push((Key::AboutUpdates.tr().into(), Key::AboutNA.tr().into())),
        }
        rows.push((
            Key::AboutSource.tr().into(),
            "github.com/FormikaDesk/rada-fm".into(),
        ));
        self.windows.modal = Some(Modal::Properties(PropsView {
            title: Key::AboutTitle.tr().into(),
            rows,
            scroll: 0,
        }));
    }

    /// The Update action: show the news, retry after a failure, or look for a new version.
    pub(super) fn start_update_action(&mut self) {
        match &self.updates.update {
            UpdateStatus::Available { notice, how } => {
                self.windows.modal = Some(Modal::Update(UpdateView::Offer {
                    notice: notice.clone(),
                    how: how.clone(),
                    scroll: 0,
                }));
            }
            UpdateStatus::Working { .. } => {
                self.toast(ToastKind::Info, Key::TUpdateRunning.tr(), 3);
            }
            UpdateStatus::Done(a) => {
                let text = if a.restart {
                    tf(Key::UpDoneRestart, &[&a.version])
                } else {
                    a.note.clone()
                };
                self.toast(ToastKind::Ok, text, 6);
            }
            UpdateStatus::Failed { message, log } => {
                self.windows.modal = Some(Modal::Update(UpdateView::Failed {
                    message: message.clone(),
                    log: log.clone(),
                }));
            }
            UpdateStatus::None => match &self.svc.updater {
                Some(u) => {
                    self.updates.update_asked = true;
                    u.check_now();
                    self.toast(ToastKind::Info, Key::TLookingNewer.tr(), 3);
                }
                None => self.toast(ToastKind::Info, Key::TUpdatesNA.tr(), 3),
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
                            self.updates.update = UpdateStatus::Working {
                                step: tf(Key::UpStarting, &[&notice.version]),
                                started: Instant::now(),
                            };
                            self.toast(ToastKind::Info, Key::TUpdatingBackground.tr(), 3);
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
        self.windows.modal = Some(Modal::Update(view));
    }

    /// Open a file with the system's program for it, from a thread of its own.
    fn open_in_background(&mut self, path: PathBuf) {
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        self.redraw.after_launch();
        std::thread::spawn(move || {
            if let Err(e) = platform.opener().open(&path) {
                let _ = notes.send(CoreEvent::Note {
                    text: tf(Key::TCannotOpenPath, &[&path.display(), &e]),
                    error: true,
                });
            }
        });
    }

    /// The notice for the status bar, from the longest wording to the shortest, and whether a
    /// click on it does something.
    pub fn update_notice(&self) -> Option<(Vec<String>, bool)> {
        let key = self.key_for(Action::Update);
        match &self.updates.update {
            UpdateStatus::None => None,
            UpdateStatus::Available { notice, .. } => {
                Some((available_wording(&notice.version, &key), true))
            }
            UpdateStatus::Working { step, started } => {
                let t = fmt::duration(started.elapsed());
                Some((
                    vec![
                        tf(Key::UpUpdatingStep, &[&step, &t]),
                        tf(Key::UpUpdating, &[&t]),
                    ],
                    false,
                ))
            }
            UpdateStatus::Done(a) if a.restart => Some((
                vec![
                    tf(Key::UpInstalledLong, &[&a.version]),
                    Key::UpRestart.tr().into(),
                ],
                false,
            )),
            UpdateStatus::Done(_) => None,
            UpdateStatus::Failed { .. } => Some((
                vec![
                    tf(Key::UpFailedLong, &[&press(&key, Key::UpWordSeeWhy.tr())]),
                    tf(Key::UpFailedShort, &[&dot(&key)]),
                ],
                true,
            )),
        }
    }
}

/// The words for a new version, the longest first.
fn available_wording(version: &str, key: &Option<String>) -> Vec<String> {
    vec![
        tf(
            Key::UpNewVersion,
            &[&version, &press(key, Key::UpWordUpdate.tr())],
        ),
        format!("{version}{}", dot(key)),
        version.to_string(),
    ]
}

/// "press U to update", when the Update action has a key.
fn press(key: &Option<String>, what: &str) -> String {
    match key {
        Some(k) => tf(Key::UpPress, &[k, &what]),
        None => String::new(),
    }
}

/// " · U", when the Update action has a key.
fn dot(key: &Option<String>) -> String {
    key.as_ref().map(|k| format!(" · {k}")).unwrap_or_default()
}

/// The rows of "About" that come from the update worker: the channel followed, how an update
/// would be made, how often it looks, and which program this is.
fn updater_rows(ctx: &Ctx) -> Vec<(String, String)> {
    use rada_core::update::Channel;
    let followed = rada_core::update::resolve_channel(ctx.settings.channel, &ctx.current);
    let name = match followed {
        Channel::Prerelease => Key::AboutPre.tr(),
        _ => Key::AboutStable.tr(),
    };
    let why = if ctx.settings.channel == Channel::Auto {
        Key::AboutChosenVersion.tr()
    } else {
        Key::AboutSetInConfig.tr()
    };
    vec![
        (
            Key::AboutChannel.tr().into(),
            tf(Key::AboutChannelValue, &[&name, &why]),
        ),
        (
            Key::AboutMode.tr().into(),
            match ctx.mode() {
                Ok(Mode::Binary) => Key::AboutBinary.tr().into(),
                Ok(Mode::Source { dir }) => tf(Key::AboutSourceMode, &[&dir.display()]),
                Ok(Mode::Package { hint }) => hint,
                Err(e) => e,
            },
        ),
        (
            Key::AboutChecks.tr().into(),
            if ctx.settings.check {
                Key::AboutDaily.tr().into()
            } else {
                Key::AboutOnlyAsked.tr().into()
            },
        ),
        (
            Key::AboutProgram.tr().into(),
            rada_core::display::path(&ctx.exe),
        ),
    ]
}
