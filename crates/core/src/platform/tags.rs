//! Tags on files, the way each system keeps them.
//!
//! * **macOS**: the Finder's tags, in the extended attribute `com.apple.metadata:_kMDItemUserTags`,
//!   a binary property list holding strings of the form `Name` or `Name\n<colour number>`. The
//!   seven colours of the Finder are numbered 1 (gray) to 7 (orange).
//! * **Linux**: `user.xdg.tags`, the freedesktop convention that Dolphin and Baloo use: tag
//!   names in UTF-8, separated by commas. It has no colours; a tag called "Red" is drawn red.
//! * **Windows**: not supported; the interface offers nothing there.
//!
//! A change of tags is an operation like any other (planned, journaled, undoable): this module
//! only reads and writes them.

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::Error;
use crate::Result;

/// The Finder's colours, as the menu lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TagColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Gray,
}

impl TagColor {
    /// In the order of the Finder's menu.
    pub const ALL: [TagColor; 7] = [
        TagColor::Red,
        TagColor::Orange,
        TagColor::Yellow,
        TagColor::Green,
        TagColor::Blue,
        TagColor::Purple,
        TagColor::Gray,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TagColor::Red => "Red",
            TagColor::Orange => "Orange",
            TagColor::Yellow => "Yellow",
            TagColor::Green => "Green",
            TagColor::Blue => "Blue",
            TagColor::Purple => "Purple",
            TagColor::Gray => "Gray",
        }
    }

    /// The number the Finder writes after the name: 1 gray, 2 green, 3 purple, 4 blue,
    /// 5 yellow, 6 red, 7 orange.
    pub fn finder_index(self) -> u8 {
        match self {
            TagColor::Gray => 1,
            TagColor::Green => 2,
            TagColor::Purple => 3,
            TagColor::Blue => 4,
            TagColor::Yellow => 5,
            TagColor::Red => 6,
            TagColor::Orange => 7,
        }
    }

    pub fn from_finder_index(i: u8) -> Option<TagColor> {
        TagColor::ALL.into_iter().find(|c| c.finder_index() == i)
    }

    /// A colour from its English name, in any case ("red", "Gray"; "grey" too).
    pub fn from_name(name: &str) -> Option<TagColor> {
        let n = name.trim().to_lowercase();
        if n == "grey" {
            return Some(TagColor::Gray);
        }
        TagColor::ALL
            .into_iter()
            .find(|c| c.name().to_lowercase() == n)
    }
}

/// One tag: a name, and the colour it is drawn in when it has one.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Tag {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TagColor>,
}

impl Tag {
    /// One of the seven colours, called what the Finder calls it.
    pub fn standard(color: TagColor) -> Tag {
        Tag {
            name: color.name().to_string(),
            color: Some(color),
        }
    }

    /// A tag by name; a name that is a colour is that colour.
    pub fn named(name: &str) -> Tag {
        Tag {
            name: name.to_string(),
            color: TagColor::from_name(name),
        }
    }

    /// Whether this is the tag called `name` (names are compared without regard to case).
    pub fn is_called(&self, name: &str) -> bool {
        self.name.to_lowercase() == name.trim().to_lowercase()
    }
}

/// A name that can be a tag: not empty, one line, no comma (the Linux convention separates
/// with it), not too long. Returns it trimmed.
pub fn validate_name(name: &str) -> std::result::Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("a tag needs a name".into());
    }
    if n.chars().count() > 100 {
        return Err("a tag name can have at most 100 characters".into());
    }
    if n.chars()
        .any(|c| c == '\n' || c == '\r' || c == ',' || c == '\0')
    {
        return Err("a tag name cannot have a comma or a line break".into());
    }
    Ok(n.to_string())
}

/// Add `tag` to `tags` unless one of that name is there.
pub fn with_tag(tags: &[Tag], tag: &Tag) -> Vec<Tag> {
    let mut out = tags.to_vec();
    if !out.iter().any(|t| t.is_called(&tag.name)) {
        out.push(tag.clone());
    }
    out
}

/// `tags` without the one called `name`.
pub fn without_tag(tags: &[Tag], name: &str) -> Vec<Tag> {
    tags.iter()
        .filter(|t| !t.is_called(name))
        .cloned()
        .collect()
}

// ----------------------------------------------------------------------------- the formats

/// The Finder's tags as the value of `com.apple.metadata:_kMDItemUserTags`.
pub fn encode_finder(tags: &[Tag]) -> Vec<u8> {
    let items: Vec<String> = tags
        .iter()
        .map(|t| match t.color {
            Some(c) => format!("{}\n{}", t.name, c.finder_index()),
            None => t.name.clone(),
        })
        .collect();
    crate::platform::bplist::encode_string_array(&items)
}

/// The tags in the value of `com.apple.metadata:_kMDItemUserTags`; `None` when it is not a
/// list of strings.
pub fn decode_finder(bytes: &[u8]) -> Option<Vec<Tag>> {
    let items = crate::platform::bplist::decode_string_array(bytes)?;
    Some(
        items
            .into_iter()
            .map(|s| {
                let (name, index) = match s.rsplit_once('\n') {
                    Some((n, i)) => (n.to_string(), i.trim().parse::<u8>().ok()),
                    None => (s, None),
                };
                let color = index
                    .and_then(TagColor::from_finder_index)
                    // A tag without a colour number but with a colour's name is that colour.
                    .or_else(|| {
                        index
                            .is_none()
                            .then(|| TagColor::from_name(&name))
                            .flatten()
                    });
                Tag { name, color }
            })
            .filter(|t| !t.name.is_empty())
            .collect(),
    )
}

/// The tags as the value of `user.xdg.tags`.
pub fn encode_xdg(tags: &[Tag]) -> Vec<u8> {
    tags.iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<_>>()
        .join(",")
        .into_bytes()
}

/// The tags in the value of `user.xdg.tags`.
pub fn decode_xdg(bytes: &[u8]) -> Vec<Tag> {
    String::from_utf8_lossy(bytes)
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(Tag::named)
        .collect()
}

// ---------------------------------------------------------------------------- the store

/// Where a system keeps tags.
pub trait TagStore: Send + Sync {
    /// The tags of `path`; none when it has none, an error when they cannot be read.
    fn read(&self, path: &Path) -> Result<Vec<Tag>>;

    /// Replace the tags of `path`. An empty list removes them.
    fn write(&self, path: &Path, tags: &[Tag]) -> Result<()>;

    /// What this system calls them, for messages ("Finder tags").
    fn what(&self) -> &'static str;
}

/// Tags kept in an extended attribute.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct XattrTags {
    format: Format,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Finder,
    Freedesktop,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl XattrTags {
    /// The Finder's tags.
    pub fn finder() -> XattrTags {
        XattrTags {
            format: Format::Finder,
        }
    }

    /// `user.xdg.tags`.
    pub fn freedesktop() -> XattrTags {
        XattrTags {
            format: Format::Freedesktop,
        }
    }

    fn attribute(&self) -> &'static str {
        match self.format {
            Format::Finder => "com.apple.metadata:_kMDItemUserTags",
            Format::Freedesktop => "user.xdg.tags",
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl TagStore for XattrTags {
    fn read(&self, path: &Path) -> Result<Vec<Tag>> {
        let Some(bytes) = xattr::get(path, self.attribute())
            .map_err(|e| Error::io("read the tags of", path, e))?
        else {
            return Ok(Vec::new());
        };
        match self.format {
            Format::Finder => decode_finder(&bytes).ok_or_else(|| {
                Error::Invalid(format!(
                    "the tags of {} are in a form rada does not read",
                    crate::display::path(path)
                ))
            }),
            Format::Freedesktop => Ok(decode_xdg(&bytes)),
        }
    }

    fn write(&self, path: &Path, tags: &[Tag]) -> Result<()> {
        let result = if tags.is_empty() {
            xattr::remove(path, self.attribute())
        } else {
            let value = match self.format {
                Format::Finder => encode_finder(tags),
                Format::Freedesktop => encode_xdg(tags),
            };
            xattr::set(path, self.attribute(), &value)
        };
        result.map_err(|e| Error::io("write the tags of", path, e))
    }

    fn what(&self) -> &'static str {
        match self.format {
            Format::Finder => "Finder tags",
            Format::Freedesktop => "tags",
        }
    }
}

/// Reading and writing one extended attribute, not following a symbolic link.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod xattr {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    fn cstrings(path: &Path, name: &str) -> io::Result<(CString, CString)> {
        let p = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        let n = CString::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains NUL"))?;
        Ok((p, n))
    }

    #[cfg(target_os = "linux")]
    fn no_attribute(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::ENODATA))
    }

    #[cfg(target_os = "macos")]
    fn no_attribute(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::ENOATTR))
    }

    /// The system does not keep attributes here (a filesystem without them).
    fn unsupported(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::ENOTSUP) | Some(libc::ENOSYS))
    }

    /// The attribute's value, `None` when the item has none (or its filesystem cannot).
    pub fn get(path: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
        let (p, n) = cstrings(path, name)?;
        loop {
            let need = raw_get(&p, &n, std::ptr::null_mut(), 0);
            if need < 0 {
                let e = io::Error::last_os_error();
                return if no_attribute(&e) || unsupported(&e) {
                    Ok(None)
                } else {
                    Err(e)
                };
            }
            let mut buf = vec![0u8; need as usize];
            let got = raw_get(&p, &n, buf.as_mut_ptr().cast(), buf.len());
            if got < 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() == Some(libc::ERANGE) {
                    continue; // it grew in between
                }
                return if no_attribute(&e) { Ok(None) } else { Err(e) };
            }
            buf.truncate(got as usize);
            return Ok(Some(buf));
        }
    }

    pub fn set(path: &Path, name: &str, value: &[u8]) -> io::Result<()> {
        let (p, n) = cstrings(path, name)?;
        // SAFETY: valid C strings and a value buffer of the length passed.
        let r = unsafe { raw_set(&p, &n, value) };
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Remove it; an item that has none is fine.
    pub fn remove(path: &Path, name: &str) -> io::Result<()> {
        let (p, n) = cstrings(path, name)?;
        let r = raw_remove(&p, &n);
        if r < 0 {
            let e = io::Error::last_os_error();
            if no_attribute(&e) {
                return Ok(());
            }
            return Err(e);
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn raw_get(p: &CString, n: &CString, buf: *mut libc::c_void, len: usize) -> isize {
        // SAFETY: a null buffer of length 0 asks for the size; otherwise `buf` is valid for `len`.
        unsafe { libc::lgetxattr(p.as_ptr(), n.as_ptr(), buf, len) }
    }

    #[cfg(target_os = "macos")]
    fn raw_get(p: &CString, n: &CString, buf: *mut libc::c_void, len: usize) -> isize {
        // SAFETY: as above; position 0, and a symbolic link is not followed.
        unsafe { libc::getxattr(p.as_ptr(), n.as_ptr(), buf, len, 0, libc::XATTR_NOFOLLOW) }
    }

    #[cfg(target_os = "linux")]
    unsafe fn raw_set(p: &CString, n: &CString, value: &[u8]) -> isize {
        // SAFETY: the caller passes valid strings; `value` is valid for its length.
        unsafe {
            libc::lsetxattr(
                p.as_ptr(),
                n.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            ) as isize
        }
    }

    #[cfg(target_os = "macos")]
    unsafe fn raw_set(p: &CString, n: &CString, value: &[u8]) -> isize {
        // SAFETY: as above; position 0, not following a link.
        unsafe {
            libc::setxattr(
                p.as_ptr(),
                n.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                libc::XATTR_NOFOLLOW,
            ) as isize
        }
    }

    #[cfg(target_os = "linux")]
    fn raw_remove(p: &CString, n: &CString) -> isize {
        // SAFETY: valid C strings.
        unsafe { libc::lremovexattr(p.as_ptr(), n.as_ptr()) as isize }
    }

    #[cfg(target_os = "macos")]
    fn raw_remove(p: &CString, n: &CString) -> isize {
        // SAFETY: valid C strings; not following a link.
        unsafe { libc::removexattr(p.as_ptr(), n.as_ptr(), libc::XATTR_NOFOLLOW) as isize }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, color: Option<TagColor>) -> Tag {
        Tag {
            name: name.into(),
            color,
        }
    }

    #[test]
    fn the_finders_colours_have_its_numbers_and_names() {
        assert_eq!(TagColor::Gray.finder_index(), 1);
        assert_eq!(TagColor::Green.finder_index(), 2);
        assert_eq!(TagColor::Purple.finder_index(), 3);
        assert_eq!(TagColor::Blue.finder_index(), 4);
        assert_eq!(TagColor::Yellow.finder_index(), 5);
        assert_eq!(TagColor::Red.finder_index(), 6);
        assert_eq!(TagColor::Orange.finder_index(), 7);
        for c in TagColor::ALL {
            assert_eq!(TagColor::from_finder_index(c.finder_index()), Some(c));
            assert_eq!(TagColor::from_name(c.name()), Some(c));
            assert_eq!(TagColor::from_name(&c.name().to_uppercase()), Some(c));
        }
        assert_eq!(TagColor::from_finder_index(0), None);
        assert_eq!(TagColor::from_finder_index(8), None);
        assert_eq!(TagColor::from_name("grey"), Some(TagColor::Gray));
        assert_eq!(TagColor::from_name("work"), None);
        assert_eq!(TagColor::ALL[0], TagColor::Red);
    }

    #[test]
    fn a_tag_called_after_a_colour_is_drawn_in_it() {
        assert_eq!(Tag::named("red").color, Some(TagColor::Red));
        assert_eq!(Tag::named("Work").color, None);
        assert_eq!(
            Tag::standard(TagColor::Blue),
            t("Blue", Some(TagColor::Blue))
        );
        assert!(Tag::standard(TagColor::Blue).is_called("blue"));
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(validate_name("  Work  ").unwrap(), "Work");
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name("a,b").is_err());
        assert!(validate_name("a\nb").is_err());
        assert!(validate_name(&"x".repeat(101)).is_err());
        assert_eq!(validate_name("Città – 日本").unwrap(), "Città – 日本");
    }

    #[test]
    fn adding_and_removing_do_not_duplicate_or_disturb_the_others() {
        let a = vec![Tag::standard(TagColor::Red), t("Work", None)];
        assert_eq!(with_tag(&a, &Tag::named("red")), a);
        assert_eq!(with_tag(&a, &Tag::standard(TagColor::Blue)).len(), 3);
        assert_eq!(without_tag(&a, "RED"), [t("Work", None)]);
        assert_eq!(without_tag(&a, "missing"), a);
    }

    #[test]
    fn the_finders_tags_round_trip_with_their_colours() {
        let tags = vec![
            Tag::standard(TagColor::Red),
            t("Work", None),
            t("Projet été", Some(TagColor::Green)),
            t("日本語", Some(TagColor::Purple)),
        ];
        let bytes = encode_finder(&tags);
        assert!(bytes.starts_with(b"bplist00"));
        assert_eq!(decode_finder(&bytes).unwrap(), tags);
        assert_eq!(
            decode_finder(&encode_finder(&[])).unwrap(),
            Vec::<Tag>::new()
        );
    }

    #[test]
    fn what_the_finder_wrote_is_read() {
        // Names and colour numbers as the Finder stores them: "Red\n6", a plain "Work", a tag
        // of the user's own with a colour, and a localized colour name.
        let bytes = crate::platform::bplist::encode_string_array(&[
            "Red\n6".into(),
            "Work".into(),
            "Clients\n4".into(),
            "Rosso\n6".into(),
            "Gray\n1".into(),
        ]);
        let tags = decode_finder(&bytes).unwrap();
        assert_eq!(
            tags,
            [
                t("Red", Some(TagColor::Red)),
                t("Work", None),
                t("Clients", Some(TagColor::Blue)),
                t("Rosso", Some(TagColor::Red)),
                t("Gray", Some(TagColor::Gray)),
            ]
        );
        // Not a property list of strings: not guessed at.
        assert!(decode_finder(b"nonsense").is_none());
    }

    #[test]
    fn freedesktop_tags_are_names_with_commas_between() {
        let tags = vec![Tag::standard(TagColor::Red), t("Work", None)];
        assert_eq!(encode_xdg(&tags), b"Red,Work");
        assert_eq!(
            decode_xdg(" red , Work,,  ,Ünï ".as_bytes()),
            [Tag::named("red"), t("Work", None), t("Ünï", None)]
        );
        assert!(decode_xdg(b"").is_empty());
        assert_eq!(decode_xdg(&encode_xdg(&tags)), tags);
    }

    #[cfg(target_os = "linux")]
    mod on_disk {
        use super::*;

        fn store() -> (tempfile::TempDir, XattrTags, std::path::PathBuf) {
            // The target folder, not /tmp: some tmpfs mounts refuse user attributes.
            let d = tempfile::Builder::new()
                .prefix("tags-")
                .tempdir_in(std::env::current_dir().unwrap())
                .unwrap();
            let f = d.path().join("a.txt");
            std::fs::write(&f, "x").unwrap();
            (d, XattrTags::freedesktop(), f)
        }

        fn supported(f: &Path) -> bool {
            xattr::set(f, "user.rada-probe", b"1").is_ok()
        }

        #[test]
        fn tags_are_written_read_and_removed_through_the_attribute() {
            let (_d, s, f) = store();
            if !supported(&f) {
                eprintln!("this filesystem keeps no user attributes: test skipped");
                return;
            }
            assert!(s.read(&f).unwrap().is_empty());
            let tags = vec![Tag::standard(TagColor::Red), t("Work", None)];
            s.write(&f, &tags).unwrap();
            assert_eq!(s.read(&f).unwrap(), tags);
            // The attribute is the freedesktop one other programs read.
            assert_eq!(
                xattr::get(&f, "user.xdg.tags").unwrap().unwrap(),
                b"Red,Work"
            );
            s.write(&f, &[]).unwrap();
            assert!(s.read(&f).unwrap().is_empty());
            assert!(xattr::get(&f, "user.xdg.tags").unwrap().is_none());
            // Removing what is not there is fine.
            s.write(&f, &[]).unwrap();
        }

        #[test]
        fn the_finder_format_works_through_the_same_calls() {
            let (_d, _s, f) = store();
            if !supported(&f) {
                return;
            }
            // The Finder's attribute name is not in the `user.` namespace Linux allows, so
            // this is tried through a user attribute carrying the same bytes.
            let tags = vec![Tag::standard(TagColor::Green), t("Done", None)];
            xattr::set(&f, "user.rada-finder-tags", &encode_finder(&tags)).unwrap();
            let back = xattr::get(&f, "user.rada-finder-tags").unwrap().unwrap();
            assert_eq!(decode_finder(&back).unwrap(), tags);
        }

        #[test]
        fn an_item_that_is_gone_is_an_error_with_its_path() {
            let (d, s, _f) = store();
            let gone = d.path().join("gone.txt");
            let e = s.read(&gone).unwrap_err().to_string();
            assert!(e.contains("gone.txt"), "{e}");
        }
    }

    #[cfg(target_os = "macos")]
    mod on_a_mac {
        use super::*;
        use crate::platform::Fidelity;

        fn file() -> (tempfile::TempDir, std::path::PathBuf) {
            let d = tempfile::Builder::new()
                .prefix("tags-")
                .tempdir_in(std::env::current_dir().unwrap())
                .unwrap();
            let f = d.path().join("a.txt");
            std::fs::write(&f, "x").unwrap();
            (d, f)
        }

        #[test]
        fn the_finders_tags_are_written_and_read_in_the_attribute_the_finder_uses() {
            let (_d, f) = file();
            let s = XattrTags::finder();
            assert!(s.read(&f).unwrap().is_empty());
            let tags = vec![Tag::standard(TagColor::Red), t("Work", None)];
            s.write(&f, &tags).unwrap();
            assert_eq!(s.read(&f).unwrap(), tags);
            let raw = xattr::get(&f, "com.apple.metadata:_kMDItemUserTags")
                .unwrap()
                .unwrap();
            assert!(raw.starts_with(b"bplist00"));
            // The system's own tool reads what rada wrote.
            let out = std::process::Command::new("xattr")
                .args(["-px", "com.apple.metadata:_kMDItemUserTags"])
                .arg(&f)
                .output()
                .unwrap();
            assert!(out.status.success());
            s.write(&f, &[]).unwrap();
            assert!(s.read(&f).unwrap().is_empty());
        }

        #[test]
        fn a_copy_keeps_the_tags() {
            let (d, f) = file();
            let s = XattrTags::finder();
            s.write(&f, &[Tag::standard(TagColor::Blue)]).unwrap();
            let copy = d.path().join("b.txt");
            std::fs::copy(&f, &copy).unwrap();
            let out = crate::platform::macos::MacFidelity.copy_attributes(
                &f,
                &copy,
                crate::platform::Preserve::default(),
                None,
            );
            assert!(out.not_preserved.is_empty(), "{out:?}");
            assert_eq!(s.read(&copy).unwrap(), [Tag::standard(TagColor::Blue)]);
        }
    }
}
