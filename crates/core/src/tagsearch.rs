//! Everything that carries a tag, as a folder-like list ("Tag: Blu").
//!
//! The view is named by a path that is not on any disk, `tag:Blu`; the directory worker knows
//! it, asks the system's index for the items (Spotlight on macOS, Baloo on Linux) and, where
//! the index has nothing to say, walks the home folder reading the tag of every item. The
//! result is a list of entries like any other: copy, move, rename, trash, tags and properties
//! work on it; creating something *in* it does not.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::fs::FsEngine;
use crate::model::{self, Entry};
use crate::platform::spawn::spawn_retrying_busy;
use crate::platform::{Platform, Tag, TagColor, TagStore};

/// What a view path starts with.
pub const SCHEME: &str = "tag:";

/// The path that stands for the view of the tag called `name`.
pub fn view_path(name: &str) -> PathBuf {
    PathBuf::from(format!("{SCHEME}{name}"))
}

/// The tag name when `path` is a view path.
pub fn view_name(path: &Path) -> Option<String> {
    if path.is_absolute() {
        return None;
    }
    let name = path.to_str()?.strip_prefix(SCHEME)?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Which items a view lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagQuery {
    pub name: String,
    /// Set when `name` is one of the seven colours, in any language of the table.
    pub color: Option<TagColor>,
}

impl TagQuery {
    pub fn new(name: &str) -> TagQuery {
        TagQuery {
            name: name.trim().to_string(),
            color: TagColor::from_name(name),
        }
    }

    /// Whether items with `tags` belong to the view: they carry a tag of that name, or (for
    /// a colour) a tag that is that colour under any of its names. A tag of the user's own
    /// that happens to be blue is a different tag from "Blu".
    pub fn matches(&self, tags: &[Tag]) -> bool {
        tags.iter().any(|t| {
            t.is_called(&self.name)
                || self
                    .color
                    .is_some_and(|c| t.color == Some(c) && TagColor::from_name(&t.name).is_some())
        })
    }
}

/// How far a walk of the disk goes before it gives up.
#[derive(Clone, Copy, Debug)]
pub struct ScanLimits {
    pub entries: usize,
    pub time: Duration,
}

impl Default for ScanLimits {
    fn default() -> Self {
        ScanLimits {
            entries: 500_000,
            time: Duration::from_secs(30),
        }
    }
}

/// Walk `root` (not into hidden folders, not through links) and collect the items whose
/// tags match. Stops when `cancel` says so, or at the limits, with what it has.
pub fn scan<S: TagStore + ?Sized>(
    store: &S,
    root: &Path,
    query: &TagQuery,
    cancel: &dyn Fn() -> bool,
    limits: ScanLimits,
) -> Vec<PathBuf> {
    let started = Instant::now();
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen.is_multiple_of(256)
                && (cancel() || started.elapsed() > limits.time || seen > limits.entries)
            {
                return found;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let path = e.path();
            if store.read(&path).is_ok_and(|t| query.matches(&t)) {
                found.push(path.clone());
            }
            let hidden = e.file_name().to_string_lossy().starts_with('.');
            if ft.is_dir() && !hidden {
                stack.push(path);
            }
        }
    }
    found
}

/// Run a program and return what it printed, or `None` if it would not start, failed, took more
/// than `timeout`, or the view was left (`cancel`).
pub fn run_tool(
    program: &Path,
    args: &[String],
    timeout: Duration,
    cancel: &dyn Fn() -> bool,
) -> Option<String> {
    use std::io::Read;
    let mut child = spawn_retrying_busy(
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )
    .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = out.read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() < timeout && !cancel() => {
                std::thread::sleep(Duration::from_millis(15))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join().ok()?;
    status
        .filter(|s| s.success())
        .map(|_| String::from_utf8_lossy(&bytes).into_owned())
}

/// The paths a search tool printed, one per line; anything that is not an absolute path
/// (a timing line, a header) is left out.
pub fn paths_from_output(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .map(str::trim_end)
        .filter(|l| l.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// The paths of the items carrying the tag, from the platform's store.
pub fn find(
    platform: &dyn Platform,
    query: &TagQuery,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<PathBuf>, String> {
    let Some(store) = platform.tags() else {
        return Err(format!("this system keeps no tags ({})", platform.name()));
    };
    store
        .find(query, &platform.dirs().home, cancel)
        .map_err(|e| e.to_string())
}

/// The entries of the view. Each entry's `name` is its full path (two items called `a.txt` in
/// different folders are different rows), its `display` the name on disk.
pub fn entries(
    fs: &dyn FsEngine,
    platform: &dyn Platform,
    query: &TagQuery,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Entry>, String> {
    let paths = find(platform, query, cancel)?;
    Ok(paths
        .into_iter()
        .filter_map(|p| model::read_entry(fs, platform, &p))
        .map(|mut e| {
            e.name = e.path.as_os_str().to_os_string();
            e
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str, color: Option<TagColor>) -> Tag {
        Tag {
            name: name.into(),
            color,
        }
    }

    #[test]
    fn a_view_path_is_not_a_place_on_the_disk() {
        let p = view_path("Blu");
        assert_eq!(p, PathBuf::from("tag:Blu"));
        assert_eq!(view_name(&p).as_deref(), Some("Blu"));
        assert_eq!(view_name(Path::new("tag:")), None);
        assert_eq!(view_name(Path::new("/home/u/tag:Blu")), None);
        assert_eq!(view_name(Path::new("tags")), None);
        assert_eq!(view_name(&view_path("My tag")).as_deref(), Some("My tag"));
    }

    #[test]
    fn a_colour_view_takes_the_colour_under_any_name_but_not_a_tag_of_the_users_own() {
        let q = TagQuery::new("Blu");
        assert_eq!(q.color, Some(TagColor::Blue));
        assert!(q.matches(&[tag("Blu", Some(TagColor::Blue))]));
        assert!(q.matches(&[tag("Blue", Some(TagColor::Blue))]));
        assert!(q.matches(&[tag("blu", None)]));
        assert!(!q.matches(&[tag("Clienti", Some(TagColor::Blue))]));
        assert!(!q.matches(&[tag("Rosso", Some(TagColor::Red))]));
        let work = TagQuery::new("Work");
        assert_eq!(work.color, None);
        assert!(work.matches(&[tag("work", None)]));
        assert!(!work.matches(&[tag("Blu", Some(TagColor::Blue))]));
        assert!(!work.matches(&[]));
    }

    #[test]
    fn only_lines_that_are_paths_are_kept() {
        let out = "/home/u/a.txt\nElapsed: 3 ms\n\n/home/u/b c.txt  \n  /not/me\n";
        assert_eq!(
            paths_from_output(out),
            [
                PathBuf::from("/home/u/a.txt"),
                PathBuf::from("/home/u/b c.txt")
            ]
        );
    }
}
