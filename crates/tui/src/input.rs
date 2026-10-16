//! The keyboard reader: a thread of its own, so that slow drawing can never lose a key press,
//! which can be stopped for a moment whenever the terminal is asked a question.
//!
//! crossterm keeps one reader for the whole process behind a lock, and `event::read()` holds
//! that lock for as long as it waits. A question to the terminal whose answer comes back as
//! input (the cursor position, `ESC [ 6 n`) waits for the same lock, never gets it, and fails
//! after two seconds. So the thread never waits for long: it asks for an event for
//! [`POLL`] at a time and, between two of those, looks whether someone needs the terminal to
//! themselves ([`InputGate::pause`]).

use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use crossbeam_channel::Sender;
use crossterm::event::Event;

/// How long the thread waits for an event before looking at the gate.
pub const POLL: Duration = Duration::from_millis(50);

/// How long [`InputGate::pause`] waits for the thread to stop. It is back from a wait in a
/// few milliseconds, so this is only reached by a thread that is stuck.
const PARK_LIMIT: Duration = Duration::from_secs(1);

/// Reading failed this many times in a row (about two seconds): the keyboard is gone.
const LOST_AFTER: u32 = 40;

/// Where events come from. The terminal in the program, a script in the tests.
pub trait EventSource: Send + 'static {
    /// Whether an event can be read now, waiting at most `timeout` for one.
    fn poll(&mut self, timeout: Duration) -> io::Result<bool>;
    /// The next event; only called after [`EventSource::poll`] said there is one.
    fn read(&mut self) -> io::Result<Event>;
}

/// The terminal, through crossterm.
pub struct Terminal;

impl EventSource for Terminal {
    fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
        crossterm::event::poll(timeout)
    }

    fn read(&mut self) -> io::Result<Event> {
        crossterm::event::read()
    }
}

/// What the thread hands over.
#[derive(Debug)]
pub enum Input {
    Event(Event),
    /// The keyboard can no longer be read (the terminal went away). Nothing follows.
    Lost(String),
}

#[derive(Default)]
struct State {
    /// How many [`Pause`]s are alive.
    pauses: u32,
    /// The thread is waiting in [`InputGate::checkpoint`].
    parked: bool,
    /// The thread has ended: there is nobody to wait for.
    ended: bool,
}

/// Lets the rest of the program stop the reader while it talks to the terminal.
#[derive(Default)]
pub struct InputGate {
    state: Mutex<State>,
    changed: Condvar,
}

impl InputGate {
    pub fn new() -> Arc<InputGate> {
        Arc::new(InputGate::default())
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Nothing in the state can be left half done by a panic.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Stop the reader and wait until it has stopped; it goes on when the returned value is
    /// dropped. What is typed in between waits in the terminal's input: nothing is lost.
    pub fn pause(self: &Arc<Self>) -> Pause {
        let mut s = self.lock();
        s.pauses += 1;
        if !s.parked && !s.ended {
            let (guard, timeout) = self
                .changed
                .wait_timeout_while(s, PARK_LIMIT, |s| !s.parked && !s.ended)
                .unwrap_or_else(|p| p.into_inner());
            s = guard;
            if timeout.timed_out() && !s.parked && !s.ended {
                tracing::warn!("the keyboard reader did not stop within {PARK_LIMIT:?}");
            }
        }
        drop(s);
        Pause(self.clone())
    }

    /// The thread's side: wait here for as long as someone holds a [`Pause`].
    fn checkpoint(&self) {
        let mut s = self.lock();
        if s.pauses == 0 {
            return;
        }
        s.parked = true;
        self.changed.notify_all();
        while s.pauses > 0 {
            s = self.changed.wait(s).unwrap_or_else(|p| p.into_inner());
        }
        s.parked = false;
    }

    fn end(&self) {
        self.lock().ended = true;
        self.changed.notify_all();
    }
}

/// The reader is stopped for as long as this is alive.
#[must_use = "the reader goes on as soon as this is dropped"]
pub struct Pause(Arc<InputGate>);

impl Drop for Pause {
    fn drop(&mut self) {
        let mut s = self.0.lock();
        s.pauses -= 1;
        drop(s);
        self.0.changed.notify_all();
    }
}

/// Start the reader. Events go to `tx`; the thread ends when nobody listens any more or when
/// the keyboard is lost.
pub fn spawn<S: EventSource>(
    mut source: S,
    tx: Sender<Input>,
    gate: Arc<InputGate>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("rada-input".into())
        .spawn(move || {
            run(&mut source, &tx, &gate);
            gate.end();
        })?;
    Ok(())
}

fn run<S: EventSource>(source: &mut S, tx: &Sender<Input>, gate: &InputGate) {
    let mut failures = 0u32;
    loop {
        gate.checkpoint();
        let read = match source.poll(POLL) {
            Ok(true) => source.read().map(Some),
            Ok(false) => Ok(None),
            Err(e) => Err(e),
        };
        match read {
            Ok(Some(ev)) => {
                failures = 0;
                if tx.send(Input::Event(ev)).is_err() {
                    return;
                }
            }
            Ok(None) => failures = 0,
            Err(e) => {
                failures += 1;
                // Once, and again every five seconds if it goes on: not one line per try.
                if failures == 1 || failures.is_multiple_of(100) {
                    tracing::warn!("cannot read the keyboard ({failures} in a row): {e}");
                }
                if failures >= LOST_AFTER {
                    let _ = tx.send(Input::Lost(e.to_string()));
                    return;
                }
                std::thread::sleep(POLL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use crossterm::event::{KeyCode, KeyEvent};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn key(c: char) -> Event {
        Event::Key(KeyEvent::from(KeyCode::Char(c)))
    }

    fn letter(i: Input) -> char {
        match i {
            Input::Event(Event::Key(k)) => match k.code {
                KeyCode::Char(c) => c,
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    /// Events waiting in "the terminal", and how many times the reader looked.
    #[derive(Default)]
    struct Script {
        waiting: Mutex<VecDeque<Event>>,
        looks: AtomicU32,
        /// This many looks fail before the script works.
        failing: AtomicU32,
    }

    struct Fake(Arc<Script>);

    impl EventSource for Fake {
        fn poll(&mut self, timeout: Duration) -> io::Result<bool> {
            self.0.looks.fetch_add(1, Ordering::SeqCst);
            if self.0.failing.load(Ordering::SeqCst) > 0 {
                self.0.failing.fetch_sub(1, Ordering::SeqCst);
                return Err(io::Error::other("fake failure"));
            }
            if !self.0.waiting.lock().unwrap().is_empty() {
                return Ok(true);
            }
            std::thread::sleep(timeout.min(Duration::from_millis(5)));
            Ok(false)
        }

        fn read(&mut self) -> io::Result<Event> {
            Ok(self.0.waiting.lock().unwrap().pop_front().expect("polled"))
        }
    }

    fn start(script: &Arc<Script>) -> (crossbeam_channel::Receiver<Input>, Arc<InputGate>) {
        let (tx, rx) = unbounded();
        let gate = InputGate::new();
        spawn(Fake(script.clone()), tx, gate.clone()).unwrap();
        (rx, gate)
    }

    fn typed(script: &Script, text: &str) {
        script.waiting.lock().unwrap().extend(text.chars().map(key));
    }

    #[test]
    fn events_reach_the_receiver_in_order() {
        let script = Arc::new(Script::default());
        let (rx, _gate) = start(&script);
        typed(&script, "abc");
        let got: String = (0..3)
            .map(|_| letter(rx.recv_timeout(Duration::from_secs(2)).unwrap()))
            .collect();
        assert_eq!(got, "abc");
    }

    #[test]
    fn a_paused_reader_has_stopped_when_pause_returns_and_loses_nothing() {
        let script = Arc::new(Script::default());
        let (rx, gate) = start(&script);
        let pause = gate.pause();
        // Stopped: it does not even look at the terminal.
        let looks = script.looks.load(Ordering::SeqCst);
        typed(&script, "xyz");
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            script.looks.load(Ordering::SeqCst),
            looks,
            "it kept looking"
        );
        assert!(rx.try_recv().is_err(), "it read while paused");
        assert_eq!(script.waiting.lock().unwrap().len(), 3, "keys were taken");
        // Going on: everything typed meanwhile arrives, in order.
        drop(pause);
        let got: String = (0..3)
            .map(|_| letter(rx.recv_timeout(Duration::from_secs(2)).unwrap()))
            .collect();
        assert_eq!(got, "xyz");
    }

    #[test]
    fn pauses_can_be_taken_one_after_the_other_and_overlap() {
        let script = Arc::new(Script::default());
        let (rx, gate) = start(&script);
        for round in 0..20 {
            let first = gate.pause();
            let second = gate.pause();
            drop(first);
            let looks = script.looks.load(Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(5));
            assert_eq!(script.looks.load(Ordering::SeqCst), looks, "round {round}");
            drop(second);
        }
        typed(&script, "k");
        assert_eq!(
            letter(rx.recv_timeout(Duration::from_secs(2)).unwrap()),
            'k'
        );
    }

    #[test]
    fn pausing_does_not_wait_for_a_reader_that_has_ended() {
        let script = Arc::new(Script::default());
        let (rx, gate) = start(&script);
        drop(rx);
        typed(&script, "q");
        // The receiver is gone: the thread ends on the next event it sends.
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_secs(2) && !gate.lock().ended {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(gate.lock().ended);
        let t = std::time::Instant::now();
        let _p = gate.pause();
        assert!(t.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn a_few_failed_reads_are_forgotten_and_the_keys_still_arrive() {
        let script = Arc::new(Script::default());
        script.failing.store(5, Ordering::SeqCst);
        let (rx, _gate) = start(&script);
        typed(&script, "a");
        assert_eq!(
            letter(rx.recv_timeout(Duration::from_secs(3)).unwrap()),
            'a'
        );
    }

    #[test]
    fn a_keyboard_that_never_works_again_is_reported_once_as_lost() {
        let script = Arc::new(Script::default());
        script.failing.store(u32::MAX, Ordering::SeqCst);
        let (rx, _gate) = start(&script);
        match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            Input::Lost(why) => assert!(why.contains("fake failure"), "{why}"),
            other => panic!("{other:?}"),
        }
        // And nothing follows: the thread has ended.
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }
}
