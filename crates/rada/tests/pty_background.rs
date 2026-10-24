//! rada on a terminal of its own (a pseudo-terminal): the terminal's background.
//!
//! While the default theme paints its night blue, rada sets the terminal's background to the
//! same colour (OSC 11), so that the margin a terminal keeps around its grid is not a frame
//! of another colour, and gives it back (OSC 111) however rada ends. Where the background is
//! left alone, nothing is written.
//!
//! Nothing real is started: the folder and the home are made for the test, and no update check
//! goes anywhere.

#![cfg(target_os = "linux")]

mod common;

use std::os::fd::OwnedFd;
use std::process::Child;
use std::time::{Duration, Instant};

use common::{Sandbox, count, open_pty, read_some, spawn_rada, write_all};

/// `#1b1d27`, the night blue of the default theme, set on a terminal of true colour.
const SET: &[u8] = b"\x1b]11;rgb:1b/1d/27\x1b\\";
const RESET: &[u8] = b"\x1b]111\x1b\\";
const LEAVE_SCREEN: &[u8] = b"\x1b[?1049l";

/// What the case is called, the configuration it runs with and the variables it adds.
type Case<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);

fn position(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read what rada writes to `screen` for a while, answering the cursor question as a terminal
/// would.
fn pump(master: &OwnedFd, seen: &mut Vec<u8>, for_: Duration) {
    let end = Instant::now() + for_;
    let mut answered = count(seen, b"\x1b[6n");
    while Instant::now() < end {
        read_some(master, seen, Duration::from_millis(50));
        while answered < count(seen, b"\x1b[6n") {
            write_all(master, b"\x1b[5;5R");
            answered += 1;
        }
    }
}

/// Read until rada has ended (or a few seconds went by); its exit status.
fn ended(master: &OwnedFd, seen: &mut Vec<u8>, child: &mut Child) -> std::process::ExitStatus {
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        pump(master, seen, Duration::from_millis(100));
        if let Some(status) = child.try_wait().unwrap() {
            pump(master, seen, Duration::from_millis(100));
            return status;
        }
        if Instant::now() > end {
            child.kill().ok();
            panic!("rada did not end ({} bytes written)", seen.len());
        }
    }
}

/// Start rada on a terminal of true colour with `config` and `env`, let it draw, press `q`.
/// Returns everything it wrote.
fn start_and_quit(config: &str, env: &[(&str, &str)]) -> Vec<u8> {
    let sandbox = Sandbox::new();
    std::fs::write(
        sandbox.p("config/rada/config.toml"),
        format!("{config}\n[update]\ncheck = false\n"),
    )
    .unwrap();
    let pty = open_pty();
    let env = [&[("COLORTERM", "truecolor")], env].concat();
    let mut child = spawn_rada(&sandbox, &pty.slave, &pty.slave, true, &env);
    drop(pty.slave);
    let mut seen = Vec::new();
    pump(&pty.master, &mut seen, Duration::from_millis(1500));
    write_all(&pty.master, b"q");
    let status = ended(&pty.master, &mut seen, &mut child);
    assert!(
        status.success(),
        "{status}: {}",
        String::from_utf8_lossy(&seen)
    );
    seen
}

#[test]
fn the_background_is_set_at_the_start_and_given_back_at_the_end() {
    let seen = start_and_quit("", &[]);
    let (set, reset) = (position(&seen, SET), position(&seen, RESET));
    assert!(
        set.is_some(),
        "never set: {:?}",
        String::from_utf8_lossy(&seen)
    );
    assert_eq!(count(&seen, SET), 1);
    assert_eq!(count(&seen, RESET), 1);
    assert!(set < reset, "given back before it was set");
    // Given back as the terminal is left, not before the interface is gone.
    assert!(position(&seen, LEAVE_SCREEN) < reset);
}

#[test]
fn the_background_is_given_back_when_rada_is_told_to_end() {
    let sandbox = Sandbox::new();
    let pty = open_pty();
    let env = [("COLORTERM", "truecolor")];
    let mut child = spawn_rada(&sandbox, &pty.slave, &pty.slave, true, &env);
    drop(pty.slave);
    let mut seen = Vec::new();
    pump(&pty.master, &mut seen, Duration::from_millis(1500));
    assert_eq!(count(&seen, SET), 1, "never set");
    // SAFETY: the pid is that of the child just started, still running.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    ended(&pty.master, &mut seen, &mut child);
    assert_eq!(count(&seen, RESET), 1, "not given back");
}

#[test]
fn nothing_is_written_where_the_background_stays() {
    let cases: [Case; 3] = [
        ("transparent", "transparent_background = true", &[]),
        ("tmux", "", &[("TMUX", "/tmp/tmux-0/default,1,0")]),
        ("NO_COLOR", "", &[("NO_COLOR", "1")]),
    ];
    for (why, config, env) in cases {
        let seen = start_and_quit(config, env);
        assert_eq!(count(&seen, b"\x1b]11;"), 0, "{why}");
        assert_eq!(count(&seen, b"\x1b]111"), 0, "{why}");
    }
}
