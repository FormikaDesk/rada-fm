//! "Open with…": the programs installed on the system that can open the file, the system's
//! choice first and those used lately after it, filtered by what is typed, with "Other
//! program…" (the free field) at the end. "Always use this for this kind of file" is kept in
//! rada's own state and never touches the system.

use rada_core::platform::openwith::{self as ow, OpenApp, Prefs};

use super::{App, InputKind, InputView, MenuView, Modal, ToastKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::events::CoreEvent;
use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};

pub struct OpenWithView {
    pub path: PathBuf,
    /// The name of the file, for the title.
    pub file: String,
    /// What the kind of this file is called to the system (a MIME type, an extension).
    pub kind: Option<String>,
    /// All the programs, in the order to list them.
    pub apps: Vec<OpenApp>,
    /// The program chosen to be used always for this kind of file.
    pub always_id: Option<String>,
    /// The programs are still being looked for.
    pub loading: bool,
    pub filter: String,
    pub cursor: usize,
    /// Index into [`rows`](Self::rows).
    pub selected: usize,
    /// The box "always use this for this kind of file", for the highlighted program.
    pub always: bool,
    /// The menu this list was opened from ("Open with ›" of the context menu): it stays in
    /// view behind the list, and Left or Esc gives it back.
    pub under: Option<Box<MenuView>>,
}

/// A line of the list.
pub enum OpenRow<'a> {
    App(&'a OpenApp),
    /// The free field of a command, as it always was.
    Other,
}

impl OpenWithView {
    fn new(path: PathBuf) -> OpenWithView {
        let file = path
            .file_name()
            .map(rada_core::display::name)
            .unwrap_or_default();
        OpenWithView {
            path,
            file,
            kind: None,
            apps: Vec::new(),
            always_id: None,
            loading: true,
            filter: String::new(),
            cursor: 0,
            selected: 0,
            always: false,
            under: None,
        }
    }

    /// The programs that match what is typed, then "Other program…".
    pub fn rows(&self) -> Vec<OpenRow<'_>> {
        let mut rows: Vec<OpenRow> = ow::filtered(&self.apps, &self.filter)
            .into_iter()
            .map(OpenRow::App)
            .collect();
        rows.push(OpenRow::Other);
        rows
    }

    /// The program under the cursor, if the cursor is on one.
    pub fn current(&self) -> Option<&OpenApp> {
        match self.rows().get(self.selected) {
            Some(OpenRow::App(a)) => Some(a),
            _ => None,
        }
    }

    pub fn select(&mut self, i: usize) {
        self.selected = i.min(self.rows().len().saturating_sub(1));
        self.sync_always();
    }

    pub fn move_by(&mut self, d: isize) {
        let last = self.rows().len().saturating_sub(1);
        self.selected = if d < 0 {
            self.selected.saturating_sub(d.unsigned_abs())
        } else {
            (self.selected + d as usize).min(last)
        };
        self.sync_always();
    }

    /// The box shows whether the program under the cursor is the one chosen for always.
    fn sync_always(&mut self) {
        self.always = match (self.current(), &self.always_id) {
            (Some(a), Some(id)) => &a.id == id,
            _ => false,
        };
    }

    pub fn toggle_always(&mut self) {
        if self.current().is_some() {
            self.always = !self.always;
        }
    }

    fn filter_changed(&mut self) {
        self.selected = 0;
        self.sync_always();
    }

    fn insert(&mut self, c: char) {
        let at = self
            .filter
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.filter.len());
        self.filter.insert(at, c);
        self.cursor += 1;
        self.filter_changed();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let at = self
            .filter
            .char_indices()
            .nth(self.cursor - 1)
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.filter.remove(at);
        self.cursor -= 1;
        self.filter_changed();
    }
}

/// Open `path` with the program chosen to be used always for its kind of file, if there is one
/// and it is still installed. `true`: done (or failed with a note), nothing more to do. Runs on
/// a thread of its own: it reads the disk.
pub(super) fn open_with_chosen(
    platform: &dyn rada_core::platform::Platform,
    state: &Path,
    path: &Path,
) -> bool {
    let prefs = Prefs::load(state);
    if prefs.always.is_empty() {
        return false;
    }
    let opener = platform.opener();
    let Some(id) = opener
        .kind_of(path)
        .and_then(|k| prefs.always.get(&k).cloned())
    else {
        return false;
    };
    let Some(app) = opener.apps_for(path).apps.into_iter().find(|a| a.id == id) else {
        return false;
    };
    if opener.open_with_app(path, &app).is_err() {
        return false;
    }
    let mut prefs = prefs;
    prefs.used(&app.id);
    let _ = prefs.save(state);
    true
}

impl App {
    /// The window, at once, and the programs when the worker has found them.
    pub(super) fn open_with_window(&mut self, path: PathBuf, under: Option<Box<MenuView>>) {
        let mut view = OpenWithView::new(path.clone());
        view.under = under;
        self.windows.modal = Some(Modal::OpenWith(Box::new(view)));
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        let state = platform.dirs().rada_state();
        std::thread::spawn(move || {
            let choices = platform.opener().apps_for(&path);
            let prefs = Prefs::load(&state);
            let kind = choices.kind.clone().unwrap_or_default();
            let apps = ow::ordered(choices.apps, &prefs, &kind);
            let always = prefs.always.get(&kind).cloned();
            let _ = notes.send(CoreEvent::OpenChoices {
                path,
                kind: choices.kind,
                apps,
                always,
            });
        });
    }

    pub(super) fn on_open_choices(
        &mut self,
        path: PathBuf,
        kind: Option<String>,
        apps: Vec<OpenApp>,
        always: Option<String>,
    ) {
        let Some(Modal::OpenWith(v)) = &mut self.windows.modal else {
            return;
        };
        if v.path != path {
            return;
        }
        v.kind = kind;
        v.apps = apps;
        v.always_id = always;
        v.loading = false;
        v.selected = 0;
        v.sync_always();
    }

    pub(super) fn on_open_with_key(&mut self, mut v: Box<OpenWithView>, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Esc (or Left, with nothing typed) closes the list; if it came from the context
            // menu, that is given back.
            KeyCode::Esc => return self.give_back_menu(&mut v),
            KeyCode::Left if v.filter.is_empty() && v.under.is_some() => {
                return self.give_back_menu(&mut v);
            }
            KeyCode::Enter => return self.open_with_enter(v),
            KeyCode::Down => v.move_by(1),
            KeyCode::Up => v.move_by(-1),
            KeyCode::PageDown => v.move_by(8),
            KeyCode::PageUp => v.move_by(-8),
            KeyCode::Char('n' | 'j') if ctrl => v.move_by(1),
            KeyCode::Char('p' | 'k') if ctrl => v.move_by(-1),
            KeyCode::Tab => v.toggle_always(),
            KeyCode::Char('u') if ctrl => {
                v.filter.clear();
                v.cursor = 0;
                v.filter_changed();
            }
            KeyCode::Backspace => v.backspace(),
            KeyCode::Char(c) if !ctrl => v.insert(c),
            _ => {}
        }
        self.windows.modal = Some(Modal::OpenWith(v));
    }

    fn give_back_menu(&mut self, v: &mut OpenWithView) {
        if let Some(m) = v.under.take() {
            self.windows.modal = Some(Modal::Menu(*m));
        }
    }

    /// Enter: open with the program chosen, or ask for another one.
    fn open_with_enter(&mut self, v: Box<OpenWithView>) {
        match v.current().cloned() {
            Some(app) => self.launch_chosen(&v, app),
            None => {
                // "Other program…": the field of a command, with what was typed.
                let mut iv = InputView::new(
                    InputKind::OpenWith {
                        path: v.path.clone(),
                    },
                    v.filter.clone(),
                );
                iv.cursor = iv.text.chars().count();
                self.windows.modal = Some(Modal::Input(iv));
            }
        }
    }

    /// Start the program, and remember the choice: used lately, and, as the box says, always (or
    /// no longer always) for this kind of file.
    fn launch_chosen(&mut self, v: &OpenWithView, app: OpenApp) {
        let (path, kind, always) = (v.path.clone(), v.kind.clone(), v.always);
        let was_always = v.always_id.as_deref() == Some(app.id.as_str());
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        let state = platform.dirs().rada_state();
        let name = app.name.clone();
        self.redraw.after_launch();
        std::thread::spawn(move || {
            if let Err(e) = platform.opener().open_with_app(&path, &app) {
                let _ = notes.send(CoreEvent::Note {
                    text: format!("cannot open with {}: {e}", app.name),
                    error: true,
                });
                return;
            }
            let mut prefs = Prefs::load(&state);
            prefs.used(&app.id);
            if let Some(kind) = kind {
                if always {
                    prefs.always.insert(kind, app.id.clone());
                } else if was_always {
                    prefs.always.remove(&kind);
                }
            }
            if let Err(e) = prefs.save(&state) {
                tracing::warn!("cannot save the open-with choices: {e}");
            }
        });
        let note = match (always, was_always) {
            (true, false) => Key::TOpeningWithNote1.tr(),
            (false, true) => Key::TOpeningWithNote2.tr(),
            _ => "",
        };
        self.toast(
            ToastKind::Info,
            tf(Key::TOpeningWith, &[&v.file, &name, &note]),
            3,
        );
    }
}
