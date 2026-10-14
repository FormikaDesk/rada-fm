//! Starting a program from rada without letting it reach rada's console.
//!
//! **The problem.** rada draws on the console it was started in. A program that Windows starts
//! for a file (`ShellExecute`, the "Open with" handlers) is a *child of rada*, and many of
//! them — every Electron editor (VS Code and the like), for one — call
//! `AttachConsole(ATTACH_PARENT_PROCESS)` at start so that they can print their logs when they
//! were started from a terminal. Their parent is rada, so they attach to rada's console and
//! write there: the screen scrolls, lines are doubled, the panels move.
//!
//! **The way out.** `AttachConsole(ATTACH_PARENT_PROCESS)` attaches to the console *of the
//! parent process*. So rada never starts such a program itself: it starts a small helper — its
//! own executable, with a hidden first argument — *without a console* (`DETACHED_PROCESS`,
//! also `CREATE_NO_WINDOW`) and with standard input and error closed. The helper has no console
//! to hand on, makes the call to the shell, and ends. The program it started has a parent with
//! no console (or no parent at all by then), and its attach fails quietly. The helper's
//! standard output is a pipe, on which it says why it failed, if it did: the message reaches
//! the person as before.
//!
//! Programs that rada starts directly (explorer, Windows Terminal, the command typed in "Open
//! with… Other program…") are started the same way: no console, no window, no standard streams.
//!
//! What is built here is data, not a process, so that the flags and the streams are tested on
//! every system.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The new process gets no console at all.
pub const DETACHED_PROCESS: u32 = 0x0000_0008;
/// A console program started with this has no window; with `DETACHED_PROCESS` it has no console
/// either (Windows ignores this flag then; both are asked for, so that either alone would do).
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Ctrl+C aimed at rada does not reach it.
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
/// A console of its own (PowerShell opened in a folder, on purpose).
pub const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// No console and no window.
pub const NO_CONSOLE: u32 = DETACHED_PROCESS | CREATE_NO_WINDOW;

/// What the first argument of rada is when it runs as the helper (not an option of the
/// command line: nobody types it).
pub const HELPER_ARG: &str = "--launch-helper";

/// What a standard stream of a started process is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// Nothing: reads end at once, writes are thrown away.
    Null,
    /// A pipe to rada.
    Piped,
}

/// A process to start: the program, its arguments, the creation flags and the streams.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Windows `dwCreationFlags` (ignored elsewhere).
    pub creation_flags: u32,
    pub stdin: Stream,
    pub stdout: Stream,
    pub stderr: Stream,
}

impl Spawn {
    /// A program started on its own: no console, no window, no streams.
    pub fn detached(program: impl Into<PathBuf>, args: Vec<OsString>, extra_flags: u32) -> Spawn {
        Spawn {
            program: program.into(),
            args,
            creation_flags: NO_CONSOLE | extra_flags,
            stdin: Stream::Null,
            stdout: Stream::Null,
            stderr: Stream::Null,
        }
    }
}

/// What the helper is asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Task {
    /// Open the item with the default program of the system.
    Open(PathBuf),
    /// Open the item with the program the shell lists under this name.
    OpenWith { path: PathBuf, app_id: String },
}

/// The helper to start for a task: this executable, `--launch-helper`, what to do. Standard
/// input and error are nothing; standard output is a pipe (the reason, when it fails).
pub fn helper_spawn(exe: &Path, task: &Task) -> Spawn {
    let mut args: Vec<OsString> = vec![HELPER_ARG.into()];
    match task {
        Task::Open(path) => {
            args.push("open".into());
            args.push(path.as_os_str().to_owned());
        }
        Task::OpenWith { path, app_id } => {
            args.push("open-with".into());
            args.push(app_id.into());
            args.push(path.as_os_str().to_owned());
        }
    }
    Spawn {
        program: exe.to_path_buf(),
        args,
        creation_flags: NO_CONSOLE,
        stdin: Stream::Null,
        stdout: Stream::Piped,
        stderr: Stream::Null,
    }
}

/// The task in the arguments a process was started with (`args` without the program name), if
/// they are those of [`helper_spawn`].
pub fn parse_helper_args(args: &[OsString]) -> Option<Task> {
    let mut it = args.iter();
    if it.next()?.to_str()? != HELPER_ARG {
        return None;
    }
    match it.next()?.to_str()? {
        "open" => Some(Task::Open(PathBuf::from(it.next()?))),
        "open-with" => {
            let app_id = it.next()?.to_str()?.to_string();
            Some(Task::OpenWith {
                path: PathBuf::from(it.next()?),
                app_id,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\rada\rada.exe")
    }

    #[test]
    fn the_helper_has_no_console_no_window_and_only_stdout_open() {
        let s = helper_spawn(&exe(), &Task::Open(PathBuf::from(r"C:\docs\CHANGELOG.md")));
        assert_eq!(s.program, exe());
        // No console to hand on, and no window of its own.
        assert_ne!(s.creation_flags & DETACHED_PROCESS, 0);
        assert_ne!(s.creation_flags & CREATE_NO_WINDOW, 0);
        // It must not get a console of its own either, which a program could then attach to.
        assert_eq!(s.creation_flags & CREATE_NEW_CONSOLE, 0);
        assert_eq!(s.stdin, Stream::Null);
        assert_eq!(s.stderr, Stream::Null);
        // The one pipe is the way the reason of a failure comes back.
        assert_eq!(s.stdout, Stream::Piped);
    }

    #[test]
    fn the_flags_have_the_values_windows_defines() {
        assert_eq!(DETACHED_PROCESS, 0x8);
        assert_eq!(CREATE_NEW_CONSOLE, 0x10);
        assert_eq!(CREATE_NEW_PROCESS_GROUP, 0x200);
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        assert_eq!(NO_CONSOLE, 0x0800_0008);
    }

    #[test]
    fn a_task_goes_through_the_arguments_and_back() {
        for task in [
            Task::Open(PathBuf::from(r"C:\a b\c & d\100%PATH%.md")),
            Task::OpenWith {
                path: PathBuf::from(r"C:\x y\file.md"),
                app_id: "Code.exe".into(),
            },
        ] {
            let s = helper_spawn(&exe(), &task);
            assert_eq!(s.args[0], OsString::from(HELPER_ARG));
            // Each path is one argument: no shell in between, so nothing in a name is special.
            assert_eq!(parse_helper_args(&s.args), Some(task));
        }
    }

    #[test]
    fn other_arguments_are_not_a_task() {
        let a = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(parse_helper_args(&a(&[])), None);
        assert_eq!(parse_helper_args(&a(&["--help"])), None);
        assert_eq!(parse_helper_args(&a(&[HELPER_ARG])), None);
        assert_eq!(parse_helper_args(&a(&[HELPER_ARG, "open"])), None);
        assert_eq!(parse_helper_args(&a(&[HELPER_ARG, "format", "C:"])), None);
        assert_eq!(
            parse_helper_args(&a(&["setup", HELPER_ARG, "open", "x"])),
            None
        );
    }

    #[test]
    fn a_program_started_on_its_own_has_no_console_window_or_streams() {
        let s = Spawn::detached("code", vec!["a.md".into()], CREATE_NEW_PROCESS_GROUP);
        assert_eq!(
            s.creation_flags,
            DETACHED_PROCESS | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
        );
        assert_eq!(
            (s.stdin, s.stdout, s.stderr),
            (Stream::Null, Stream::Null, Stream::Null)
        );
        assert_eq!(s.args, vec![OsString::from("a.md")]);
    }
}
