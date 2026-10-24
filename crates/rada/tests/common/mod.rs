//! Shared by the tests that run rada on a pseudo-terminal: the terminal, a sandbox to run in,
//! and the program started on it.

#![cfg(target_os = "linux")]
#![allow(dead_code)]

use std::fs;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct Pty {
    pub master: OwnedFd,
    pub slave: OwnedFd,
}

pub fn open_pty() -> Pty {
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

pub fn read_some(fd: &OwnedFd, out: &mut Vec<u8>, wait: Duration) {
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

pub fn write_all(fd: &OwnedFd, bytes: &[u8]) {
    use std::os::fd::AsRawFd;
    // SAFETY: a valid descriptor and a slice.
    let n = unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
    assert_eq!(n, bytes.len() as isize);
}

pub fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

pub fn tail(s: &str) -> String {
    let n = s.chars().count();
    s.chars().skip(n.saturating_sub(300)).collect()
}

/// A home, the folders of the XDG layout and one folder with one file, all made for the test.
pub struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        let root = tempfile::Builder::new()
            .prefix("rada-pty-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        let sandbox = Sandbox { root };
        for d in [
            "home",
            "config/rada",
            "data",
            "state",
            "cache",
            "bin",
            "docs",
        ] {
            fs::create_dir_all(sandbox.p(d)).unwrap();
        }
        fs::write(sandbox.p("docs/a.txt"), "hello\n").unwrap();
        fs::write(
            sandbox.p("config/rada/config.toml"),
            "[update]\ncheck = false\n",
        )
        .unwrap();
        sandbox
    }

    pub fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    pub fn path(&self) -> &Path {
        self.root.path()
    }
}

/// Start rada in the sandbox's `docs` folder, with the given descriptors as its input and its
/// screen. With `controlling` the first one is also the terminal of the session (so closing
/// it hangs the program up); without, it is only read, and closing it is an error to rada.
pub fn spawn_rada(
    sandbox: &Sandbox,
    input: &OwnedFd,
    screen: &OwnedFd,
    controlling: bool,
    env: &[(&str, &str)],
) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rada"));
    cmd.arg(sandbox.p("docs"))
        .env_clear()
        .env("HOME", sandbox.p("home"))
        .env("XDG_CONFIG_HOME", sandbox.p("config"))
        .env("XDG_DATA_HOME", sandbox.p("data"))
        .env("XDG_STATE_HOME", sandbox.p("state"))
        .env("XDG_CACHE_HOME", sandbox.p("cache"))
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", sandbox.p("bin").display()),
        )
        .env("TERM", "xterm-256color")
        .env("LC_ALL", "en_US.UTF-8")
        // No question to the terminal at start-up, no font offer, no update server.
        .env("RADA_NO_TERM_QUERY", "1")
        .env("RADA_ICONS", "ascii")
        .env("RADA_UPDATE_API", "http://127.0.0.1:9/none")
        .envs(env.iter().copied())
        .stdin(Stdio::from(input.try_clone().unwrap()))
        .stdout(Stdio::from(screen.try_clone().unwrap()))
        .stderr(Stdio::from(screen.try_clone().unwrap()));
    // SAFETY: only async-signal-safe calls between fork and exec: the child becomes the
    // leader of a session, whose terminal is its input if it is to be.
    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() < 0 || (controlling && libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0) {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn().unwrap()
}
