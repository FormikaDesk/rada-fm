//! Tags on files, the way each system keeps them.
//!
//! * **macOS**: the Finder's tags, in the extended attribute `com.apple.metadata:_kMDItemUserTags`,
//!   a binary property list holding strings of the form `Name` or `Name\n<colour number>`. The
//!   seven colours of the Finder are numbered 1 (gray) to 7 (orange). The name of a standard
//!   colour is written in the language of the system (`Blu\n4` on an Italian Mac). The colour
//!   a tag is drawn in is the one the Finder's own list of tags gives its name; the number in
//!   the file only counts for a tag that is not in the list (see [`FinderTagList::color_of`]).
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
use crate::tagsearch::{self, ScanLimits, TagQuery};

pub use super::finderlist::FinderTagList;

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

    /// The name the system of language `lang` (`"it"`, `"fr"`… the first two letters of the
    /// locale) gives this colour as a tag; English for a language not in the table.
    pub fn label(self, lang: &str) -> &'static str {
        let lang = lang.get(..2).unwrap_or(lang).to_ascii_lowercase();
        let names = LABELS
            .iter()
            .find(|(l, _)| *l == lang)
            .map(|(_, n)| n)
            .unwrap_or(&LABELS[0].1);
        let at = TagColor::ALL.iter().position(|c| *c == self).unwrap_or(0);
        names[at]
    }

    /// A colour from its name in any language of the table, in any case ("red", "Gray",
    /// "grey", "Rosso", "Grün").
    pub fn from_name(name: &str) -> Option<TagColor> {
        let n = name.trim().to_lowercase();
        if n == "grey" {
            return Some(TagColor::Gray);
        }
        LABELS.iter().find_map(|(_, names)| {
            names
                .iter()
                .position(|l| l.to_lowercase() == n)
                .map(|i| TagColor::ALL[i])
        })
    }
}

/// What the Finder calls its seven colours (in the order of [`TagColor::ALL`]: red, orange,
/// yellow, green, blue, purple, gray), by language. English first: it is the fallback.
const LABELS: [(&str, [&str; 7]); 9] = [
    (
        "en",
        ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Gray"],
    ),
    (
        "it",
        [
            "Rosso",
            "Arancione",
            "Giallo",
            "Verde",
            "Blu",
            "Viola",
            "Grigio",
        ],
    ),
    (
        "fr",
        ["Rouge", "Orange", "Jaune", "Vert", "Bleu", "Violet", "Gris"],
    ),
    (
        "de",
        ["Rot", "Orange", "Gelb", "Grün", "Blau", "Violett", "Grau"],
    ),
    (
        "es",
        [
            "Rojo", "Naranja", "Amarillo", "Verde", "Azul", "Morado", "Gris",
        ],
    ),
    (
        "pt",
        [
            "Vermelho", "Laranja", "Amarelo", "Verde", "Azul", "Roxo", "Cinza",
        ],
    ),
    (
        "nl",
        ["Rood", "Oranje", "Geel", "Groen", "Blauw", "Paars", "Grijs"],
    ),
    ("sv", ["Röd", "Orange", "Gul", "Grön", "Blå", "Lila", "Grå"]),
    (
        "da",
        ["Rød", "Orange", "Gul", "Grøn", "Blå", "Lilla", "Grå"],
    ),
];

/// The language of the system as the first two letters of a locale (`it_IT.UTF-8` is `it`):
/// from `RADA_TAG_LANGUAGE` (to choose by hand), `LC_ALL`, `LC_MESSAGES`, `LANG`; English
/// when none says.
pub fn language_from_env() -> String {
    // The language the person chose for the interface wins over the environment, unless the
    // tag language itself was set by hand.
    if std::env::var_os("RADA_TAG_LANGUAGE").is_none()
        && let Some(l) = crate::i18n::chosen_lang()
    {
        return l.code().to_string();
    }
    ["RADA_TAG_LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .map(|v| v.chars().take(2).collect::<String>().to_ascii_lowercase())
        .unwrap_or_else(|| "en".to_string())
}

/// The first language of `defaults read -g AppleLanguages` (`("it-IT", "en-IT")`): `it`.
pub fn language_from_apple_languages(output: &str) -> Option<String> {
    let first = output.split('"').nth(1)?;
    let lang: String = first
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    (lang.len() >= 2).then(|| lang.to_ascii_lowercase())
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

    /// One of the seven colours, called what the system of language `lang` calls it.
    pub fn standard_in(color: TagColor, lang: &str) -> Tag {
        Tag {
            name: color.label(lang).to_string(),
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
        // The number is always written, 0 for a tag with no colour: what the Finder reads back
        // is then what was meant, whatever the name says.
        .map(|t| format!("{}\n{}", t.name, t.color.map_or(0, TagColor::finder_index)))
        .collect();
    crate::platform::bplist::encode_string_array(&items)
}

/// The tags in the value of `com.apple.metadata:_kMDItemUserTags`, coloured by the numbers
/// of the file alone; `None` when it is not a list of strings.
pub fn decode_finder(bytes: &[u8]) -> Option<Vec<Tag>> {
    decode_finder_with(bytes, &FinderTagList::empty())
}

/// As [`decode_finder`], with the colours the Finder's list of tags gives.
pub fn decode_finder_with(bytes: &[u8], list: &FinderTagList) -> Option<Vec<Tag>> {
    let items = crate::platform::bplist::decode_string_array(bytes)?;
    Some(
        items
            .into_iter()
            .map(|s| {
                let (name, number) = match s.rsplit_once('\n') {
                    Some((n, i)) => (n.to_string(), i.trim().parse::<u8>().ok()),
                    None => (s, None),
                };
                Tag {
                    color: list.color_of(&name, number),
                    name,
                }
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

    /// The language the names of the seven colours are written in (see
    /// [`TagColor::label`]). The system's, where the system says.
    fn language(&self) -> String {
        language_from_env()
    }

    /// The items under `root` (or anywhere, where the system has an index) that carry the tag.
    /// Slow: call it from a worker, and stop early when `cancel` says so.
    fn find(
        &self,
        query: &TagQuery,
        root: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<std::path::PathBuf>> {
        Ok(tagsearch::scan(
            self,
            root,
            query,
            cancel,
            ScanLimits::default(),
        ))
    }
}

/// Tags kept in an extended attribute.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct XattrTags {
    format: Format,
    /// Where to look for the search tool (`mdfind`, `baloosearch`); `None`: in `PATH`.
    tool_dir: Option<std::path::PathBuf>,
    /// The Finder's list of tags, which has the colours (the Finder's format only).
    finder_list: Option<super::finderlist::FinderListFile>,
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
            tool_dir: None,
            finder_list: None,
        }
    }

    /// The colours of the tags come from the Finder's list in `file`.
    pub fn with_finder_list(mut self, file: std::path::PathBuf) -> XattrTags {
        self.finder_list = Some(super::finderlist::FinderListFile::new(file));
        self
    }

    /// `user.xdg.tags`.
    pub fn freedesktop() -> XattrTags {
        XattrTags {
            format: Format::Freedesktop,
            tool_dir: None,
            finder_list: None,
        }
    }

    /// Look for the search tool in this folder only (for tests, with a stand-in).
    pub fn with_tool_dir(mut self, dir: std::path::PathBuf) -> XattrTags {
        self.tool_dir = Some(dir);
        self
    }

    /// The first of `names` that is a program, in the tool folder or in `PATH`.
    fn tool(&self, names: &[&str]) -> Option<std::path::PathBuf> {
        let dirs: Vec<std::path::PathBuf> = match &self.tool_dir {
            Some(d) => vec![d.clone()],
            None => super::which::path_dirs(&std::env::var_os("PATH")?).collect(),
        };
        names
            .iter()
            .find_map(|n| super::which::which_in_dirs(&dirs, n))
    }

    /// What the system's own index says carries the tag: Spotlight on a Mac, Baloo where it
    /// runs. `None` when there is no index to ask or it did not answer.
    fn indexed(
        &self,
        query: &TagQuery,
        cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<std::path::PathBuf>> {
        const WAIT: std::time::Duration = std::time::Duration::from_secs(20);
        let (program, args) = match self.format {
            Format::Finder => {
                let q = query.name.replace('\\', "\\\\").replace('\'', "\\'");
                (
                    self.tool(&["mdfind"])?,
                    vec![format!("kMDItemUserTags == '{q}'c")],
                )
            }
            Format::Freedesktop => {
                let q = query.name.replace('"', "");
                (
                    self.tool(&["baloosearch6", "baloosearch"])?,
                    vec![format!("tag:\"{q}\"")],
                )
            }
        };
        let out = tagsearch::run_tool(&program, &args, WAIT, cancel)?;
        Some(tagsearch::paths_from_output(&out))
    }

    /// The Finder's tags in the value of the attribute, with the colours of its list.
    fn decode(&self, bytes: &[u8]) -> Option<Vec<Tag>> {
        match &self.finder_list {
            Some(file) => decode_finder_with(bytes, &file.current()),
            None => decode_finder(bytes),
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
            Format::Finder => self.decode(&bytes).ok_or_else(|| {
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

    fn find(
        &self,
        query: &TagQuery,
        root: &Path,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<std::path::PathBuf>> {
        // The index is asked first; what it names is checked against the tags themselves (an
        // index lags behind, and matches loosely). An index with nothing to offer, or none,
        // sends the search to the disk.
        if let Some(listed) = self.indexed(query, cancel) {
            let found: Vec<_> = listed
                .into_iter()
                .filter(|p| self.read(p).is_ok_and(|t| query.matches(&t)))
                .collect();
            if !found.is_empty() {
                return Ok(found);
            }
        }
        Ok(tagsearch::scan(
            self,
            root,
            query,
            cancel,
            ScanLimits::default(),
        ))
    }

    fn language(&self) -> String {
        #[cfg(target_os = "macos")]
        if self.format == Format::Finder {
            // The terminal's LANG says little on a Mac: the system's list of languages does.
            static LANG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            return LANG
                .get_or_init(|| {
                    std::process::Command::new("defaults")
                        .args(["read", "-g", "AppleLanguages"])
                        .stdin(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                        .ok()
                        .and_then(|o| {
                            language_from_apple_languages(&String::from_utf8_lossy(&o.stdout))
                        })
                        .unwrap_or_else(language_from_env)
                })
                .clone();
        }
        language_from_env()
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
    fn the_colours_are_called_in_the_language_of_the_system() {
        let names = |lang: &str| TagColor::ALL.map(|c| c.label(lang));
        assert_eq!(
            names("it"),
            [
                "Rosso",
                "Arancione",
                "Giallo",
                "Verde",
                "Blu",
                "Viola",
                "Grigio"
            ]
        );
        assert_eq!(
            names("en"),
            ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Gray"]
        );
        // The first two letters of a locale are the language; an unknown one is English.
        assert_eq!(TagColor::Blue.label("it_IT.UTF-8"), "Blu");
        assert_eq!(TagColor::Blue.label("pt-BR"), "Azul");
        assert_eq!(TagColor::Blue.label("xx"), "Blue");
        assert_eq!(TagColor::Blue.label(""), "Blue");
        // Any of the names finds its colour, in any case.
        for lang in ["en", "it", "fr", "de", "es", "pt", "nl", "sv", "da"] {
            for c in TagColor::ALL {
                assert_eq!(TagColor::from_name(c.label(lang)), Some(c), "{lang} {c:?}");
                assert_eq!(
                    TagColor::from_name(&c.label(lang).to_uppercase()),
                    Some(c),
                    "{lang} {c:?}"
                );
            }
        }
        assert_eq!(Tag::standard_in(TagColor::Blue, "it").name, "Blu");
        assert_eq!(Tag::named("Rosso").color, Some(TagColor::Red));
    }

    #[test]
    fn the_language_is_read_from_the_locale_and_from_the_macs_list() {
        assert_eq!(
            language_from_apple_languages("(\n    \"it-IT\",\n    \"en-IT\"\n)\n").as_deref(),
            Some("it")
        );
        assert_eq!(
            language_from_apple_languages("(\n    \"zh-Hans-CN\"\n)").as_deref(),
            Some("zh")
        );
        assert_eq!(language_from_apple_languages("garbage"), None);
        assert_eq!(language_from_apple_languages("()"), None);
    }

    #[test]
    fn a_colour_chosen_on_an_italian_mac_is_written_as_the_finder_writes_it() {
        // "Blu" and the number 4: what the Finder itself stores for the blue tag.
        let bytes = encode_finder(&[Tag::standard_in(TagColor::Blue, "it")]);
        assert_eq!(
            crate::platform::bplist::decode_string_array(&bytes).unwrap(),
            ["Blu\n4"]
        );
        // And back: the colour comes from the number.
        let back = decode_finder(&bytes).unwrap();
        assert_eq!(back, [t("Blu", Some(TagColor::Blue))]);
        // Every standard colour, in every language, goes out and comes back with its number.
        for lang in ["en", "it", "fr", "de", "es"] {
            for c in TagColor::ALL {
                let tag = Tag::standard_in(c, lang);
                let bytes = encode_finder(std::slice::from_ref(&tag));
                let items = crate::platform::bplist::decode_string_array(&bytes).unwrap();
                assert_eq!(items, [format!("{}\n{}", tag.name, c.finder_index())]);
                assert_eq!(decode_finder(&bytes).unwrap(), [tag]);
            }
        }
    }

    #[test]
    fn the_colour_number_is_always_written_and_zero_means_none() {
        let bytes = encode_finder(&[
            t("Lavoro", None),
            Tag::standard_in(TagColor::Red, "it"),
            t("Clienti", Some(TagColor::Blue)),
        ]);
        assert_eq!(
            crate::platform::bplist::decode_string_array(&bytes).unwrap(),
            ["Lavoro\n0", "Rosso\n6", "Clienti\n4"]
        );
        // And back: the number is the colour, 0 is none.
        assert_eq!(
            decode_finder(&bytes).unwrap(),
            [
                t("Lavoro", None),
                t("Rosso", Some(TagColor::Red)),
                t("Clienti", Some(TagColor::Blue))
            ]
        );
    }

    #[test]
    fn real_finder_values_in_italian_are_read_by_their_numbers() {
        // What an Italian Finder writes for its seven colours, a tag of the user's own, one
        // with a colour, one with the number 0, and a tag whose name lies about its number.
        let bytes = crate::platform::bplist::encode_string_array(&[
            "Rosso\n6".into(),
            "Arancione\n7".into(),
            "Giallo\n5".into(),
            "Verde\n2".into(),
            "Blu\n4".into(),
            "Viola\n3".into(),
            "Grigio\n1".into(),
            "Lavoro".into(),
            "Clienti\n4".into(),
            "Progetto\n0".into(),
            "Blue\n0".into(),
            "Blu\n6".into(),
        ]);
        let got = decode_finder(&bytes).unwrap();
        let colours: Vec<Option<TagColor>> = got.iter().map(|t| t.color).collect();
        use TagColor::*;
        assert_eq!(
            colours,
            [
                Some(Red),
                Some(Orange),
                Some(Yellow),
                Some(Green),
                Some(Blue),
                Some(Purple),
                Some(Gray),
                None,
                Some(Blue),
                None,
                None,
                Some(Red),
            ]
        );
        assert_eq!(got[4].name, "Blu");
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_files_tags_are_drawn_in_the_colours_of_the_finders_list() {
        use crate::platform::finderlist::sample_plist;
        let d = tempfile::tempdir().unwrap();
        let list = d.path().join("com.apple.finder.plist");
        std::fs::write(
            &list,
            sample_plist(&[("Verde", 2), ("Clienti", 4), ("Arancio", 7)]),
        )
        .unwrap();
        let store = XattrTags::finder().with_finder_list(list);
        // What the Finder wrote for a real file: "Verde" with the number 1 (gray), and yet the
        // Finder draws it green; a tag of the person's own that the file numbers 0; one the
        // list does not have, which keeps the file's number; one with no number.
        let bytes = crate::platform::bplist::encode_string_array(&[
            "Verde\n1".into(),
            "Clienti\n0".into(),
            "Altro\n3".into(),
            "Rosso".into(),
            "Lavoro".into(),
        ]);
        assert_eq!(
            store.decode(&bytes).unwrap(),
            [
                t("Verde", Some(TagColor::Green)),
                t("Clienti", Some(TagColor::Blue)),
                t("Altro", Some(TagColor::Purple)),
                t("Rosso", Some(TagColor::Red)),
                t("Lavoro", None),
            ]
        );
        // Without the list the number of the file is all there is.
        assert_eq!(
            decode_finder(&bytes).unwrap()[0],
            t("Verde", Some(TagColor::Gray))
        );
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
