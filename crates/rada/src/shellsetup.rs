//! `rada setup shell`: make the shell follow the folder rada was left in (`v` instead of
//! `rada`), without editing anything by hand.
//!
//! The function itself is `rada --init <shell>` ([`crate::shell::snippet`]); this only decides
//! where the call goes:
//!
//! - fish: a file of its own, `conf.d/rada.fish` (fish reads every file there; `config.fish` is
//!   not touched);
//! - bash and zsh: a block in `~/.bashrc` (`~/.bash_profile` on a Mac, whose terminals start
//!   login shells) or in `$ZDOTDIR/.zshrc`;
//! - PowerShell: a block in the profile;
//! - nushell cannot run what a command prints (`source` wants a file when the script is
//!   read), so it gets a file of its own in its autoload folder, with the function in it.
//!
//! In a file that is the person's the block is delimited by `# >>> rada >>>` and
//! `# <<< rada <<<`, and `--remove` takes away those lines and nothing else.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rada_core::i18n::{Key, tf};
use rada_core::platform::Dirs;

const START: &str = "# >>> rada >>>";
const END: &str = "# <<< rada <<<";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Nushell,
    Pwsh,
}

impl Shell {
    /// `bash`, a path to it (`/bin/zsh`), `nu`, `pwsh`…
    pub fn parse(name: &str) -> Option<Shell> {
        let base = Path::new(name)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let base = base.trim_end_matches(".exe");
        Some(match base {
            "bash" | "sh" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            "nu" | "nushell" => Shell::Nushell,
            "pwsh" | "powershell" => Shell::Pwsh,
            _ => return None,
        })
    }

    /// The word `rada --init` knows it by.
    pub fn init_name(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Nushell => "nushell",
            Shell::Pwsh => "powershell",
        }
    }

    /// The line that loads the function, in this shell's own words.
    fn call(self) -> String {
        match self {
            Shell::Bash | Shell::Zsh => format!("eval \"$(rada --init {})\"", self.init_name()),
            Shell::Fish => "rada --init fish | source".into(),
            Shell::Pwsh => "rada --init powershell | Out-String | Invoke-Expression".into(),
            Shell::Nushell => String::new(),
        }
    }
}

/// Where things go (replaceable in tests).
pub struct ShellSetup {
    pub shell: Shell,
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`, or `~/.config`: where fish and PowerShell (off Windows) look.
    pub xdg_config: PathBuf,
    /// Where nushell keeps its data (its `vendor/autoload` is in it).
    pub nushell_data: PathBuf,
    /// `$ZDOTDIR`, if it is set.
    pub zdotdir: Option<PathBuf>,
    pub macos: bool,
    pub windows: bool,
}

/// What there is to do.
enum Todo {
    /// It is installed already.
    Nothing,
    /// The person's file already calls `rada --init`: this line.
    ByHand(String),
    /// Write, after showing these lines.
    Write(Vec<String>),
}

/// What would be written.
enum Plan {
    /// A file of rada's own, with this content.
    Own { path: PathBuf, content: String },
    /// A block in the person's file.
    Block { path: PathBuf, line: String },
}

impl Plan {
    fn path(&self) -> &Path {
        match self {
            Plan::Own { path, .. } | Plan::Block { path, .. } => path,
        }
    }
}

impl ShellSetup {
    pub fn from_env(dirs: &Dirs, shell: Option<&str>) -> Result<ShellSetup> {
        let shell = match shell {
            Some(name) => Shell::parse(name).with_context(|| tf(Key::SsUnknownShell, &[&name]))?,
            None if cfg!(windows) => Shell::Pwsh,
            None => {
                let var = std::env::var("SHELL").unwrap_or_default();
                Shell::parse(&var).with_context(|| tf(Key::SsUnknownShell, &[&var]))?
            }
        };
        let abs = |k: &str| {
            std::env::var_os(k)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        Ok(ShellSetup {
            shell,
            home: dirs.home.clone(),
            xdg_config: abs("XDG_CONFIG_HOME").unwrap_or_else(|| dirs.home.join(".config")),
            nushell_data: if cfg!(windows) {
                dirs.config.join("nushell")
            } else {
                dirs.data.join("nushell")
            },
            zdotdir: abs("ZDOTDIR"),
            macos: cfg!(target_os = "macos"),
            windows: cfg!(windows),
        })
    }

    fn plan(&self) -> Plan {
        match self.shell {
            Shell::Fish => Plan::Own {
                path: self.xdg_config.join("fish/conf.d/rada.fish"),
                content: format!(
                    "# Written by `rada setup shell`; `rada setup shell --remove` takes it away.\n{}\n",
                    self.shell.call()
                ),
            },
            Shell::Nushell => Plan::Own {
                path: self.nushell_data.join("vendor/autoload/rada.nu"),
                content: format!(
                    "# Written by `rada setup shell`; `rada setup shell --remove` takes it away.\n{}",
                    crate::shell::snippet("nushell")
                ),
            },
            Shell::Bash => Plan::Block {
                path: self.home.join(if self.macos {
                    ".bash_profile"
                } else {
                    ".bashrc"
                }),
                line: self.shell.call(),
            },
            Shell::Zsh => Plan::Block {
                path: self
                    .zdotdir
                    .clone()
                    .unwrap_or_else(|| self.home.clone())
                    .join(".zshrc"),
                line: self.shell.call(),
            },
            Shell::Pwsh => Plan::Block {
                path: if self.windows {
                    self.home
                        .join("Documents/PowerShell/Microsoft.PowerShell_profile.ps1")
                } else {
                    self.xdg_config
                        .join("powershell/Microsoft.PowerShell_profile.ps1")
                },
                line: self.shell.call(),
            },
        }
    }

    /// What the command says before it asks: the files and what goes in them.
    fn describe(&self, plan: &Plan) -> Result<Todo> {
        let old = read(plan.path())?;
        let mut out = Vec::new();
        match plan {
            Plan::Own { path, content } => {
                if old.as_deref() == Some(content.as_str()) {
                    return Ok(Todo::Nothing);
                }
                out.push(tf(Key::SsIntro, &[&self.shell.init_name()]));
                out.push(String::new());
                out.push(if old.is_some() {
                    tf(Key::SsReplace, &[&path.display()])
                } else {
                    tf(Key::FpCreate, &[&path.display()])
                });
                out.extend(content.lines().map(|l| format!("  + {l}")));
            }
            Plan::Block { path, line } => {
                if let Some(text) = &old {
                    if find_block(text).is_some() {
                        return Ok(Todo::Nothing);
                    }
                    if let Some(l) = text
                        .lines()
                        .find(|l| !l.trim_start().starts_with('#') && l.contains("rada --init"))
                    {
                        return Ok(Todo::ByHand(tf(
                            Key::SsByHand,
                            &[&path.display(), &l.trim()],
                        )));
                    }
                }
                out.push(tf(Key::SsIntro, &[&self.shell.init_name()]));
                out.push(String::new());
                out.push(if old.is_some() {
                    tf(Key::SsAdd, &[&path.display()])
                } else {
                    tf(Key::FpCreate, &[&path.display()])
                });
                out.extend(block(line).lines().map(|l| format!("  + {l}")));
            }
        }
        Ok(Todo::Write(out))
    }

    /// Write what was planned.
    fn install(&self, plan: &Plan) -> Result<()> {
        match plan {
            Plan::Own { path, content } => write(path, content),
            Plan::Block { path, line } => {
                let mut text = read(path)?.unwrap_or_default();
                if !text.is_empty() {
                    if !text.ends_with('\n') {
                        text.push('\n');
                    }
                    // A blank line apart from what was there; `remove` takes it away too.
                    text.push('\n');
                }
                text.push_str(&block(line));
                write(path, &text)
            }
        }
    }

    /// Take away what `install` wrote and nothing else.
    pub fn remove(&self) -> Result<Vec<String>> {
        let plan = self.plan();
        let path = plan.path().to_path_buf();
        let Some(old) = read(&path)? else {
            return Ok(vec![Key::SsNothingToRemove.tr().to_string()]);
        };
        match plan {
            Plan::Own { content, .. } => {
                if old != content {
                    return Ok(vec![tf(Key::FpChangedSince, &[&path.display()])]);
                }
                std::fs::remove_file(&path).with_context(|| path.display().to_string())?;
                Ok(vec![tf(Key::SsRemovedFile, &[&path.display()])])
            }
            Plan::Block { .. } => {
                let Some(new) = without_block(&old) else {
                    return Ok(vec![Key::SsNothingToRemove.tr().to_string()]);
                };
                write(&path, &new)?;
                Ok(vec![tf(Key::SsRemovedBlock, &[&path.display()])])
            }
        }
    }
}

/// The block as it is written: the markers around the call.
fn block(line: &str) -> String {
    format!("{START}\n{line}\n{END}\n")
}

/// Lines (from, to included) of a block in `text`.
fn find_block(text: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.iter().position(|l| l.trim_end() == START)?;
    let to = from + lines[from..].iter().position(|l| l.trim_end() == END)?;
    Some((from, to))
}

/// `text` without its block, and without the blank line that was put before it.
fn without_block(text: &str) -> Option<String> {
    let (from, to) = find_block(text)?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut from = from;
    if from > 0 && lines[from - 1].trim().is_empty() {
        from -= 1;
    }
    Some(
        lines[..from]
            .iter()
            .chain(&lines[to + 1..])
            .copied()
            .collect(),
    )
}

fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| path.display().to_string()),
    }
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    // Written beside the file and moved over it: a shell that starts meanwhile reads the old
    // file or the new one, never half of one.
    let tmp = path.with_extension("rada-tmp");
    std::fs::write(&tmp, text).with_context(|| tmp.display().to_string())?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).with_context(|| path.display().to_string())
}

/// The command: show, ask, write; or with `remove`, take away.
pub fn run(
    dirs: &Dirs,
    shell: Option<&str>,
    remove: bool,
    yes: bool,
    input: &mut dyn BufRead,
) -> Result<Vec<String>> {
    let setup = ShellSetup::from_env(dirs, shell)?;
    run_with(&setup, remove, yes, input)
}

fn run_with(
    setup: &ShellSetup,
    remove: bool,
    yes: bool,
    input: &mut dyn BufRead,
) -> Result<Vec<String>> {
    if remove {
        return setup.remove();
    }
    let plan = setup.plan();
    let path = plan.path().to_path_buf();
    let lines = match setup.describe(&plan)? {
        Todo::Nothing => return Ok(vec![tf(Key::SsAlready, &[&path.display()])]),
        Todo::ByHand(said) => return Ok(vec![said]),
        Todo::Write(lines) => lines,
    };
    for l in &lines {
        println!("{l}");
    }
    println!();
    if !yes {
        print!("{}", Key::FpAsk.tr());
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut answer = String::new();
        input.read_line(&mut answer).ok();
        if !matches!(
            answer.trim().to_lowercase().as_str(),
            "y" | "yes" | "s" | "si" | "sì"
        ) {
            return Ok(vec![Key::FpAborted.tr().to_string()]);
        }
    }
    setup.install(&plan)?;
    Ok(vec![
        tf(Key::FpWrote, &[&path.display()]),
        String::new(),
        tf(Key::SsDone, &[&"v"]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(root: &Path, shell: Shell) -> ShellSetup {
        ShellSetup {
            shell,
            home: root.join("home"),
            xdg_config: root.join("home/.config"),
            nushell_data: root.join("home/.local/share/nushell"),
            zdotdir: None,
            macos: false,
            windows: false,
        }
    }

    fn go(s: &ShellSetup, remove: bool) -> Vec<String> {
        run_with(s, remove, true, &mut std::io::empty()).unwrap()
    }

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn shells_are_known_by_name_or_path() {
        assert_eq!(Shell::parse("/usr/bin/zsh"), Some(Shell::Zsh));
        assert_eq!(Shell::parse("/bin/bash"), Some(Shell::Bash));
        assert_eq!(Shell::parse("fish"), Some(Shell::Fish));
        assert_eq!(Shell::parse("/usr/bin/nu"), Some(Shell::Nushell));
        assert_eq!(Shell::parse("pwsh.exe"), Some(Shell::Pwsh));
        assert_eq!(Shell::parse("tcsh"), None);
        assert_eq!(Shell::parse(""), None);
    }

    #[test]
    fn fish_gets_a_file_of_its_own_and_config_fish_is_not_touched() {
        let d = tmp();
        let s = setup(d.path(), Shell::Fish);
        let config = d.path().join("home/.config/fish/config.fish");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "set -x A 1\n").unwrap();
        go(&s, false);
        let file = d.path().join("home/.config/fish/conf.d/rada.fish");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\nrada --init fish | source\n"), "{text}");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "set -x A 1\n");
        // Again: nothing changes, and it says so.
        let again = go(&s, false);
        assert!(again[0].contains("rada.fish"), "{again:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        // Removing takes the file away and only that.
        go(&s, true);
        assert!(!file.exists());
        assert!(config.exists());
        assert_eq!(go(&s, true).len(), 1, "nothing to remove the second time");
    }

    #[test]
    fn bash_gets_one_block_once_and_remove_leaves_the_rest_as_it_was() {
        let d = tmp();
        let s = setup(d.path(), Shell::Bash);
        let rc = d.path().join("home/.bashrc");
        std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
        let before = "export A=1\n\nalias l='ls -l'\n";
        std::fs::write(&rc, before).unwrap();
        go(&s, false);
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.starts_with(before), "{text}");
        assert_eq!(
            &text[before.len()..],
            "\n# >>> rada >>>\neval \"$(rada --init bash)\"\n# <<< rada <<<\n"
        );
        // A second time writes nothing, however often.
        go(&s, false);
        go(&s, false);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), text);
        assert_eq!(text.matches("rada --init").count(), 1);
        // Out again, byte for byte.
        go(&s, true);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), before);
    }

    #[test]
    fn a_file_that_does_not_end_in_a_newline_and_one_that_is_missing() {
        let d = tmp();
        let s = setup(d.path(), Shell::Bash);
        let rc = d.path().join("home/.bashrc");
        // Missing: made, with the block only.
        go(&s, false);
        assert_eq!(
            std::fs::read_to_string(&rc).unwrap(),
            "# >>> rada >>>\neval \"$(rada --init bash)\"\n# <<< rada <<<\n"
        );
        go(&s, true);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "");
        // No newline at the end: the block starts on a line of its own.
        std::fs::write(&rc, "export A=1").unwrap();
        go(&s, false);
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.starts_with("export A=1\n\n# >>> rada >>>\n"), "{text}");
        go(&s, true);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "export A=1\n");
    }

    #[test]
    fn text_after_the_block_stays_where_it_is() {
        let d = tmp();
        let s = setup(d.path(), Shell::Bash);
        let rc = d.path().join("home/.bashrc");
        std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
        std::fs::write(&rc, "a\n").unwrap();
        go(&s, false);
        let mut text = std::fs::read_to_string(&rc).unwrap();
        text.push_str("b\n# a comment of mine\n");
        std::fs::write(&rc, &text).unwrap();
        go(&s, true);
        assert_eq!(
            std::fs::read_to_string(&rc).unwrap(),
            "a\nb\n# a comment of mine\n"
        );
    }

    #[test]
    fn zsh_goes_to_zdotdir_when_it_is_set_and_to_home_when_not() {
        let d = tmp();
        let mut s = setup(d.path(), Shell::Zsh);
        go(&s, false);
        let rc = d.path().join("home/.zshrc");
        assert!(
            std::fs::read_to_string(&rc)
                .unwrap()
                .contains("rada --init zsh")
        );
        go(&s, true);

        s.zdotdir = Some(d.path().join("zd"));
        go(&s, false);
        let zrc = d.path().join("zd/.zshrc");
        assert!(
            std::fs::read_to_string(&zrc)
                .unwrap()
                .contains("rada --init zsh")
        );
        // Nothing went to the home folder this time.
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "");
        go(&s, true);
        assert_eq!(std::fs::read_to_string(&zrc).unwrap(), "");
    }

    #[test]
    fn a_call_written_by_hand_is_left_alone() {
        let d = tmp();
        let s = setup(d.path(), Shell::Bash);
        let rc = d.path().join("home/.bashrc");
        std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
        let mine = "eval \"$(rada --init bash)\"\n";
        std::fs::write(&rc, mine).unwrap();
        let said = go(&s, false);
        assert!(said[0].contains("rada --init bash"), "{said:?}");
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), mine);
        // And remove has no block of ours to take.
        go(&s, true);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), mine);
    }

    #[test]
    fn a_no_answers_nothing_is_written() {
        let d = tmp();
        let s = setup(d.path(), Shell::Bash);
        let said = run_with(&s, false, false, &mut std::io::Cursor::new(b"n\n")).unwrap();
        assert_eq!(said, [Key::FpAborted.tr().to_string()]);
        assert!(!d.path().join("home/.bashrc").exists());
        run_with(&s, false, false, &mut std::io::Cursor::new(b"y\n")).unwrap();
        assert!(d.path().join("home/.bashrc").exists());
    }

    #[test]
    fn nushell_and_powershell_have_their_own_places() {
        let d = tmp();
        let n = setup(d.path(), Shell::Nushell);
        go(&n, false);
        let file = d
            .path()
            .join("home/.local/share/nushell/vendor/autoload/rada.nu");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("def --env v"), "{text}");
        go(&n, true);
        assert!(!file.exists());

        let p = setup(d.path(), Shell::Pwsh);
        go(&p, false);
        let profile = d
            .path()
            .join("home/.config/powershell/Microsoft.PowerShell_profile.ps1");
        assert!(
            std::fs::read_to_string(&profile)
                .unwrap()
                .contains("rada --init powershell | Out-String | Invoke-Expression")
        );
        let mut w = setup(d.path(), Shell::Pwsh);
        w.windows = true;
        assert!(
            w.plan()
                .path()
                .ends_with("Documents/PowerShell/Microsoft.PowerShell_profile.ps1")
        );
    }

    #[test]
    fn a_file_of_ours_that_was_changed_since_is_not_removed() {
        let d = tmp();
        let s = setup(d.path(), Shell::Fish);
        go(&s, false);
        let file = d.path().join("home/.config/fish/conf.d/rada.fish");
        std::fs::write(&file, "my own things\n").unwrap();
        let said = go(&s, true);
        assert!(said[0].contains("rada.fish"), "{said:?}");
        assert!(file.exists());
    }

    #[test]
    fn the_function_comes_from_init_not_from_a_copy() {
        assert!(crate::shell::snippet("nushell").contains("def --env v"));
        let call = Shell::Zsh.call();
        assert_eq!(call, "eval \"$(rada --init zsh)\"");
    }
}
