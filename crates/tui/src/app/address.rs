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

use super::{App, FilterState};
use crate::keymap::Action;
use crate::palette::{PaletteItem, PaletteKind, PaletteView};
use crate::{fmt, fuzzy};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Most commands and places offered under the field in the search mode.
const MAX_COMMANDS: usize = 5;
const MAX_PLACES: usize = 3;

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
fn offered(a: Action) -> bool {
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
    names: Vec<String>,
    listed: Option<PathBuf>,
    /// The folder a listing was last asked for (so typing does not ask again and again).
    asked: Option<PathBuf>,
    /// The suggestion highlighted with the arrow keys.
    pub selected: Option<usize>,
    /// The whole text is selected: the first thing typed replaces it (Ctrl+L shows the path
    /// that way), moving the cursor leaves it to be edited.
    pub fresh: bool,
    /// The places the search mode may offer, and the matching done over them.
    palette: Option<Box<PaletteView>>,
    /// What the search mode offers under the field, and the one picked with the arrows.
    pub hits: Vec<BarHit>,
    pub hit_selected: Option<usize>,
    /// The bar put the filter on the folder (and takes it off when it is left empty).
    pub filtering: bool,
    /// The folders zoxide gave for the text it was asked about (empty until it answers).
    zoxide: Vec<PathBuf>,
    /// The text zoxide was last asked about, so that typing does not ask again and again.
    zoxide_asked: String,
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
    fn type_char(&mut self, c: char) {
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

    fn backspace(&mut self) {
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

    fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
            self.selected = None;
        }
    }

    /// The folder whose subfolders are being completed, and the start of the name typed in it.
    fn target(&self, cwd: &Path, home: &Path) -> (PathBuf, String) {
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
    fn with_name(&self, name: &str) -> String {
        let cut = self.text.rfind(is_separator).map_or(0, |i| i + 1);
        format!("{}{}{}", &self.text[..cut], name, SEP)
    }
}

const SEP: char = '/';

fn is_separator(c: char) -> bool {
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

impl App {
    /// Turn the address bar into a text field holding the current path.
    pub fn start_address_edit(&mut self) {
        let mut text = fmt::short_path(&self.address_base(), &self.svc.home);
        if !text.ends_with(is_separator) {
            text.push(SEP);
        }
        let mut edit = AddressEdit::new(text);
        edit.fresh = true;
        self.windows.address = Some(edit);
        self.filter_editing_off();
        self.address_changed();
    }

    /// Ctrl+P and Ctrl+F: the bar from nothing (or from the filter in force), ready for a
    /// word or a path.
    pub fn start_bar(&mut self, keep_filter: bool) {
        let text = if keep_filter {
            self.tab
                .filter
                .as_ref()
                .map(|f| f.text.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let mut edit = AddressEdit::new(text);
        edit.palette = Some(Box::new(PaletteView::new(
            self.palette_items(),
            self.svc.home.clone(),
        )));
        // A filter already on the folder is the bar's to take off.
        edit.filtering = keep_filter && self.tab.filter.is_some();
        self.windows.address = Some(edit);
        self.filter_editing_off();
        self.bar_changed();
    }

    /// The folder shown in the address bar: inside an archive, the archive's own path.
    fn address_base(&self) -> PathBuf {
        self.tab.cwd.clone()
    }

    fn filter_editing_off(&mut self) {
        if let Some(f) = &mut self.tab.filter {
            f.editing = false;
        }
    }

    pub fn cancel_address_edit(&mut self) {
        let filtering = self.windows.address.take().is_some_and(|a| a.filtering);
        if filtering {
            self.set_filter(None);
        }
        self.redraw.dirty = true;
    }

    /// The text changed: in a path, ask for its subfolders; in words, filter the files and
    /// work out the commands and places to offer.
    pub(super) fn bar_changed(&mut self) {
        self.address_changed();
        let Some(a) = &self.windows.address else {
            return;
        };
        match a.mode() {
            BarMode::Search => {
                let text = a.text.trim().to_string();
                self.set_filter(Some(FilterState {
                    text,
                    editing: false,
                }));
                if let Some(a) = &mut self.windows.address {
                    a.filtering = true;
                }
                self.refresh_bar_hits();
            }
            _ => {
                let was = a.filtering;
                if let Some(a) = &mut self.windows.address {
                    a.hits.clear();
                    a.hit_selected = None;
                    a.filtering = false;
                }
                if was {
                    self.set_filter(None);
                }
            }
        }
    }

    /// The commands and the places that match what is typed, best first.
    fn refresh_bar_hits(&mut self) {
        let chooser = self.session.chooser.clone();
        let Some(a) = &mut self.windows.address else {
            return;
        };
        let q = a.text.trim().to_string();
        let q_text = q.clone();
        // An entry of the folder with exactly that name is a path that exists.
        let named_here = self
            .tab
            .listing
            .all()
            .iter()
            .any(|e| e.name == q_text.as_str());
        let mut commands: Vec<(i64, BarHit)> = Vec::new();
        for &action in Action::ALL
            .iter()
            .filter(|a| offered(**a) && chooser.as_ref().is_none_or(|c| c.allows(**a)))
        {
            let label = action.label();
            let english = action.label_key().in_lang(rada_core::i18n::Lang::En);
            let best = match (fuzzy::score(&q, label), fuzzy::score(&q, english)) {
                (Some((a, pos)), Some((b, _))) if a >= b => Some((a, pos)),
                (_, Some((b, _))) => Some((b, Vec::new())),
                (Some((a, pos)), None) => Some((a, pos)),
                (None, None) => None,
            };
            if let Some((score, label_pos)) = best {
                commands.push((
                    score,
                    BarHit {
                        item: BarItem::Command(action),
                        label_pos,
                    },
                ));
            }
        }
        commands.sort_by_key(|c| std::cmp::Reverse(c.0));
        let mut hits: Vec<BarHit> = commands
            .into_iter()
            .take(MAX_COMMANDS)
            .map(|(_, h)| h)
            .collect();
        if let Some(p) = &mut a.palette {
            p.query = q;
            p.cursor = p.query.chars().count();
            p.refilter();
            let places: Vec<BarHit> = p
                .hits
                .iter()
                .filter(|h| {
                    !matches!(
                        p.items[h.item].kind,
                        PaletteKind::Folder | PaletteKind::Typed | PaletteKind::Parent
                    )
                })
                .take(MAX_PLACES)
                .map(|h| BarHit {
                    item: BarItem::Place(p.items[h.item].clone()),
                    label_pos: h.label_pos.clone(),
                })
                .collect();
            hits.extend(places);
        }
        // Nothing among the commands and places, and no entry of this folder by that name:
        // the folders zoxide knows.
        if hits.is_empty() && !named_here {
            if a.zoxide_asked != q_text {
                a.zoxide_asked = q_text.clone();
                a.zoxide.clear();
                self.svc.zoxide_query(&q_text);
            }
            hits.extend(a.zoxide.iter().map(|p| BarHit {
                item: BarItem::Zoxide(p.clone()),
                label_pos: Vec::new(),
            }));
        } else {
            a.zoxide.clear();
            a.zoxide_asked.clear();
        }
        a.hit_selected = a.hit_selected.filter(|i| *i < hits.len());
        a.hits = hits;
    }

    /// zoxide answered: the folders count if they are for the text still in the bar.
    pub(super) fn on_zoxide(&mut self, query: String, dirs: Vec<PathBuf>) {
        let Some(a) = &mut self.windows.address else {
            return;
        };
        if a.mode() != BarMode::Search || a.text.trim() != query || a.zoxide_asked != query {
            return;
        }
        a.zoxide = dirs;
        self.refresh_bar_hits();
        self.redraw.dirty = true;
    }

    /// Run the `i`-th suggestion of the search mode: the bar closes, the filter it made goes.
    pub(super) fn run_bar_hit(&mut self, i: usize) {
        let Some(a) = self.windows.address.take() else {
            return;
        };
        let Some(hit) = a.hits.get(i).cloned() else {
            return;
        };
        if a.filtering {
            self.set_filter(None);
        }
        match hit.item {
            BarItem::Command(action) => self.dispatch(action),
            BarItem::Place(item) => self.open_dir(item.path),
            BarItem::Zoxide(path) => self.open_dir(path),
        }
    }

    /// Enter in the search mode: run what was picked; else keep the filter if it shows
    /// something; else run the best suggestion; else leave.
    fn bar_enter(&mut self) {
        let Some(a) = &self.windows.address else {
            return;
        };
        if let Some(i) = a.hit_selected {
            return self.run_bar_hit(i);
        }
        if a.text.trim().is_empty() {
            return self.cancel_address_edit();
        }
        if !self.tab.visible.is_empty() {
            // The files that match stay: the bar is done, the filter stays on the folder.
            self.windows.address = None;
            return;
        }
        if !a.hits.is_empty() {
            return self.run_bar_hit(0);
        }
        self.cancel_address_edit();
    }

    /// The text changed: ask for the subfolders of the folder it is in, if not asked yet.
    fn address_changed(&mut self) {
        let (home, cwd) = (self.svc.home.clone(), self.tab.cwd.clone());
        let Some(a) = &mut self.windows.address else {
            return;
        };
        let (dir, _) = a.target(&cwd, &home);
        if a.asked.as_deref() != Some(dir.as_path()) {
            a.asked = Some(dir.clone());
            self.svc.complete_dirs(dir);
        }
    }

    /// A list of subfolders arrived from the worker.
    pub(super) fn on_complete(&mut self, dir: PathBuf, names: Vec<String>) {
        if let Some(a) = &mut self.windows.address
            && a.asked.as_deref() == Some(dir.as_path())
        {
            a.names = names;
            a.listed = Some(dir);
            a.selected = None;
        }
    }

    /// What is offered under the field right now.
    pub fn address_suggestions(&self) -> Vec<String> {
        match &self.windows.address {
            Some(a) => a.suggestions(&self.tab.cwd, &self.svc.home),
            None => Vec::new(),
        }
    }

    /// A suggestion was clicked: go there.
    pub(super) fn pick_suggestion(&mut self, i: usize) {
        let sugg = self.address_suggestions();
        if let (Some(a), Some(name)) = (&mut self.windows.address, sugg.get(i)) {
            a.text = a.with_name(name);
        }
        self.submit_address();
    }

    pub(super) fn on_address_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let sugg = self.address_suggestions();
        let Some(a) = &mut self.windows.address else {
            return;
        };
        match key.code {
            KeyCode::Esc => return self.cancel_address_edit(),
            KeyCode::Enter if a.mode() != BarMode::Path => return self.bar_enter(),
            KeyCode::Enter => {
                if let Some(name) = a.selected.and_then(|i| sugg.get(i)) {
                    let text = a.with_name(name);
                    a.text = text;
                }
                return self.submit_address();
            }
            KeyCode::Tab | KeyCode::BackTab if a.mode() == BarMode::Search => {
                let n = a.hits.len();
                if n > 0 {
                    a.hit_selected = Some(a.hit_selected.map_or(0, |i| (i + 1) % n));
                }
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let pick = a.selected.and_then(|i| sugg.get(i)).cloned().or_else(|| {
                    if sugg.len() == 1 {
                        sugg.first().cloned()
                    } else {
                        None
                    }
                });
                match pick {
                    Some(name) => {
                        a.text = a.with_name(&name);
                        a.cursor = a.text.chars().count();
                    }
                    None => {
                        // Several names: as much as they all share.
                        let common = common_prefix(&sugg);
                        let cut = a.text.rfind(is_separator).map_or(0, |i| i + 1);
                        if common.chars().count() > a.text[cut..].chars().count() {
                            a.text = format!("{}{}", &a.text[..cut], common);
                            a.cursor = a.text.chars().count();
                        }
                    }
                }
                a.selected = None;
            }
            KeyCode::Down | KeyCode::Up if a.mode() == BarMode::Search => {
                let down = key.code == KeyCode::Down;
                let n = a.hits.len();
                if n == 0 {
                    // Nothing to pick: the arrows move through the files that match.
                    return self.dispatch(if down { Action::Down } else { Action::Up });
                }
                a.hit_selected = match (a.hit_selected, down) {
                    (None, true) => Some(0),
                    (None, false) => Some(n - 1),
                    (Some(i), true) => Some((i + 1) % n),
                    (Some(i), false) => Some((i + n - 1) % n),
                };
                return;
            }
            KeyCode::Down => {
                if !sugg.is_empty() {
                    a.selected = Some(a.selected.map_or(0, |i| (i + 1) % sugg.len()));
                }
            }
            KeyCode::Up => {
                if !sugg.is_empty() {
                    a.selected = Some(
                        a.selected
                            .map_or(sugg.len() - 1, |i| (i + sugg.len() - 1) % sugg.len()),
                    );
                }
            }
            KeyCode::Left => {
                a.fresh = false;
                a.cursor = a.cursor.saturating_sub(1)
            }
            KeyCode::Right => {
                a.fresh = false;
                a.cursor = (a.cursor + 1).min(a.text.chars().count())
            }
            KeyCode::Home => {
                a.fresh = false;
                a.cursor = 0
            }
            KeyCode::End => {
                a.fresh = false;
                a.cursor = a.text.chars().count()
            }
            // Backspace with nothing left to delete leaves the bar (as it took the filter off).
            KeyCode::Backspace if a.text.is_empty() && !a.fresh => {
                return self.cancel_address_edit();
            }
            KeyCode::Backspace => a.backspace(),
            KeyCode::Delete => a.delete(),
            KeyCode::Char('u') if ctrl => {
                a.text.clear();
                a.cursor = 0;
                a.selected = None;
                a.fresh = false;
            }
            KeyCode::Char(c) if !ctrl && !alt => a.type_char(c),
            _ => {}
        }
        self.bar_changed();
    }

    /// Go where the text says.
    pub fn submit_address(&mut self) {
        let Some(a) = self.windows.address.take() else {
            return;
        };
        if a.text.trim().is_empty() {
            return;
        }
        let target = expand(&a.text, &self.tab.cwd, &self.svc.home);
        if target != self.tab.cwd {
            self.open_dir(target);
        }
    }
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
