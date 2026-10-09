//! What an archive says about one of its members, and the rules that make a member's name
//! safe to turn into a path.
//!
//! Archive names are data from a stranger. They may climb out of the destination
//! (`../../.bashrc`), start at the root (`/etc/cron.d/x`), carry a drive letter or a NUL, or
//! use backslashes that one platform means as separators. Every name goes through
//! [`sanitize`], which both builds the relative path that is shown and used for lookups, and
//! says what is wrong with the original so that the plan can refuse it and give the reason.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use crate::fs::FileKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EntryKind {
    #[default]
    File,
    Dir,
    Symlink,
    /// A name for a file stored earlier in the same archive.
    Hardlink,
    /// A device, a pipe, a socket: never extracted.
    Special,
}

impl EntryKind {
    pub fn file_kind(self) -> FileKind {
        match self {
            EntryKind::File | EntryKind::Hardlink => FileKind::File,
            EntryKind::Dir => FileKind::Dir,
            EntryKind::Symlink => FileKind::Symlink,
            EntryKind::Special => FileKind::Other,
        }
    }
}

/// Why a stored name cannot be extracted as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameIssue {
    /// A `..` component: the path could leave the destination (zip-slip).
    Traversal,
    /// Starts at the root or a drive: the path would ignore the destination.
    Absolute,
    /// A NUL byte, or nothing left once the name is cleaned.
    Invalid,
}

impl NameIssue {
    pub fn reason(self) -> &'static str {
        match self {
            NameIssue::Traversal => "its path climbs out of the destination with \"..\"",
            NameIssue::Absolute => "its path is absolute and would ignore the destination",
            NameIssue::Invalid => "its name is not a valid path",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// Position among the archive's members, in archive order.
    pub index: usize,
    /// The cleaned relative path: no root, no `.`, no `..`, `/` separators only between
    /// components. Empty for an entry that names the archive root.
    pub path: PathBuf,
    pub kind: EntryKind,
    /// Uncompressed size (0 for anything but a file).
    pub size: u64,
    /// Compressed size, where the format keeps one per member.
    pub compressed: Option<u64>,
    pub mode: Option<u32>,
    pub mtime: Option<SystemTime>,
    /// For a symlink or hard link: where it points, exactly as stored.
    pub link: Option<PathBuf>,
    pub encrypted: bool,
    /// What is wrong with the stored name, if anything.
    pub issue: Option<NameIssue>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    pub fn name(&self) -> OsString {
        self.path.file_name().map(|n| n.to_os_string()).unwrap_or_default()
    }
}

/// Clean a stored name. `backslash_separates` is for ZIP, whose names are written with `/`
/// but come from Windows tools that sometimes wrote `\`.
pub fn sanitize(raw: &[u8], backslash_separates: bool) -> (PathBuf, Option<NameIssue>) {
    if raw.contains(&0) {
        return (PathBuf::new(), Some(NameIssue::Invalid));
    }
    let mut issue = None;
    let mut parts: Vec<Vec<u8>> = Vec::new();
    let is_sep = |b: u8| b == b'/' || (backslash_separates && b == b'\\');
    let mut first = true;
    for comp in raw.split(|b| is_sep(*b)) {
        if first {
            first = false;
            // An empty first component means the name began with a separator.
            if comp.is_empty() && raw.len() > 1 {
                issue.get_or_insert(NameIssue::Absolute);
                continue;
            }
            // `C:` / `C:foo`: a drive on Windows, an odd but legal name elsewhere. Refused
            // everywhere so that one archive means the same thing on every system.
            if comp.len() >= 2 && comp[0].is_ascii_alphabetic() && comp[1] == b':' {
                issue.get_or_insert(NameIssue::Absolute);
            }
        }
        match comp {
            b"" | b"." => {}
            b".." => {
                issue.get_or_insert(NameIssue::Traversal);
            }
            c => parts.push(c.to_vec()),
        }
    }
    let mut path = PathBuf::new();
    for p in parts {
        path.push(bytes_to_os(&p));
    }
    if raw.is_empty() {
        issue.get_or_insert(NameIssue::Invalid);
    }
    (path, issue)
}

#[cfg(unix)]
pub fn bytes_to_os(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b.to_vec())
}

#[cfg(not(unix))]
pub fn bytes_to_os(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

#[cfg(unix)]
pub fn os_to_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
pub fn os_to_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// IBM code page 437, the encoding ZIP uses for names when the UTF-8 flag is not set.
pub fn cp437(bytes: &[u8]) -> String {
    let high: Vec<char> = CP437_HIGH.chars().collect();
    bytes
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { high[(b - 0x80) as usize] })
        .collect()
}

/// A ZIP member name as bytes of UTF-8. Names flagged as UTF-8, and old names that happen to
/// be valid UTF-8 anyway (written by tools that never set the flag), are taken as they are;
/// anything else is code page 437, which can decode every byte and so never fails.
pub fn zip_name_bytes(raw: &[u8], flagged_utf8: bool) -> Vec<u8> {
    if flagged_utf8 || std::str::from_utf8(raw).is_ok() {
        raw.to_vec()
    } else {
        cp437(raw).into_bytes()
    }
}

/// Whether a symlink stored at `entry` (relative to the archive root) with the given target
/// would lead out of the extracted tree.
pub fn link_escapes(entry: &Path, target: &Path) -> bool {
    if target.has_root() || target.components().any(|c| matches!(c, Component::Prefix(_))) {
        return true;
    }
    let mut depth: i64 = entry.components().count().saturating_sub(1) as i64;
    for c in target.components() {
        match c {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            Component::Normal(_) => depth += 1,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(raw: &str) -> (String, Option<NameIssue>) {
        let (p, i) = sanitize(raw.as_bytes(), false);
        (p.to_string_lossy().into_owned(), i)
    }

    #[test]
    fn ordinary_names_pass() {
        assert_eq!(s("a/b/c.txt"), ("a/b/c.txt".into(), None));
        assert_eq!(s("dir/"), ("dir".into(), None));
        assert_eq!(s("./x"), ("x".into(), None));
        assert_eq!(s("a//b"), ("a/b".into(), None));
        assert_eq!(s("v1.2/x.txt"), ("v1.2/x.txt".into(), None));
    }

    #[test]
    fn traversal_and_absolute_names_are_flagged() {
        assert_eq!(s("../evil").1, Some(NameIssue::Traversal));
        assert_eq!(s("a/../../evil").1, Some(NameIssue::Traversal));
        assert_eq!(s("a/..").1, Some(NameIssue::Traversal));
        assert_eq!(s("/etc/passwd").1, Some(NameIssue::Absolute));
        assert_eq!(s("C:/Windows/x").1, Some(NameIssue::Absolute));
        assert_eq!(s("C:evil").1, Some(NameIssue::Absolute));
        assert_eq!(s("a\0b").1, Some(NameIssue::Invalid));
        assert_eq!(s("").1, Some(NameIssue::Invalid));
        // The cleaned path never contains what made the name unsafe.
        assert_eq!(s("../../etc/x").0, "etc/x");
        assert_eq!(s("/etc/x").0, "etc/x");
    }

    #[test]
    fn backslashes_separate_only_where_asked() {
        let (p, i) = sanitize(b"..\\..\\evil", true);
        assert_eq!(i, Some(NameIssue::Traversal));
        assert_eq!(p, PathBuf::from("evil"));
        let (p, i) = sanitize(b"dir\\file.txt", true);
        assert_eq!((p, i), (PathBuf::from("dir/file.txt"), None));
        let (p, i) = sanitize(b"dir\\file.txt", false);
        assert_eq!((p, i), (PathBuf::from("dir\\file.txt"), None));
    }

    #[test]
    fn cp437_table_is_complete() {
        assert_eq!(CP437_HIGH.chars().count(), 128);
        assert_eq!(cp437(b"abc"), "abc");
        assert_eq!(cp437(&[0x82]), "é");
        assert_eq!(cp437(&[0x81, 0x84, 0x94]), "üäö");
        assert_eq!(cp437(&[0xE1]), "ß");
    }

    #[test]
    fn zip_names_are_utf8_when_they_are_and_cp437_when_they_are_not() {
        assert_eq!(zip_name_bytes("caffè.txt".as_bytes(), true), "caffè.txt".as_bytes());
        // Valid UTF-8 without the flag (older Linux tools) is not turned into mojibake.
        assert_eq!(zip_name_bytes("caffè.txt".as_bytes(), false), "caffè.txt".as_bytes());
        // 0x8A is "è" in CP437 and not valid UTF-8 on its own.
        assert_eq!(zip_name_bytes(b"caff\x8a.txt", false), "caffè.txt".as_bytes());
    }

    #[test]
    fn links_that_leave_the_tree() {
        let l = |e: &str, t: &str| link_escapes(Path::new(e), Path::new(t));
        assert!(l("a/link", "/etc/passwd"));
        assert!(l("link", "../x"));
        assert!(l("a/link", "../../x"));
        assert!(!l("a/link", "../x"));
        assert!(!l("a/link", "b/c"));
        assert!(!l("a/b/link", "../../a/x"));
    }
}
