//! Going to another folder: opening, back, up, entering what is under the cursor, and the refusals that belong to a list that is not a folder.

use std::path::{Path, PathBuf};

use rada_core::events::CoreEvent;
use rada_core::i18n::{Key, tf};

use super::{App, ToastKind, openwith};
use crate::keymap::Action;
use crate::nav::NavMove;

impl App {
    pub fn open_dir(&mut self, path: PathBuf) {
        // A device that is plugged in and not mounted is not a folder yet: opening it mounts it.
        if let Some(id) = self.devices.unmounted_id(&path) {
            self.start_mount(&id);
            return;
        }
        if let Some(name) = self.tab.cursor_name() {
            self.tab.remembered.insert(self.tab.cwd.clone(), name);
        }
        self.request_dir(path);
    }

    /// Back to the previous folder of the history (or forward to the next one).
    pub(super) fn go_history(&mut self, back: bool) {
        let target = if back {
            self.tab.nav.back_target()
        } else {
            self.tab.nav.forward_target()
        };
        let Some(target) = target.map(Path::to_path_buf) else {
            return;
        };
        self.tab.nav_move = Some(if back {
            NavMove::Back(target.clone())
        } else {
            NavMove::Forward(target.clone())
        });
        self.open_dir(target);
    }

    pub(super) fn go_parent(&mut self) {
        // A tag's items have no parent: leave the way one came.
        if self.tab.tag_view().is_some() {
            if self.tab.nav.back_target().is_some() {
                self.go_history(true);
            } else {
                self.request_dir(self.svc.home.clone());
            }
            return;
        }
        let Some(parent) = self.tab.cwd.parent().map(|p| p.to_path_buf()) else {
            self.toast(ToastKind::Info, Key::TAtTop.tr(), 2);
            return;
        };
        if let Some(name) = self.tab.cwd.file_name() {
            self.tab
                .remembered
                .insert(parent.clone(), name.to_os_string());
        }
        self.request_dir(parent);
    }

    pub(super) fn enter(&mut self) {
        if self.session.chooser.is_some() {
            return self.chooser_enter(false, false);
        }
        let Some(e) = self.tab.current() else { return };
        if e.is_navigable_dir() {
            let target = e.path.clone();
            self.open_dir(target);
        } else if self.tab.archive.is_some() {
            self.say_archive_is_read_only();
        } else if self.archive_candidate(e) {
            let target = e.path.clone();
            self.open_dir(target);
        } else if e.error.is_none() {
            let path = e.path.clone();
            self.open_with_system(path);
        }
    }

    /// Enter on a file inside an archive: it is not opened, it is copied or extracted.
    fn say_archive_is_read_only(&mut self) {
        self.toast(
            ToastKind::Info,
            tf(
                Key::TInArchive,
                &[
                    &self
                        .key_for(Action::Copy)
                        .unwrap_or_else(|| Key::KeyCopyWord.tr().into()),
                    &self
                        .key_for(Action::ExtractHere)
                        .unwrap_or_else(|| Key::KeyExtractWord.tr().into()),
                ],
            ),
            4,
        );
    }

    /// Open a file with the program the system, or the person's "Open with…", has for it.
    fn open_with_system(&mut self, path: PathBuf) {
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        let state = platform.dirs().rada_state();
        // Spawning a process is I/O: keep it off the UI thread.
        self.redraw.after_launch();
        std::thread::spawn(move || {
            // A program chosen in "Open with…" to be used always for this kind of file
            // takes the place of the system's default.
            if openwith::open_with_chosen(platform.as_ref(), &state, &path) {
                return;
            }
            if let Err(err) = platform.opener().open(&path) {
                tracing::warn!("open {}: {err}", path.display());
                let _ = notes.send(CoreEvent::Note {
                    text: tf(Key::TCannotOpen, &[&path.display(), &err]),
                    error: true,
                });
            }
        });
        self.toast(
            ToastKind::Info,
            tf(
                Key::TOpening,
                &[&self
                    .tab
                    .current()
                    .map(|e| e.display.clone())
                    .unwrap_or_default()],
            ),
            2,
        );
    }

    /// Go inside a package (`.app`, `.photoslibrary`…) instead of opening it.
    pub(super) fn show_package(&mut self) {
        match self.tab.current() {
            Some(e) if e.package => {
                let path = e.path.clone();
                self.open_dir(path);
            }
            _ => self.toast(ToastKind::Info, Key::TNotPackage.tr(), 2),
        }
    }

    /// From a list of tagged or recent items: open the folder the item under the cursor is in,
    /// with the cursor on it.
    pub(super) fn show_in_folder(&mut self) {
        if self.tab.tag_view().is_none() {
            self.toast(ToastKind::Info, Key::TShowInFolderHere.tr(), 2);
            return;
        }
        let Some(path) = self.tab.current().map(|e| e.path.clone()) else {
            return;
        };
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        self.tab
            .remembered
            .insert(dir.to_path_buf(), name.to_os_string());
        self.open_dir(dir.to_path_buf());
    }

    /// The folder to use for something made next to the items: the one the first of them is
    /// in when the list is a tag's items, the folder shown otherwise.
    pub(super) fn folder_for_new(&self, items: &[PathBuf]) -> PathBuf {
        if self.tab.tag_view().is_some() {
            if let Some(dir) = items.first().and_then(|p| p.parent()) {
                return dir.to_path_buf();
            }
            return self.svc.home.clone();
        }
        self.tab.cwd.clone()
    }

    /// Say why nothing can be made here when the list is a tag's items, not a folder.
    pub(super) fn refuse_in_tag_view(&mut self, what: &str) -> bool {
        if self.tab.recents_view() {
            self.toast(ToastKind::Warn, tf(Key::TRecentRefuse, &[&what]), 5);
            return true;
        }
        let Some(tag) = self.tab.tag_view() else {
            return false;
        };
        self.toast(ToastKind::Warn, tf(Key::TTagViewRefuse, &[&tag, &what]), 5);
        true
    }

    /// The system's quick look at the item under the cursor (macOS).
    pub(super) fn quick_look(&mut self) {
        let Some(path) = self.tab.current().map(|e| e.path.clone()) else {
            return;
        };
        if !self.svc.platform.opener().can_quick_look() {
            self.toast(ToastKind::Info, Key::TQuickLookMac.tr(), 3);
            return;
        }
        if self.refuse_in_archive(Key::WhatQuickLook.tr()) {
            return;
        }
        let platform = self.svc.platform.clone();
        let notes = self.svc.notes.clone();
        self.redraw.after_launch();
        std::thread::spawn(move || {
            if let Err(e) = platform.opener().quick_look(&path) {
                let _ = notes.send(CoreEvent::Note {
                    text: tf(Key::TQuickLookErr, &[&e]),
                    error: true,
                });
            }
        });
    }

    /// Whether this system has a Quick Look to offer.
    pub fn quick_look_available(&self) -> bool {
        self.svc.platform.opener().can_quick_look()
    }
}
