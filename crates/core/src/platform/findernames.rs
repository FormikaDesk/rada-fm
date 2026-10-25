//! The names a Mac gives to the seven colours of the Finder's tags once the person has renamed
//! some of them (the orange tag called "🌈"). The Finder keeps them in its preferences, in
//! `FavoriteTagNames`: eight entries, an empty one and then the seven colours in a fixed order
//! (red, orange, yellow, green, blue, purple, gray), the same as [`TagColor::ALL`]. A file
//! tagged with a renamed colour carries the new name and a number that need not agree with it,
//! so the name is looked up here.
//!
//! The list is believed only when it looks like the Finder's: any other shape is ignored as a
//! whole, never half used.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use super::tags::TagColor;

/// An empty entry, then one for each colour.
const ENTRIES: usize = 8;
/// How many entries must be the standard name of their colour, in its place, for the list to
/// be taken for the Finder's (a list renamed from end to end could not be told from any other).
const MIN_STANDARD: usize = 3;
/// The preferences of a Finder are far smaller than this; a larger file is not read.
const MAX_BYTES: u64 = 8 << 20;

/// `names` if they are the Finder's list: eight entries, the first empty, and every entry that
/// is the name of a standard colour (in any language) in the place of that colour, at least
/// [`MIN_STANDARD`] of them.
pub fn clean(names: Vec<String>) -> Option<Vec<String>> {
    if names.len() != ENTRIES || !names[0].is_empty() {
        return None;
    }
    let mut standard = 0;
    for (i, name) in names.iter().enumerate().skip(1) {
        match TagColor::from_name(name) {
            Some(c) if c == TagColor::ALL[i - 1] => standard += 1,
            Some(_) => return None,
            None => {}
        }
    }
    (standard >= MIN_STANDARD).then_some(names)
}

/// The colour that `list` (a clean one, or none) gives to the tag called `name`.
pub fn colour_of(list: &[String], name: &str) -> Option<TagColor> {
    let name = name.trim().to_lowercase();
    list.iter()
        .skip(1)
        .position(|n| !n.is_empty() && n.to_lowercase() == name)
        .map(|i| TagColor::ALL[i])
}

/// The Finder's list as the preferences file holds it, read again only when the file changes.
pub struct FinderNames {
    /// `None`: there is nothing to read (every system but a Mac).
    path: Option<PathBuf>,
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    /// The modification time the list was read at (`Some(None)`: the file was not there).
    read_at: Option<Option<SystemTime>>,
    list: Arc<Vec<String>>,
}

impl FinderNames {
    /// The preferences of the person who runs the program.
    pub fn standard() -> FinderNames {
        let path = std::env::var_os("HOME")
            .map(|h| Path::new(&h).join("Library/Preferences/com.apple.finder.plist"));
        FinderNames::with_path(path)
    }

    /// No list: every colour keeps the name it has.
    pub fn none() -> FinderNames {
        FinderNames::with_path(None)
    }

    /// The preferences at `path` (for tests, with a file of their own).
    pub fn at(path: PathBuf) -> FinderNames {
        FinderNames::with_path(Some(path))
    }

    fn with_path(path: Option<PathBuf>) -> FinderNames {
        FinderNames {
            path,
            cache: Mutex::default(),
        }
    }

    /// The clean list, or an empty one when the file is missing, unreadable or not the Finder's.
    pub fn list(&self) -> Arc<Vec<String>> {
        let Some(path) = &self.path else {
            return Arc::default();
        };
        let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.read_at != Some(stamp) {
            cache.list = Arc::new(stamp.and_then(|_| read(path)).unwrap_or_default());
            cache.read_at = Some(stamp);
        }
        cache.list.clone()
    }
}

fn read(path: &Path) -> Option<Vec<String>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    clean(favorite_tag_names(&bytes)?)
}

#[cfg(any(target_os = "macos", test))]
fn favorite_tag_names(bytes: &[u8]) -> Option<Vec<String>> {
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    value
        .into_dictionary()?
        .remove("FavoriteTagNames")?
        .into_array()?
        .into_iter()
        .map(plist::Value::into_string)
        .collect()
}

/// Only a Mac has these preferences, and only a Mac links the library that reads them.
#[cfg(not(any(target_os = "macos", test)))]
fn favorite_tag_names(_: &[u8]) -> Option<Vec<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a Mac's preferences hold when the orange and the green tags were renamed.
    const REAL: [&str; 8] = ["", "Rosso", "🌈", "Giallo", "👵🏼", "Blu", "Viola", "Grigio"];

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A preferences file with other keys beside the list, in the binary form a Mac writes.
    fn write_prefs(path: &Path, list: &[&str]) {
        let mut dict = plist::Dictionary::new();
        dict.insert("Other".into(), plist::Value::Boolean(true));
        let items = list.iter().map(|s| plist::Value::String(s.to_string()));
        dict.insert(
            "FavoriteTagNames".into(),
            plist::Value::Array(items.collect()),
        );
        plist::Value::Dictionary(dict).to_file_binary(path).unwrap();
    }

    #[test]
    fn the_list_a_mac_really_has_is_clean() {
        assert_eq!(clean(names(&REAL)), Some(names(&REAL)));
        // The standard names of another language are in their places as well.
        let english = [
            "", "Red", "Orange", "Yellow", "Green", "Blue", "Purple", "Gray",
        ];
        assert!(clean(names(&english)).is_some());
    }

    #[test]
    fn a_list_that_is_not_the_finders_is_not_believed() {
        let reordered = [
            "", "Rosso", "🌈", "Verde", "Giallo", "Blu", "Viola", "Grigio",
        ];
        assert_eq!(
            clean(names(&reordered)),
            None,
            "Giallo at the place of green"
        );
        let five = ["", "Rosso", "🌈", "Giallo", "👵🏼"];
        assert_eq!(clean(names(&five)), None, "five entries");
        let no_blank = [
            "Nessuno", "Rosso", "🌈", "Giallo", "👵🏼", "Blu", "Viola", "Grigio",
        ];
        assert_eq!(
            clean(names(&no_blank)),
            None,
            "the first entry is not empty"
        );
        let renamed = ["", "a", "b", "Giallo", "c", "Blu", "d", "e"];
        assert_eq!(
            clean(names(&renamed)),
            None,
            "only two standard names in place"
        );
        assert_eq!(clean(Vec::new()), None);
    }

    #[test]
    fn a_name_has_the_colour_of_its_place_in_the_list() {
        let list = names(&REAL);
        assert_eq!(colour_of(&list, "👵🏼"), Some(TagColor::Green));
        assert_eq!(colour_of(&list, "🌈"), Some(TagColor::Orange));
        assert_eq!(colour_of(&list, " rosso "), Some(TagColor::Red));
        assert_eq!(colour_of(&list, "Progetto"), None);
        assert_eq!(colour_of(&list, ""), None);
        assert_eq!(colour_of(&[], "🌈"), None);
    }

    #[test]
    fn the_list_is_read_from_the_binary_preferences_and_only_when_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("com.apple.finder.plist");
        let source = FinderNames::at(path.clone());
        assert!(source.list().is_empty(), "no file, no list");

        write_prefs(&path, &REAL);
        assert_eq!(*source.list(), names(&REAL));
        let first = source.list();
        assert!(Arc::ptr_eq(&first, &source.list()), "not read again");

        // Another list, with the file stamped a moment later so that the change is seen even
        // on a file system whose clock is coarse.
        write_prefs(
            &path,
            &[
                "", "Rosso", "Giallo", "Verde", "Blu", "Viola", "Grigio", "x",
            ],
        );
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(source.list().is_empty(), "a list out of order is ignored");

        std::fs::write(&path, b"not a property list").unwrap();
        file.set_modified(SystemTime::now() + std::time::Duration::from_secs(10))
            .unwrap();
        assert!(source.list().is_empty());
    }

    /// What the Finder writes for these tags, decoded with the list in the preferences at `path`.
    fn colours_with(path: &Path) -> Vec<Option<TagColor>> {
        let value = crate::platform::bplist::encode_string_array(&names(&[
            "👵🏼\n0",
            "🌈\n1",
            "Arancio\n7",
            "Progetto\n0",
            "Verde\n1",
        ]));
        let list = FinderNames::at(path.to_path_buf()).list();
        let tags = crate::platform::tags::decode_finder_with(&value, &list).unwrap();
        tags.into_iter().map(|t| t.color).collect()
    }

    #[test]
    fn a_renamed_colour_is_drawn_in_its_colour_whatever_number_the_file_carries() {
        use TagColor::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("com.apple.finder.plist");

        write_prefs(&path, &REAL);
        assert_eq!(
            colours_with(&path),
            [Some(Green), Some(Orange), Some(Orange), None, Some(Green)]
        );

        // A list out of order or of five entries is ignored: only the numbers are left (and
        // the standard names, which never needed the list).
        let numbers = [None, Some(Gray), Some(Orange), None, Some(Green)];
        write_prefs(
            &path,
            &[
                "", "Rosso", "🌈", "Verde", "Giallo", "Blu", "Viola", "Grigio",
            ],
        );
        assert_eq!(colours_with(&path), numbers);
        write_prefs(&path, &["", "Rosso", "🌈", "Giallo", "👵🏼"]);
        assert_eq!(colours_with(&path), numbers);
        assert_eq!(colours_with(&dir.path().join("missing.plist")), numbers);
    }

    #[test]
    fn preferences_without_the_key_or_with_other_values_give_no_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.plist");
        plist::Value::Dictionary(plist::Dictionary::new())
            .to_file_binary(&path)
            .unwrap();
        assert!(FinderNames::at(path.clone()).list().is_empty());
        let mut dict = plist::Dictionary::new();
        dict.insert("FavoriteTagNames".into(), plist::Value::Integer(3.into()));
        plist::Value::Dictionary(dict)
            .to_file_binary(&path)
            .unwrap();
        assert!(FinderNames::at(path).list().is_empty());
        assert!(FinderNames::none().list().is_empty());
    }
}
