use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use std::sync::OnceLock;

use crate::platform::openwith::{self, AppDirs, DesktopEntry, MimeDb, OpenApp, OpenChoices};
use crate::platform::which::on_path;
use crate::platform::{Opener, split_command};
use crate::{Error, Result};

/// Opens things with what the desktop has: `xdg-open`, the applications of the freedesktop.org
/// folders it was given, a terminal emulator.
pub struct XdgOpener {
    dirs: AppDirs,
    db: OnceLock<MimeDb>,
}

impl XdgOpener {
    pub fn new(dirs: AppDirs) -> XdgOpener {
        XdgOpener {
            dirs,
            db: OnceLock::new(),
        }
    }

    /// The MIME database, read the first time it is needed.
    fn db(&self) -> &MimeDb {
        self.db.get_or_init(|| MimeDb::load(&self.dirs.data))
    }
}

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

/// Terminal emulators to try when `$TERMINAL` is not set.
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

/// The terminal to run a program in: `$TERMINAL`, else the first emulator found.
fn terminal_words() -> Option<Vec<String>> {
    std::env::var("TERMINAL")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .map(|t| split_command(&t))
        .filter(|w| !w.is_empty())
        .or_else(|| {
            TERMINALS
                .iter()
                .find(|t| on_path(t))
                .map(|t| vec![t.to_string()])
        })
}

/// What a terminal wants between its own name and the program it should run.
fn run_flag(terminal: &str) -> &'static [&'static str] {
    let name = Path::new(terminal)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(terminal);
    match name {
        "gnome-terminal" => &["--"],
        "wezterm" => &["start", "--"],
        "kitty" | "foot" => &[],
        _ => &["-e"],
    }
}

/// The command that opens `path` with `app`: its `Exec` line with the field codes filled in,
/// inside a terminal (`terminal` finds one) when the application asks for it.
fn command_for(
    path: &Path,
    app: &OpenApp,
    terminal: impl FnOnce() -> Option<Vec<String>>,
) -> Result<Command> {
    let entry = DesktopEntry {
        name: app.name.clone(),
        exec: app.exec.clone(),
        icon: app.icon.clone(),
        terminal: app.terminal,
        ..DesktopEntry::default()
    };
    let args = openwith::expand_exec(&app.exec, &[path], &entry, app.source.as_deref());
    let Some((program, rest)) = args.split_first() else {
        return Err(Error::Invalid(format!("{} has nothing to run", app.name)));
    };
    if !app.terminal {
        let mut cmd = Command::new(program);
        cmd.args(rest);
        return Ok(cmd);
    }
    let Some(words) = terminal() else {
        return Err(Error::Invalid(
            "no terminal emulator found: install one, or set $TERMINAL".into(),
        ));
    };
    let mut cmd = Command::new(&words[0]);
    cmd.args(&words[1..])
        .args(run_flag(&words[0]))
        .arg(program)
        .args(rest);
    Ok(cmd)
}

impl Opener for XdgOpener {
    fn kind_of(&self, path: &Path) -> Option<String> {
        Some(openwith::mime_of(self.db(), path))
    }

    fn apps_for(&self, path: &Path) -> OpenChoices {
        let lang = crate::platform::tags::language_from_env();
        let (kind, apps) = openwith::find_apps(&self.dirs, self.db(), path, &lang);
        OpenChoices {
            kind: Some(kind),
            apps,
        }
    }

    fn open_with_app(&self, path: &Path, app: &OpenApp) -> Result<()> {
        let cmd = command_for(path, app, terminal_words)?;
        detach(cmd, "start the program for", path)
    }

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
        let Some(words) = terminal_words() else {
            return Err(Error::Invalid(
                "no terminal emulator found: install one, or set $TERMINAL".into(),
            ));
        };
        let mut cmd = Command::new(&words[0]);
        cmd.args(&words[1..]).current_dir(dir);
        detach(cmd, "start the terminal in", dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exec: &str, terminal: bool) -> OpenApp {
        OpenApp {
            id: "x.desktop".into(),
            name: "X".into(),
            exec: exec.into(),
            terminal,
            is_default: false,
            source: None,
            icon: None,
        }
    }

    fn words(c: &Command) -> Vec<String> {
        std::iter::once(c.get_program())
            .chain(c.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_program_is_run_with_the_file_where_its_line_puts_it() {
        let c = command_for(
            Path::new("/d/a b.txt"),
            &app("edit --new %f", false),
            || None,
        )
        .unwrap();
        assert_eq!(words(&c), ["edit", "--new", "/d/a b.txt"]);
    }

    #[test]
    fn an_application_that_wants_a_terminal_gets_one_the_way_that_terminal_asks() {
        let f = Path::new("/d/a.txt");
        let vim = app("vim %F", true);
        let run = |t: &str| {
            words(&command_for(f, &vim, || Some(t.split(' ').map(String::from).collect())).unwrap())
        };
        assert_eq!(run("xterm"), ["xterm", "-e", "vim", "/d/a.txt"]);
        assert_eq!(
            run("gnome-terminal"),
            ["gnome-terminal", "--", "vim", "/d/a.txt"]
        );
        assert_eq!(
            run("wezterm"),
            ["wezterm", "start", "--", "vim", "/d/a.txt"]
        );
        assert_eq!(run("kitty"), ["kitty", "vim", "/d/a.txt"]);
        assert_eq!(
            run("/usr/bin/alacritty --class x"),
            [
                "/usr/bin/alacritty",
                "--class",
                "x",
                "-e",
                "vim",
                "/d/a.txt"
            ]
        );
        // No terminal: said, not guessed.
        assert!(command_for(f, &vim, || None).is_err());
        // A line with nothing in it is an error, not an empty command.
        assert!(command_for(f, &app("   ", false), || None).is_err());
    }
}
