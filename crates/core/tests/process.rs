//! Running another program: its exit code, its output, its environment, and that nothing of
//! it is left when it is stopped or ends.
//!
//! The program is never a real one: it is this very test binary, started again to run the one
//! test `stand_in`, which plays the part named in `RADA_STAND_IN`. So no shell and no tool of
//! the system is needed, and the same tests run on Windows.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rada_core::platform::process::{self, DEFAULT_CAPTURE, EnvPolicy, ProcessOutcome, ProcessSpec};
use rada_core::testutil::Sandbox;

const ROLE: &str = "RADA_STAND_IN";

/// The arguments that make the test binary run only the stand-in.
fn stand_in_args() -> Vec<OsString> {
    ["stand_in", "--exact", "--ignored", "--nocapture"]
        .iter()
        .map(OsString::from)
        .collect()
}

fn spec(sb: &Sandbox, role: &str) -> ProcessSpec {
    let mut env = EnvPolicy::new(sb.mkdir("tmp"));
    env.extra.push((ROLE.into(), role.into()));
    ProcessSpec {
        program: std::env::current_exe().unwrap(),
        args: stand_in_args(),
        cwd: sb.path(""),
        env,
        timeout: Duration::from_secs(60),
        max_capture: DEFAULT_CAPTURE,
    }
}

fn run(spec: &ProcessSpec) -> ProcessOutcome {
    process::run(spec, &|| false, &mut |_| {}).unwrap()
}

/// How many times the program that writes to `file` has written.
fn beats(file: &Path) -> u64 {
    std::fs::metadata(file).map(|m| m.len()).unwrap_or(0)
}

/// The program that was writing to `file` did write, and no longer does.
fn assert_stopped(file: &Path) {
    let before = beats(file);
    assert!(before > 0, "it never ran");
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(beats(file), before, "something is still running");
}

#[test]
fn the_exit_code_is_the_programs() {
    let sb = Sandbox::new();
    let out = run(&spec(&sb, "exit 7"));
    assert_eq!(out.status, Some(7));
    assert!(!out.timed_out && !out.interrupted && !out.truncated);

    assert_eq!(run(&spec(&sb, "exit 0")).status, Some(0));
}

#[test]
fn a_program_that_is_not_there_is_an_error() {
    let sb = Sandbox::new();
    let mut s = spec(&sb, "exit 0");
    s.program = sb.path("no-such-program");
    assert!(process::run(&s, &|| false, &mut |_| {}).is_err());
}

#[test]
fn a_timeout_kills_the_program_and_what_it_started() {
    let sb = Sandbox::new();
    let file = sb.path("beat");
    let mut s = spec(&sb, &format!("family {}", file.display()));
    s.timeout = Duration::from_millis(1500);

    let out = run(&s);

    assert!(out.timed_out && !out.interrupted, "{out:?}");
    assert_eq!(out.status, None);
    assert_stopped(&file);
}

#[test]
fn giving_up_kills_the_program_and_what_it_started() {
    let sb = Sandbox::new();
    let file = sb.path("beat");
    let s = spec(&sb, &format!("family {}", file.display()));
    let mut ticks = 0;

    let out = process::run(&s, &|| beats(&file) > 0, &mut |_| ticks += 1).unwrap();

    assert!(out.interrupted && !out.timed_out, "{out:?}");
    assert_eq!(out.status, None);
    assert!(ticks > 0, "the caller is told while it runs");
    assert_stopped(&file);
}

#[cfg(unix)]
#[test]
fn a_program_that_ignores_the_request_to_stop_is_killed_after_a_moment() {
    let sb = Sandbox::new();
    let file = sb.path("beat");
    let s = spec(&sb, &format!("stubborn {}", file.display()));
    let started = Instant::now();

    let out = process::run(&s, &|| beats(&file) > 0, &mut |_| {}).unwrap();

    assert!(out.interrupted);
    assert!(
        started.elapsed() >= process::GRACE,
        "it was given its moment"
    );
    assert_stopped(&file);
}

#[test]
fn what_a_program_leaves_running_ends_with_it() {
    let sb = Sandbox::new();
    let file = sb.path("beat");
    let started = Instant::now();

    let out = run(&spec(&sb, &format!("leave {}", file.display())));

    assert_eq!(out.status, Some(0), "{out:?}");
    assert!(started.elapsed() < Duration::from_secs(30));
    assert_stopped(&file);
}

#[test]
fn output_past_the_limit_does_not_block_the_program_and_its_end_is_kept() {
    let sb = Sandbox::new();
    let mut s = spec(&sb, "flood 200000");
    s.max_capture = 4096;

    let out = run(&s);

    assert_eq!(out.status, Some(0), "{out:?}");
    assert!(out.truncated);
    assert!(out.stdout_tail.len() <= 4096 && out.stderr_tail.len() <= 4096);
    assert!(
        out.stdout_tail.ends_with("last-out\n"),
        "{}",
        out.stdout_tail
    );
    assert!(
        out.stderr_tail.ends_with("last-err\n"),
        "{}",
        out.stderr_tail
    );
}

#[test]
fn what_it_prints_is_made_safe_to_show() {
    let sb = Sandbox::new();
    let out = run(&spec(&sb, "escape"));
    assert!(out.stdout_tail.contains("[31mred"), "{}", out.stdout_tail);
    assert!(!out.stdout_tail.contains('\x1b'));
    assert!(out.stdout_tail.contains("second line"));
}

#[test]
fn the_program_has_nothing_to_read() {
    let sb = Sandbox::new();
    let out = run(&spec(&sb, "stdin"));
    assert_eq!(out.status, Some(0), "{out:?}");
    assert!(out.stdout_tail.contains("stdin had 0 bytes"), "{out:?}");
}

/// The names and values the stand-in printed.
fn printed_vars(out: &ProcessOutcome) -> Vec<(String, String)> {
    out.stdout_tail
        .lines()
        .filter_map(|l| l.strip_prefix("var "))
        .filter_map(|l| l.split_once('='))
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect()
}

#[test]
fn only_the_listed_variables_reach_the_program() {
    const LISTED: [&str; 6] = [
        "PATH",
        "HOME",
        "USERPROFILE",
        "LANG",
        "SYSTEMROOT",
        "WINDIR",
    ];
    let sb = Sandbox::new();
    let s = spec(&sb, "env");

    let out = run(&s);

    let vars = printed_vars(&out);
    assert!(vars.iter().any(|(n, _)| n == ROLE), "{out:?}");
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy().to_uppercase();
        // The one variable macOS gives to every process by itself.
        if LISTED.contains(&name.as_str()) || name.starts_with("LC_") || name.starts_with("__CF") {
            continue;
        }
        if ["TMPDIR", "TEMP", "TMP"].contains(&name.as_str()) {
            continue;
        }
        assert!(
            !vars.iter().any(|(n, _)| n.to_uppercase() == name),
            "{name} was passed on"
        );
    }
    let temp = s.env.temp.display().to_string();
    for name in ["TMPDIR", "TEMP", "TMP"] {
        let value = vars
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str());
        assert_eq!(value, Some(temp.as_str()), "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// The stand-in
// ---------------------------------------------------------------------------------------------

/// Not a test: the program the tests above run. Without a part to play it does nothing.
#[test]
#[ignore = "the stand-in program of the other tests"]
fn stand_in() {
    let Some(role) = std::env::var_os(ROLE) else {
        return;
    };
    let role = role.to_string_lossy().to_string();
    let (part, arg) = role.split_once(' ').unwrap_or((role.as_str(), ""));
    let code = play(part, arg);
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}

fn play(part: &str, arg: &str) -> i32 {
    match part {
        "exit" => arg.parse().unwrap(),
        "beat" => beat(Path::new(arg)),
        "family" => {
            start_beating(arg);
            std::thread::sleep(Duration::from_secs(60));
            0
        }
        "leave" => {
            start_beating(arg);
            // Not before the other has run: the test looks for what it wrote.
            while beats(Path::new(arg)) == 0 {
                std::thread::sleep(Duration::from_millis(10));
            }
            0
        }
        "stubborn" => {
            ignore_the_request_to_stop();
            beat(Path::new(arg))
        }
        "flood" => flood(arg.parse().unwrap()),
        "escape" => say("\x1b[31mred\nsecond line\n"),
        "stdin" => {
            let mut all = Vec::new();
            std::io::stdin().read_to_end(&mut all).unwrap();
            say(&format!("stdin had {} bytes\n", all.len()))
        }
        "env" => {
            for (name, value) in std::env::vars() {
                say(&format!("var {name}={value}\n"));
            }
            0
        }
        other => panic!("no part called {other}"),
    }
}

fn say(text: &str) -> i32 {
    std::io::stdout().write_all(text.as_bytes()).unwrap();
    0
}

/// Write to `file` many times a second, for a minute at most.
fn beat(file: &Path) -> i32 {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .unwrap();
        f.write_all(b"x").unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    0
}

/// Start another stand-in that beats on `file`, and do not wait for it: the tests are about
/// what becomes of a program that nobody waits for.
#[allow(clippy::zombie_processes)]
fn start_beating(file: &str) {
    let exe: PathBuf = std::env::current_exe().unwrap();
    std::process::Command::new(exe)
        .args(stand_in_args())
        .env(ROLE, format!("beat {file}"))
        .spawn()
        .unwrap();
}

fn flood(lines: usize) -> i32 {
    let (out, err) = (std::io::stdout(), std::io::stderr());
    let (mut out, mut err) = (out.lock(), err.lock());
    for i in 0..lines {
        writeln!(out, "out {i:08}").unwrap();
        writeln!(err, "err {i:08}").unwrap();
    }
    out.write_all(b"last-out\n").unwrap();
    err.write_all(b"last-err\n").unwrap();
    0
}

#[cfg(unix)]
fn ignore_the_request_to_stop() {
    // SAFETY: sets how this process, a stand-in about to be killed, treats one signal.
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
    }
}

#[cfg(not(unix))]
fn ignore_the_request_to_stop() {}
