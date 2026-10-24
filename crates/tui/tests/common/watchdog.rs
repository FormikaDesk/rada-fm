//! A test that stops for good says where. The harness records the last thing each test did
//! (a key, a wait, a frame); if one test lasts more than `LIMIT` the process names it, says
//! what it was doing and aborts, so that the log of a hung run is not "running for over 60
//! seconds" and nothing else.

use std::io::Write;
use std::sync::{Mutex, Once};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// A test that is slower than this is stuck: the slowest ones take about ten seconds.
const LIMIT: Duration = Duration::from_secs(120);

struct Entry {
    thread: ThreadId,
    test: String,
    started: Instant,
    /// The last thing the test asked of the harness (a key, a wait...).
    call: String,
    call_at: Instant,
    /// What the harness was doing inside it when it last said.
    step: String,
}

static TESTS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static MONITOR: Once = Once::new();

fn tests() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    TESTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Held by the harness for as long as the test lives: dropped last, so that tearing the
/// interface and the sandbox down is watched too.
pub struct Watch;

/// Dropped first: the next step is the teardown.
pub struct Teardown;

impl Watch {
    pub fn start() -> Watch {
        let now = Instant::now();
        let thread = std::thread::current();
        tests().push(Entry {
            thread: thread.id(),
            test: thread.name().unwrap_or("(a thread without a name)").into(),
            started: now,
            call: "starting".into(),
            call_at: now,
            step: String::new(),
        });
        MONITOR.call_once(|| {
            // The thread of the first test lends it the capture of its output, which an abort
            // would lose: the report below is written to the stream itself.
            let _ = std::thread::Builder::new()
                .name("test-watchdog".into())
                .spawn(monitor);
        });
        Watch
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let me = std::thread::current().id();
        tests().retain(|e| e.thread != me);
    }
}

impl Drop for Teardown {
    fn drop(&mut self) {
        call("tearing down: the interface, its workers and the sandbox".into());
    }
}

fn with_mine(f: impl FnOnce(&mut Entry)) {
    let me = std::thread::current().id();
    if let Some(e) = tests().iter_mut().find(|e| e.thread == me) {
        f(e);
    }
}

/// The test on this thread asks the harness for `what`.
pub fn call(what: String) {
    with_mine(|e| {
        e.call = what;
        e.call_at = Instant::now();
        e.step.clear();
    });
}

/// The harness is now doing `what` for that call.
pub fn step(what: &str) {
    with_mine(|e| e.step = what.into());
}

fn monitor() {
    loop {
        std::thread::sleep(Duration::from_secs(5));
        let now = Instant::now();
        let stuck = tests()
            .iter()
            .find(|e| now.duration_since(e.started) > LIMIT)
            .map(|e| {
                format!(
                    "\nwatchdog: the test {} has run for {}s.\nIts last call to the harness, {}s \
                     ago: {}\nInside it: {}\n",
                    e.test,
                    now.duration_since(e.started).as_secs(),
                    now.duration_since(e.call_at).as_secs(),
                    e.call,
                    if e.step.is_empty() {
                        "(nothing said)"
                    } else {
                        &e.step
                    }
                )
            });
        if let Some(report) = stuck {
            let _ = std::io::stderr().write_all(report.as_bytes());
            std::process::abort();
        }
    }
}
