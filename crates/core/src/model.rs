//! Directory entries and their ordering.
//!
//! Sorting is a *total* order (key, then natural name order, then raw bytes), so the
//! result is deterministic: equal sizes or dates never shuffle between refreshes
//! (a known failure of other file managers). Sorting happens immediately, in memory; it never waits for I/O.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::display;
use crate::fs::{FileKind, FsEngine, FsMeta};
use crate::ops::LinkState;
use crate::platform::{Platform, ReparseKind};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: OsString,
    pub path: PathBuf,
    pub kind: FileKind,
    pub link: Option<LinkInfo>,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    /// When it was created, where the filesystem records that.
    pub created: Option<SystemTime>,
    pub mode: Option<u32>,
    pub hidden: bool,
    /// A system file (the Windows attribute).
    pub system: bool,
    pub readonly: bool,
    pub executable: bool,
    pub reparse: Option<ReparseKind>,
    /// Set when the entry itself could not be inspected.
    pub error: Option<String>,
    /// Escaped for display (control characters, bidi, invalid bytes made visible).
    pub display: String,
    /// Case-folded form used for ordering, computed once.
    pub sort_name: String,
    /// What kind of thing it is in words ("JPEG image"), worked out once from the name.
    pub type_label: Cow<'static, str>,
}

#[derive(Clone, Debug)]
pub struct LinkInfo {
    pub target: PathBuf,
    pub state: LinkState,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == FileKind::Dir || matches!(&self.link, Some(l) if l.state == LinkState::ToDir)
    }

    pub fn from_meta(
        fs: &dyn FsEngine,
        platform: &dyn Platform,
        path: PathBuf,
        name: OsString,
        meta: &FsMeta,
    ) -> Entry {
        let attrs = platform.attributes().attrs(&name, meta);
        let link = (meta.kind == FileKind::Symlink).then(|| {
            let target = fs.read_link(&path).unwrap_or_default();
            let state = match fs.stat(&path) {
                Ok(m) if m.is_dir() => LinkState::ToDir,
                Ok(_) => LinkState::ToFile,
                Err(e) if is_loop(&e) => LinkState::Circular,
                Err(_) => LinkState::Broken,
            };
            LinkInfo { target, state }
        });
        let shown = display::name(&name);
        let type_label = match &link {
            Some(l) if matches!(l.state, LinkState::Broken | LinkState::Circular) => {
                Cow::Borrowed("Broken link")
            }
            Some(l) => crate::filetype::label(
                &shown,
                FileKind::File,
                l.state == LinkState::ToDir,
                attrs.executable,
            ),
            None => crate::filetype::label(&shown, meta.kind, false, attrs.executable),
        };
        Entry {
            sort_name: shown.to_lowercase(),
            display: shown,
            name,
            path,
            kind: meta.kind,
            link,
            size: meta.size,
            mtime: meta.mtime,
            created: meta.btime,
            type_label,
            mode: meta.mode,
            hidden: attrs.hidden,
            system: attrs.system,
            readonly: attrs.readonly,
            executable: attrs.executable,
            reparse: attrs.reparse,
            error: None,
        }
    }

    /// A placeholder for an entry whose metadata could not be read.
    pub fn broken(path: PathBuf, name: OsString, error: String) -> Entry {
        let shown = display::name(&name);
        Entry {
            sort_name: shown.to_lowercase(),
            display: shown,
            name,
            path,
            kind: FileKind::Other,
            link: None,
            size: 0,
            mtime: None,
            created: None,
            type_label: Cow::Borrowed("Unreadable"),
            mode: None,
            hidden: false,
            system: false,
            readonly: false,
            executable: false,
            reparse: None,
            error: Some(error),
        }
    }
}

fn is_loop(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::ELOOP)
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
    }
}

/// Read one directory. A bad entry becomes a visible placeholder; only failing to open
/// the directory itself is an error (with the real reason, e.g. "Permission denied").
pub fn read_entries(
    fs: &dyn FsEngine,
    platform: &dyn Platform,
    dir: &Path,
) -> crate::Result<Vec<Entry>> {
    let items = fs
        .read_dir(dir)
        .map_err(|e| crate::Error::io("read folder", dir, e))?;
    Ok(items
        .into_iter()
        .map(|it| match it.meta {
            Ok(m) => Entry::from_meta(fs, platform, it.path, it.name, &m),
            Err(e) => {
                let msg = crate::Error::io("read", &it.path, e).to_string();
                Entry::broken(it.path, it.name, msg)
            }
        })
        .collect())
}

/// Re-inspect one path (after a watcher event). `None` when it no longer exists.
pub fn read_entry(fs: &dyn FsEngine, platform: &dyn Platform, path: &Path) -> Option<Entry> {
    let name = path.file_name()?.to_os_string();
    match fs.lstat(path) {
        Ok(m) => Some(Entry::from_meta(fs, platform, path.to_path_buf(), name, &m)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Entry::broken(
            path.to_path_buf(),
            name,
            crate::Error::io("read", path, e).to_string(),
        )),
    }
}

// ------------------------------------------------------------------------------ sorting

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortKey {
    Name,
    Size,
    Modified,
    /// By the kind of file ("JPEG image"), then by name.
    Type,
}

impl SortKey {
    pub fn next(self) -> SortKey {
        match self {
            SortKey::Name => SortKey::Size,
            SortKey::Size => SortKey::Modified,
            SortKey::Modified => SortKey::Type,
            SortKey::Type => SortKey::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Size => "size",
            SortKey::Modified => "date",
            SortKey::Type => "type",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub key: SortKey,
    pub reverse: bool,
    pub dirs_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        SortSpec {
            key: SortKey::Name,
            reverse: false,
            dirs_first: true,
        }
    }
}

/// Natural order: runs of digits compare by value (`file2` < `file10`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (a.char_indices().peekable(), b.char_indices().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some((ia, ca)), Some((ib, cb))) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let ra = digit_run(a, ia);
                    let rb = digit_run(b, ib);
                    let (ta, tb) = (ra.trim_start_matches('0'), rb.trim_start_matches('0'));
                    let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    // Same value: fewer leading zeros first, then continue after the run.
                    let ord = ra.len().cmp(&rb.len());
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    for _ in 0..ra.chars().count() {
                        ai.next();
                        bi.next();
                    }
                } else {
                    if ca != cb {
                        return ca.cmp(&cb);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn digit_run(s: &str, from: usize) -> &str {
    let rest = &s[from..];
    let end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    &rest[..end]
}

fn os_bytes_cmp(a: &OsString, b: &OsString) -> Ordering {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        a.as_bytes().cmp(b.as_bytes())
    }
    #[cfg(not(unix))]
    {
        a.cmp(b)
    }
}

pub fn compare(a: &Entry, b: &Entry, spec: &SortSpec) -> Ordering {
    if spec.dirs_first {
        match (a.is_dir(), b.is_dir()) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }
    let by_name = || {
        natural_cmp(&a.sort_name, &b.sort_name)
            .then_with(|| a.display.cmp(&b.display))
            .then_with(|| os_bytes_cmp(&a.name, &b.name))
    };
    let primary = match spec.key {
        SortKey::Name => by_name(),
        SortKey::Size => {
            // Folders have no size of their own: keep them in name order.
            if a.is_dir() && b.is_dir() {
                Ordering::Equal
            } else {
                a.size.cmp(&b.size)
            }
        }
        SortKey::Modified => a.mtime.cmp(&b.mtime),
        SortKey::Type => a
            .type_label
            .to_lowercase()
            .cmp(&b.type_label.to_lowercase()),
    };
    let primary = if spec.reverse {
        primary.reverse()
    } else {
        primary
    };
    primary.then_with(by_name)
}

pub fn sort_entries(entries: &mut [Entry], spec: &SortSpec) {
    entries.sort_by(|a, b| compare(a, b, spec));
}

// ------------------------------------------------------------------------------ listing

/// A sorted directory listing owned by the UI thread. Never performs I/O.
#[derive(Clone, Debug, Default)]
pub struct DirListing {
    pub path: PathBuf,
    entries: Vec<Entry>,
    spec: SortSpec,
}

/// One change to a listing. The entry is large and updates are few; boxing it would only
/// add an allocation to every watcher event.
#[allow(clippy::large_enum_variant)]
pub enum EntryUpdate {
    Upsert(Entry),
    Remove(OsString),
}

impl DirListing {
    pub fn new(path: PathBuf, mut entries: Vec<Entry>, spec: SortSpec) -> Self {
        sort_entries(&mut entries, &spec);
        DirListing {
            path,
            entries,
            spec,
        }
    }

    pub fn spec(&self) -> SortSpec {
        self.spec
    }

    pub fn set_sort(&mut self, spec: SortSpec) {
        self.spec = spec;
        sort_entries(&mut self.entries, &spec);
    }

    pub fn all(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn position(&self, name: &std::ffi::OsStr) -> Option<usize> {
        self.entries.iter().position(|e| e.name == name)
    }

    pub fn apply(&mut self, updates: Vec<EntryUpdate>) {
        for u in updates {
            match u {
                EntryUpdate::Remove(name) => {
                    if let Some(i) = self.position(&name) {
                        self.entries.remove(i);
                    }
                }
                EntryUpdate::Upsert(e) => {
                    if let Some(i) = self.position(&e.name) {
                        self.entries.remove(i);
                    }
                    let spec = self.spec;
                    let at = self
                        .entries
                        .binary_search_by(|x| compare(x, &e, &spec))
                        .unwrap_or_else(|i| i);
                    self.entries.insert(at, e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn e(name: &str, kind: FileKind, size: u64, secs: u64) -> Entry {
        let mut x = Entry::broken(
            PathBuf::from("/d").join(name),
            OsString::from(name),
            String::new(),
        );
        x.error = None;
        x.kind = kind;
        x.size = size;
        x.mtime = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs));
        x
    }

    fn names(v: &[Entry]) -> Vec<String> {
        v.iter().map(|e| e.display.clone()).collect()
    }

    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Greater);
        assert_eq!(natural_cmp("img9.png", "img10.png"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "abd"), Ordering::Less);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
    }

    #[test]
    fn dirs_first_then_name_case_insensitive() {
        let mut v = vec![
            e("b.txt", FileKind::File, 1, 1),
            e("Zeta", FileKind::Dir, 0, 1),
            e("a.txt", FileKind::File, 1, 1),
            e("alpha", FileKind::Dir, 0, 1),
            e("B2.txt", FileKind::File, 1, 1),
        ];
        sort_entries(&mut v, &SortSpec::default());
        assert_eq!(names(&v), ["alpha", "Zeta", "a.txt", "b.txt", "B2.txt"]);
    }

    #[test]
    fn ties_are_broken_deterministically_whatever_the_input_order() {
        // Regression: equal sizes used to be shuffled between refreshes.
        let base: Vec<Entry> = (0..50)
            .map(|i| e(&format!("f{i:02}"), FileKind::File, 7, 100))
            .collect();
        for key in [SortKey::Size, SortKey::Modified] {
            let spec = SortSpec {
                key,
                reverse: false,
                dirs_first: true,
            };
            let mut expected = base.clone();
            sort_entries(&mut expected, &spec);
            for rot in 1..50 {
                let mut shuffled = base.clone();
                shuffled.rotate_left(rot);
                shuffled.reverse();
                sort_entries(&mut shuffled, &spec);
                assert_eq!(names(&shuffled), names(&expected));
            }
        }
    }

    #[test]
    fn size_and_date_sorting_with_reverse() {
        let mut v = vec![
            e("small", FileKind::File, 1, 30),
            e("big", FileKind::File, 900, 10),
            e("mid", FileKind::File, 50, 20),
        ];
        let by = |key, reverse| SortSpec {
            key,
            reverse,
            dirs_first: true,
        };
        sort_entries(&mut v, &by(SortKey::Size, false));
        assert_eq!(names(&v), ["small", "mid", "big"]);
        sort_entries(&mut v, &by(SortKey::Size, true));
        assert_eq!(names(&v), ["big", "mid", "small"]);
        sort_entries(&mut v, &by(SortKey::Modified, false));
        assert_eq!(names(&v), ["big", "mid", "small"]);
    }

    #[test]
    fn listing_updates_keep_order() {
        let mut l = DirListing::new(
            PathBuf::from("/d"),
            vec![e("a", FileKind::File, 1, 1), e("c", FileKind::File, 1, 1)],
            SortSpec::default(),
        );
        l.apply(vec![EntryUpdate::Upsert(e("b", FileKind::File, 1, 1))]);
        assert_eq!(names(l.all()), ["a", "b", "c"]);
        l.apply(vec![
            EntryUpdate::Remove(OsString::from("a")),
            EntryUpdate::Upsert(e("c", FileKind::File, 99, 1)),
        ]);
        assert_eq!(names(l.all()), ["b", "c"]);
        assert_eq!(l.all()[1].size, 99);
        assert!(l.position(OsStr::new("zzz")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_names_sort_and_display_without_panicking() {
        use std::os::unix::ffi::OsStringExt;
        let mut v = vec![
            Entry::broken(
                PathBuf::from("/d/a"),
                OsString::from_vec(vec![b'a', 0xff]),
                String::new(),
            ),
            Entry::broken(
                PathBuf::from("/d/b"),
                OsString::from_vec(vec![b'a', 0xfe]),
                String::new(),
            ),
            Entry::broken(PathBuf::from("/d/c"), OsString::from("a"), String::new()),
        ];
        sort_entries(&mut v, &SortSpec::default());
        assert_eq!(names(&v), ["a", "a\\xfe", "a\\xff"]);
        assert_eq!(v[0].display, "a");
    }
}
