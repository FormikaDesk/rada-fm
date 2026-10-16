//! rada on a terminal of its own (a pseudo-terminal), opening a file with another program.
//!
//! Opening a program makes rada draw the screen again from nothing, and clearing the screen
//! used to ask the terminal where the cursor is while the keyboard thread held the only
//! reader of the answer: the question failed after two seconds and rada ended with
//! "The cursor position could not be read within a normal duration". Here a terminal that
//! answers and one that does not both see rada stay where it is.
//!
//! Nothing real is started: the folder, the home and the programs are made for the test, the
//! program that "opens" the file does nothing, and no update check goes anywhere.

#![cfg(target_os = "linux")]

use std::fs;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
}

fn open_pty() -> Pty {
    let (mut m, mut s) = (0, 0);
    let size = libc::winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: both descriptors are fresh and owned from here on; the null name and termios
    // are allowed by openpty.
    unsafe {
        let r = libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null(),
            &size,
        );
        assert_eq!(r, 0, "openpty");
        Pty {
            master: OwnedFd::from_raw_fd(m),
            slave: OwnedFd::from_raw_fd(s),
        }
    }
}

fn read_some(fd: &OwnedFd, out: &mut Vec<u8>, wait: Duration) {
    use std::os::fd::AsRawFd;
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd, and a buffer of the length given.
    unsafe {
        if libc::poll(&mut p, 1, wait.as_millis() as i32) > 0 && p.revents & libc::POLLIN != 0 {
            let mut buf = [0u8; 65536];
            let n = libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len());
            if n > 0 {
                out.extend_from_slice(&buf[..n as usize]);
            }
        }
    }
}

fn write_all(fd: &OwnedFd, bytes: &[u8]) {
    use std::os::fd::AsRawFd;
    // SAFETY: a valid descriptor and a slice.
    let n = unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
    assert_eq!(n, bytes.len() as isize);
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// Start rada on a pseudo-terminal, press Enter on the one file, and watch for a while.
/// Returns what rada wrote, whether it was still running, and what it logged.
fn open_a_file(answers_cursor: bool) -> (Vec<u8>, bool, String) {
    let root = tempfile::Builder::new()
        .prefix("rada-pty-")
        .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .unwrap();
    let p = |rel: &str| root.path().join(rel);
    for d in [
        "home",
        "config/rada",
        "data",
        "state",
        "cache",
        "bin",
        "docs",
    ] {
        fs::create_dir_all(p(d)).unwrap();
    }
    fs::write(p("docs/a.txt"), "hello\n").unwrap();
    fs::write(p("config/rada/config.toml"), "[update]\ncheck = false\n").unwrap();
    // Whatever the system would open the file with: a program that does nothing.
    for name in [
        "xdg-open",
        "gio",
        "kde-open",
        "kde-open5",
        "kde-open6",
        "gnome-open",
        "exo-open",
    ] {
        let f = p("bin").join(name);
        fs::write(&f, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let pty = open_pty();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rada"));
    cmd.arg(p("docs"))
        .env_clear()
        .env("HOME", p("home"))
        .env("XDG_CONFIG_HOME", p("config"))
        .env("XDG_DATA_HOME", p("data"))
        .env("XDG_STATE_HOME", p("state"))
        .env("XDG_CACHE_HOME", p("cache"))
        .env("PATH", format!("{}:/usr/bin:/bin", p("bin").display()))
        .env("TERM", "xterm-256color")
        .env("LC_ALL", "en_US.UTF-8")
        // No question to the terminal at start-up, no font offer, no update server.
        .env("RADA_NO_TERM_QUERY", "1")
        .env("RADA_ICONS", "ascii")
        .env("RADA_UPDATE_API", "http://127.0.0.1:9/none")
        .stdin(Stdio::from(pty.slave.try_clone().unwrap()))
        .stdout(Stdio::from(pty.slave.try_clone().unwrap()))
        .stderr(Stdio::from(pty.slave.try_clone().unwrap()));
    // SAFETY: only async-signal-safe calls between fork and exec: the child becomes the
    // leader of a session whose terminal is the pseudo-terminal.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    drop(cmd);
    drop(pty.slave);

    let mut seen = Vec::new();
    let mut answered = 0;
    let mut pump = |seen: &mut Vec<u8>, for_: Duration| {
        let end = Instant::now() + for_;
        while Instant::now() < end {
            read_some(&pty.master, seen, Duration::from_millis(50));
            let asked = count(seen, b"\x1b[6n");
            while answers_cursor && answered < asked {
                write_all(&pty.master, b"\x1b[5;5R");
                answered += 1;
            }
        }
    };
    pump(&mut seen, Duration::from_millis(2000));
    write_all(&pty.master, b"\r");
    // The screen is drawn again after 0.3, 1.5 and 4 seconds.
    pump(&mut seen, Duration::from_millis(5500));
    let running = child.try_wait().unwrap().is_none();
    child.kill().ok();
    child.wait().ok();

    let mut log = String::new();
    let mut stack = vec![root.path().to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.to_string_lossy().contains("rada.log") {
                log.push_str(&fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    (seen, running, log)
}

#[test]
fn rada_stays_when_a_file_is_opened_on_a_terminal_that_answers_the_cursor_question() {
    let (seen, running, log) = open_a_file(true);
    let text = String::from_utf8_lossy(&seen).to_string();
    assert!(running, "rada ended: {}\nlog: {log}", tail(&text));
    assert!(!text.contains("cursor position"), "{}", tail(&text));
    // It did ask, and the answer was read: nothing to warn about.
    assert!(
        count(&seen, b"\x1b[6n") >= 1,
        "the cursor was never asked for"
    );
    assert!(!log.contains("did not say where the cursor is"), "{log}");
}

#[test]
fn rada_stays_when_a_file_is_opened_on_a_terminal_that_never_answers() {
    let (seen, running, log) = open_a_file(false);
    let text = String::from_utf8_lossy(&seen).to_string();
    assert!(running, "rada ended: {}\nlog: {log}", tail(&text));
    assert!(!text.contains("cursor position"), "{}", tail(&text));
    // The failure is in the log as a warning, and it was asked once only.
    assert_eq!(count(&seen, b"\x1b[6n"), 1, "it asked again");
    assert!(log.contains("did not say where the cursor is"), "{log}");
}

fn tail(s: &str) -> String {
    let n = s.chars().count();
    s.chars().skip(n.saturating_sub(300)).collect()
}
