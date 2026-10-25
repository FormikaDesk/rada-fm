//! What is put over the explorer: the window that is open, the cards of notifications, the
//! address bar while it is edited, the scroll of the help and the advice to enlarge a small
//! window.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use super::{AddressEdit, App, Modal, Notice, NoticeKind, NoticeQueue, ToastKind};

#[derive(Default)]
pub struct Windows {
    /// The window that is open and has the keyboard, if any.
    pub modal: Option<Modal>,
    /// What to tell the person: a card at the bottom right, one at a time (see `notice.rs`).
    pub notices: NoticeQueue,
    pub help_scroll: usize,
    /// The address bar while its text is being edited.
    pub address: Option<AddressEdit>,
    /// The advice to enlarge a small window was closed (for the rest of this session).
    pub size_notice_dismissed: bool,
}

impl Windows {
    pub fn help_down(&mut self, by: usize) {
        self.help_scroll += by;
    }

    pub fn help_up(&mut self, by: usize) {
        self.help_scroll = self.help_scroll.saturating_sub(by);
    }
}

impl App {
    pub(super) fn on_note(&mut self, text: String, error: bool) {
        let kind = if error {
            ToastKind::Error
        } else {
            ToastKind::Info
        };
        self.toast(kind, text, 5);
    }

    /// The time notifications go by: the clock, and what tests have moved it on.
    pub fn notice_now(&self) -> Instant {
        Instant::now() + self.hooks.notice_skew
    }

    /// Move the notifications' clock on (tests and screenshots: nobody waits six seconds).
    pub fn advance_notices(&mut self, by: Duration) {
        self.hooks.notice_skew += by;
        self.tick_notices();
    }

    pub(super) fn tick_notices(&mut self) {
        let now = self.notice_now();
        if self.windows.notices.tick(now) {
            self.redraw.dirty = true;
        }
    }

    /// A word about what happened, as a card. `secs` is how long a long message needs; a
    /// short one goes after about six seconds, an error stays until it is closed.
    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>, secs: u64) {
        let kind = match kind {
            ToastKind::Info => NoticeKind::Info,
            ToastKind::Ok => NoticeKind::Ok,
            ToastKind::Warn => NoticeKind::Warn,
            ToastKind::Error => NoticeKind::Error,
        };
        let ttl = Notice::TTL.max(Duration::from_secs(secs));
        self.notify(Notice::new(kind, text).lasting(ttl));
    }

    /// Queue a notification.
    pub fn notify(&mut self, notice: Notice) {
        let now = self.notice_now();
        self.windows.notices.push(notice, now);
        self.redraw.dirty = true;
    }

    /// Take every notification away (tests: so that what comes next is not a left-over).
    pub fn clear_notices(&mut self) {
        self.windows.notices = NoticeQueue::default();
        self.redraw.dirty = true;
    }

    /// The oldest notification on screen.
    pub fn current_notice(&self) -> Option<&Notice> {
        self.windows.notices.front()
    }

    /// Every notification on screen, the oldest first.
    pub fn shown_notices(&self) -> Vec<&Notice> {
        self.windows
            .notices
            .shown(self.notice_now())
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    }

    pub(super) fn on_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.windows.help_down(1);
                self.windows.modal = Some(Modal::Help);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.windows.help_up(1);
                self.windows.modal = Some(Modal::Help);
            }
            KeyCode::PageDown => {
                self.windows.help_down(10);
                self.windows.modal = Some(Modal::Help);
            }
            KeyCode::PageUp => {
                self.windows.help_up(10);
                self.windows.modal = Some(Modal::Help);
            }
            KeyCode::Home => {
                self.windows.help_scroll = 0;
                self.windows.modal = Some(Modal::Help);
            }
            // Closing is deliberate: Esc, q, ? , F1, Enter or Space.
            KeyCode::Esc | KeyCode::Char('q' | '?' | ' ') | KeyCode::F(1) | KeyCode::Enter => {}
            _ => self.windows.modal = Some(Modal::Help),
        }
    }
}
