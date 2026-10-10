use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::platform::{Opener, split_command};
use crate::{Error, Result};

pub struct XdgOpener;

fn detach(mut cmd: Command, what: &'static str, subject: &Path) -> Result<()> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| Error::io(what, subject, e))?;
    // Reap it without blocking anybody.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn spawn_detached(program: &str, arg: &Path) -> Result<()> {
    let mut cmd = Command::new(program);
    cmd.arg(arg);
    detach(cmd, "start xdg-open for", arg)
}

/// Terminal emulators to try when `$TERMINAL` is not set, with the option that gives the
/// folder to start in (`None`: it starts in the folder rada sets for the process).
const TERMINALS: &[&str] = &[
    "x-terminal-emulator",
    "kitty",
    "alacritty",
    "foot",
    "wezterm",
    "ghostty",
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "tilix",
    "xterm",
];

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|d| {
            let p = d.join(name);
            p.is_file()
                && std::fs::metadata(&p).is_ok_and(|m| {
                    std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0
                })
        })
    })
}

impl Opener for XdgOpener {
    fn open(&self, path: &Path) -> Result<()> {
        spawn_detached("xdg-open", path)
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        spawn_detached("xdg-open", path.parent().unwrap_or(path))
    }

    fn open_with(&self, path: &Path, program: &str) -> Result<()> {
        let words = split_command(program);
        let Some((exe, args)) = words.split_first() else {
            return Err(Error::Invalid("no program given".into()));
        };
        let mut cmd = Command::new(exe);
        cmd.args(args).arg(path);
        detach(cmd, "start the program for", path)
    }

    fn open_terminal(&self, dir: &Path) -> Result<()> {
        let chosen = std::env::var("TERMINAL")
            .ok()
            .filter(|t| !t.trim().is_empty())
            .map(|t| split_command(&t))
            .filter(|w| !w.is_empty())
            .or_else(|| {
                TERMINALS
                    .iter()
                    .find(|t| on_path(t))
                    .map(|t| vec![t.to_string()])
            });
        let Some(words) = chosen else {
            return Err(Error::Invalid(
                "no terminal emulator found: install one, or set $TERMINAL".into(),
            ));
        };
        let mut cmd = Command::new(&words[0]);
        cmd.args(&words[1..]).current_dir(dir);
        detach(cmd, "start the terminal in", dir)
    }
}
