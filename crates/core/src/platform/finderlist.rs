//! The Finder's own list of tags: the colour of a tag is the one this list gives its name, not
//! the number written in each file (a file tagged long ago can carry another number, and the
//! Finder still draws the tag in the colour of the list).
//!
//! The list is in the Finder's synced preferences (`FinderTags`: one dictionary per tag, `n`
//! its name and `l` its colour number). Where it is missing or unreadable the list is empty and
//! the file's own number decides.

use std::collections::HashMap;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::{Arc, Mutex};

use super::plist::{self, Value};
use super::tags::TagColor;

/// Where the Finder keeps the list, under the home folder.
pub const FINDER_LIST: &str = "Library/SyncedPreferences/com.apple.finder.plist";

/// The tags of the Finder's list and the colour number of each, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FinderTagList {
    numbers: HashMap<String, u8>,
}

impl FinderTagList {
    pub fn empty() -> FinderTagList {
        FinderTagList::default()
    }

    /// The list in the property list `bytes`; `None` when it is not one, or has no tags.
    pub fn from_plist(bytes: &[u8]) -> Option<FinderTagList> {
        let root = plist::decode(bytes)?;
        let Some(Value::Array(tags)) = root.find("FinderTags", 4) else {
            return None;
        };
        let mut numbers = HashMap::new();
        for tag in tags {
            let Some(name) = tag.get("n").and_then(Value::as_str) else {
                continue;
            };
            let number = tag.get("l").and_then(Value::as_int).unwrap_or(0);
            numbers
                .entry(name.to_lowercase())
                .or_insert(u8::try_from(number).unwrap_or(0));
        }
        Some(FinderTagList { numbers })
    }

    /// The colour of the tag called `name`, given the number its file wrote after the name
    /// (`None`: no number there). The one answer for a tag, wherever it is shown:
    /// 1. the Finder's list, if the tag is in it (whatever its number says: 0 is no colour);
    /// 2. else the number of the file (0 is no colour);
    /// 3. else, with no number at all, the colour the name itself says, in any language.
    pub fn color_of(&self, name: &str, file_number: Option<u8>) -> Option<TagColor> {
        match self.numbers.get(&name.to_lowercase()) {
            Some(n) => TagColor::from_finder_index(*n),
            None => match file_number {
                Some(n) => TagColor::from_finder_index(n),
                None => TagColor::from_name(name),
            },
        }
    }
}

/// The list in its file, read again when the file changes (the person changes a tag in the
/// Finder while rada runs).
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct FinderListFile {
    path: PathBuf,
    cached: Mutex<Option<(std::time::SystemTime, u64, Arc<FinderTagList>)>>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl FinderListFile {
    pub fn new(path: PathBuf) -> FinderListFile {
        FinderListFile {
            path,
            cached: Mutex::new(None),
        }
    }

    /// The list as the file has it now; an empty one when there is no file or it is not a
    /// list.
    pub fn current(&self) -> Arc<FinderTagList> {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return Arc::new(FinderTagList::empty());
        };
        let stamp = (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len());
        let mut cached = self.cached.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((time, len, list)) = cached.as_ref()
            && (*time, *len) == stamp
        {
            return list.clone();
        }
        let list = std::fs::read(&self.path)
            .ok()
            .and_then(|b| FinderTagList::from_plist(&b))
            .unwrap_or_default();
        let list = Arc::new(list);
        *cached = Some((stamp.0, stamp.1, list.clone()));
        list
    }
}

/// A list the way the Finder's synced preferences hold it (XML, so a test can write it).
#[cfg(test)]
pub(crate) fn sample_plist(tags: &[(&str, u8)]) -> Vec<u8> {
    let items: String = tags
        .iter()
        .map(|(n, l)| {
            format!(
                "<dict><key>n</key><string>{n}</string><key>l</key><integer>{l}</integer>\
                 <key>v</key><integer>1</integer></dict>"
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>values</key><dict>\
         <key>FinderTagDict</key><dict><key>value</key><dict><key>FinderTags</key>\
         <array>{items}</array></dict></dict></dict></dict></plist>"
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(tags: &[(&str, u8)]) -> Vec<u8> {
        sample_plist(tags)
    }

    fn list(tags: &[(&str, u8)]) -> FinderTagList {
        FinderTagList::from_plist(&plist(tags)).unwrap()
    }

    #[test]
    fn the_list_wins_over_the_number_in_the_file() {
        use TagColor::*;
        let l = list(&[("Verde", 2), ("Clienti", 4), ("Vecchio", 0)]);
        // The case seen on a real Mac: the file says "Verde\n1" (gray), the Finder draws green.
        assert_eq!(l.color_of("Verde", Some(1)), Some(Green));
        assert_eq!(l.color_of("verde", Some(1)), Some(Green));
        // A tag of the person's own, with a colour in the list.
        assert_eq!(l.color_of("Clienti", Some(0)), Some(Blue));
        assert_eq!(l.color_of("Clienti", None), Some(Blue));
        // In the list with no colour: none, whatever the file says.
        assert_eq!(l.color_of("Vecchio", Some(6)), None);
    }

    #[test]
    fn a_tag_not_in_the_list_falls_back_to_the_file_and_then_to_its_name() {
        use TagColor::*;
        let l = list(&[("Verde", 2)]);
        assert_eq!(l.color_of("Arancio", Some(7)), Some(Orange));
        assert_eq!(l.color_of("Arancio", Some(0)), None);
        assert_eq!(l.color_of("Rosso", None), Some(Red));
        assert_eq!(l.color_of("Grün", None), Some(Green));
        assert_eq!(l.color_of("Lavoro", None), None);
        // With no list at all it is the same.
        assert_eq!(FinderTagList::empty().color_of("Blu", Some(6)), Some(Red));
        assert_eq!(FinderTagList::empty().color_of("Blu", None), Some(Blue));
    }

    #[test]
    fn a_file_that_is_not_the_list_gives_none() {
        assert!(FinderTagList::from_plist(b"garbage").is_none());
        assert!(
            FinderTagList::from_plist(
                b"<plist><dict><key>a</key><string>b</string></dict></plist>"
            )
            .is_none()
        );
        // Entries without a name are passed over; a missing number is no colour.
        let x = b"<plist><dict><key>FinderTags</key><array><dict><key>l</key><integer>2</integer></dict>\
                  <dict><key>n</key><string>Lavoro</string></dict></array></dict></plist>";
        let l = FinderTagList::from_plist(x).unwrap();
        assert_eq!(l.color_of("Lavoro", Some(4)), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_file_is_read_again_when_it_changes() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("com.apple.finder.plist");
        let file = FinderListFile::new(f.clone());
        // No file: an empty list.
        assert_eq!(
            file.current().color_of("Verde", Some(1)),
            Some(TagColor::Gray)
        );
        std::fs::write(&f, plist(&[("Verde", 2)])).unwrap();
        assert_eq!(
            file.current().color_of("Verde", Some(1)),
            Some(TagColor::Green)
        );
        // A different length is a change even within the same tick of the clock.
        std::fs::write(&f, plist(&[("Verde", 2), ("Altro", 3)])).unwrap();
        assert_eq!(
            file.current().color_of("Altro", None),
            Some(TagColor::Purple)
        );
        // Unreadable: empty again.
        std::fs::write(&f, b"nonsense").unwrap();
        assert_eq!(file.current().color_of("Altro", None), None);
    }
}
