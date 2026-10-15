//! Finding a program by name in `PATH`: one place, one rule for everything that looks for one.
//!
//! An empty entry of `PATH` (`/usr/bin::/bin`, or a `PATH` that ends in `:`) is skipped.
//! `std::env::split_paths` turns it into the current folder, and a program of that name in
//! whatever folder rada happens to be in would be started.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The suffixes a program can have here: Windows runs `name.exe` when asked for `name`.
const SUFFIXES: &[&str] = if cfg!(windows) {
    &["", ".exe", ".cmd", ".bat"]
} else {
    &[""]
};

/// Whether `p` is a file (a link to one counts) that can be started.
pub fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// The folders of a `PATH`-style list, in order, without the empty entries.
pub fn path_dirs(path_var: &OsStr) -> impl Iterator<Item = PathBuf> {
    std::env::split_paths(path_var).filter(|d| !d.as_os_str().is_empty())
}

/// The first program called `name` in these folders, in their order. Empty folders are
/// skipped like the empty entries of a `PATH`.
pub fn which_in_dirs<I>(dirs: I, name: &str) -> Option<PathBuf>
where
    I: IntoIterator,
    I::Item: AsRef<Path>,
{
    dirs.into_iter()
        .filter(|d| !d.as_ref().as_os_str().is_empty())
        .find_map(|d| {
            SUFFIXES
                .iter()
                .map(|s| d.as_ref().join(format!("{name}{s}")))
                .find(|p| is_executable(p))
        })
}

/// The first program called `name` in a `PATH`-style list.
pub fn which_in(path_var: &OsStr, name: &str) -> Option<PathBuf> {
    which_in_dirs(path_dirs(path_var), name)
}

/// The program called `name` in the process's own `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    which_in(&std::env::var_os("PATH")?, name)
}

/// Whether a program called `name` is in the process's own `PATH`.
pub fn on_path(name: &str) -> bool {
    which(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn program(dir: &Path, name: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[cfg(unix)]
    #[test]
    fn the_first_runnable_program_in_path_order_is_found() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        program(&a, "tool", 0o644); // there, but not runnable
        let want = program(&b, "tool", 0o755);
        let path = std::env::join_paths([&a, &b]).unwrap();
        assert_eq!(which_in(&path, "tool"), Some(want));
        assert_eq!(which_in(&path, "other"), None);
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_entry_of_path_is_not_the_current_folder() {
        let t = tempfile::tempdir().unwrap();
        let bin = t.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        // A program in the folder rada is started from; `std` would find it through ``.
        let here = std::env::current_dir().unwrap();
        let name = format!("rada-which-test-{}", std::process::id());
        let planted = program(&here, &name, 0o755);
        let found = {
            let path = std::ffi::OsString::from(format!("{}::", bin.display()));
            let r = which_in(&path, &name);
            // The empty entry in front, as well.
            let lead = std::ffi::OsString::from(format!(":{}", bin.display()));
            (r, which_in(&lead, &name))
        };
        std::fs::remove_file(&planted).unwrap();
        assert_eq!(found, (None, None));
        assert_eq!(path_dirs(OsStr::new(":/a::/b:")).count(), 2);
    }

    #[test]
    fn a_name_that_is_not_there_is_not_found() {
        assert_eq!(which_in(OsStr::new(""), "ls"), None);
        assert_eq!(which_in_dirs([PathBuf::new()], "ls"), None);
    }
}
