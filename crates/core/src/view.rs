//! The lists that are not a folder: the items of a tag, the files used lately.
//!
//! Each is named by a path that is not on any disk. This is the one place that tells such a
//! path from a real one, so a window, a menu or a worker asks "what is this list?" instead of
//! knowing how the paths are spelled.

use std::path::Path;

use crate::{recents, tagsearch};

/// What a path stands for when it is not a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// The items that carry the tag called so.
    Tag(String),
    /// The files used lately.
    Recent,
}

impl ViewKind {
    /// The kind of list `path` stands for, `None` for a place on the disk.
    pub fn of(path: &Path) -> Option<ViewKind> {
        if recents::is_view(path) {
            return Some(ViewKind::Recent);
        }
        tagsearch::view_name(path).map(ViewKind::Tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_path_off_the_disk_says_what_list_it_is() {
        assert_eq!(
            ViewKind::of(&tagsearch::view_path("Blu")),
            Some(ViewKind::Tag("Blu".into()))
        );
        assert_eq!(ViewKind::of(&recents::view_path()), Some(ViewKind::Recent));
    }

    #[test]
    fn a_place_on_the_disk_is_not_a_list() {
        for p in ["/home/u", "/home/u/tag:Blu", "tags", "tag:"] {
            assert_eq!(ViewKind::of(&PathBuf::from(p)), None, "{p}");
        }
    }
}
