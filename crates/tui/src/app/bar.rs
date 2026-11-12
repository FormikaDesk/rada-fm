//! The address bar and the search bar as the app drives them: starting and ending the editing,
//! the keys, what the search offers, and where Enter goes.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::address::{MAX_COMMANDS, MAX_PLACES, SEP, expand, is_separator};
use super::{AddressEdit, App, BarHit, BarItem, BarMode, FilterState, ToastKind};
use crate::keymap::Action;
use crate::palette::{PaletteKind, PaletteView};
use crate::registry::{Avail, Surface};
use crate::{fmt, fuzzy};
use rada_core::i18n::Key;

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
            // A command is not a word to filter the files by: only what matches it is offered.
            BarMode::Command => {
                self.bar_stops_filtering(false);
                self.refresh_bar_hits();
            }
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
            _ => self.bar_stops_filtering(true),
        }
    }

    /// The files are no longer filtered by what the bar holds; with `clear`, nothing is
    /// offered under it either.
    fn bar_stops_filtering(&mut self, clear: bool) {
        let Some(a) = &mut self.windows.address else {
            return;
        };
        let was = a.filtering;
        a.filtering = false;
        if clear {
            a.hits.clear();
            a.hit_selected = None;
        }
        if was {
            self.set_filter(None);
        }
    }

    /// The commands and the places that match what is typed, best first.
    pub(super) fn refresh_bar_hits(&mut self) {
        let chooser = self.session.chooser.clone();
        let offered = self.bar_commands();
        let Some(a) = &mut self.windows.address else {
            return;
        };
        let q = words_of(&a.text);
        let q_text = q.clone();
        // An entry of the folder with exactly that name is a path that exists.
        let named_here = self
            .tab
            .listing
            .all()
            .iter()
            .any(|e| e.name == q_text.as_str());
        let mut hits = command_hits(&q, &offered);
        if let Some(p) = &mut a.palette {
            p.query = q;
            p.cursor = p.query.chars().count();
            p.refilter();
            hits.extend(place_hits(p));
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
        // Last, and only when nothing in this folder matches: the arrows keep moving through
        // the files that do, and Enter keeps running the best command.
        let free = chooser.is_none() && a.mode() != BarMode::Command;
        if let Some(hit) = search_hit(&q_text, free, &self.tab) {
            hits.push(hit);
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
            BarItem::Search(words) => self.search_below(&words),
            BarItem::Problem(_) => {}
        }
    }

    /// Ctrl+Enter in the search mode: look for what is typed in the folders below too,
    /// whether or not something in this one matches. Where there is no folder to search in (a
    /// list that is not one, an archive, the choice of a file) it is Enter.
    fn bar_search_below(&mut self) {
        let here = self.session.chooser.is_none()
            && self.tab.archive.is_none()
            && self.tab.virtual_view().is_none();
        let Some(a) = self.windows.address.take_if(|_| here) else {
            return self.bar_enter();
        };
        if a.filtering {
            self.set_filter(None);
        }
        self.search_below(a.text.trim());
    }

    /// List what has `words` in its name in this folder and the ones under it.
    pub(super) fn search_below(&mut self, words: &str) {
        match rada_core::search::view_path(&self.tab.cwd, words) {
            Some(path) => self.open_dir(path),
            None => self.toast(ToastKind::Warn, Key::TSearchUnavailable.tr(), 4),
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
        // The search below is never the "best" suggestion: it takes a choice of its own.
        if a.hits
            .first()
            .is_some_and(|h| !matches!(h.item, BarItem::Search(_)))
        {
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
        a.cycle = None;
        let dir = match a.mode() {
            // On the command line the path is the argument being typed, if it is one.
            BarMode::Command => match super::cmdline::typed_path(&a.text, &cwd, &home) {
                Some((dir, _, _)) => dir,
                None => return,
            },
            _ => a.target(&cwd, &home).0,
        };
        if a.asked.as_deref() != Some(dir.as_path()) {
            a.asked = Some(dir.clone());
            self.svc.complete_dirs(dir);
        }
    }

    /// A list of subfolders arrived from the worker.
    pub(super) fn on_complete(&mut self, dir: PathBuf, names: Vec<String>, files: Vec<String>) {
        if let Some(a) = &mut self.windows.address
            && a.asked.as_deref() == Some(dir.as_path())
        {
            a.names = names;
            a.files = files;
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
            KeyCode::Enter if ctrl && asks_to_search(a) => return self.bar_search_below(),
            KeyCode::Enter if a.mode() != BarMode::Path => return self.bar_enter(),
            KeyCode::Enter => {
                a.choose(&sugg);
                return self.submit_address();
            }
            KeyCode::Tab | KeyCode::BackTab if a.mode() == BarMode::Search => {
                a.next_hit();
                return;
            }
            KeyCode::Tab | KeyCode::BackTab => a.complete(&sugg),
            KeyCode::Down | KeyCode::Up if a.mode() == BarMode::Search => {
                return self.search_arrow(key.code == KeyCode::Down);
            }
            KeyCode::Down => a.step_suggestion(&sugg, true),
            KeyCode::Up => a.step_suggestion(&sugg, false),
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => {
                a.move_cursor(key.code)
            }
            // Backspace with nothing left to delete leaves the bar (as it took the filter off).
            KeyCode::Backspace if a.text.is_empty() && !a.fresh => {
                return self.cancel_address_edit();
            }
            KeyCode::Backspace => a.backspace(),
            KeyCode::Delete => a.delete(),
            KeyCode::Char('u') if ctrl => a.clear_line(),
            KeyCode::Char(c) if !ctrl && !alt => a.type_char(c),
            _ => {}
        }
        self.bar_changed();
    }

    /// An arrow in the search mode: the hit above or below, or, when the bar offers none, the
    /// file above or below.
    fn search_arrow(&mut self, down: bool) {
        let Some(a) = &mut self.windows.address else {
            return;
        };
        if !a.hits.is_empty() {
            a.step_hit(down);
            return;
        }
        // Nothing to pick: the arrows move through the files that match.
        self.dispatch(if down { Action::Down } else { Action::Up });
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

/// Whether Ctrl+Enter on this text is a search: words, or a text between slashes, which reads
/// as a path and is a regular expression.
fn asks_to_search(a: &AddressEdit) -> bool {
    a.mode() == BarMode::Search || rada_core::search::is_regex(a.text.trim())
}

/// The suggestion to search under this folder, when nothing here matches and there is a folder
/// to search in: not in a list that is not one, not inside an archive, not while choosing a
/// file.
fn search_hit(words: &str, free: bool, tab: &super::TabState) -> Option<BarHit> {
    let usable =
        free && tab.visible.is_empty() && tab.archive.is_none() && tab.virtual_view().is_none();
    (usable && rada_core::search::view_path(&tab.cwd, words).is_some()).then(|| BarHit {
        item: BarItem::Search(words.to_string()),
        label_pos: Vec::new(),
    })
}

impl App {
    /// The commands the bar may offer here, in the order of the list of actions.
    pub fn bar_commands(&self) -> Vec<Action> {
        let facts = self.facts(Surface::Bar);
        Action::ALL
            .iter()
            .copied()
            .filter(|a| a.spec().availability(&facts) == Avail::Enabled)
            .collect()
    }
}

/// The words a text of the bar is matched by: on the command line, what comes after the colon.
fn words_of(text: &str) -> String {
    text.trim().trim_start_matches(':').trim().to_string()
}

/// The commands whose names match what is typed (in this language or in English), best first.
fn command_hits(q: &str, offered: &[Action]) -> Vec<BarHit> {
    let mut commands: Vec<(i64, BarHit)> = Vec::new();
    for &action in offered {
        let label = action.label();
        let english = action.label_key().in_lang(rada_core::i18n::Lang::En);
        let best = match (fuzzy::score(q, label), fuzzy::score(q, english)) {
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
    commands
        .into_iter()
        .take(MAX_COMMANDS)
        .map(|(_, h)| h)
        .collect()
}

/// The places among what the palette found for the text, the folders of the list left out.
fn place_hits(p: &PaletteView) -> Vec<BarHit> {
    p.hits
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
        .collect()
}
