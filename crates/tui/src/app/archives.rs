//! Archives in the interface: telling which items are archives, extracting, compressing.

use rada_core::archive::{
    self, ArchiveKind, ArchiveView, name_is_zip_document, name_suggests_archive,
};
use rada_core::ops::{ConflictPolicy, ExtractInto, OpRequest};

use super::{App, InputKind, InputView, Modal, ToastKind};
use crate::keymap::Action;
use rada_core::i18n::{Key, tf};
use rada_core::model::Entry;
use rada_core::preview::Preview;
use std::path::{Path, PathBuf};

impl App {
    /// Whether Enter should open `e` as a folder: its name says archive, or its content did
    /// (the preview read it) and it is not a document that merely is a ZIP.
    pub(super) fn archive_candidate(&self, e: &Entry) -> bool {
        if e.is_dir() || e.kind != rada_core::fs::FileKind::File {
            return false;
        }
        if name_suggests_archive(&e.name) {
            return true;
        }
        self.preview.path.as_deref() == Some(e.path.as_path())
            && !name_is_zip_document(&e.name)
            && matches!(&self.preview.content, Some(Preview::Archive(a)) if a.problem.is_none())
    }

    pub fn in_archive(&self) -> bool {
        self.tab.archive.is_some()
    }

    /// The cursor is on an archive (outside one): extraction is on offer.
    pub fn on_archive_item(&self) -> bool {
        self.tab.archive.is_none()
            && self
                .tab
                .current()
                .is_some_and(|e| self.archive_candidate(e))
    }

    /// The entries an action applies to: the marked ones, else the one under the cursor.
    fn target_entries(&self) -> Vec<&Entry> {
        if !self.tab.sel.marked.is_empty() {
            return self
                .tab
                .listing
                .all()
                .iter()
                .filter(|e| self.tab.sel.marked.contains(&e.name))
                .collect();
        }
        self.tab.current().into_iter().collect()
    }

    /// Extract here (or, with `here` false, into a folder named after the archive).
    pub(super) fn start_extract(&mut self, here: bool) {
        if self.refuse_if_busy() {
            return;
        }
        if let Some(view) = self.tab.archive.clone() {
            return self.extract_open_archive(view, here);
        }
        let archives: Vec<PathBuf> = self
            .target_entries()
            .into_iter()
            .filter(|e| self.archive_candidate(e))
            .map(|e| e.path.clone())
            .collect();
        match archives.len() {
            0 => self.toast(ToastKind::Info, Key::TSelectArchive.tr(), 3),
            1 => {
                let archive = archives[0].clone();
                let dest = self.folder_for_new(std::slice::from_ref(&archive));
                if here {
                    self.plan_extract_now(archive, Vec::new(), dest, ExtractInto::Here);
                } else {
                    self.ask_extract_folder(archive, dest);
                }
            }
            _ => self.toast(ToastKind::Info, Key::TOneArchive.tr(), 3),
        }
    }

    /// Inside an archive: the marked members, or everything. The folder is the archive's.
    fn extract_open_archive(&mut self, view: ArchiveView, here: bool) {
        let only: Vec<PathBuf> = if self.tab.sel.marked.is_empty() {
            Vec::new()
        } else {
            self.tab
                .targets()
                .iter()
                .filter_map(|p| p.strip_prefix(&view.archive).ok().map(Path::to_path_buf))
                .collect()
        };
        let dest = view
            .archive
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.tab.cwd.clone());
        if !only.is_empty() && !here {
            return self.say_extract_part_hint();
        }
        if only.is_empty() && !here {
            return self.ask_extract_folder(view.archive, dest);
        }
        self.plan_extract_now(view.archive, only, dest, ExtractInto::Here);
    }

    /// Part of an archive cannot be extracted into a folder: copy it out instead.
    fn say_extract_part_hint(&mut self) {
        self.toast(
            ToastKind::Info,
            tf(
                Key::TExtractPartHint,
                &[
                    &self
                        .key_for(Action::Copy)
                        .unwrap_or_else(|| Key::KeyCopyWord.tr().into()),
                    &self
                        .key_for(Action::Paste)
                        .unwrap_or_else(|| Key::KeyPasteWord.tr().into()),
                ],
            ),
            5,
        );
    }

    fn plan_extract_now(
        &mut self,
        archive: PathBuf,
        only: Vec<PathBuf>,
        dest: PathBuf,
        into: ExtractInto,
    ) {
        let h = self.svc.jobs.plan_request(OpRequest::Extract {
            archive,
            destination: dest,
            into,
            only,
            conflict: ConflictPolicy::Skip,
        });
        self.begin_plan(Key::ReadingArchive.tr(), h);
    }

    fn ask_extract_folder(&mut self, archive: PathBuf, dest: PathBuf) {
        let default = archive
            .file_name()
            .map(|n| archive::archive_stem(n).to_string_lossy().into_owned())
            .unwrap_or_default();
        self.windows.modal = Some(Modal::Input(InputView::new(
            InputKind::ExtractTo {
                archive,
                dest,
                default: default.clone(),
            },
            default,
        )));
    }

    /// Ask for a name and a format, then plan the new archive.
    pub(super) fn start_compress(&mut self) {
        if self.refuse_if_busy() || self.refuse_in_archive(Key::WhatCompressHere.tr()) {
            return;
        }
        let sources = self.tab.targets();
        if sources.is_empty() {
            return;
        }
        let format = ArchiveKind::Zip;
        let base = self.archive_base_name(&sources);
        let text = format!("{base}{}", format.extension());
        let dir = self.folder_for_new(&sources);
        self.windows.modal = Some(Modal::Input(InputView::new(
            InputKind::Compress {
                sources,
                dir,
                format,
            },
            text,
        )));
    }

    /// What the new archive is called before the extension: the name of the one item, or of
    /// the folder when there are several.
    fn archive_base_name(&self, sources: &[PathBuf]) -> String {
        if sources.len() == 1 {
            let n = sources[0].file_name().unwrap_or_default();
            // A file loses its extension (report.pdf → report.zip); a folder keeps its name.
            let is_dir = self.tab.current().is_some_and(|e| e.is_dir());
            if is_dir {
                n.to_string_lossy().into_owned()
            } else {
                Path::new(n)
                    .file_stem()
                    .unwrap_or(n)
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            self.tab
                .cwd
                .file_name()
                .filter(|_| self.tab.tag_view().is_none())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive".into())
        }
    }

    /// Archives are read-only: say so, with the way to do what was meant.
    pub(super) fn refuse_in_archive(&mut self, what: &str) -> bool {
        if self.tab.archive.is_none() {
            return false;
        }
        self.toast(
            ToastKind::Warn,
            tf(
                Key::TArchiveReadOnly,
                &[
                    &what,
                    &self
                        .key_for(Action::Copy)
                        .unwrap_or_else(|| Key::KeyCopyWord.tr().into()),
                ],
            ),
            5,
        );
        true
    }
}

impl InputView {
    /// Tab in the name of a new archive: the next format, with the extension changed to match.
    pub(super) fn cycle_format(&mut self) {
        let InputKind::Compress { format, .. } = &mut self.kind else {
            return;
        };
        let old = *format;
        let next = old.next();
        let stem = self
            .text
            .strip_suffix(old.extension())
            .unwrap_or(&self.text)
            .to_string();
        *format = next;
        self.text = format!("{stem}{}", next.extension());
        self.cursor = self.text.chars().count();
        self.error = None;
    }
}
