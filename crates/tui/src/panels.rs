//! The panels view: what it is made of and when it is used. Drawing is in `ui/panels/`; this is
//! the choice between it and the classic layout, the tree of folders of the NAV box and the
//! rows that box lists.
//!
//! The view is one screen of rounded boxes — NAV and SPAZIO on the left, the folder in the
//! middle, ANTEPRIMA and DETTAGLI on the right — under three rows of bars. It needs room: below
//! [`AUTO_FROM`] the classic layout is drawn, and it comes back by itself when the window is
//! made larger.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use rada_core::workers::TreeKid;

use crate::view::LayoutKind;

/// Which layout the person asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutPref {
    /// Panels when the window is at least [`AUTO_FROM`] big, the classic layout otherwise.
    #[default]
    Auto,
    /// Panels whenever there is room for them at all ([`PANELS_MIN`]).
    Panels,
    /// Never panels.
    Classic,
}

impl LayoutPref {
    pub const NAMES: [&'static str; 3] = ["auto", "panels", "classic"];

    /// The three words, in English or Italian (`pannelli`, `classico`); `explorer` is the name
    /// the classic layout had before there were panels.
    pub fn parse(s: &str) -> Option<LayoutPref> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(LayoutPref::Auto),
            "panels" | "pannelli" => Some(LayoutPref::Panels),
            "classic" | "classico" | "explorer" => Some(LayoutPref::Classic),
            _ => None,
        }
    }

    /// The word used in the configuration and in the saved state.
    pub fn id(self) -> &'static str {
        match self {
            LayoutPref::Auto => "auto",
            LayoutPref::Panels => "panels",
            LayoutPref::Classic => "classic",
        }
    }
}

/// What the `layout` setting says: the arrangement of the classic layout and the choice
/// between it and the panels. `compact` is the arrangement with no bars, which never has
/// panels; `explorer` is the older name of `classic`.
pub fn resolve_layout(word: &str) -> Option<(LayoutKind, LayoutPref)> {
    if let Some(LayoutKind::Compact) = LayoutKind::parse(word) {
        return Some((LayoutKind::Compact, LayoutPref::Classic));
    }
    LayoutPref::parse(word).map(|pref| (LayoutKind::Explorer, pref))
}

/// The smallest window (columns, rows) in which `auto` draws the panels.
pub const AUTO_FROM: (u16, u16) = (140, 36);
/// The smallest window in which the panels can be drawn at all, when they are asked for.
pub const PANELS_MIN: (u16, u16) = (100, 24);
/// Below this many rows the blank rows between the bars at the top and at the bottom go, so
/// that more files are seen.
pub const AIRY_FROM: u16 = 42;

/// Whether a window `width` × `height` is drawn as panels, given the person's choice, the
/// arrangement of the classic layout and whether this is a file chooser (which never is).
pub fn use_panels(
    pref: LayoutPref,
    layout: LayoutKind,
    chooser: bool,
    width: u16,
    height: u16,
) -> bool {
    if chooser || layout != LayoutKind::Explorer {
        return false;
    }
    let (need_w, need_h) = match pref {
        LayoutPref::Classic => return false,
        LayoutPref::Panels => PANELS_MIN,
        LayoutPref::Auto => AUTO_FROM,
    };
    width >= need_w && height >= need_h
}

/// The tree of folders of the NAV box: Home at the top, the folders that are open below it, and
/// what is known of each (the answers of the worker that lists them).
#[derive(Default)]
pub struct Tree {
    /// The folders shown open. The ones that lead to the current folder are added to it as the
    /// person goes there; an arrow closes or opens any of them.
    pub open: BTreeSet<PathBuf>,
    kids: HashMap<PathBuf, Vec<TreeKid>>,
    /// The folders a worker was asked about, and the generation of the listing then.
    asked: HashMap<PathBuf, u64>,
    /// The current folder the open set was last made to lead to.
    followed: Option<PathBuf>,
    /// Whether hidden folders are in `kids`.
    hidden: bool,
}

impl Tree {
    /// The subfolders of `dir`, once the worker has answered.
    pub fn kids(&self, dir: &Path) -> Option<&[TreeKid]> {
        self.kids.get(dir).map(Vec::as_slice)
    }

    pub fn is_open(&self, dir: &Path) -> bool {
        self.open.contains(dir)
    }

    pub fn toggle(&mut self, dir: &Path) {
        if !self.open.remove(dir) {
            self.open.insert(dir.to_path_buf());
        }
    }

    /// Open every folder from `root` down to the parent of `cwd`, once for each `cwd`.
    pub fn follow(&mut self, root: &Path, cwd: &Path) {
        if self.followed.as_deref() == Some(cwd) {
            return;
        }
        self.followed = Some(cwd.to_path_buf());
        self.open.insert(root.to_path_buf());
        if cwd.starts_with(root) {
            for a in cwd.ancestors().take_while(|a| a.starts_with(root)).skip(1) {
                self.open.insert(a.to_path_buf());
            }
        }
    }

    /// The folders that are open and have no answer yet (or an old one: `generation` moves
    /// when the folder shown is read again), as the ones to ask about now.
    pub fn wanted(&mut self, hidden: bool, generation: u64) -> Vec<PathBuf> {
        if self.hidden != hidden {
            self.hidden = hidden;
            self.kids.clear();
            self.asked.clear();
        }
        let mut out = Vec::new();
        for dir in &self.open {
            if self.asked.get(dir) != Some(&generation) {
                out.push(dir.clone());
            }
        }
        for dir in &out {
            self.asked.insert(dir.clone(), generation);
        }
        out
    }

    /// The worker's answer.
    pub fn answer(&mut self, dir: PathBuf, hidden: bool, kids: Vec<TreeKid>) {
        if hidden == self.hidden {
            self.kids.insert(dir, kids);
        }
    }
}

/// What a row of the NAV box is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavKind {
    /// A folder of the tree. `open` is `None` for one with no folders in it.
    Folder {
        open: Option<bool>,
    },
    /// A title (PREFERITI): nothing to choose.
    Heading,
    /// A blank row between two groups.
    Gap,
    /// One of the favourites, by its index among the items of the sidebar.
    Favorite(usize),
    /// The row of the seven tag colours.
    Tags,
    Trash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavRow {
    pub kind: NavKind,
    /// How far the folder is indented.
    pub depth: usize,
    /// Where a click goes (empty for a title and for the row of tags).
    pub path: PathBuf,
    pub name: String,
    /// The number of items in a folder of the tree, once known.
    pub count: Option<usize>,
}

impl NavRow {
    /// Whether the keyboard can stop on this row.
    pub fn selectable(&self) -> bool {
        !matches!(self.kind, NavKind::Heading | NavKind::Gap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_of_the_layouts_parse_in_both_languages() {
        for (word, pref) in [
            ("auto", LayoutPref::Auto),
            ("Panels", LayoutPref::Panels),
            ("pannelli", LayoutPref::Panels),
            ("classic", LayoutPref::Classic),
            (" classico ", LayoutPref::Classic),
            ("explorer", LayoutPref::Classic),
        ] {
            assert_eq!(LayoutPref::parse(word), Some(pref), "{word}");
        }
        assert_eq!(LayoutPref::parse("compact"), None);
        for n in LayoutPref::NAMES {
            assert_eq!(LayoutPref::parse(n).map(LayoutPref::id), Some(n));
        }
        assert_eq!(LayoutPref::default(), LayoutPref::Auto);
    }

    #[test]
    fn the_layout_setting_keeps_its_old_words() {
        use LayoutKind::*;
        assert_eq!(
            resolve_layout("explorer"),
            Some((Explorer, LayoutPref::Classic))
        );
        assert_eq!(
            resolve_layout("compact"),
            Some((Compact, LayoutPref::Classic))
        );
        assert_eq!(resolve_layout("auto"), Some((Explorer, LayoutPref::Auto)));
        assert_eq!(
            resolve_layout("pannelli"),
            Some((Explorer, LayoutPref::Panels))
        );
        assert_eq!(
            resolve_layout("classico"),
            Some((Explorer, LayoutPref::Classic))
        );
        assert_eq!(resolve_layout("wide"), None);
    }

    #[test]
    fn auto_draws_panels_from_140_by_36_and_never_in_a_chooser() {
        use LayoutKind::*;
        let auto = |c: bool, w, h| use_panels(LayoutPref::Auto, Explorer, c, w, h);
        assert!(auto(false, 140, 36));
        assert!(auto(false, 200, 60));
        assert!(!auto(false, 139, 36));
        assert!(!auto(false, 140, 35));
        assert!(!auto(false, 120, 40));
        assert!(!auto(true, 200, 60), "a file chooser is always classic");
        // The compact arrangement is a choice of no panels.
        assert!(!use_panels(LayoutPref::Auto, Compact, false, 200, 60));
        assert!(!use_panels(LayoutPref::Classic, Explorer, false, 200, 60));
        // Asked for, they are drawn in a smaller window too, but not in a tiny one.
        let asked = |w, h| use_panels(LayoutPref::Panels, Explorer, false, w, h);
        assert!(asked(100, 24) && asked(120, 40));
        assert!(!asked(99, 40) && !asked(120, 23));
        assert!(!use_panels(LayoutPref::Panels, Explorer, true, 200, 60));
    }

    #[test]
    fn the_tree_opens_the_way_to_the_current_folder_once() {
        let mut t = Tree::default();
        let home = PathBuf::from("/h");
        t.follow(&home, Path::new("/h/projects/demo"));
        for d in ["/h", "/h/projects"] {
            assert!(t.is_open(Path::new(d)), "{d}");
        }
        assert!(!t.is_open(Path::new("/h/projects/demo")));
        // The person closes a folder: it stays closed while the current folder does not change.
        t.toggle(Path::new("/h/projects"));
        t.follow(&home, Path::new("/h/projects/demo"));
        assert!(!t.is_open(Path::new("/h/projects")));
        // A folder outside home opens only the root.
        let mut t = Tree::default();
        t.follow(&home, Path::new("/etc/x"));
        assert!(t.is_open(&home) && t.open.len() == 1);
    }

    #[test]
    fn each_open_folder_is_asked_about_once_per_listing() {
        let mut t = Tree::default();
        t.follow(Path::new("/h"), Path::new("/h/a"));
        assert_eq!(t.wanted(false, 1), vec![PathBuf::from("/h")]);
        assert!(t.wanted(false, 1).is_empty(), "already asked");
        assert_eq!(t.wanted(false, 2).len(), 1, "the folder was read again");
        // Showing hidden folders is another question.
        t.answer(PathBuf::from("/h"), false, Vec::new());
        assert!(t.kids(Path::new("/h")).is_some());
        assert_eq!(t.wanted(true, 2).len(), 1);
        assert!(t.kids(Path::new("/h")).is_none());
        // An answer to the old question is not kept.
        t.answer(PathBuf::from("/h"), false, Vec::new());
        assert!(t.kids(Path::new("/h")).is_none());
    }
}
