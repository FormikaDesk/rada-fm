//! The session that chooses files for another program: what Enter, Esc and a double click do
//! there, and the window that asks for the name of a file to save. What may not be done is
//! decided by [`crate::chooser::Chooser::allows`] and enforced in `dispatch`.

use super::{App, InputKind, InputView, Modal, ToastKind};
use crate::chooser::{self, Kind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::i18n::{Key, tf};
use rada_core::model::Entry;
use std::path::PathBuf;

/// How the session ends, and whether it is a file chooser for another program.
pub struct Session {
    pub should_quit: bool,
    /// Set when this session is a file chooser for another program.
    pub chooser: Option<chooser::Chooser>,
    /// What was chosen, when the person chose: the paths to hand over. `None` is a cancel.
    pub chosen: Option<Vec<PathBuf>>,
}

impl Session {
    pub fn new(chooser: Option<chooser::Chooser>) -> Session {
        Session {
            should_quit: false,
            chooser,
            chosen: None,
        }
    }
}

impl App {
    /// A key that means something only to a chooser. `true` when it was taken.
    pub(super) fn on_chooser_key(&mut self, key: KeyEvent) -> bool {
        let Some(ch) = self.session.chooser.clone() else {
            return false;
        };
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Esc takes away what is in the way first (a filter), then it is "no thanks".
            KeyCode::Esc => {
                if self.filter.is_some() {
                    self.set_filter(None);
                } else {
                    self.session.chosen = None;
                    self.session.should_quit = true;
                }
                true
            }
            KeyCode::Enter if !self.side_nav.focus && !ctrl => {
                self.chooser_enter(alt, true);
                true
            }
            KeyCode::Char('s') if ctrl && ch.is_save() => {
                self.chooser_save_prompt(None);
                true
            }
            _ => false,
        }
    }

    /// Hand the choice over and leave. `false` when it cannot travel (a path with a line
    /// break in it cannot be written one to a line).
    fn chooser_finish(&mut self, paths: Vec<PathBuf>) -> bool {
        if paths.is_empty() {
            return false;
        }
        if paths.iter().any(|p| !chooser::fits_a_line(p)) {
            self.toast(ToastKind::Error, Key::ChooseLineBreak.tr(), 4);
            return false;
        }
        self.session.chosen = Some(paths);
        self.session.should_quit = true;
        true
    }

    /// The folder being shown, when it is a real one a choice can be about.
    fn chooser_here(&self) -> Option<PathBuf> {
        (self.archive.is_none() && self.tag_view().is_none() && !self.recents_view())
            .then(|| self.cwd.clone())
    }

    /// The marked entries that satisfy `keep`, as paths.
    fn marked_paths(&self, keep: impl Fn(&Entry) -> bool) -> Vec<PathBuf> {
        self.listing
            .all()
            .iter()
            .filter(|e| self.marked.contains(&e.name) && keep(e))
            .map(|e| e.path.clone())
            .collect()
    }

    /// Enter (`by_key`) or a double click on the item under the cursor. `here` (Alt+Enter) is
    /// "the folder I am in".
    pub(super) fn chooser_enter(&mut self, here: bool, by_key: bool) {
        let Some(ch) = self.session.chooser.clone() else {
            return;
        };
        let under = self
            .current()
            .map(|e| (e.path.clone(), e.is_dir(), e.is_navigable_dir()));
        match ch.kind {
            Kind::File => {
                if by_key && ch.multiple && !self.marked.is_empty() {
                    let files = self.marked_paths(|e| !e.is_dir() && e.error.is_none());
                    if files.is_empty() {
                        self.toast(ToastKind::Info, Key::ChooseOnlyFiles.tr(), 3);
                    } else {
                        self.chooser_finish(files);
                    }
                    return;
                }
                match under {
                    Some((path, _, true)) => self.open_dir(path),
                    Some((_, true, false)) => {}
                    Some((path, false, _)) => {
                        if self.current().is_some_and(|e| e.error.is_none())
                            && self.archive.is_none()
                        {
                            self.chooser_finish(vec![path]);
                        }
                    }
                    None => {
                        if ch.multiple && by_key {
                            self.toast(ToastKind::Info, Key::ChooseNothingMarked.tr(), 3);
                        }
                    }
                }
            }
            Kind::Folder => {
                // A double click goes into a folder, as it does everywhere.
                if !by_key {
                    if let Some((path, _, true)) = under {
                        self.open_dir(path);
                    }
                    return;
                }
                if ch.multiple && !here && !self.marked.is_empty() {
                    let dirs = self.marked_paths(|e| e.is_dir());
                    self.chooser_finish(dirs);
                    return;
                }
                let picked = match under {
                    Some((path, true, _)) if !here => Some(path),
                    _ => self.chooser_here(),
                };
                if let Some(path) = picked {
                    self.chooser_finish(vec![path]);
                } else {
                    self.toast(ToastKind::Info, Key::ChooseRefused.tr(), 3);
                }
            }
            Kind::Save { .. } => {
                if here {
                    self.chooser_save_prompt(None);
                    return;
                }
                match under {
                    Some((path, _, true)) => self.open_dir(path),
                    Some((_, true, false)) => {}
                    // A file under the cursor: saving over it is what Enter on it means,
                    // and the name is asked for with its own name filled in.
                    Some((path, false, _)) => {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                        self.chooser_save_prompt(name);
                    }
                    None => self.chooser_save_prompt(None),
                }
            }
        }
    }

    /// The right arrow: into the folder under the cursor. A file is not opened, ever.
    pub(super) fn chooser_navigate(&mut self) {
        if let Some(path) = self
            .current()
            .filter(|e| e.is_navigable_dir())
            .map(|e| e.path.clone())
        {
            self.open_dir(path);
        }
    }

    /// Space on an entry, where a file chooser takes only files.
    pub(super) fn chooser_may_mark(&mut self) -> bool {
        let is_dir = self.current().is_some_and(|e| e.is_dir());
        if is_dir
            && self
                .session
                .chooser
                .as_ref()
                .is_some_and(|c| c.kind == Kind::File)
        {
            self.toast(ToastKind::Info, Key::ChooseOnlyFiles.tr(), 3);
            self.set_cursor(self.cursor + 1);
            return false;
        }
        true
    }

    /// The window that asks for the name of the file to save, in the folder being shown.
    fn chooser_save_prompt(&mut self, name: Option<String>) {
        let Some(Kind::Save { name: suggested }) =
            self.session.chooser.as_ref().map(|c| c.kind.clone())
        else {
            return;
        };
        if self.chooser_here().is_none() {
            self.toast(ToastKind::Info, Key::ChooseNoParent.tr(), 3);
            return;
        }
        let text = name.unwrap_or(suggested);
        self.windows.modal = Some(Modal::Input(InputView::new(InputKind::SaveAs, text)));
    }

    /// The name was typed: check it, ask once before replacing a file, and hand it over.
    pub(super) fn submit_save_as(&mut self, mut iv: InputView) {
        let name = iv.text.trim_end_matches('\n').to_string();
        if let Err(why) = chooser::check_name(&name) {
            iv.error = Some(why.tr().to_string());
            self.windows.modal = Some(Modal::Input(iv));
            return;
        }
        let Some(dir) = self.chooser_here() else {
            return;
        };
        let target = dir.join(&name);
        match std::fs::symlink_metadata(&target) {
            Ok(m) if m.is_dir() => {
                iv.error = Some(tf(Key::ChooseIsFolder, &[&name]));
                self.windows.modal = Some(Modal::Input(iv));
                return;
            }
            // The first Enter warns, a second one with the same name goes ahead.
            Ok(_) if iv.error.is_none() => {
                iv.error = Some(tf(Key::ChooseExists, &[&name]));
                self.windows.modal = Some(Modal::Input(iv));
                return;
            }
            _ => {}
        }
        self.chooser_finish(vec![target]);
    }
}
