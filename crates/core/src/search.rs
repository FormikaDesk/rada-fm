//! Search by name in a folder and the folders under it, as a list like any other ("Search: foo").
//!
//! The view is named by a path that is not on any disk, `find:` followed by the words and the
//! folder, so that a tab can hold it, go back from it and come back to it. The walk is bounded:
//! it never follows a link, never leaves the filesystem it started on, stops at a number of
//! matches and a number of entries looked at, and gives way as soon as a newer request replaces
//! it. When a bound stops it, the list says so.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::fs::FsEngine;
use crate::model::Entry;
use crate::platform::Platform;

/// What a view path starts with.
pub const SCHEME: &str = "find:";

/// Between the words and the folder: no one types it.
const SEP: char = '\u{1}';

/// Most matches listed.
pub const MAX_MATCHES: usize = 2000;

/// Most entries looked at, matching or not.
pub const MAX_VISITED: usize = 300_000;

/// The path that stands for the search of `query` under `root`. `None` when it cannot be
/// written (a folder whose name is not text, no words): such a search is not offered.
pub fn view_path(root: &Path, query: &str) -> Option<PathBuf> {
    let words = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let root = root.to_str()?;
    if words.is_empty() || words.contains(SEP) || root.contains(SEP) {
        return None;
    }
    Some(PathBuf::from(format!("{SCHEME}{words}{SEP}{root}")))
}

/// The words and the folder when `path` is a search view path.
pub fn parse(path: &Path) -> Option<(String, PathBuf)> {
    if path.is_absolute() {
        return None;
    }
    let rest = path.to_str()?.strip_prefix(SCHEME)?;
    let (words, root) = rest.split_once(SEP)?;
    let root = PathBuf::from(root);
    (!words.is_empty() && root.is_absolute()).then(|| (words.to_string(), root))
}

/// What a search found.
pub struct Found {
    pub entries: Vec<Entry>,
    /// A bound stopped the walk: there may be more.
    pub truncated: bool,
}

/// Whether every one of `words` (lower case) is in `name`.
fn matches(name: &str, words: &[String]) -> bool {
    let name = name.to_lowercase();
    words.iter().all(|w| name.contains(w.as_str()))
}

/// Whether the folder at `path` is on the filesystem the search started on. A listing does not
/// say which one its items are on everywhere (not on Windows): then the folder itself is asked.
fn on_filesystem(fs: &dyn FsEngine, path: &Path, listed: Option<u64>, start: Option<u64>) -> bool {
    let dev = listed.or_else(|| fs.lstat(path).ok().and_then(|m| m.dev));
    dev == start
}

/// The entries under `root` whose name has all the words, in the order they were found. Each
/// entry's `name` is its full path (two files called `a.txt` in different folders are different
/// rows), its `display` the name on disk.
pub fn entries(
    fs: &dyn FsEngine,
    platform: &dyn Platform,
    root: &Path,
    query: &str,
    cancel: &dyn Fn() -> bool,
) -> Result<Found, String> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let start = fs
        .lstat(root)
        .map_err(|e| format!("{}: {e}", root.display()))?;
    if !start.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    let mut found = Found {
        entries: Vec::new(),
        truncated: false,
    };
    let mut visited = 0usize;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        if cancel() {
            return Ok(found);
        }
        // A folder that cannot be read is skipped: the search is of what can be seen.
        let Ok(listing) = fs.read_dir(&dir) else {
            continue;
        };
        for item in listing.items {
            visited += 1;
            let Ok(meta) = item.meta else { continue };
            if meta.is_dir() && on_filesystem(fs, &item.path, meta.dev, start.dev) {
                pending.push(item.path.clone());
            }
            if matches(&item.name.to_string_lossy(), &words) {
                let mut e = Entry::from_meta(fs, platform, item.path.clone(), item.name, &meta);
                e.name = OsString::from(item.path.as_os_str());
                found.entries.push(e);
            }
            if found.entries.len() >= MAX_MATCHES || visited >= MAX_VISITED {
                found.truncated = true;
                return Ok(found);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder written the way this system writes a whole path: `/home/u` is not one on
    /// Windows, where it lacks the drive.
    fn folder() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\home\u\src"
        } else {
            "/home/u/src"
        })
    }

    #[test]
    fn a_view_path_names_the_words_and_the_folder() {
        let p = view_path(&folder(), "  main   rs ").unwrap();
        assert!(!p.is_absolute());
        assert_eq!(parse(&p), Some(("main rs".to_string(), folder())));
    }

    #[test]
    fn what_cannot_be_written_is_not_a_view() {
        assert!(view_path(Path::new("/home/u"), "   ").is_none());
        assert!(view_path(Path::new("/home/u"), "a\u{1}b").is_none());
        assert!(view_path(Path::new("/home/\u{1}u"), "a").is_none());
    }

    #[test]
    fn other_paths_are_not_views() {
        for p in [
            "/home/u",
            "tag:Blu",
            "find:",
            "find:a",
            "find:a\u{1}rel/dir",
            "find:\u{1}/x",
        ] {
            assert_eq!(parse(Path::new(p)), None, "{p}");
        }
        assert_eq!(parse(Path::new("/home/u/find:a\u{1}/x")), None);
    }

    #[test]
    fn every_word_must_be_in_the_name_whatever_its_case() {
        let w = |s: &str| {
            s.split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        };
        assert!(matches("Main.RS", &w("main rs")));
        assert!(matches("Main.RS", &w("RS")));
        assert!(!matches("main.c", &w("main rs")));
    }
}
