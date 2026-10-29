//! Notifications: small cards at the bottom right of the list that say what just happened,
//! with the way to take it back and the way to see it.
//!
//! Up to three are on screen at once, piled from the bottom; each has its own six seconds
//! (the line along the bottom of a card shortens as they pass), the rest wait their turn. A
//! notification goes by itself after about six seconds, or at the first key pressed after it
//! has been on screen a moment (the key is not taken: it does what it would have done). It
//! never takes the keyboard. An error stays until it is closed (a click, or Esc).
//!
//! The queue knows nothing of the clock: every call is given the time, so that the tests
//! run the seconds without waiting for them.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rada_core::display;

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
    /// Which card it is, for a click on it; given when it is queued.
    pub id: u64,
    pub kind: NoticeKind,
    /// "3 items copied to Documents".
    pub text: String,
    pub action: NoticeAction,
    /// Where "Show" goes: the folder the result is in.
    pub show: Option<PathBuf>,
    /// The name to put the cursor on when going there.
    pub select: Option<std::ffi::OsString>,
    /// How long it stays on screen once it is shown (never, for an error).
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
            id: 0,
            kind,
            // Most of what is said is built here, but some of it is the answer of a program.
            text: display::text(&text.into()),
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

    /// The same words, whatever card it is.
    fn same_as(&self, other: &Notice) -> bool {
        self.kind == other.kind
            && self.text == other.text
            && self.action == other.action
            && self.show == other.show
    }
}

struct Slot {
    notice: Notice,
    /// When it came on screen; `None` while it waits for room.
    since: Option<Instant>,
}

/// Cards on screen at once.
pub const MAX_SHOWN: usize = 3;
/// Most notifications kept, shown and waiting; the oldest that is not an error is dropped
/// past it.
const MAX_KEPT: usize = 10;

#[derive(Default)]
pub struct NoticeQueue {
    slots: VecDeque<Slot>,
    next_id: u64,
}

impl NoticeQueue {
    /// Add a notification at the end. The same words twice in a row are one card, whose time
    /// starts over; a plain message replaces a plain one that has not been acted on (pressing a
    /// key that sorts twice must not pile two messages).
    pub fn push(&mut self, mut notice: Notice, now: Instant) {
        // Something to act on (a result, an error) makes the chatter before it stale: "1 item
        // copied, go to the destination…" means nothing once it has been pasted.
        if !notice.is_plain() && self.slots.iter().any(|s| s.notice.is_plain()) {
            self.slots.retain(|s| !s.notice.is_plain());
            self.promote(now);
        }
        if let Some(last) = self.slots.back_mut() {
            if last.notice.same_as(&notice) {
                // Said again: the time starts over.
                if last.since.is_some() {
                    last.since = Some(now);
                }
                return;
            }
            if notice.is_plain() && last.notice.is_plain() {
                notice.id = last.notice.id;
                let since = last.since.map(|_| now);
                *last = Slot { notice, since };
                return;
            }
        }
        while self.slots.len() >= MAX_KEPT {
            // Drop the oldest that is not an error.
            match self.slots.iter().position(|s| !s.notice.is_sticky()) {
                Some(i) => {
                    self.slots.remove(i);
                }
                None => break,
            }
        }
        self.next_id += 1;
        notice.id = self.next_id;
        self.slots.push_back(Slot {
            notice,
            since: None,
        });
        self.promote(now);
    }

    /// Bring the next ones on screen while there is room.
    fn promote(&mut self, now: Instant) {
        for s in self.slots.iter_mut().take(MAX_SHOWN) {
            if s.since.is_none() {
                s.since = Some(now);
            }
        }
    }

    /// The cards on screen, the oldest first, with the time each has been shown.
    pub fn shown(&self, now: Instant) -> Vec<(&Notice, Duration)> {
        self.slots
            .iter()
            .take(MAX_SHOWN)
            .filter_map(|s| {
                s.since
                    .map(|t| (&s.notice, now.saturating_duration_since(t)))
            })
            .collect()
    }

    /// The oldest card on screen.
    pub fn front(&self) -> Option<&Notice> {
        self.slots.front().map(|s| &s.notice)
    }

    /// The card with this id, shown or waiting.
    pub fn get(&self, id: u64) -> Option<&Notice> {
        self.slots.iter().map(|s| &s.notice).find(|n| n.id == id)
    }

    /// How many are waiting for room.
    pub fn waiting(&self) -> usize {
        self.slots.len().saturating_sub(MAX_SHOWN)
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Whether a card whose time is running is on screen (the clock must be redrawn).
    pub fn has_clock(&self) -> bool {
        self.slots
            .iter()
            .take(MAX_SHOWN)
            .any(|s| s.since.is_some() && !s.notice.is_sticky())
    }

    /// Time passes: the cards whose time is up go (an error does not), and the next ones take
    /// their places. Returns whether anything changed on screen.
    pub fn tick(&mut self, now: Instant) -> bool {
        let before = self.slots.len();
        let mut shown = 0;
        self.slots.retain(|s| {
            if shown >= MAX_SHOWN {
                return true;
            }
            shown += 1;
            match s.since {
                Some(since) if !s.notice.is_sticky() => {
                    now.saturating_duration_since(since) < s.notice.ttl
                }
                _ => true,
            }
        });
        let had_waiting_before = before > MAX_SHOWN;
        let changed = self.slots.len() != before;
        let promoted = self.slots.iter().take(MAX_SHOWN).any(|s| s.since.is_none());
        self.promote(now);
        changed || (had_waiting_before && promoted)
    }

    /// A key was pressed: the cards on screen go, except errors (those wait for Esc, see
    /// [`NoticeQueue::dismiss_error`]) and those that appeared a moment ago. The key itself is
    /// not touched. Returns whether any card went.
    pub fn on_key(&mut self, now: Instant) -> bool {
        let before = self.slots.len();
        let mut shown = 0;
        self.slots.retain(|s| {
            if shown >= MAX_SHOWN {
                return true;
            }
            shown += 1;
            match s.since {
                Some(since) => {
                    s.notice.is_sticky() || now.saturating_duration_since(since) < Notice::MIN_SHOWN
                }
                None => true,
            }
        });
        self.promote(now);
        self.slots.len() != before
    }

    /// Esc: closes the newest error on screen (and says so, so that the key is used up).
    pub fn dismiss_error(&mut self, now: Instant) -> bool {
        let at = self
            .slots
            .iter()
            .take(MAX_SHOWN)
            .rposition(|s| s.notice.is_sticky());
        match at {
            Some(i) => {
                self.slots.remove(i);
                self.promote(now);
                true
            }
            None => false,
        }
    }

    /// Close the card with this id (a click on it).
    pub fn dismiss(&mut self, id: u64, now: Instant) {
        self.slots.retain(|s| s.notice.id != id);
        self.promote(now);
    }

    /// Close whatever is in front (tests).
    pub fn dismiss_front(&mut self, now: Instant) {
        self.slots.pop_front();
        self.promote(now);
    }

    /// The share of its time a card has left, 1.0 to 0.0; `None` for one that stays.
    pub fn left(&self, id: u64, now: Instant) -> Option<f64> {
        let s = self.slots.iter().find(|s| s.notice.id == id)?;
        if s.notice.is_sticky() {
            return None;
        }
        let since = s.since?;
        let used = now.saturating_duration_since(since).as_secs_f64();
        Some((1.0 - used / s.notice.ttl.as_secs_f64().max(0.001)).clamp(0.0, 1.0))
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

    fn texts(q: &NoticeQueue, now: Instant) -> Vec<String> {
        q.shown(now).iter().map(|(n, _)| n.text.clone()).collect()
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
    fn three_cards_are_on_screen_at_once_each_with_its_own_six_seconds() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("one"), t0);
        q.push(ok("two"), secs(t0, 1.0));
        q.push(ok("three"), secs(t0, 2.0));
        assert_eq!(texts(&q, secs(t0, 2.0)), ["one", "two", "three"]);
        assert_eq!(q.waiting(), 0);
        // Each goes when its own time is up: one at 6 s, two at 7 s, three at 8 s.
        assert!(q.tick(secs(t0, 6.0)));
        assert_eq!(texts(&q, secs(t0, 6.0)), ["two", "three"]);
        assert!(!q.tick(secs(t0, 6.9)));
        assert!(q.tick(secs(t0, 7.0)));
        assert_eq!(texts(&q, secs(t0, 7.0)), ["three"]);
        assert!(q.tick(secs(t0, 8.0)));
        assert!(q.is_empty());
    }

    #[test]
    fn a_fourth_card_waits_for_room_and_then_gets_its_own_six_seconds() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
            q.push(ok(name), secs(t0, i as f64 * 0.1));
        }
        assert_eq!(texts(&q, secs(t0, 0.4)), ["a", "b", "c"]);
        assert_eq!(q.waiting(), 1);
        // "a" goes at 6 s and "d" takes its place from there.
        assert!(q.tick(secs(t0, 6.0)));
        assert_eq!(texts(&q, secs(t0, 6.0)), ["b", "c", "d"]);
        assert!(!q.tick(secs(t0, 11.9)) || q.front().is_some());
        assert!(q.shown(secs(t0, 11.0)).iter().any(|(n, _)| n.text == "d"));
        q.tick(secs(t0, 12.0));
        assert!(q.is_empty(), "d had six seconds from 6 s");
    }

    #[test]
    fn a_key_closes_what_has_been_seen_and_brings_the_next() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        for n in ["a", "b", "c", "d"] {
            q.push(ok(n), t0);
        }
        assert!(q.on_key(secs(t0, 1.0)));
        assert_eq!(texts(&q, secs(t0, 1.0)), ["d"]);
        assert!(!q.on_key(secs(t0, 1.1)), "the new card has just appeared");
        assert!(q.on_key(secs(t0, 2.0)));
        assert!(q.is_empty());
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
        assert!(q.tick(secs(t0, 3600.0)), "the other one has gone");
        assert!(!q.on_key(secs(t0, 3600.0)), "a key does not close an error");
        assert_eq!(q.front().unwrap().kind, NoticeKind::Error);
        let id = q.front().unwrap().id;
        assert_eq!(q.left(id, secs(t0, 5.0)), None);
        assert!(q.dismiss_error(secs(t0, 3601.0)), "Esc closes it");
        assert!(q.is_empty());
        assert!(
            !q.dismiss_error(secs(t0, 3602.0)),
            "Esc leaves the others alone"
        );
    }

    #[test]
    fn esc_closes_the_newest_error_and_leaves_the_rest() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(Notice::new(NoticeKind::Error, "first"), t0);
        q.push(ok("moved"), t0);
        q.push(Notice::new(NoticeKind::Error, "second"), t0);
        assert!(q.dismiss_error(secs(t0, 1.0)));
        assert_eq!(texts(&q, secs(t0, 1.0)), ["first", "moved"]);
    }

    #[test]
    fn what_is_said_twice_in_a_row_is_one_card_that_starts_over() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("copied"), t0);
        q.push(ok("copied"), secs(t0, 4.0));
        assert_eq!(q.shown(secs(t0, 4.0)).len(), 1);
        // The time started over at 4 s: it is still there at 9 s, gone at 10 s.
        assert!(!q.tick(secs(t0, 9.0)));
        assert!(q.tick(secs(t0, 10.0)));
    }

    #[test]
    fn plain_messages_replace_each_other_instead_of_piling() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        for i in 0..5 {
            q.push(
                Notice::new(NoticeKind::Info, format!("sorted by Name {i}")),
                secs(t0, i as f64 * 0.1),
            );
        }
        assert_eq!(q.shown(secs(t0, 1.0)).len(), 1, "the last one wins");
        assert_eq!(q.front().unwrap().text, "sorted by Name 4");
        // A result with something to click takes their place; two results pile.
        q.push(ok("moved"), secs(t0, 1.0));
        assert_eq!(texts(&q, secs(t0, 1.0)), ["moved"]);
        q.push(ok("copied"), secs(t0, 1.1));
        assert_eq!(texts(&q, secs(t0, 1.1)), ["moved", "copied"]);
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
        assert_eq!(texts(&q, secs(t0, 2.0)), ["1 item copied to Documents"]);
        // The result is on screen from the moment it came: it gets its six seconds from there.
        assert!(!q.tick(secs(t0, 7.9)));
        assert!(q.tick(secs(t0, 8.0)));
    }

    #[test]
    fn the_queue_is_bounded_and_never_drops_an_error() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(Notice::new(NoticeKind::Error, "first"), t0);
        for i in 0..30 {
            q.push(ok(&format!("n{i}")), t0);
        }
        assert!(q.slots.len() <= MAX_KEPT);
        assert_eq!(q.front().unwrap().text, "first");
        q.push(Notice::new(NoticeKind::Error, "second"), t0);
        // Whatever was dropped, the errors are still queued.
        let mut seen = Vec::new();
        while let Some(n) = q.front() {
            seen.push(n.text.clone());
            q.dismiss_front(t0);
        }
        assert!(seen.contains(&"first".to_string()), "{seen:?}");
        assert!(seen.contains(&"second".to_string()), "{seen:?}");
    }

    #[test]
    fn the_time_left_counts_down_from_six_seconds() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("x"), t0);
        let id = q.front().unwrap().id;
        assert_eq!(q.left(id, t0), Some(1.0));
        let half = q.left(id, secs(t0, 3.0)).unwrap();
        assert!((half - 0.5).abs() < 1e-6);
        assert_eq!(q.left(id, secs(t0, 10.0)), Some(0.0));
    }

    #[test]
    fn every_card_has_an_id_of_its_own_and_a_click_closes_that_card() {
        let t0 = Instant::now();
        let mut q = NoticeQueue::default();
        q.push(ok("a"), t0);
        q.push(ok("b"), t0);
        let ids: Vec<u64> = q.shown(t0).iter().map(|(n, _)| n.id).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        q.dismiss(ids[0], t0);
        assert_eq!(texts(&q, t0), ["b"]);
        assert_eq!(q.get(ids[1]).map(|n| n.text.as_str()), Some("b"));
    }
}
