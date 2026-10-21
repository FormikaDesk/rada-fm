//! The state of a folder that is in a git repository: the branch, how many files are changed
//! or new, and which of the items of the folder are.
//!
//! The panels view asks for it with `git status --porcelain=v2 -b -z` on a thread of its own,
//! with a limit on the time it may take, and shows what comes back. With git not installed,
//! switched off in `config.toml`, a folder that is not in a repository, a repository that
//! cannot be read or a status that takes too long, nothing is sent and nothing is shown: there
//! is no error to report.
//!
//! git is asked not to take locks (`--no-optional-locks`), not to run the file system monitor a
//! repository may name in its own configuration, and not to ask anything on the terminal.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;

use crate::events::CoreEvent;

/// How long git may take, for each of the two questions it is asked, before it is given up on.
const PATIENCE: Duration = Duration::from_millis(2500);

/// What is known of an item of the folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileState {
    /// Changed since the last commit, in the working tree or in the index; for a folder, with
    /// something changed inside it.
    Modified,
    /// Not known to git (and not ignored); for a folder, with something new inside it.
    New,
}

/// The state of a repository seen from one of its folders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    /// The branch, or the start of the commit's name where no branch is checked out.
    pub branch: String,
    /// Commits the branch has that its upstream does not, and the other way round.
    pub ahead: u32,
    pub behind: u32,
    /// Files changed in the whole repository, and files that are new.
    pub modified: usize,
    pub new: usize,
    /// The items of the folder asked about, by name.
    pub items: HashMap<String, FileState>,
}

impl GitStatus {
    /// Nothing changed and nothing new.
    pub fn is_clean(&self) -> bool {
        self.modified == 0 && self.new == 0
    }
}

/// Read what `git status --porcelain=v2 -b -z` wrote. `prefix` is where the folder asked
/// about is in the repository (`sub/dir/`, empty at the top): the paths in the answer start
/// at the top, and the items of the folder are the first names after the prefix.
pub fn parse(text: &str, prefix: &str) -> GitStatus {
    let mut st = GitStatus::default();
    let mut oid = String::new();
    let mut records = text.split('\0').filter(|r| !r.is_empty());
    let mark = |st: &mut GitStatus, path: &str, state: FileState| {
        let Some(inside) = path.strip_prefix(prefix) else {
            return;
        };
        let name = inside.split('/').next().unwrap_or("");
        if name.is_empty() {
            return;
        }
        // A change wins over something new inside the same folder.
        let entry = st.items.entry(name.to_string()).or_insert(state);
        if state == FileState::Modified {
            *entry = FileState::Modified;
        }
    };
    while let Some(r) = records.next() {
        if let Some(h) = r.strip_prefix("# ") {
            let mut parts = h.splitn(2, ' ');
            match (parts.next(), parts.next()) {
                (Some("branch.oid"), Some(o)) => oid = o.to_string(),
                (Some("branch.head"), Some(name)) => st.branch = name.to_string(),
                (Some("branch.ab"), Some(ab)) => {
                    for n in ab.split_whitespace() {
                        if let Some(a) = n.strip_prefix('+') {
                            st.ahead = a.parse().unwrap_or(0);
                        } else if let Some(b) = n.strip_prefix('-') {
                            st.behind = b.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = r.chars().next();
        match kind {
            // An ordinary change: `1 XY sub mH mI mW hH hI path`.
            Some('1') => {
                if let Some(path) = r.splitn(9, ' ').nth(8) {
                    st.modified += 1;
                    mark(&mut st, path, FileState::Modified);
                }
            }
            // A rename or a copy: `2 XY sub mH mI mW hH hI Xscore path`, then the old path as a
            // record of its own.
            Some('2') => {
                if let Some(path) = r.splitn(10, ' ').nth(9) {
                    st.modified += 1;
                    mark(&mut st, path, FileState::Modified);
                }
                records.next();
            }
            // Unmerged: `u XY sub m1 m2 m3 mW h1 h2 h3 path`.
            Some('u') => {
                if let Some(path) = r.splitn(11, ' ').nth(10) {
                    st.modified += 1;
                    mark(&mut st, path, FileState::Modified);
                }
            }
            Some('?') => {
                if let Some(path) = r.strip_prefix("? ") {
                    st.new += 1;
                    mark(&mut st, path, FileState::New);
                }
            }
            _ => {}
        }
    }
    // Before the first commit, or with no branch checked out, the name is what there is.
    if st.branch == "(detached)" || st.branch.is_empty() {
        let short: String = oid.chars().take(7).collect();
        st.branch = if short.is_empty() || oid == "(initial)" {
            "(no commits)".to_string()
        } else {
            short
        };
    }
    st
}

/// Whether and how git is called.
#[derive(Clone, Debug)]
pub struct Git {
    enabled: bool,
    program: PathBuf,
    /// Variables set for the program (tests keep it away from the person's own settings).
    env: Vec<(String, String)>,
    /// How long it may take to answer each of the two questions.
    patience: Duration,
}

impl Git {
    /// git is never called.
    pub fn off() -> Git {
        Git {
            enabled: false,
            program: PathBuf::from("git"),
            env: Vec::new(),
            patience: PATIENCE,
        }
    }

    /// git is called when `enabled`: the `git` found in the PATH, or `program`.
    pub fn new(enabled: bool, program: Option<PathBuf>) -> Git {
        Git {
            enabled,
            program: program.unwrap_or_else(|| PathBuf::from("git")),
            env: Vec::new(),
            patience: PATIENCE,
        }
    }

    /// The same with `name=value` set in the environment of the program.
    pub fn with_env(mut self, name: &str, value: &str) -> Git {
        self.env.push((name.to_string(), value.to_string()));
        self
    }

    /// The same, giving up on a question after `patience`.
    pub fn with_patience(mut self, patience: Duration) -> Git {
        self.patience = patience;
        self
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Ask about `dir` and send the answer as a [`CoreEvent::Git`]: `None` when `dir` is not in
    /// a repository (so that what was shown for the folder before goes away). Nothing is sent
    /// when git is off or cannot be run or does not answer in time.
    pub fn status(&self, dir: &Path, out: Sender<CoreEvent>) {
        if !self.enabled {
            return;
        }
        let me = self.clone();
        let dir = dir.to_path_buf();
        let _ = std::thread::Builder::new()
            .name("rada-git".into())
            .spawn(move || {
                let Some(prefix) = me.run(&dir, &["rev-parse", "--show-prefix"]) else {
                    // Not a repository (git says so with an error), or no git at all: the same
                    // to a folder, which has nothing to show. A git that is missing is told
                    // from one that refuses only by there being nothing to send for both.
                    if me.exists() {
                        let _ = out.send(CoreEvent::Git { dir, status: None });
                    }
                    return;
                };
                let prefix = prefix.trim_end_matches(['\n', '\r']).to_string();
                let Some(text) = me.run(
                    &dir,
                    &[
                        "status",
                        "--porcelain=v2",
                        "-b",
                        "-z",
                        "--no-renames",
                        "--untracked-files=normal",
                    ],
                ) else {
                    return;
                };
                let status = parse(&text, &prefix);
                let _ = out.send(CoreEvent::Git {
                    dir,
                    status: Some(status),
                });
            });
    }

    /// Whether the program can be started at all.
    fn exists(&self) -> bool {
        self.command()
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.envs(self.env.iter().map(|(k, v)| (k, v)))
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        cmd
    }

    /// Run `git -C dir <args>` and return what it wrote if it ended well in time.
    fn run(&self, dir: &Path, args: &[&str]) -> Option<String> {
        let mut cmd = self.command();
        cmd.arg("--no-optional-locks")
            .args(["-c", "core.fsmonitor=false"])
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().ok()?;
        let mut pipe = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        });
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if start.elapsed() < self.patience => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let bytes = reader.join().ok()?;
        status
            .success()
            .then(|| String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A status as git writes it with `-z`: records ended by NUL.
    fn z(records: &[&str]) -> String {
        records.iter().map(|r| format!("{r}\0")).collect()
    }

    const OID: &str = "# branch.oid 4a7c1f0e2d3b4a5c6d7e8f9012345678abcdef01";

    #[test]
    fn the_branch_the_counts_and_the_items_of_the_folder_are_read() {
        let text = z(&[
            OID,
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 aaaa bbbb src/main.rs",
            "1 M. N... 100644 100644 100644 aaaa bbbb README.md",
            "1 .M N... 100644 100644 100644 aaaa bbbb src/lib.rs",
            "? notes.txt",
            "? target/",
            "! ignored.log",
        ]);
        let st = parse(&text, "");
        assert_eq!(st.branch, "main");
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert_eq!((st.modified, st.new), (3, 2));
        assert!(!st.is_clean());
        // At the top: a folder is changed when something inside it is.
        assert_eq!(st.items["src"], FileState::Modified);
        assert_eq!(st.items["README.md"], FileState::Modified);
        assert_eq!(st.items["notes.txt"], FileState::New);
        assert_eq!(st.items["target"], FileState::New);
        assert!(
            !st.items.contains_key("ignored.log"),
            "ignored files are not shown"
        );
    }

    #[test]
    fn a_folder_inside_the_repository_sees_only_its_own_items() {
        let text = z(&[
            OID,
            "# branch.head feat/x",
            "1 .M N... 100644 100644 100644 aaaa bbbb src/main.rs",
            "1 .M N... 100644 100644 100644 aaaa bbbb docs/guide.md",
            "? src/new.rs",
            "? src/deep/er/file",
        ]);
        let st = parse(&text, "src/");
        assert_eq!(st.branch, "feat/x");
        // The counts are the repository's; the items are the folder's.
        assert_eq!((st.modified, st.new), (2, 2));
        assert_eq!(st.items.len(), 3);
        assert_eq!(st.items["main.rs"], FileState::Modified);
        assert_eq!(st.items["new.rs"], FileState::New);
        assert_eq!(st.items["deep"], FileState::New);
        assert!(!st.items.contains_key("guide.md"));
        // A name that only starts like the prefix is not in it.
        let st = parse(&z(&["? srcs/a"]), "src/");
        assert!(st.items.is_empty());
    }

    #[test]
    fn a_change_wins_over_something_new_in_the_same_folder() {
        let text = z(&[
            "? src/new.rs",
            "1 .M N... 100644 100644 100644 aaaa bbbb src/main.rs",
            "? src/other.rs",
        ]);
        assert_eq!(parse(&text, "").items["src"], FileState::Modified);
    }

    #[test]
    fn renames_conflicts_and_names_with_spaces_are_counted_once() {
        let text = z(&[
            "2 R. N... 100644 100644 100644 aaaa bbbb R100 new name.txt",
            "old name.txt",
            "u UU N... 100644 100644 100644 100644 aaaa bbbb cccc merge conflict.rs",
            "1 A. N... 000000 100644 100644 0000 bbbb added file.txt",
        ]);
        let st = parse(&text, "");
        assert_eq!(
            st.modified, 3,
            "the old name of a rename is not a record of its own"
        );
        assert_eq!(st.items["new name.txt"], FileState::Modified);
        assert_eq!(st.items["merge conflict.rs"], FileState::Modified);
        assert_eq!(st.items["added file.txt"], FileState::Modified);
        assert!(!st.items.contains_key("old name.txt"));
    }

    #[test]
    fn a_detached_head_and_a_repository_without_commits_have_a_name_too() {
        let detached = parse(&z(&[OID, "# branch.head (detached)"]), "");
        assert_eq!(detached.branch, "4a7c1f0");
        let fresh = parse(
            &z(&["# branch.oid (initial)", "# branch.head main", "? a.txt"]),
            "",
        );
        assert_eq!(fresh.branch, "main");
        let nothing = parse(
            &z(&["# branch.oid (initial)", "# branch.head (detached)"]),
            "",
        );
        assert_eq!(nothing.branch, "(no commits)");
        assert!(parse("", "").is_clean());
    }

    #[test]
    fn nothing_is_run_while_git_is_off() {
        let (tx, rx) = crossbeam_channel::unbounded();
        Git::off().status(Path::new("."), tx);
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        assert!(!Git::off().enabled() && Git::new(true, None).enabled());
    }
}
