//! Running another program for a bounded time, with nothing of it left behind.
//!
//! The program is given its arguments as a list, never through a shell. It runs with no
//! input, a short list of environment variables, a low priority and a folder of its own for
//! temporary files. It is started in a group of its own (a process group on Unix, a Job Object
//! on Windows), so that stopping it stops what it started too: on a timeout, when the caller
//! gives up, and when it ends leaving something running. What it prints is always read, so a
//! full pipe never blocks it, and only the end of it is kept.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::spawn::spawn_retrying_busy;
use crate::display;

#[cfg(unix)]
#[path = "process/unix.rs"]
mod sys;
#[cfg(windows)]
#[path = "process/windows.rs"]
mod sys;

/// How often the program, the caller's wish to stop and the clock are looked at.
const POLL: Duration = Duration::from_millis(25);

/// How long a program that was asked to stop is given before it is killed.
pub const GRACE: Duration = Duration::from_secs(3);

/// How long the readers are waited for once the program has ended: something that left the
/// group may still hold the pipe open, and must not hold the caller too.
const DRAIN: Duration = Duration::from_secs(2);

/// What is kept of each stream unless the caller says otherwise.
pub const DEFAULT_CAPTURE: usize = 64 * 1024;

/// What to run and within which bounds.
#[derive(Clone, Debug)]
pub struct ProcessSpec {
    /// The program, as a whole path: nothing is looked up.
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: EnvPolicy,
    pub timeout: Duration,
    /// The bytes kept from the end of each stream.
    pub max_capture: usize,
}

/// The environment the program gets: the few variables it cannot do without, its temporary
/// files sent to `temp`, and what the caller adds.
#[derive(Clone, Debug)]
pub struct EnvPolicy {
    pub temp: PathBuf,
    pub extra: Vec<(OsString, OsString)>,
}

impl EnvPolicy {
    pub fn new(temp: impl Into<PathBuf>) -> EnvPolicy {
        EnvPolicy {
            temp: temp.into(),
            extra: Vec::new(),
        }
    }

    /// The variables to give, from those of this process.
    fn vars(&self) -> Vec<(OsString, OsString)> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(name, _)| is_passed_on(name))
            .collect();
        for name in ["TMPDIR", "TEMP", "TMP"] {
            vars.push((name.into(), self.temp.clone().into_os_string()));
        }
        vars.extend(self.extra.iter().cloned());
        vars
    }
}

/// Whether a variable of this process is given to the program.
fn is_passed_on(name: &OsStr) -> bool {
    const NAMES: [&str; 6] = [
        "PATH",
        "HOME",
        "USERPROFILE",
        "LANG",
        "SystemRoot",
        "windir",
    ];
    let Some(name) = name.to_str() else {
        return false;
    };
    if cfg!(windows) {
        let upper = name.to_ascii_uppercase();
        return NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)) || upper.starts_with("LC_");
    }
    NAMES.contains(&name) || name.starts_with("LC_")
}

/// Where a running program is, for whoever shows it.
#[derive(Clone, Copy, Debug)]
pub struct Tick {
    pub elapsed: Duration,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

/// How the program ended.
#[derive(Clone, Debug)]
pub struct ProcessOutcome {
    /// The exit code; `None` when the program was killed instead of ending.
    pub status: Option<i32>,
    /// The end of what it printed, safe to show.
    pub stdout_tail: String,
    pub stderr_tail: String,
    /// More was printed than is kept.
    pub truncated: bool,
    pub timed_out: bool,
    /// The caller asked to stop.
    pub interrupted: bool,
    pub elapsed: Duration,
}

/// Why the watch over the program ended.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Exited,
    TimedOut,
    Interrupted,
}

/// Run the program and wait for it. `cancel` is asked many times a second whether to give up;
/// `tick` is told as often how long it has been running and how much it has printed. An error
/// is a program that could not be started: one that started always gives an outcome.
pub fn run(
    spec: &ProcessSpec,
    cancel: &dyn Fn() -> bool,
    tick: &mut dyn FnMut(&Tick),
) -> io::Result<ProcessOutcome> {
    let started = Instant::now();
    let mut child = spawn_retrying_busy(&mut command(spec))?;
    let guard = match sys::adopt(&child) {
        Ok(guard) => guard,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let out = Capture::start(child.stdout.take(), spec.max_capture);
    let err = Capture::start(child.stderr.take(), spec.max_capture);
    let end = watch(spec, &mut child, &guard, started, cancel, &mut |elapsed| {
        tick(&Tick {
            elapsed,
            stdout_bytes: out.seen(),
            stderr_bytes: err.seen(),
        })
    });
    match end {
        // What it left running goes with it: nothing outlives the step that started it.
        End::Exited => guard.sweep(),
        End::TimedOut | End::Interrupted => guard.stop(&mut child),
    }
    let status = child.wait()?;
    let code = if end == End::Exited {
        status.code()
    } else {
        None
    };
    Ok(outcome(end, code, out, err, started.elapsed()))
}

/// What is told of a program that has ended.
fn outcome(
    end: End,
    status: Option<i32>,
    out: Capture,
    err: Capture,
    elapsed: Duration,
) -> ProcessOutcome {
    let (stdout_tail, cut_out) = out.finish();
    let (stderr_tail, cut_err) = err.finish();
    ProcessOutcome {
        status,
        stdout_tail,
        stderr_tail,
        truncated: cut_out || cut_err,
        timed_out: end == End::TimedOut,
        interrupted: end == End::Interrupted,
        elapsed,
    }
}

/// The command for `spec`, with everything but the system's own part set.
fn command(spec: &ProcessSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(spec.env.vars());
    sys::prepare(&mut cmd);
    cmd
}

/// Wait until the program ends, the time is over or the caller gives up.
fn watch(
    spec: &ProcessSpec,
    child: &mut Child,
    guard: &sys::Guard,
    started: Instant,
    cancel: &dyn Fn() -> bool,
    tick: &mut dyn FnMut(Duration),
) -> End {
    loop {
        if guard.exited(child) {
            return End::Exited;
        }
        if cancel() {
            return End::Interrupted;
        }
        let elapsed = started.elapsed();
        if elapsed >= spec.timeout {
            return End::TimedOut;
        }
        tick(elapsed);
        std::thread::sleep(POLL);
    }
}

/// The end of a stream, kept while the rest goes by.
struct Tail {
    bytes: Vec<u8>,
    max: usize,
    cut: bool,
}

impl Tail {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        // Trimmed when it has doubled, not at every read: the copy stays rare.
        if self.bytes.len() > self.max.saturating_mul(2) {
            self.trim();
        }
    }

    fn trim(&mut self) {
        if self.bytes.len() > self.max {
            let extra = self.bytes.len() - self.max;
            self.bytes.drain(..extra);
            self.cut = true;
        }
    }
}

/// A thread that reads one stream to its end.
struct Capture {
    tail: Arc<Mutex<Tail>>,
    seen: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
}

impl Capture {
    fn start<R: Read + Send + 'static>(stream: Option<R>, max: usize) -> Capture {
        let capture = Capture {
            tail: Arc::new(Mutex::new(Tail {
                bytes: Vec::new(),
                max,
                cut: false,
            })),
            seen: Arc::new(AtomicU64::new(0)),
            done: Arc::new(AtomicBool::new(stream.is_none())),
        };
        if let Some(stream) = stream {
            let (tail, seen, done) = (
                capture.tail.clone(),
                capture.seen.clone(),
                capture.done.clone(),
            );
            let reader = move || {
                read_all(stream, &tail, &seen);
                done.store(true, Ordering::SeqCst);
            };
            if std::thread::Builder::new()
                .name("rada-process-output".into())
                .spawn(reader)
                .is_err()
            {
                capture.done.store(true, Ordering::SeqCst);
            }
        }
        capture
    }

    fn seen(&self) -> u64 {
        self.seen.load(Ordering::Relaxed)
    }

    /// What was kept, as text safe to show, and whether something is missing. The reader is
    /// waited for a short while only: if the pipe is still open then, what there is is taken
    /// and the reader is left to end by itself.
    fn finish(self) -> (String, bool) {
        let until = Instant::now() + DRAIN;
        while !self.done.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        let unfinished = !self.done.load(Ordering::SeqCst);
        let mut tail = self.tail.lock().unwrap_or_else(|e| e.into_inner());
        tail.trim();
        let text = display::block(&String::from_utf8_lossy(&tail.bytes));
        (text, tail.cut || unfinished)
    }
}

/// Read `stream` until it closes, keeping its end.
fn read_all(mut stream: impl Read, tail: &Mutex<Tail>, seen: &AtomicU64) {
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                seen.fetch_add(n as u64, Ordering::Relaxed);
                tail.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(&buf[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_end_of_a_stream_is_kept_and_the_cut_is_told() {
        let mut tail = Tail {
            bytes: Vec::new(),
            max: 4,
            cut: false,
        };
        tail.push(b"ab");
        tail.trim();
        assert!(!tail.cut);
        for chunk in [&b"cdefgh"[..], b"ij", b"klmnop"] {
            tail.push(chunk);
        }
        tail.trim();
        assert_eq!(tail.bytes, b"mnop");
        assert!(tail.cut);
    }

    #[test]
    fn only_the_listed_variables_are_passed_on() {
        for name in ["PATH", "HOME", "LANG", "LC_ALL", "LC_MESSAGES"] {
            assert!(is_passed_on(OsStr::new(name)), "{name}");
        }
        for name in [
            "LD_PRELOAD",
            "SSH_AUTH_SOCK",
            "EDITOR",
            "PATHEXT",
            "LANGUAGE",
        ] {
            assert!(!is_passed_on(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn the_temporary_folder_is_the_one_given_and_what_is_added_comes_last() {
        let mut env = EnvPolicy::new("/work/tmp");
        env.extra.push(("TMP".into(), "/other".into()));
        let vars = env.vars();
        for name in ["TMPDIR", "TEMP"] {
            let value = vars.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
            assert_eq!(value, Some(OsString::from("/work/tmp")), "{name}");
        }
        assert_eq!(vars.last().unwrap().1, OsString::from("/other"));
    }
}
