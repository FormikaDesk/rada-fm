//! rada as the system's file chooser (`rada --chooser-file FILE`).
//!
//! The explorer is the same one; what changes is what it is for. It starts on a folder, the
//! person moves through it as always, and Enter hands the choice to whoever started it: the
//! paths are written to a file, one per line, and rada quits. Esc quits and writes nothing.
//! A file chooser must not be able to harm what it shows, so nothing that deletes, moves,
//! renames, copies or changes a file is reachable here; the one thing that can be made is a
//! new folder, where the choice is a folder or a place to save.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rada_core::display;
use rada_core::i18n::Key;
use rada_core::model::{SortKey, SortSpec};

use crate::keymap::Action;

/// What is being chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An existing file.
    File,
    /// An existing folder.
    Folder,
    /// A place to save a file, with the name to suggest.
    Save { name: String },
}

/// A chooser session: what is chosen, and whether more than one thing may be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chooser {
    pub kind: Kind,
    pub multiple: bool,
}

impl Chooser {
    pub fn file(multiple: bool) -> Chooser {
        Chooser {
            kind: Kind::File,
            multiple,
        }
    }

    pub fn folder(multiple: bool) -> Chooser {
        Chooser {
            kind: Kind::Folder,
            multiple,
        }
    }

    /// Saving always chooses one place.
    pub fn save(name: impl Into<String>) -> Chooser {
        Chooser {
            kind: Kind::Save { name: name.into() },
            multiple: false,
        }
    }

    /// The words of the bar at the top: what is being asked.
    pub fn title(&self) -> &'static str {
        match (&self.kind, self.multiple) {
            (Kind::File, false) => Key::ChooseTitleFile.tr(),
            (Kind::File, true) => Key::ChooseTitleFiles.tr(),
            (Kind::Folder, false) => Key::ChooseTitleFolder.tr(),
            (Kind::Folder, true) => Key::ChooseTitleFolders.tr(),
            (Kind::Save { .. }, _) => Key::ChooseTitleSave.tr(),
        }
    }

    /// The keys that matter here, for the bar: the full line when it fits in `room` cells, the
    /// short one when it does not.
    pub fn hint(&self, room: usize) -> &'static str {
        let (long, short) = match (&self.kind, self.multiple) {
            (Kind::File, false) => (Key::ChooseHintFile, Key::ChooseHintFileShort),
            (Kind::File, true) => (Key::ChooseHintFiles, Key::ChooseHintFilesShort),
            (Kind::Folder, false) => (Key::ChooseHintFolder, Key::ChooseHintFolderShort),
            (Kind::Folder, true) => (Key::ChooseHintFolders, Key::ChooseHintFoldersShort),
            (Kind::Save { .. }, _) => (Key::ChooseHintSave, Key::ChooseHintSaveShort),
        };
        if display::width(long.tr()) <= room {
            long.tr()
        } else {
            short.tr()
        }
    }

    /// Only folders are listed when a folder is what is asked for.
    pub fn folders_only(&self) -> bool {
        self.kind == Kind::Folder
    }

    pub fn is_save(&self) -> bool {
        matches!(self.kind, Kind::Save { .. })
    }

    /// Whether an action may be done. Everything that looks, moves around or shows is
    /// allowed; whatever changes a file, starts another program or touches the settings is
    /// not. A new folder is the one thing made, and only where a folder is the answer.
    pub fn allows(&self, action: Action) -> bool {
        use Action::*;
        match action {
            Up | Down | Parent | Open | First | Last | PageUp | PageDown | HalfPageUp
            | HalfPageDown | GoHome | Back | Forward | ClearSelection | Filter | Palette | Sort
            | SortReverse | ToggleHidden | ToggleHex | PreviewDown | PreviewUp | SwitchPane
            | ToggleSidebar | Help | Quit | Refresh | FocusAddress | ToggleDetails
            | ViewDetails | ViewIcons | ViewColumns | ToggleView | ToggleTags | Properties
            | CopyPath | NewTab | CloseTab | NextTab | PrevTab | ShowInFolder => true,
            ToggleMark | SelectAll | SelectUp | SelectDown | SelectToFirst | SelectToLast => {
                self.multiple
            }
            NewFolder => matches!(self.kind, Kind::Folder | Kind::Save { .. }),
            Copy | Cut | Paste | Trash | DeletePermanently | Rename | BulkRename | ExtractHere
            | ExtractToFolder | Compress | Undo | Redo | History | Bookmark | NewFile
            | OpenWith | OpenTerminal | ContextMenu | Update | QuickLook | ShowPackage => false,
        }
    }
}

/// The order a chooser starts in, whatever the `sort` setting says: by date of modification,
/// the most recent first (a date that is not reversed) (what is being looked for is nearly always what was just made or
/// downloaded). The rest of the setting (folders first, natural names) is the person's, and
/// the order can be changed as usual while choosing.
pub fn default_sort(base: SortSpec) -> SortSpec {
    SortSpec {
        key: SortKey::Modified,
        reverse: false,
        ..base
    }
}

/// Why a name cannot be the name of a file to save, if it cannot.
pub fn check_name(name: &str) -> Result<(), Key> {
    let name = name.trim_end_matches('\n');
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || (cfg!(windows) && name.contains('\\'))
        || name.contains('\0')
    {
        return Err(Key::ChooseNameBad);
    }
    if name.contains('\n') || name.contains('\r') {
        return Err(Key::ChooseLineBreak);
    }
    Ok(())
}

/// Whether a path can travel in the answer file, which has one path per line.
pub fn fits_a_line(path: &Path) -> bool {
    !path.to_string_lossy().contains(['\n', '\r'])
}

/// The answer file's content: each path on a line of its own, an end of line after the last.
/// On Unix the bytes of the names are kept as they are, whatever their encoding.
pub fn render(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in paths {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            out.extend_from_slice(p.as_os_str().as_bytes());
        }
        #[cfg(not(unix))]
        out.extend_from_slice(p.to_string_lossy().as_bytes());
        out.push(b'\n');
    }
    out
}

/// Where to start and what to suggest, from what the caller gave: a folder, a file (its
/// folder, with the file under the cursor) or, for saving, a path that is not there yet (its
/// folder, and its name as the one to suggest).
pub struct Start {
    pub dir: PathBuf,
    pub select: Option<OsString>,
    pub name: Option<String>,
}

pub fn start_from(given: &Path, saving: bool) -> Start {
    if given.is_dir() {
        return Start {
            dir: given.to_path_buf(),
            select: None,
            name: None,
        };
    }
    let parent = given
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file = given.file_name().map(|n| n.to_os_string());
    // The caller's own folder may be gone (a stale "last folder"): fall back on the nearest
    // one that is there.
    let dir = parent
        .ancestors()
        .find(|a| a.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or(parent);
    let name = saving
        .then(|| file.as_ref().map(|n| n.to_string_lossy().into_owned()))
        .flatten();
    Start {
        dir,
        select: if given.exists() { file } else { None },
        name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Action;

    #[test]
    fn nothing_that_changes_a_file_is_allowed() {
        for chooser in [
            Chooser::file(false),
            Chooser::file(true),
            Chooser::folder(false),
            Chooser::folder(true),
            Chooser::save("a.txt"),
        ] {
            for a in [
                Action::Copy,
                Action::Cut,
                Action::Paste,
                Action::Trash,
                Action::DeletePermanently,
                Action::Rename,
                Action::BulkRename,
                Action::ExtractHere,
                Action::ExtractToFolder,
                Action::Compress,
                Action::Undo,
                Action::Redo,
                Action::History,
                Action::NewFile,
                Action::OpenWith,
                Action::OpenTerminal,
                Action::ContextMenu,
                Action::Update,
                Action::Bookmark,
            ] {
                assert!(!chooser.allows(a), "{a:?} allowed in {chooser:?}");
            }
        }
    }

    #[test]
    fn every_action_is_decided_here() {
        // A new action must be put on one side or the other: this fails to compile in
        // `allows` when it is not, and here it must at least answer.
        for a in Action::ALL {
            let _ = Chooser::file(false).allows(a);
        }
    }

    #[test]
    fn marks_are_for_choosing_several_and_new_folders_for_folders_and_saving() {
        assert!(!Chooser::file(false).allows(Action::ToggleMark));
        assert!(!Chooser::file(false).allows(Action::SelectAll));
        assert!(Chooser::file(true).allows(Action::ToggleMark));
        assert!(Chooser::folder(true).allows(Action::SelectAll));
        assert!(!Chooser::file(true).allows(Action::NewFolder));
        assert!(Chooser::folder(false).allows(Action::NewFolder));
        assert!(Chooser::save("x").allows(Action::NewFolder));
        assert!(!Chooser::save("x").allows(Action::ToggleMark));
    }

    #[test]
    fn a_chooser_starts_with_the_newest_first_whatever_the_setting() {
        let base = SortSpec {
            key: SortKey::Size,
            reverse: false,
            dirs_first: false,
            natural: false,
        };
        let s = default_sort(base);
        assert_eq!((s.key, s.reverse), (SortKey::Modified, false));
        assert!(
            s.descending(),
            "the arrow of a date that runs from the newest"
        );
        // The rest is the person's own.
        assert_eq!((s.dirs_first, s.natural), (false, false));
        assert_eq!(default_sort(SortSpec::default()).key, SortKey::Modified);
    }

    #[test]
    fn names_to_save_are_checked() {
        assert!(check_name("report.pdf").is_ok());
        assert!(check_name("with space & ünï.txt").is_ok());
        for bad in ["", ".", "..", "a/b", "/etc/passwd", "a\0b"] {
            assert_eq!(check_name(bad), Err(Key::ChooseNameBad), "{bad:?}");
        }
        assert_eq!(check_name("a\nb"), Err(Key::ChooseLineBreak));
        assert!(!fits_a_line(Path::new("/tmp/a\nb")));
        assert!(fits_a_line(Path::new("/tmp/a b")));
    }

    #[test]
    fn the_answer_has_one_path_per_line() {
        let out = render(&[PathBuf::from("/a/one"), PathBuf::from("/b/two words")]);
        assert_eq!(out, b"/a/one\n/b/two words\n");
        assert!(render(&[]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn names_that_are_not_utf8_are_kept_byte_for_byte() {
        use std::os::unix::ffi::OsStringExt;
        let odd = PathBuf::from(OsString::from_vec(b"/x/\xff\xfe".to_vec()));
        assert_eq!(render(&[odd]), b"/x/\xff\xfe\n");
    }

    #[test]
    fn the_start_is_the_folder_given_or_the_folder_of_the_file_given() {
        let sb = rada_core::testutil::Sandbox::new();
        let dir = sb.mkdir("here");
        let file = sb.write("here/a.txt", "x");
        let s = start_from(&dir, false);
        assert_eq!((s.dir.as_path(), s.select.is_none()), (dir.as_path(), true));
        let s = start_from(&file, false);
        assert_eq!(s.dir, dir);
        assert_eq!(s.select.as_deref(), Some(std::ffi::OsStr::new("a.txt")));
        // Saving: a name that is not there yet is the name to suggest.
        let s = start_from(&dir.join("new.pdf"), true);
        assert_eq!(s.dir, dir);
        assert_eq!(s.name.as_deref(), Some("new.pdf"));
        assert!(s.select.is_none());
        // A folder that is gone: the nearest one that is there.
        let s = start_from(&dir.join("gone/deeper/x.txt"), true);
        assert_eq!(s.dir, dir);
    }
}
