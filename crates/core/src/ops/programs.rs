//! The programs an action may run: a short list, by name, each turned into the path of a file
//! when the plan is made.
//!
//! A recipe says `qpdf`, never a path and never a shell. The name is looked up here: in the
//! table below, where it depends on the system (Ghostscript is `gswin64c` on Windows), or in
//! the person's own `[recipes] programs` of the configuration. Programs that run whatever
//! text they are handed (shells, interpreters) are refused under any name, because with one of
//! those "a list of arguments, never a shell" would mean nothing.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::action::ResolvedProgram;
use crate::platform::which;

/// The system the table is asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// Why a name did not become a program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unresolved {
    /// A shell or an interpreter: never run.
    Refused(String),
    /// Not a name rada knows, nor one the configuration gives.
    Unknown(String),
    /// Known, but not installed here.
    Missing(String),
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unresolved::Refused(n) => write!(
                f,
                "{n} runs whatever text it is given: an action never starts it"
            ),
            Unresolved::Unknown(n) => write!(
                f,
                "{n} is not a program rada knows; name its file under [recipes] programs in the configuration"
            ),
            Unresolved::Missing(n) => write!(f, "{n} is not installed"),
        }
    }
}

/// Names that are never run, whatever they are called to get here.
const REFUSED: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "ash",
    "busybox",
    "cmd",
    "powershell",
    "pwsh",
    "python",
    "pypy",
    "perl",
    "ruby",
    "node",
    "deno",
    "bun",
    "php",
    "lua",
    "luajit",
    "tclsh",
    "wish",
    "osascript",
    "wscript",
    "cscript",
    "mshta",
    "env",
    "xargs",
    "sudo",
    "doas",
    "su",
    "nohup",
    "find",
    "awk",
    "gawk",
    "mawk",
    "sed",
    "make",
    "ssh",
    "eval",
    "exec",
];

/// File types that are text handed to an interpreter.
const SCRIPT_EXTENSIONS: &[&str] = &["bat", "cmd", "ps1", "vbs", "js", "sh", "py", "pl", "rb"];

/// Whether `name` (a name or the name of a file) is a shell, an interpreter or a script.
/// `python3.12` and `Python.EXE` are `python`.
pub fn is_refused(name: &str) -> bool {
    let lower = name.to_lowercase();
    let (stem, ext) = match lower.rsplit_once('.') {
        Some((stem, ext)) if ext.chars().any(|c| c.is_ascii_alphabetic()) => (stem, ext),
        _ => (lower.as_str(), ""),
    };
    if SCRIPT_EXTENSIONS.contains(&ext) {
        return true;
    }
    let stem = if matches!(ext, "exe" | "com" | "") {
        stem
    } else {
        lower.as_str()
    };
    let bare = stem.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-');
    REFUSED.contains(&bare)
}

/// The names of the file that `name` may be, in the order they are tried. `None` for a name
/// that is not in the table.
pub fn candidates(name: &str, os: Os) -> Option<Vec<&'static str>> {
    Some(match (name, os) {
        ("qpdf", _) => vec!["qpdf"],
        ("gs", Os::Windows) => vec!["gswin64c", "gswin32c"],
        ("gs", _) => vec!["gs"],
        // `convert` is ImageMagick only on Linux: on Windows it is the system's own tool that
        // converts a disk, and must never be taken for it.
        ("magick", Os::Linux) => vec!["magick", "convert"],
        ("magick", _) => vec!["magick"],
        ("sips", Os::MacOs) => vec!["sips"],
        ("sips", _) => vec![],
        ("ffmpeg", _) => vec!["ffmpeg"],
        ("ffprobe", _) => vec!["ffprobe"],
        ("soffice", _) => vec!["soffice", "libreoffice"],
        _ => return None,
    })
}

/// Where a program is installed without being on `PATH`.
fn fixed_places(name: &str, os: Os) -> Vec<PathBuf> {
    match (name, os) {
        ("soffice", Os::MacOs) => {
            vec![PathBuf::from(
                "/Applications/LibreOffice.app/Contents/MacOS/soffice",
            )]
        }
        ("soffice", Os::Windows) => std::env::var_os("ProgramFiles")
            .map(|pf| {
                PathBuf::from(pf)
                    .join("LibreOffice")
                    .join("program")
                    .join("soffice.exe")
            })
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// The programs the configuration names, over the table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Programs {
    overrides: BTreeMap<String, PathBuf>,
    /// Looked in instead of the process's `PATH`.
    path: Option<std::ffi::OsString>,
}

impl Programs {
    pub fn new(overrides: BTreeMap<String, PathBuf>) -> Programs {
        Programs {
            overrides,
            path: None,
        }
    }

    /// Look for programs in these folders instead of the process's `PATH`: for tests, which
    /// must not find what the machine has installed.
    pub fn with_path(mut self, path: impl Into<std::ffi::OsString>) -> Programs {
        self.path = Some(path.into());
        self
    }

    /// The file `name` stands for now, looked up in the process's `PATH`.
    pub fn resolve(&self, name: &str) -> Result<ResolvedProgram, Unresolved> {
        let path = match &self.path {
            Some(p) => p.clone(),
            None => std::env::var_os("PATH").unwrap_or_default(),
        };
        self.resolve_in(name, &path, Os::current())
    }

    /// The file `name` stands for, with the `PATH` and the system given.
    pub fn resolve_in(
        &self,
        name: &str,
        path_var: &OsStr,
        os: Os,
    ) -> Result<ResolvedProgram, Unresolved> {
        if is_refused(name) {
            return Err(Unresolved::Refused(name.to_string()));
        }
        let found = match self.overrides.get(name) {
            Some(file) => usable(file).then(|| file.clone()),
            None => {
                let names = candidates(name, os).ok_or_else(|| unknown(name))?;
                find(&names, path_var).or_else(|| {
                    fixed_places(name, os)
                        .into_iter()
                        .find(|p| which::is_executable(p))
                })
            }
        };
        let path = found.ok_or_else(|| Unresolved::Missing(name.to_string()))?;
        if refused_file(&path) {
            return Err(Unresolved::Refused(name.to_string()));
        }
        Ok(ResolvedProgram {
            name: name.to_string(),
            path,
        })
    }
}

fn unknown(name: &str) -> Unresolved {
    Unresolved::Unknown(name.to_string())
}

/// A file the configuration names can be run: a whole path to a program.
fn usable(file: &Path) -> bool {
    file.is_absolute() && which::is_executable(file)
}

/// The first of `names` on `path_var`, as a whole path.
fn find(names: &[&str], path_var: &OsStr) -> Option<PathBuf> {
    names
        .iter()
        .filter_map(|n| which::which_in(path_var, n))
        .find(|p| p.is_absolute())
}

/// Whether the file itself is a shell, an interpreter or a script, whatever name led to it.
pub fn refused_file(path: &Path) -> bool {
    path.file_name()
        .is_none_or(|n| is_refused(&n.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells_and_interpreters_are_refused_under_any_spelling() {
        for name in [
            "sh",
            "bash",
            "zsh",
            "fish",
            "cmd",
            "CMD.EXE",
            "powershell",
            "pwsh",
            "python",
            "python3",
            "python3.12",
            "Python.exe",
            "perl",
            "perl5.38",
            "run.bat",
            "x.ps1",
            "tool.sh",
        ] {
            assert!(is_refused(name), "{name}");
        }
        for name in [
            "qpdf",
            "gs",
            "magick",
            "ffmpeg",
            "soffice",
            "sips",
            "gswin64c.exe",
            "7z",
        ] {
            assert!(!is_refused(name), "{name}");
        }
    }

    #[test]
    fn convert_stands_for_imagemagick_on_linux_only() {
        assert_eq!(
            candidates("magick", Os::Linux).unwrap(),
            ["magick", "convert"]
        );
        assert_eq!(candidates("magick", Os::Windows).unwrap(), ["magick"]);
        assert_eq!(candidates("magick", Os::MacOs).unwrap(), ["magick"]);
    }

    #[test]
    fn each_system_has_its_own_names() {
        assert_eq!(
            candidates("gs", Os::Windows).unwrap(),
            ["gswin64c", "gswin32c"]
        );
        assert_eq!(candidates("gs", Os::Linux).unwrap(), ["gs"]);
        assert!(candidates("sips", Os::Linux).unwrap().is_empty());
        assert_eq!(candidates("sips", Os::MacOs).unwrap(), ["sips"]);
        assert_eq!(
            candidates("soffice", Os::Linux).unwrap(),
            ["soffice", "libreoffice"]
        );
        assert!(candidates("rm", Os::Linux).is_none());
    }

    #[test]
    fn a_name_that_is_not_known_or_not_installed_says_which() {
        let programs = Programs::default();
        let nowhere = OsStr::new("");
        assert_eq!(
            programs.resolve_in("rm", nowhere, Os::Linux),
            Err(Unresolved::Unknown("rm".into()))
        );
        assert_eq!(
            programs.resolve_in("qpdf", nowhere, Os::Linux),
            Err(Unresolved::Missing("qpdf".into()))
        );
        assert_eq!(
            programs.resolve_in("bash", nowhere, Os::Linux),
            Err(Unresolved::Refused("bash".into()))
        );
    }
}
