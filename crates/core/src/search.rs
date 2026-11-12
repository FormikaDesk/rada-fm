//! Search by name in a folder and the folders under it, as a list like any other ("Search: foo").
//!
//! The view is named by a path that is not on any disk, `find:` followed by what is looked for
//! and the folder, so that a tab can hold it, go back from it and come back to it.
//!
//! What is looked for is read in one of three ways. Words are found anywhere in a name, in any
//! order, whatever their case. A text with `*` or `?` is a pattern for the whole name
//! (`*.pdf`). A text between slashes is a regular expression (`/^img_[0-9]+/`), also without
//! regard to case unless it says otherwise.
//!
//! The walk keeps its own list of the folders still to read, so a deep tree cannot exhaust
//! the stack. It never follows a link, never looks into an archive, never leaves the
//! filesystem it started on, and leaves hidden items alone unless they are shown. It is
//! bounded in entries looked at, in time, in matches and in depth; every 256 entries it asks
//! whether to stop. A folder that cannot be read is a problem to tell, not the end of the
//! search. When a bound is reached, the list says which.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::fs::{DirItem, FsEngine, FsMeta};
use crate::model::Entry;
use crate::views::{Limit, Problem, ViewCx, ViewEnd, ViewProgress, ViewSink, ViewSource};

/// What a view path starts with.
pub const SCHEME: &str = "find:";

/// Between the words and the folder: no one types it.
const SEP: char = '\u{1}';

/// How far a search goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchLimits {
    /// Most entries looked at, matching or not.
    pub visited: usize,
    /// Most matches listed.
    pub results: usize,
    pub time: Duration,
    /// How many folders below the first one are entered.
    pub depth: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        SearchLimits {
            visited: 2_000_000,
            results: 50_000,
            time: Duration::from_secs(60),
            depth: 64,
        }
    }
}

/// How many entries are looked at between two questions to the sink.
const CHECK_EVERY: usize = 256;

/// How many matches are handed over at once.
const BATCH: usize = 200;

/// The path that stands for the search of `query` under `root`. `None` when it cannot be
/// written (a folder whose name is not text, no words): such a search is not offered.
pub fn view_path(root: &Path, query: &str) -> Option<PathBuf> {
    let words = match query.trim() {
        // A regular expression is kept as it was typed: its spaces mean something.
        text if is_regex(text) => text.to_string(),
        text => text.split_whitespace().collect::<Vec<_>>().join(" "),
    };
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

/// Whether `text` is written as a regular expression: between two slashes.
pub fn is_regex(text: &str) -> bool {
    text.len() > 2 && text.starts_with('/') && text.ends_with('/')
}

/// What is looked for in a name.
#[derive(Clone, Debug)]
pub enum Query {
    /// Every word is somewhere in the name (lower case).
    Words(Vec<String>),
    /// The whole name fits the pattern (lower case), `*` for any run and `?` for one character.
    Glob(Vec<char>),
    Regex(regex::Regex),
}

impl Query {
    /// Read what the person typed (see the module documentation). An error is a regular
    /// expression that cannot be read.
    pub fn parse(text: &str) -> Result<Query, String> {
        let text = text.trim();
        if is_regex(text) {
            let pattern = &text[1..text.len() - 1];
            return regex::RegexBuilder::new(pattern)
                .case_insensitive(true)
                // A pattern is typed by hand: one that needs more than this is a mistake.
                .size_limit(1 << 20)
                .build()
                .map(Query::Regex)
                .map_err(|e| crate::display::block(&e.to_string()));
        }
        if text.contains(['*', '?']) {
            return Ok(Query::Glob(text.to_lowercase().chars().collect()));
        }
        Ok(Query::Words(
            text.split_whitespace().map(str::to_lowercase).collect(),
        ))
    }

    pub fn matches(&self, name: &str) -> bool {
        match self {
            Query::Words(words) => {
                let name = name.to_lowercase();
                words.iter().all(|w| name.contains(w.as_str()))
            }
            Query::Glob(pattern) => {
                let name: Vec<char> = name.to_lowercase().chars().collect();
                glob_matches(pattern, &name)
            }
            Query::Regex(re) => re.is_match(name),
        }
    }
}

/// Whether `name` fits `pattern` from start to end. On a mismatch after a `*` the star takes
/// one character more and the rest is tried again: no recursion, and never worse than the
/// product of the two lengths.
fn glob_matches(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(c) if *c == '?' || *c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    star = Some((sp, sn + 1));
                    p = sp + 1;
                    n = sn + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// Whether the folder at `path` is on the filesystem the search started on. A listing does not
/// say which one its items are on everywhere (not on Windows): then the folder itself is asked.
fn on_filesystem(fs: &dyn FsEngine, path: &Path, listed: Option<u64>, start: Option<u64>) -> bool {
    let dev = listed.or_else(|| fs.lstat(path).ok().and_then(|m| m.dev));
    dev == start
}

/// The search of `query` under `root`, as the source of a list.
pub struct SearchSource {
    pub root: PathBuf,
    pub query: String,
    pub limits: SearchLimits,
}

impl ViewSource for SearchSource {
    fn run(&self, cx: &ViewCx<'_>, sink: &mut dyn ViewSink) -> Result<ViewEnd, String> {
        let query = Query::parse(&self.query)?;
        let root = &self.root;
        let start = cx
            .fs
            .lstat(root)
            .map_err(|e| format!("{}: {e}", root.display()))?;
        if !start.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        let mut walk = Walk {
            cx,
            sink,
            query,
            limits: self.limits,
            dev: start.dev,
            started: Instant::now(),
            progress: ViewProgress::default(),
            ready: Vec::new(),
            end: ViewEnd::default(),
        };
        walk.run(root);
        Ok(walk.end)
    }
}

/// A search that is under way.
struct Walk<'a, 'b> {
    cx: &'a ViewCx<'b>,
    sink: &'a mut dyn ViewSink,
    query: Query,
    limits: SearchLimits,
    /// The filesystem the search started on.
    dev: Option<u64>,
    started: Instant,
    progress: ViewProgress,
    /// Matches not handed over yet.
    ready: Vec<Entry>,
    end: ViewEnd,
}

impl Walk<'_, '_> {
    fn run(&mut self, root: &Path) {
        let mut pending = vec![(root.to_path_buf(), 0usize)];
        while let Some((dir, depth)) = pending.pop() {
            let listing = match self.cx.fs.read_dir(&dir) {
                Ok(listing) => listing,
                Err(e) => {
                    let reason = crate::workers::unreadable_reason(&e);
                    self.sink.problem(Problem { path: dir, reason });
                    continue;
                }
            };
            for item in listing.items {
                if !self.look_at(item, depth, &mut pending) {
                    self.flush();
                    return;
                }
            }
        }
        self.flush();
    }

    /// One entry of a folder: listed if it matches, entered later if it is a folder. `false`
    /// when the search is over.
    fn look_at(
        &mut self,
        item: DirItem,
        depth: usize,
        pending: &mut Vec<(PathBuf, usize)>,
    ) -> bool {
        self.progress.visited += 1;
        if self.progress.visited.is_multiple_of(CHECK_EVERY) && !self.may_go_on() {
            return false;
        }
        let Ok(meta) = &item.meta else { return true };
        let attrs = self.cx.platform.attributes().attrs(&item.name, meta);
        if attrs.hidden && !self.cx.show_hidden {
            return true;
        }
        if self.is_entered(&item.path, meta) {
            if depth < self.limits.depth {
                pending.push((item.path.clone(), depth + 1));
            } else {
                self.reached(Limit::Depth(self.limits.depth));
            }
        }
        if self.query.matches(&item.name.to_string_lossy()) {
            return self.found(item);
        }
        true
    }

    /// A folder of this filesystem: a link is listed, never followed.
    fn is_entered(&self, path: &Path, meta: &FsMeta) -> bool {
        meta.is_dir() && on_filesystem(self.cx.fs, path, meta.dev, self.dev)
    }

    /// A match: its `name` is its full path (two files called `a.txt` in different folders
    /// are different rows), its `display` the name on disk. `false` at the last one allowed.
    fn found(&mut self, item: DirItem) -> bool {
        let Ok(meta) = &item.meta else { return true };
        let (fs, platform) = (self.cx.fs, self.cx.platform);
        let mut entry = Entry::from_meta(fs, platform, item.path.clone(), item.name, meta);
        entry.name = OsString::from(item.path.as_os_str());
        self.ready.push(entry);
        self.progress.found += 1;
        if self.progress.found >= self.limits.results {
            self.reached(Limit::Results(self.limits.results));
            return false;
        }
        if self.ready.len() >= BATCH {
            self.flush();
        }
        true
    }

    /// Asked every [`CHECK_EVERY`] entries: tell how far the search is, and whether it goes on.
    fn may_go_on(&mut self) -> bool {
        self.flush();
        if self.sink.cancelled() {
            self.end.cancelled = true;
        } else if self.progress.visited >= self.limits.visited {
            self.reached(Limit::Visited(self.limits.visited));
        } else if self.started.elapsed() >= self.limits.time {
            self.reached(Limit::Time(self.limits.time));
        } else {
            return true;
        }
        false
    }

    fn reached(&mut self, limit: Limit) {
        if !self.end.limits.contains(&limit) {
            self.end.limits.push(limit);
        }
    }

    /// Hand over what was found since the last time.
    fn flush(&mut self) {
        self.sink.progress(self.progress);
        if !self.ready.is_empty() {
            self.sink.batch(std::mem::take(&mut self.ready));
        }
    }
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

    fn hit(query: &str, name: &str) -> bool {
        Query::parse(query).unwrap().matches(name)
    }

    #[test]
    fn every_word_must_be_in_the_name_whatever_its_case_and_order() {
        assert!(hit("main rs", "Main.RS"));
        assert!(hit("RS main", "Main.RS"));
        assert!(hit("RS", "Main.RS"));
        assert!(!hit("main rs", "main.c"));
    }

    #[test]
    fn a_pattern_with_a_star_is_for_the_whole_name() {
        assert!(hit("*.pdf", "Report.PDF"));
        assert!(!hit("*.pdf", "report.pdf.bak"));
        assert!(hit("img_??.jpg", "IMG_07.jpg"));
        assert!(!hit("img_??.jpg", "img_123.jpg"));
        assert!(hit("a*b*c", "a--b--b--c"));
        assert!(!hit("a*b*c", "a--b--b--"));
        assert!(hit("*", ""));
    }

    #[test]
    fn a_text_between_slashes_is_a_regular_expression() {
        assert!(hit(r"/^img_\d+\.jpg$/", "IMG_123.jpg"));
        assert!(!hit(r"/^img_\d+\.jpg$/", "img_x.jpg"));
        assert!(hit("/(?-i)^README/", "README.md"));
        assert!(!hit("/(?-i)^README/", "readme.md"));
        assert!(Query::parse("/(unclosed/").is_err());
        // One slash, or two with nothing between, is not one.
        assert!(matches!(Query::parse("/").unwrap(), Query::Words(_)));
        assert!(matches!(Query::parse("//").unwrap(), Query::Words(_)));
    }

    #[test]
    fn a_regular_expression_keeps_its_spaces_in_the_view_path() {
        let p = view_path(&folder(), " /a  b/ ").unwrap();
        assert_eq!(parse(&p).unwrap().0, "/a  b/");
    }
}
