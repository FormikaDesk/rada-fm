//! The bar at the top when it is a text field. One field does three things, told apart by
//! what is typed:
//!
//! - a path (`/etc`, `~/src`, `a/b`): the subfolders of the folder being typed in are offered
//!   and Enter goes there;
//! - a word: the files of the folder are filtered as it is typed, and the commands (and the
//!   places) that match are offered under the field; Enter runs the one picked with the
//!   arrow keys, or keeps the filter;
//! - nothing yet: the placeholder, "Search files or type a command…".
//!
//! Ctrl+L shows the path all selected, so that typing a word replaces it and typing a path
//! does too; Ctrl+P and Ctrl+F start from nothing; Esc goes back to the path.
//!
//! The subfolders come from a worker (`Services::complete_dirs`): this code never reads the
//! disk. What it keeps is the last list it was given and filters it as the text changes.

use std::path::{Component, Path, PathBuf};

use crate::keymap::Action;
use crate::palette::{PaletteItem, PaletteView};
use crossterm::event::KeyCode;

/// Most commands and places offered under the field in the search mode.
pub(super) const MAX_COMMANDS: usize = 5;
pub(super) const MAX_PLACES: usize = 3;

/// What the text of the bar is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarMode {
    /// Nothing typed.
    Empty,
    /// A path to go to.
    Path,
    /// Words to filter the files by and to find a command with.
    Search,
}

/// What a suggestion of the search mode does.
#[derive(Clone, Debug)]
pub enum BarItem {
    /// Run an action, as its key would.
    Command(Action),
    /// Go to a folder.
    Place(PaletteItem),
    /// Go to a folder zoxide knows.
    Zoxide(PathBuf),
}

#[derive(Clone, Debug)]
pub struct BarHit {
    pub item: BarItem,
    /// Matched characters of the label, to pick out.
    pub label_pos: Vec<usize>,
}

/// Whether a text is meant as a path: it starts like one or holds a separator.
pub fn looks_like_path(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('/')
        || t.starts_with('~')
        || t == "."
        || t == ".."
        || t.contains(is_separator)
        || (cfg!(windows)
            && t.chars().nth(1) == Some(':')
            && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
}

/// Whether a command is worth offering in the bar: what is done to files, to the view, to
/// the tabs and to the program; not each step of the cursor.
pub(super) fn offered(a: Action) -> bool {
    use crate::keymap::Group;
    matches!(
        a.group(),
        Group::Files | Group::View | Group::Tabs | Group::Program
    ) || matches!(a, Action::SelectAll)
}

/// Most suggestions shown under the field.
pub const MAX_SUGGESTIONS: usize = 8;

pub struct AddressEdit {
    pub text: String,
    /// Cursor position in chars.
    pub cursor: usize,
    /// Subfolders of `listed`, as last reported.
    pub(super) names: Vec<String>,
    pub(super) listed: Option<PathBuf>,
    /// The folder a listing was last asked for (so typing does not ask again and again).
    pub(super) asked: Option<PathBuf>,
    /// The suggestion highlighted with the arrow keys.
    pub selected: Option<usize>,
    /// The whole text is selected: the first thing typed replaces it (Ctrl+L shows the path
    /// that way), moving the cursor leaves it to be edited.
    pub fresh: bool,
    /// The places the search mode may offer, and the matching done over them.
    pub(super) palette: Option<Box<PaletteView>>,
    /// What the search mode offers under the field, and the one picked with the arrows.
    pub hits: Vec<BarHit>,
    pub hit_selected: Option<usize>,
    /// The bar put the filter on the folder (and takes it off when it is left empty).
    pub filtering: bool,
    /// The folders zoxide gave for the text it was asked about (empty until it answers).
    pub(super) zoxide: Vec<PathBuf>,
    /// The text zoxide was last asked about, so that typing does not ask again and again.
    pub(super) zoxide_asked: String,
}

impl AddressEdit {
    pub fn new(text: String) -> AddressEdit {
        let cursor = text.chars().count();
        AddressEdit {
            text,
            cursor,
            names: Vec::new(),
            listed: None,
            asked: None,
            selected: None,
            fresh: false,
            palette: None,
            hits: Vec::new(),
            hit_selected: None,
            filtering: false,
            zoxide: Vec::new(),
            zoxide_asked: String::new(),
        }
    }

    pub fn mode(&self) -> BarMode {
        let t = self.text.trim();
        if t.is_empty() {
            BarMode::Empty
        } else if looks_like_path(t) {
            BarMode::Path
        } else {
            BarMode::Search
        }
    }

    /// A typed character: it replaces the text when that was all selected.
    pub(super) fn type_char(&mut self, c: char) {
        if self.fresh {
            self.text.clear();
            self.cursor = 0;
            self.fresh = false;
        }
        self.insert(c);
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
        self.selected = None;
    }

    pub(super) fn backspace(&mut self) {
        if self.fresh {
            self.text.clear();
            self.cursor = 0;
            self.fresh = false;
            self.selected = None;
            return;
        }
        if self.cursor > 0 {
            let at = self.byte_at(self.cursor - 1);
            self.text.remove(at);
            self.cursor -= 1;
            self.selected = None;
        }
    }

    pub(super) fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
            self.selected = None;
        }
    }

    /// Enter on a highlighted name: the path takes it before it is submitted.
    pub(super) fn choose(&mut self, sugg: &[String]) {
        if let Some(name) = self.selected.and_then(|i| sugg.get(i)) {
            let text = self.with_name(name);
            self.text = text;
        }
    }

    /// Tab: the highlighted name, the only one there is, or as much as they all share.
    pub(super) fn complete(&mut self, sugg: &[String]) {
        let pick = self
            .selected
            .and_then(|i| sugg.get(i))
            .cloned()
            .or_else(|| {
                if sugg.len() == 1 {
                    sugg.first().cloned()
                } else {
                    None
                }
            });
        match pick {
            Some(name) => {
                self.text = self.with_name(&name);
                self.cursor = self.text.chars().count();
            }
            None => {
                // Several names: as much as they all share.
                let common = common_prefix(sugg);
                let cut = self.text.rfind(is_separator).map_or(0, |i| i + 1);
                if common.chars().count() > self.text[cut..].chars().count() {
                    self.text = format!("{}{}", &self.text[..cut], common);
                    self.cursor = self.text.chars().count();
                }
            }
        }
        self.selected = None;
    }

    /// Tab in the search mode: the next of the hits.
    pub(super) fn next_hit(&mut self) {
        let n = self.hits.len();
        if n > 0 {
            self.hit_selected = Some(self.hit_selected.map_or(0, |i| (i + 1) % n));
        }
    }

    /// The arrows in the search mode, when there are hits: the one above or below.
    pub(super) fn step_hit(&mut self, down: bool) {
        let n = self.hits.len();
        self.hit_selected = match (self.hit_selected, down) {
            (None, true) => Some(0),
            (None, false) => Some(n - 1),
            (Some(i), true) => Some((i + 1) % n),
            (Some(i), false) => Some((i + n - 1) % n),
        };
    }

    /// The arrows in the path mode: the suggestion below or above.
    pub(super) fn step_suggestion(&mut self, sugg: &[String], down: bool) {
        let n = sugg.len();
        if n == 0 {
            return;
        }
        self.selected = Some(match (self.selected, down) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        });
    }

    /// Left, Right, Home and End: the text is no longer all selected.
    pub(super) fn move_cursor(&mut self, code: KeyCode) {
        self.fresh = false;
        self.cursor = match code {
            KeyCode::Left => self.cursor.saturating_sub(1),
            KeyCode::Right => (self.cursor + 1).min(self.text.chars().count()),
            KeyCode::Home => 0,
            _ => self.text.chars().count(),
        };
    }

    pub(super) fn clear_line(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selected = None;
        self.fresh = false;
    }

    /// The folder whose subfolders are being completed, and the start of the name typed in it.
    pub(super) fn target(&self, cwd: &Path, home: &Path) -> (PathBuf, String) {
        match self.text.rfind(is_separator) {
            Some(i) => {
                let dir_text = &self.text[..=i];
                (expand(dir_text, cwd, home), self.text[i + 1..].to_string())
            }
            None => (cwd.to_path_buf(), self.text.clone()),
        }
    }

    /// Names that continue what is typed, best first.
    pub fn suggestions(&self, cwd: &Path, home: &Path) -> Vec<String> {
        let (dir, prefix) = self.target(cwd, home);
        if self.listed.as_deref() != Some(dir.as_path()) {
            return Vec::new();
        }
        let lower = prefix.to_lowercase();
        let hidden_ok = prefix.starts_with('.');
        let mut out: Vec<&String> = self
            .names
            .iter()
            .filter(|n| (hidden_ok || !n.starts_with('.')) && n.to_lowercase().starts_with(&lower))
            .collect();
        // What matches with the case as typed comes first.
        out.sort_by_key(|n| !n.starts_with(&prefix));
        out.into_iter().take(MAX_SUGGESTIONS).cloned().collect()
    }

    /// The text with the name being typed replaced by `name` and a separator after it.
    pub(super) fn with_name(&self, name: &str) -> String {
        let cut = self.text.rfind(is_separator).map_or(0, |i| i + 1);
        format!("{}{}{}", &self.text[..cut], name, SEP)
    }
}

pub(super) const SEP: char = '/';

pub(super) fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// Where typed text leads: `~` is the home folder, a relative path starts at the folder
/// being looked at, and `.` and `..` are resolved by the text alone (no link is followed).
pub fn expand(text: &str, cwd: &Path, home: &Path) -> PathBuf {
    let text = text.trim();
    let raw = if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text
        .strip_prefix("~/")
        .or_else(|| text.strip_prefix("~\\").filter(|_| cfg!(windows)))
    {
        home.join(rest)
    } else {
        let p = PathBuf::from(text);
        if p.is_absolute() { p } else { cwd.join(p) }
    };
    let mut out = PathBuf::new();
    for c in raw.components() {
        match c {
            Component::ParentDir => {
                // Never above the root.
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir) | None
                ) {
                    out.pop();
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(std::path::MAIN_SEPARATOR_STR);
    }
    out
}

fn common_prefix(names: &[String]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut len = first.chars().count();
    for n in &names[1..] {
        len = len.min(
            first
                .chars()
                .zip(n.chars())
                .take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
                .count(),
        );
    }
    first.chars().take(len).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_paths_are_resolved_by_their_text() {
        let (cwd, home) = (Path::new("/home/u/projects"), Path::new("/home/u"));
        assert_eq!(expand("~", cwd, home), PathBuf::from("/home/u"));
        assert_eq!(expand("~/docs", cwd, home), PathBuf::from("/home/u/docs"));
        assert_eq!(expand("/etc/", cwd, home), PathBuf::from("/etc"));
        assert_eq!(
            expand("demo", cwd, home),
            PathBuf::from("/home/u/projects/demo")
        );
        assert_eq!(expand("..", cwd, home), PathBuf::from("/home/u"));
        assert_eq!(expand("../../..", cwd, home), PathBuf::from("/"));
        assert_eq!(expand("../../../../..", cwd, home), PathBuf::from("/"));
        assert_eq!(
            expand("./a/./b/../c", cwd, home),
            PathBuf::from("/home/u/projects/a/c")
        );
        assert_eq!(expand("  /tmp  ", cwd, home), PathBuf::from("/tmp"));
    }

    #[test]
    fn suggestions_follow_the_name_being_typed() {
        let (cwd, home) = (Path::new("/home/u"), Path::new("/home/u"));
        let mut a = AddressEdit::new("~/pr".to_string());
        a.names = ["projects", "Pictures", ".private", "docs", "prints"]
            .map(String::from)
            .to_vec();
        a.listed = Some(PathBuf::from("/home/u"));
        // Hidden folders only when asked for with a dot; case is ignored but exact case first.
        assert_eq!(a.suggestions(cwd, home), ["projects", "prints"]);
        a.text = "~/P".into();
        assert_eq!(a.suggestions(cwd, home), ["Pictures", "projects", "prints"]);
        a.text = "~/.".into();
        assert_eq!(a.suggestions(cwd, home), [".private"]);
        // A list for another folder is no use for this text.
        a.text = "/etc/".into();
        assert!(a.suggestions(cwd, home).is_empty());
    }

    #[test]
    fn a_chosen_name_replaces_the_one_being_typed() {
        let a = AddressEdit::new("~/projects/de".to_string());
        assert_eq!(a.with_name("demo"), "~/projects/demo/");
        let b = AddressEdit::new("de".to_string());
        assert_eq!(b.with_name("demo"), "demo/");
    }

    #[test]
    fn what_the_names_share_is_what_tab_completes() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(common_prefix(&n(&["projects", "Prints", "prisma"])), "pr");
        assert_eq!(common_prefix(&n(&["alpha"])), "alpha");
        assert_eq!(common_prefix(&[]), "");
    }
}
