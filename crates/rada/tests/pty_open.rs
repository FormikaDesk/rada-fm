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

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::{Sandbox, count, open_pty, read_some, spawn_rada, tail, write_all};

/// Start rada on a pseudo-terminal, press Enter on the one file, and watch for a while.
/// Returns what rada wrote, whether it was still running, and what it logged.
fn open_a_file(answers_cursor: bool) -> (Vec<u8>, bool, String) {
    let sandbox = Sandbox::new();
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
        let f = sandbox.p("bin").join(name);
        fs::write(&f, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let pty = open_pty();
    let mut child = spawn_rada(&sandbox, &pty.slave, &pty.slave, true, &[]);
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
    let mut stack = vec![sandbox.path().to_path_buf()];
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
fn open_file_keeps_rada_on_cursor_reply() {
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
