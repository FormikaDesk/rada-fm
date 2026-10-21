//! zoxide, the program that remembers the folders a person goes to, as a source of places.
//!
//! rada asks it for the folders that match what is typed in the command bar
//! (`zoxide query --list`) and tells it about each folder it enters (`zoxide add`). Both run on
//! a thread of their own and neither ever reports a failure: with zoxide not installed, or
//! switched off in `config.toml`, nothing happens and nothing is shown.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;

use crate::events::CoreEvent;

/// Most folders asked of zoxide for one text.
pub const MAX_RESULTS: usize = 5;

/// How long zoxide may take to answer before it is given up on.
const PATIENCE: Duration = Duration::from_millis(1500);

/// Whether and how zoxide is called.
#[derive(Clone, Debug)]
pub struct Zoxide {
    enabled: bool,
    program: PathBuf,
}

impl Zoxide {
    /// zoxide is never called.
    pub fn off() -> Zoxide {
        Zoxide {
            enabled: false,
            program: PathBuf::from("zoxide"),
        }
    }

    /// zoxide is called when `enabled`: the `zoxide` found in the PATH, or `program`.
    pub fn new(enabled: bool, program: Option<PathBuf>) -> Zoxide {
        Zoxide {
            enabled,
            program: program.unwrap_or_else(|| PathBuf::from("zoxide")),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Ask for the folders that match `text` and send them as a [`CoreEvent::Zoxide`]. Nothing
    /// is sent when zoxide is off, absent, fails or finds nothing.
    pub fn query(&self, text: &str, out: Sender<CoreEvent>) {
        let keywords: Vec<String> = text.split_whitespace().map(str::to_string).collect();
        if !self.enabled || keywords.is_empty() {
            return;
        }
        let program = self.program.clone();
        let query = text.trim().to_string();
        let _ = std::thread::Builder::new()
            .name("rada-zoxide".into())
            .spawn(move || {
                let mut cmd = Command::new(&program);
                cmd.args(["query", "--list", "--"]).args(&keywords);
                let Some(stdout) = run(cmd) else { return };
                let dirs: Vec<PathBuf> = stdout
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute() && p.is_dir())
                    .take(MAX_RESULTS)
                    .collect();
                if !dirs.is_empty() {
                    let _ = out.send(CoreEvent::Zoxide { query, dirs });
                }
            });
    }

    /// Tell zoxide that `dir` was entered.
    pub fn add(&self, dir: &Path) {
        if !self.enabled {
            return;
        }
        let (program, dir) = (self.program.clone(), dir.to_path_buf());
        let _ = std::thread::Builder::new()
            .name("rada-zoxide-add".into())
            .spawn(move || {
                let mut cmd = Command::new(&program);
                cmd.args(["add", "--"]).arg(&dir);
                let _ = run(cmd);
            });
    }
}

/// Run `cmd` with no input and no window, and return what it wrote if it ended well in time.
fn run(mut cmd: Command) -> Option<String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let mut pipe = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < PATIENCE => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let text = reader.join().ok()?;
    status.success().then_some(text)
}
