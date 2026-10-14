//! The launch helper (rada started by rada, with no console, to call the shell and end): it
//! ends at once and leaves nothing running, and it is nowhere in the help.

use std::process::Command;

fn rada() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_rada"));
    c.env("LC_ALL", "en_US.UTF-8")
        .env("RADA_NO_TERM_QUERY", "1");
    c
}

// Not on Windows: there the shell, asked to open a file that is not there, may show a dialog
// and wait for a click, which a test must not do. The helper's command line is tested as data
// in `rada_core::platform::launch`.
#[cfg(not(windows))]
#[test]
fn the_helper_ends_at_once_by_itself_and_starts_nothing_else() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let dir = tempfile::Builder::new()
        .prefix("rada-helper-")
        .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .unwrap();
    // A file that is not there, so that on Windows the shell has nothing to start either.
    let missing = dir.path().join("not-there.md");
    let started = Instant::now();
    let mut child = rada()
        .args(["--launch-helper", "open"])
        .arg(&missing)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // It must end by itself, quickly: nobody is going to kill it.
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the helper is still running"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    // 1: the shell said no (Windows); 2: there is no shell to call (the other systems). It
    // did not open the interface, which would have waited for keys.
    assert!(
        matches!(status.code(), Some(1) | Some(2)),
        "unexpected end: {status:?}"
    );
}

#[test]
fn the_helper_is_in_no_help() {
    for args in [
        &["--help"][..],
        &["-h"],
        &["update", "--help"],
        &["setup", "--help"],
        &["doctor", "--help"],
    ] {
        let out = rada().args(args).output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !text.contains("launch-helper") && !text.contains("launch_helper"),
            "{args:?}:\n{text}"
        );
    }
}

#[test]
fn what_is_not_a_helper_task_is_an_ordinary_command_line() {
    // Not the shape of a task: clap deals with it as with any unknown option.
    let out = rada().args(["--launch-helper"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected argument"), "{err}");
}
