//! The questions asked with a line of text: rename, new folder or file, compress, extract, new
//! tag, open with; their keys and what each one starts.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::archive::ArchiveKind;
use rada_core::events::CoreEvent;
use rada_core::i18n::Key;
use rada_core::ops::{ConflictPolicy, ExtractInto, OpRequest, Pattern};

use super::{App, InputKind, InputView, Modal};

impl App {
    pub(super) fn ask_new_folder(&mut self) {
        if !self.refuse_in_archive(Key::WhatCreateFolder.tr())
            && !self.refuse_in_tag_view(Key::WhatCreateFolder.tr())
        {
            self.windows.modal = Some(Modal::Input(InputView::new(
                InputKind::NewDir,
                String::new(),
            )))
        }
    }

    pub(super) fn ask_new_file(&mut self) {
        if !self.refuse_if_busy()
            && !self.refuse_in_archive(Key::WhatCreateFile.tr())
            && !self.refuse_in_tag_view(Key::WhatCreateFile.tr())
        {
            self.windows.modal = Some(Modal::Input(InputView::new(
                InputKind::NewFile,
                String::new(),
            )))
        }
    }

    pub(super) fn start_rename(&mut self) {
        if self.refuse_if_busy() || self.refuse_in_archive(Key::WhatRename.tr()) {
            return;
        }
        if let Some(e) = self.tab.current() {
            // The name on the disk (in a list of tagged items `name` is the whole path).
            let text = e
                .path
                .file_name()
                .unwrap_or(&e.name)
                .to_string_lossy()
                .into_owned();
            let from = e.path.clone();
            self.windows.modal = Some(Modal::Input(InputView::new(
                InputKind::Rename { from },
                text,
            )));
        }
    }

    pub(super) fn start_bulk_rename(&mut self) {
        if self.refuse_if_busy() || self.refuse_in_archive(Key::WhatRename.tr()) {
            return;
        }
        let items = self.tab.targets();
        if items.is_empty() {
            return;
        }
        self.windows.modal = Some(Modal::Input(InputView::new(
            InputKind::BulkRename { items },
            "{name}_{n:2}.{ext}".to_string(),
        )));
    }

    pub(super) fn on_input_key(&mut self, mut iv: InputView, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter if !iv.text.is_empty() => self.submit_input(iv),
            code => {
                iv.press(code, ctrl);
                self.windows.modal = Some(Modal::Input(iv));
            }
        }
    }

    fn submit_input(&mut self, iv: InputView) {
        match &iv.kind {
            InputKind::Rename { from } => self.submit_rename(from, &iv.text),
            InputKind::SaveAs => self.submit_save_as(iv),
            InputKind::NewDir => self.submit_new(true, &iv.text),
            InputKind::ExtractTo {
                archive,
                dest,
                default,
            } => self.submit_extract(archive, dest, default, &iv.text),
            InputKind::Compress {
                sources,
                dir,
                format,
            } => {
                if let Err(e) = self.submit_compress(sources, dir, *format, &iv.text) {
                    self.reject_input(iv, e);
                }
            }
            InputKind::NewFile => self.submit_new(false, &iv.text),
            InputKind::NewTag { items } => {
                if let Err(e) = self.submit_new_tag(items, &iv.text) {
                    self.reject_input(iv, e);
                }
            }
            InputKind::OpenWith { path } => {
                let program = iv.text.trim().to_string();
                if program.is_empty() {
                    self.windows.modal = Some(Modal::Input(iv));
                } else {
                    self.open_with_program(path, program);
                }
            }
            InputKind::BulkRename { items } => {
                if let Err(e) = self.submit_bulk_rename(items, &iv.text) {
                    self.reject_input(iv, e);
                }
            }
        }
    }

    /// The question stays open, with what is wrong with the answer under it.
    fn reject_input(&mut self, mut iv: InputView, error: String) {
        iv.error = Some(error);
        self.windows.modal = Some(Modal::Input(iv));
    }

    fn submit_rename(&mut self, from: &Path, text: &str) {
        let h = self
            .svc
            .jobs
            .plan_rename(from.to_path_buf(), text.trim_end_matches('\n').to_string());
        self.begin_plan(Key::PlanningRename.tr(), h);
    }

    /// A new folder (`dir`) or a new empty file in the folder shown.
    fn submit_new(&mut self, dir: bool, text: &str) {
        let (cwd, name) = (self.tab.cwd.clone(), text.to_string());
        let h = if dir {
            self.svc.jobs.plan_mkdir(cwd, name)
        } else {
            self.svc.jobs.plan_mkfile(cwd, name)
        };
        self.begin_plan(Key::PlanningGeneric.tr(), h);
    }

    fn submit_extract(&mut self, archive: &Path, dest: &Path, default: &str, text: &str) {
        let name = text.trim().to_string();
        let into = if name == default {
            ExtractInto::Auto
        } else {
            ExtractInto::Folder { name }
        };
        let h = self.svc.jobs.plan_request(OpRequest::Extract {
            archive: archive.to_path_buf(),
            destination: dest.to_path_buf(),
            into,
            only: Vec::new(),
            conflict: ConflictPolicy::Skip,
        });
        self.begin_plan(Key::ReadingArchive.tr(), h);
    }

    fn submit_compress(
        &mut self,
        sources: &[PathBuf],
        dir: &Path,
        format: ArchiveKind,
        text: &str,
    ) -> Result<(), String> {
        let mut name = text.trim().to_string();
        if name.contains(['/', '\\']) {
            return Err("a name cannot contain a slash".into());
        }
        let kind = match ArchiveKind::from_name(std::ffi::OsStr::new(&name)) {
            Some(k) => k,
            None => {
                name.push_str(format.extension());
                format
            }
        };
        let h = self.svc.jobs.plan_request(OpRequest::Compress {
            sources: sources.to_vec(),
            archive: dir.join(&name),
            format: kind,
            conflict: ConflictPolicy::Skip,
        });
        self.begin_plan(Key::PlanningCompress.tr(), h);
        Ok(())
    }

    fn submit_new_tag(&mut self, items: &[PathBuf], text: &str) -> Result<(), String> {
        let name = rada_core::platform::tags::validate_name(text)?;
        let h = self.svc.jobs.plan_request(OpRequest::SetTags {
            items: items.to_vec(),
            change: rada_core::ops::TagChange::Add {
                tag: rada_core::platform::Tag::named(&name),
            },
        });
        self.begin_plan(Key::PlanningGeneric.tr(), h);
        Ok(())
    }

    fn submit_bulk_rename(&mut self, items: &[PathBuf], text: &str) -> Result<(), String> {
        Pattern::parse(text)?;
        let h = self
            .svc
            .jobs
            .plan_bulk_rename(items.to_vec(), text.to_string());
        self.begin_plan(Key::PlanningRename.tr(), h);
        Ok(())
    }

    /// Start the program the person typed on the file, off the UI thread.
    fn open_with_program(&mut self, path: &Path, program: String) {
        let (path, platform, notes) = (
            path.to_path_buf(),
            self.svc.platform.clone(),
            self.svc.notes.clone(),
        );
        self.redraw.after_launch();
        std::thread::spawn(move || {
            if let Err(e) = platform.opener().open_with(&path, &program) {
                let _ = notes.send(CoreEvent::Note {
                    text: e.to_string(),
                    error: true,
                });
            }
        });
    }
}

impl InputView {
    /// What a key does to the text being typed (and the format, for a compression).
    fn press(&mut self, code: KeyCode, ctrl: bool) {
        match code {
            KeyCode::Tab if matches!(self.kind, InputKind::Compress { .. }) => self.cycle_format(),
            KeyCode::Char(c) if !ctrl => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.text.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.chars().count(),
            KeyCode::Char('u') if ctrl => {
                self.text.clear();
                self.cursor = 0;
            }
            _ => {}
        }
    }
}
