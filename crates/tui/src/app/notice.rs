//! Notifications: a small card at the bottom right that says what just happened, with the
//! way to take it back and the way to see it.
//!
//! They queue: one is shown at a time, the next appears when the one in front is gone. A
//! notification goes by itself after about six seconds, or at the first key pressed after it
//! has been on screen a moment (the key is not taken: it does what it would have done). It
//! never takes the keyboard. An error stays until it is closed (a click, or Esc).
//!
//! The queue knows nothing of the clock: every call is given the time, so that the tests
//! run the seconds without waiting for them.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How a notification looks and how long it lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    /// Something that is only said.
    Info,
    /// Something was done.
    Ok,
    Warn,
    /// Stays until closed.
    Error,
}

/// The way back offered under the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    None,
    /// "Undo  Ctrl+Z"
    Undo,
    /// "Redo  Ctrl+Y", after an undo.
    Redo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub kind: NoticeKind,
    /// "3 items copied to Documents".
    pub text: String,
    pub action: NoticeAction,
    /// Where "Show" goes: the folder the result is in.
    pub show: Option<PathBuf>,
    /// The name to put the cursor on when going there.
    pub select: Option<std::ffi::OsString>,
    /// How long it stays on screen once it is in front (never, for an error).
    pub ttl: Duration,
}

impl Notice {
    /// The time a notification stays: about six seconds.
    pub const TTL: Duration = Duration::from_secs(6);
    /// A key pressed sooner than this after the notification appeared does not close it (it
    /// may have been typed before it was seen).
    pub const MIN_SHOWN: Duration = Duration::from_millis(500);

    pub fn new(kind: NoticeKind, text: impl Into<String>) -> Notice {
        Notice {
            kind,
            text: text.into(),
            action: NoticeAction::None,
            show: None,
            select: None,
            ttl: Notice::TTL,
        }
    }

    pub fn with_action(mut self, action: NoticeAction) -> Notice {
        self.action = action;
        self
    }

    pub fn showing(mut self, folder: PathBuf, select: Option<std::ffi::OsString>) -> Notice {
        self.show = Some(folder);
        self.select = select;
        self
    }

    pub fn lasting(mut self, ttl: Duration) -> Notice {
        self.ttl = ttl;
        self
    }

    /// Whether it stays until somebody closes it.
    pub fn is_sticky(&self) -> bool {
        self.kind == NoticeKind::Error
    }

    /// Only said, nothing to click: a later one of the same sort replaces it.
    fn is_plain(&self) -> bool {
        matches!(self.kind, NoticeKind::Info | NoticeKind::Warn)
            && self.action == NoticeAction::None
            && self.show.is_none()
    }
}

struct Slot {
    notice: Notice,
    /// When it came to the front; `None` while it waits.
    since: Option<Instant>,
}

/// Most notifications kept waiting; the oldest that is not an error is dropped past it.
const MAX_WAITING: usize = 8;

#[derive(Default)]
pub struct NoticeQueue {
    slots: VecDeque<Slot>,
}

impl NoticeQueue {
    /// Add a notification at the end. The same text twice in a row is one; a plain one
    /// replaces a plain one that has not been seen yet (pressing a key that sorts twice must
    /// not queue two messages).
    pub fn push(&mut self, notice: Notice, now: Instant) {
        // Something to act on (a result, an error) makes the chatter before it stale: "1 item
        // copied, go to the destination…" means nothing once it has been pasted.
        if !notice.is_plain() && self.slots.iter().any(|s| s.notice.is_plain()) {
            let front_was_plain = self.slots.front().is_some_and(|s| s.notice.is_plain());
            self.slots.retain(|s| !s.notice.is_plain());
            if front_was_plain && let Some(next) = self.slots.front_mut() {
                next.since = Some(now);
            }
        }
        if let Some(last) = self.slots.back_mut() {
            if last.notice == notice {
                // Said again: the time starts over.
                if last.since.is_some() {
                    last.since = Some(now);
                }
                return;
            }
            let replaceable = notice.is_plain() && last.notice.is_plain();
            if replaceable {
                let since = last.since.map(|_| now);
                *last = Slot { notice, since };
                return;
            }
        }
        while self.slots.len() >= MAX_WAITING {
            // Drop the oldest that is neither in front nor an error.
            match self
                .slots
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, s)| !s.notice.is_sticky())
            {
                Some((i, _)) => {
                    self.slots.remove(i);
                }
                None => break,
            }
        }
        let since = self.slots.is_empty().then_some(now);
        self.slots.push_back(Slot { notice, since });
    }

    /// The notification on screen.
    pub fn front(&self) -> Option<&Notice> {
        self.slots.front().map(|s| &s.notice)
    }

    /// How many are waiting behind the one in front.
    pub fn waiting(&self) -> usize {
        self.slots.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Time passes: the one in front goes when its time is up (an error does not), and the
    /// next one takes its place. Returns whether anything changed on screen.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(front) = self.slots.front() else {
            return false;
        };
        let Some(since) = front.since else {
            // Nothing is in front yet (cannot happen: the first one starts at once).
            self.slots.front_mut().unwrap().since = Some(now);
            return true;
        };
        if front.notice.is_sticky() || now.saturating_duration_since(since) < front.notice.ttl {
            return false;
        }
        self.pop(now);
        true
    }

    /// A key was pressed: the notification in front goes, unless it is an error (those wait
    /// for Esc, see [`NoticeQueue::dismiss_error`]) or it appeared a moment ago. The key
    /// itself is not touched. Returns whether the card went.
    pub fn on_key(&mut self, now: Instant) -> bool {
        let Some(front) = self.slots.front() else {
            return false;
        };
        let Some(since) = front.since else {
            return false;
        };
        if front.notice.is_sticky() || now.saturating_duration_since(since) < Notice::MIN_SHOWN {
            return false;
        }
        self.pop(now);
        true
    }

    /// Esc: closes an error in front (and says so, so that the key is used up).
    pub fn dismiss_error(&mut self, now: Instant) -> bool {
        if self.slots.front().is_some_and(|s| s.notice.is_sticky()) {
            self.pop(now);
            true
        } else {
            false
        }
    }

    /// Close the one in front, whatever it is (a click on it).
    pub fn dismiss(&mut self, now: Instant) {
        self.pop(now);
    }

    fn pop(&mut self, now: Instant) {
        self.slots.pop_front();
        if let Some(next) = self.slots.front_mut() {
            next.since = Some(now);
        }
    }

    /// Seconds left of the one in front (for the tests and the screenshots).
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        let s = self.slots.front()?;
        if s.notice.is_sticky() {
            return None;
        }
        let since = s.since?;
        Some(
            s.notice
                .ttl
                .saturating_sub(now.saturating_duration_since(since)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Notice {
        Notice::new(NoticeKind::Ok, text).with_action(NoticeAction::Undo)
    }

    fn secs(base: Instant, s: f64) -> Instant {
        base + Duration::from_secs_f64(s)
    }

    #[test]
    fn a_notification_goes_by_itself_after_six_seconds() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("3 items copied to Documents"), t0);
        assert_eq!(
            q.front().map(|n| n.text.as_str()),
            Some("3 items copied to Documents")
        );
        assert!(!q.tick(secs(t0, 5.9)), "still there at 5.9 s");
        assert!(q.front().is_some());
        assert!(q.tick(secs(t0, 6.0)), "gone at 6 s");
        assert!(q.front().is_none());
        assert!(!q.tick(secs(t0, 7.0)), "nothing left to change");
    }

    #[test]
    fn the_first_key_closes_it_but_not_a_key_typed_before_it_was_seen() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("moved"), t0);
        assert!(
            !q.on_key(secs(t0, 0.1)),
            "typed before it was on screen long enough"
        );
        assert!(q.front().is_some());
        assert!(
            q.on_key(secs(t0, 1.0)),
            "the first key after that closes it"
        );
        assert!(q.front().is_none());
        assert!(!q.on_key(secs(t0, 2.0)), "no card, nothing to close");
    }

    #[test]
    fn notifications_queue_and_each_gets_its_own_six_seconds() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("one"), t0);
        q.push(ok("two"), secs(t0, 1.0));
        q.push(ok("three"), secs(t0, 2.0));
        assert_eq!(q.front().unwrap().text, "one");
        assert_eq!(q.waiting(), 2);
        assert!(q.tick(secs(t0, 6.0)));
        assert_eq!(q.front().unwrap().text, "two");
        // "two" came to the front at 6 s: it lasts until 12 s, not 7 s.
        assert!(!q.tick(secs(t0, 11.9)));
        assert!(q.tick(secs(t0, 12.0)));
        assert_eq!(q.front().unwrap().text, "three");
        // A key closes the one in front and brings the next, which starts afresh.
        assert!(q.on_key(secs(t0, 13.0)));
        assert!(q.front().is_none());
    }

    #[test]
    fn a_key_brings_the_next_one_and_it_is_not_closed_by_the_same_key() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("one"), t0);
        q.push(ok("two"), t0);
        assert!(q.on_key(secs(t0, 1.0)));
        assert_eq!(q.front().unwrap().text, "two");
        assert!(!q.on_key(secs(t0, 1.1)), "the new card has just appeared");
        assert!(q.on_key(secs(t0, 2.0)));
    }

    #[test]
    fn an_error_stays_until_it_is_closed() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(
            Notice::new(NoticeKind::Error, "cannot copy: permission denied"),
            t0,
        );
        q.push(ok("later"), t0);
        assert!(!q.tick(secs(t0, 3600.0)), "an hour later it is still there");
        assert!(!q.on_key(secs(t0, 3600.0)), "a key does not close an error");
        assert_eq!(q.front().unwrap().kind, NoticeKind::Error);
        assert_eq!(q.remaining(secs(t0, 5.0)), None);
        assert!(q.dismiss_error(secs(t0, 3601.0)), "Esc closes it");
        assert_eq!(q.front().unwrap().text, "later");
        assert!(
            !q.dismiss_error(secs(t0, 3602.0)),
            "Esc leaves the others alone"
        );
    }

    #[test]
    fn what_is_said_twice_in_a_row_is_one_card_that_starts_over() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("copied"), t0);
        q.push(ok("copied"), secs(t0, 4.0));
        assert_eq!(q.waiting(), 0);
        // The time started over at 4 s: it is still there at 9 s, gone at 10 s.
        assert!(!q.tick(secs(t0, 9.0)));
        assert!(q.tick(secs(t0, 10.0)));
    }

    #[test]
    fn plain_messages_replace_each_other_instead_of_queuing() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        for i in 0..5 {
            q.push(
                Notice::new(NoticeKind::Info, format!("sorted by Name {i}")),
                secs(t0, i as f64 * 0.1),
            );
        }
        assert_eq!(q.waiting(), 0, "the last one wins");
        assert_eq!(q.front().unwrap().text, "sorted by Name 4");
        // A result with something to click takes their place; two results queue.
        q.push(ok("moved"), secs(t0, 1.0));
        assert_eq!(q.front().unwrap().text, "moved");
        q.push(ok("copied"), secs(t0, 1.1));
        assert_eq!(q.waiting(), 1);
    }

    #[test]
    fn a_result_makes_the_chatter_before_it_stale() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(
            Notice::new(NoticeKind::Info, "1 item copied — go to the destination"),
            t0,
        );
        q.push(
            Notice::new(NoticeKind::Warn, "nothing to paste"),
            secs(t0, 0.1),
        );
        q.push(ok("1 item copied to Documents"), secs(t0, 2.0));
        assert_eq!(q.front().unwrap().text, "1 item copied to Documents");
        assert_eq!(q.waiting(), 0, "the chatter is gone, not waiting");
        // The result is on screen from the moment it came: it gets its six seconds from there.
        assert!(!q.tick(secs(t0, 7.9)));
        assert!(q.tick(secs(t0, 8.0)));
        // A result in front is left alone by later chatter, which waits behind it? No: it
        // replaces nothing and queues.
        q.push(ok("moved"), secs(t0, 9.0));
        q.push(
            Notice::new(NoticeKind::Info, "sorted by name"),
            secs(t0, 9.5),
        );
        assert_eq!(q.front().unwrap().text, "moved");
        assert_eq!(q.waiting(), 1);
    }

    #[test]
    fn the_queue_is_bounded_and_never_drops_an_error() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(Notice::new(NoticeKind::Error, "first"), t0);
        for i in 0..20 {
            q.push(ok(&format!("n{i}")), t0);
        }
        assert!(q.waiting() < MAX_WAITING);
        assert_eq!(q.front().unwrap().text, "first");
        q.push(Notice::new(NoticeKind::Error, "second"), t0);
        // Whatever was dropped, the errors are still queued.
        assert!(q.dismiss_error(t0));
        let mut seen = Vec::new();
        while let Some(n) = q.front() {
            seen.push(n.text.clone());
            q.dismiss(t0);
        }
        assert!(seen.contains(&"second".to_string()), "{seen:?}");
    }

    #[test]
    fn remaining_time_counts_down_from_six_seconds() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("x"), t0);
        assert_eq!(q.remaining(t0), Some(Duration::from_secs(6)));
        assert_eq!(
            q.remaining(secs(t0, 2.5)),
            Some(Duration::from_millis(3500))
        );
        assert_eq!(q.remaining(secs(t0, 10.0)), Some(Duration::ZERO));
    }
}

// ---------------------------------------------------------------------------------- the app

use rada_core::i18n::{Key, tf};

use super::*;

/// Where a finished operation left its result, for the "Show" of its card.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// The folder the result is in.
    pub folder: Option<PathBuf>,
    /// The item to put the cursor on there.
    pub select: Option<std::ffi::OsString>,
    /// What the result is called, where the message names it (a new folder, an archive).
    pub subject: String,
}

impl App {
    /// What a plan will leave behind and where, worked out before it runs.
    pub(super) fn outcome_of(&self, plan: &Plan) -> Outcome {
        use rada_core::ops::Step;
        let name_of = |p: &Path| {
            p.file_name()
                .map(std::ffi::OsStr::to_os_string)
                .unwrap_or_default()
        };
        match plan.kind {
            OpKind::Copy | OpKind::Move | OpKind::Extract => Outcome {
                folder: plan.destination.clone(),
                ..Outcome::default()
            },
            OpKind::Compress => {
                let archive = plan.destination.clone().unwrap_or_default();
                Outcome {
                    folder: archive.parent().map(Path::to_path_buf),
                    subject: rada_core::display::name(&name_of(&archive)),
                    select: Some(name_of(&archive)),
                }
            }
            OpKind::Rename | OpKind::BulkRename => {
                let first = plan.steps.iter().find_map(|s| match s {
                    Step::Rename { to, .. } => Some(to.clone()),
                    _ => None,
                });
                Outcome {
                    folder: first
                        .as_deref()
                        .and_then(Path::parent)
                        .map(Path::to_path_buf),
                    select: first.as_deref().map(name_of),
                    subject: first
                        .as_deref()
                        .map(|p| rada_core::display::name(&name_of(p)))
                        .unwrap_or_default(),
                }
            }
            OpKind::MakeDir | OpKind::MakeFile => {
                let made = plan.steps.iter().find_map(|s| match s {
                    Step::MakeDir { path, .. } | Step::MakeFile { path, .. } => Some(path.clone()),
                    _ => None,
                });
                Outcome {
                    folder: plan.destination.clone(),
                    select: made.as_deref().map(name_of),
                    subject: made
                        .as_deref()
                        .map(|p| rada_core::display::name(&name_of(p)))
                        .unwrap_or_default(),
                }
            }
            OpKind::Trash => Outcome {
                folder: self
                    .paths
                    .places
                    .iter()
                    .find(|p| p.kind == rada_core::platform::PlaceKind::Trash)
                    .map(|p| p.path.clone()),
                ..Outcome::default()
            },
            OpKind::SetTags => Outcome {
                folder: Some(self.cwd.clone()),
                ..Outcome::default()
            },
            OpKind::Delete | OpKind::Undo => Outcome::default(),
        }
    }

    /// [`folder_label`](Self::folder_label), for the demo scene of the README's pictures.
    pub(super) fn folder_label_for_demo(&self, p: &Path) -> String {
        self.folder_label(p)
    }

    /// A folder as a message names it: "Documents", or "Home" for the home folder.
    fn folder_label(&self, p: &Path) -> String {
        if p == self.home() {
            return Key::HomeLabel.tr().to_string();
        }
        let on_disk = p
            .file_name()
            .map(rada_core::display::name)
            .unwrap_or_else(|| rada_core::display::path(p));
        self.shown_name(p, on_disk)
    }

    /// The card for an operation that went through cleanly: what was done, to how many items
    /// and where, with the way to take it back and the way to see it.
    pub(super) fn done_notice(&self, r: &Running) -> Notice {
        let n = r.totals.items.max(1);
        let count = fmt::thousands(n);
        let one = n == 1;
        let dest = r
            .outcome
            .folder
            .as_deref()
            .map(|p| self.folder_label(p))
            .unwrap_or_default();
        let subject = r.outcome.subject.clone();
        // "1 item copied to X" / "3 items copied to X", and the ones with nothing to name.
        let two = |a: Key, b: Key, last: &str| {
            if one {
                tf(a, &[&last])
            } else {
                tf(b, &[&count, &last])
            }
        };
        let counted = |a: Key, b: Key| {
            if one {
                a.tr().to_string()
            } else {
                tf(b, &[&count])
            }
        };
        let text = match r.kind {
            OpKind::Copy => two(Key::NCopyOne, Key::NCopyMany, &dest),
            OpKind::Move => two(Key::NMoveOne, Key::NMoveMany, &dest),
            OpKind::Trash => counted(Key::NTrashOne, Key::NTrashMany),
            OpKind::Rename | OpKind::BulkRename => {
                if one {
                    tf(Key::NRenameOne, &[&subject])
                } else {
                    tf(Key::NRenameMany, &[&count])
                }
            }
            OpKind::Compress => two(Key::NCompressOne, Key::NCompressMany, &subject),
            OpKind::Extract => two(Key::NExtractOne, Key::NExtractMany, &dest),
            OpKind::SetTags => counted(Key::NTagsOne, Key::NTagsMany),
            OpKind::MakeDir => tf(Key::NFolderCreated, &[&subject]),
            OpKind::MakeFile => tf(Key::NFileCreated, &[&subject]),
            OpKind::Delete => counted(Key::NDeleteOne, Key::NDeleteMany),
            OpKind::Undo => Key::NUndone.tr().to_string(),
        };
        let action = match r.kind {
            OpKind::Undo => NoticeAction::Redo,
            _ if r.reversible => NoticeAction::Undo,
            _ => NoticeAction::None,
        };
        let mut notice = Notice::new(NoticeKind::Ok, text).with_action(action);
        if let Some(folder) = &r.outcome.folder
            && r.kind != OpKind::Delete
            && r.kind != OpKind::Undo
        {
            notice = notice.showing(folder.clone(), r.outcome.select.clone());
        }
        notice
    }

    /// A click on the notification card.
    pub(super) fn click_notice(&mut self, hit: crate::hits::NoticeHit) {
        use crate::hits::NoticeHit;
        let now = self.notice_now();
        let shown = self.notices.front().cloned();
        self.notices.dismiss(now);
        self.dirty = true;
        match hit {
            NoticeHit::Undo => self.dispatch(Action::Undo),
            NoticeHit::Redo => self.dispatch(Action::Redo),
            NoticeHit::Show => {
                if let Some(Notice {
                    show: Some(folder),
                    select,
                    ..
                }) = shown
                {
                    if let Some(name) = select {
                        self.remembered.insert(folder.clone(), name);
                    }
                    self.open_dir(folder);
                }
            }
            NoticeHit::Close | NoticeHit::Card => {}
        }
    }
}
