//! The panels view: what it is made of and when it is used. Drawing is in `ui/panels/`; this is
//! the choice between it and the classic layout, the tree of folders of the NAV box and the
//! rows that box lists.
//!
//! The view is one screen of rounded boxes — NAV and SPAZIO on the left, the folder in the
//! middle, ANTEPRIMA and DETTAGLI on the right — under three rows of bars. It needs room: from
//! [`AUTO_FROM`] it has the three columns, from [`TWO_FROM`] two (NAV on the left and the folder,
//! the preview taking the place of NAV when asked), and below that the classic layout is drawn;
//! it all changes by itself when the window is made larger or smaller.

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

/// The layout to start with: what was chosen in the View menu (`saved`, from the remembered
/// state) in place of the configuration's, as for the panes; a remembered word that is not a
/// layout is ignored. `None` when the configuration itself names no layout.
pub fn start_layout(config: Option<&str>, saved: Option<&str>) -> Option<(LayoutKind, LayoutPref)> {
    let (layout, pref) = match config {
        None => (LayoutKind::Explorer, LayoutPref::Auto),
        Some(word) => resolve_layout(word)?,
    };
    let pref = saved.and_then(LayoutPref::parse).unwrap_or(pref);
    Some((layout, pref))
}

/// What the NAV box lists under the places.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavMode {
    /// The places, then only the way to the folder shown: its ancestors and its subfolders.
    #[default]
    Path,
    /// The places of the tree: every folder of Home that is open.
    Tree,
}

impl NavMode {
    pub const NAMES: [&'static str; 2] = ["percorso", "albero"];

    /// The two words, in Italian or English (`path`, `tree`).
    pub fn parse(s: &str) -> Option<NavMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "percorso" | "path" => Some(NavMode::Path),
            "albero" | "tree" => Some(NavMode::Tree),
            _ => None,
        }
    }
}

/// How far the folders of the way in NAV are indented: deeper ones stay at this level, so that
/// the names keep their room.
pub const NAV_INDENT_MAX: usize = 3;
/// The smallest window (columns, rows) in which `auto` draws the three columns of the panels.
pub const AUTO_FROM: (u16, u16) = (140, 36);
/// The smallest window in which `auto` draws the panels at all, as two columns (NAV or the
/// preview, and the folder) while it is smaller than [`AUTO_FROM`].
pub const TWO_FROM: (u16, u16) = (70, 24);
/// The smallest window in which the three columns are drawn when the panels are asked for
/// (`layout = "panels"`); below it, down to [`TWO_FROM`], they are two.
pub const PANELS_THREE_MIN: (u16, u16) = (100, 24);
/// Rows from which the two columns have SPAZIO under NAV.
pub const TWO_SPACE_FROM_ROWS: u16 = 36;
/// Width of the left column of the two-column view: a third of the window, between these.
pub const TWO_LEFT_WIDTH: (u16, u16) = (24, 28);
/// Percent of the window's width the left column of the two-column view takes, before it is held
/// between [`TWO_LEFT_WIDTH`]: the figures of SPAZIO need the room.
pub const TWO_LEFT_PERCENT: u16 = 36;
/// From this many rows every bar has a blank row after it, and the boxes one under their title.
pub const AIRY_FROM: u16 = 40;
/// From this many rows, below [`AIRY_FROM`], one blank row is left under the buttons and one
/// under the status bar; below it every blank row between the bars goes, so that more files are
/// seen.
pub const SOME_AIR_FROM: u16 = 30;

/// How many columns of boxes the panels view has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Columns {
    /// NAV, the folder, ANTEPRIMA.
    Three,
    /// NAV (or ANTEPRIMA in its place) and the folder.
    Two,
}

/// The columns a window `width` × `height` is drawn with, given the person's choice, the
/// arrangement of the classic layout and whether this is a file chooser (which never has
/// panels); `None`: the classic layout.
pub fn columns_for(
    pref: LayoutPref,
    layout: LayoutKind,
    chooser: bool,
    width: u16,
    height: u16,
) -> Option<Columns> {
    if chooser || layout != LayoutKind::Explorer {
        return None;
    }
    let three = match pref {
        LayoutPref::Classic => return None,
        LayoutPref::Panels => PANELS_THREE_MIN,
        LayoutPref::Auto => AUTO_FROM,
    };
    if width >= three.0 && height >= three.1 {
        Some(Columns::Three)
    } else if width >= TWO_FROM.0 && height >= TWO_FROM.1 {
        Some(Columns::Two)
    } else {
        None
    }
}

/// Whether a window `width` × `height` is drawn as panels (see [`columns_for`]).
pub fn use_panels(
    pref: LayoutPref,
    layout: LayoutKind,
    chooser: bool,
    width: u16,
    height: u16,
) -> bool {
    columns_for(pref, layout, chooser, width, height).is_some()
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
    /// "… 12 more": the subfolders the box has no room for. Nothing to choose.
    More,
    /// One of the favourites, by its index among the items of the sidebar.
    Favorite(usize),
    /// A line of tag dots: those from `first` on, `len` of them, of the seven colours and
    /// then the user's own tags (the items of the TAGS section). `more` is how many tags the
    /// box has no room for, on the last line only.
    Tags {
        first: usize,
        len: usize,
        more: usize,
    },
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
    /// Why the folder cannot be read, when it cannot: its row has a sign instead of a count.
    pub unreadable: Option<String>,
}

/// The width of the title (" TAGS ") before the first dot of a line, and of a dot with the
/// space after it.
pub const TAG_TITLE: usize = 6;
const TAG_DOT: usize = 2;
/// How many lines of dots the box has at most.
pub const TAG_LINES_MOST: usize = 3;
/// The dots that are always there: the seven colours.
const TAG_COLOURS: usize = 7;

/// How `total` dots go in a box `width` wide, in `lines` lines at the most: the `(first, len)`
/// of each line, and how many dots are left out. The seven colours are never left out (they
/// take more lines than asked for if the box is narrow), and no dot is cut: one that does not
/// fit goes to the next line. Where some are left out the last line ends with "+N".
pub fn tag_lines(total: usize, width: usize, lines: usize) -> (Vec<(usize, usize)>, usize) {
    // The column of the scroll thumb is not for dots; a dot needs its glyph, not the space.
    let width = if width == 0 {
        TAG_TITLE + TAG_COLOURS * TAG_DOT
    } else {
        width - 1
    };
    let per_line = width.saturating_sub(TAG_TITLE).div_ceil(TAG_DOT).max(1);
    let allowed = lines
        .clamp(1, TAG_LINES_MOST)
        .max(TAG_COLOURS.min(total).div_ceil(per_line));
    let used = total.div_ceil(per_line).clamp(1, allowed);
    let before_last = (used - 1) * per_line;
    let mut last = (total - before_last).min(per_line);
    if total > before_last + last {
        // Room for "+N" after the last dot, without taking one of the seven colours.
        let room = width.saturating_sub(TAG_TITLE);
        let fits = |k: usize| k * TAG_DOT + 1 + (total - before_last - k).to_string().len() <= room;
        last = (0..=per_line)
            .rev()
            .find(|k| fits(*k))
            .unwrap_or(0)
            .max(TAG_COLOURS.saturating_sub(before_last))
            .min(per_line);
    }
    let layout = (0..used)
        .map(|i| (i * per_line, if i + 1 == used { last } else { per_line }))
        .collect();
    (layout, total - before_last - last)
}

impl NavRow {
    /// Whether the keyboard can stop on this row.
    pub fn selectable(&self) -> bool {
        !matches!(self.kind, NavKind::Heading | NavKind::Gap | NavKind::More)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dots on each line, and what is left out.
    fn dots(total: usize, width: usize, lines: usize) -> (Vec<usize>, usize) {
        let (l, more) = tag_lines(total, width, lines);
        (l.into_iter().map(|(_, n)| n).collect(), more)
    }

    #[test]
    fn the_seven_colours_fit_one_line_as_they_always_did() {
        assert_eq!(dots(7, 24, 3), (vec![7], 0));
        assert_eq!(dots(7, 21, 1), (vec![7], 0));
        // 6 for the title, 7 dots of 2: a box of 20 has the last glyph on its last column, and
        // the column of the thumb is for the thumb.
        assert_eq!(dots(7, 20, 1), (vec![7], 0));
        assert_eq!(dots(7, 19, 1), (vec![6, 1], 0));
    }

    #[test]
    fn the_users_tags_follow_on_lines_of_their_own_and_none_is_cut() {
        // 28 wide: 11 dots to a line; 7 colours and 4 of the user's.
        let (layout, more) = tag_lines(11, 28, 3);
        assert_eq!((layout, more), (vec![(0, 11)], 0));
        // Nine of the user's own: a second line, from where the first ends.
        let (layout, more) = tag_lines(16, 24, 3);
        assert_eq!(layout, vec![(0, 9), (9, 7)]);
        assert_eq!(more, 0);
        for width in 8..60 {
            for total in 7..70 {
                for lines in 1..=3 {
                    let (layout, more) = tag_lines(total, width, lines);
                    let shown: usize = layout.iter().map(|(_, n)| n).sum();
                    assert_eq!(shown + more, total, "{total} dots in {width}");
                    assert!(shown >= 7, "the colours stay: {total} in {width}");
                    let per = (width - 1).saturating_sub(6).div_ceil(2).max(1);
                    assert!(layout.len() <= 3.max(7_usize.div_ceil(per)), "{layout:?}");
                    let mut next = 0;
                    for (first, len) in &layout {
                        assert_eq!(*first, next, "lines follow each other");
                        next = first + len;
                        assert!(6 + len * 2 - 1 < width, "{len} dots in {width}");
                    }
                }
            }
        }
    }

    #[test]
    fn what_does_not_fit_is_counted_and_gives_way_to_the_plus() {
        // 24 wide, one line: 9 dots would fit, but two give their place to "+5".
        let (layout, more) = tag_lines(12, 24, 1);
        assert_eq!(layout, vec![(0, 7)]);
        assert_eq!(more, 5);
        // The colours are not given up for it.
        let (layout, more) = tag_lines(30, 20, 1);
        assert_eq!(layout, vec![(0, 7)]);
        assert_eq!(more, 23);
    }

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
    fn the_menu_choice_is_used_in_place_of_the_configuration() {
        use LayoutKind::*;
        // Nothing written, nothing chosen: auto.
        assert_eq!(start_layout(None, None), Some((Explorer, LayoutPref::Auto)));
        // The file says classic; the menu was used to choose panels.
        assert_eq!(
            start_layout(Some("classic"), Some("panels")),
            Some((Explorer, LayoutPref::Panels))
        );
        // A remembered word that means nothing leaves the file's choice.
        assert_eq!(
            start_layout(Some("panels"), Some("sideways")),
            Some((Explorer, LayoutPref::Panels))
        );
        // The compact arrangement stays, whatever the menu said.
        assert_eq!(
            start_layout(Some("compact"), Some("auto")),
            Some((Compact, LayoutPref::Auto))
        );
        // A configuration that names no layout is the only thing that is refused.
        assert_eq!(start_layout(Some("wide"), Some("auto")), None);
    }

    #[test]
    fn auto_draws_three_columns_from_140_by_36_two_from_70_by_24_and_never_in_a_chooser() {
        use LayoutKind::*;
        let cols = |c: bool, w, h| columns_for(LayoutPref::Auto, Explorer, c, w, h);
        assert_eq!(cols(false, 140, 36), Some(Columns::Three));
        assert_eq!(cols(false, 200, 60), Some(Columns::Three));
        assert_eq!(cols(false, 139, 36), Some(Columns::Two));
        assert_eq!(cols(false, 140, 35), Some(Columns::Two));
        assert_eq!(cols(false, 120, 40), Some(Columns::Two));
        assert_eq!(cols(false, 70, 24), Some(Columns::Two));
        assert_eq!(cols(false, 69, 40), None, "too narrow");
        assert_eq!(cols(false, 100, 23), None, "too short");
        assert_eq!(
            cols(true, 200, 60),
            None,
            "a file chooser is always classic"
        );
        let auto = |c: bool, w, h| use_panels(LayoutPref::Auto, Explorer, c, w, h);
        assert!(auto(false, 140, 36) && !auto(false, 69, 36));
        // The compact arrangement is a choice of no panels.
        assert!(!use_panels(LayoutPref::Auto, Compact, false, 200, 60));
        assert!(!use_panels(LayoutPref::Classic, Explorer, false, 200, 60));
        // Asked for, the choice is between three columns and two, and never the classic layout
        // while two fit.
        let asked = |w, h| columns_for(LayoutPref::Panels, Explorer, false, w, h);
        assert_eq!(asked(100, 24), Some(Columns::Three));
        assert_eq!(asked(120, 40), Some(Columns::Three));
        assert_eq!(asked(99, 40), Some(Columns::Two));
        assert_eq!(asked(69, 40), None);
        assert_eq!(asked(120, 23), None);
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
