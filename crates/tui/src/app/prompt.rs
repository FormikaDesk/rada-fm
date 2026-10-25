//! The questions asked with a line of text: rename, new folder or file, compress, extract, new tag, open with; their keys and what each one starts.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::archive::ArchiveKind;
use rada_core::events::CoreEvent;
use rada_core::i18n::Key;
use rada_core::ops::{ConflictPolicy, ExtractInto, OpRequest, Pattern};

use super::{App, InputKind, InputView, Modal};

impl App {
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
            KeyCode::Tab if matches!(iv.kind, InputKind::Compress { .. }) => {
                iv.cycle_format();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Enter => {
                if iv.text.is_empty() {
                    self.windows.modal = Some(Modal::Input(iv));
                } else {
                    self.submit_input(iv);
                }
            }
            KeyCode::Char(c) if !ctrl => {
                iv.insert(c);
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Backspace => {
                iv.backspace();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Delete => {
                iv.delete();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Left => {
                iv.cursor = iv.cursor.saturating_sub(1);
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Right => {
                iv.cursor = (iv.cursor + 1).min(iv.text.chars().count());
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Home => {
                iv.cursor = 0;
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::End => {
                iv.cursor = iv.text.chars().count();
                self.windows.modal = Some(Modal::Input(iv));
            }
            KeyCode::Char('u') if ctrl => {
                iv.text.clear();
                iv.cursor = 0;
                self.windows.modal = Some(Modal::Input(iv));
            }
            _ => self.windows.modal = Some(Modal::Input(iv)),
        }
    }

    fn submit_input(&mut self, iv: InputView) {
        match &iv.kind {
            InputKind::Rename { from } => {
                let h = self
                    .svc
                    .jobs
                    .plan_rename(from.clone(), iv.text.trim_end_matches('\n').to_string());
                self.begin_plan(Key::PlanningRename.tr(), h);
            }
            InputKind::SaveAs => self.submit_save_as(iv),
            InputKind::NewDir => {
                let h = self
                    .svc
                    .jobs
                    .plan_mkdir(self.tab.cwd.clone(), iv.text.clone());
                self.begin_plan(Key::PlanningGeneric.tr(), h);
            }
            InputKind::ExtractTo {
                archive,
                dest,
                default,
            } => {
                let name = iv.text.trim().to_string();
                let into = if name == *default {
                    ExtractInto::Auto
                } else {
                    ExtractInto::Folder { name }
                };
                let h = self.svc.jobs.plan_request(OpRequest::Extract {
                    archive: archive.clone(),
                    destination: dest.clone(),
                    into,
                    only: Vec::new(),
                    conflict: ConflictPolicy::Skip,
                });
                self.begin_plan(Key::ReadingArchive.tr(), h);
            }
            InputKind::Compress {
                sources,
                dir,
                format,
            } => {
                let mut name = iv.text.trim().to_string();
                if name.contains(['/', '\\']) {
                    let mut iv = iv;
                    iv.error = Some("a name cannot contain a slash".into());
                    self.windows.modal = Some(Modal::Input(iv));
                    return;
                }
                let kind = match ArchiveKind::from_name(std::ffi::OsStr::new(&name)) {
                    Some(k) => k,
                    None => {
                        name.push_str(format.extension());
                        *format
                    }
                };
                let h = self.svc.jobs.plan_request(OpRequest::Compress {
                    sources: sources.clone(),
                    archive: dir.join(&name),
                    format: kind,
                    conflict: ConflictPolicy::Skip,
                });
                self.begin_plan(Key::PlanningCompress.tr(), h);
            }
            InputKind::NewFile => {
                let h = self
                    .svc
                    .jobs
                    .plan_mkfile(self.tab.cwd.clone(), iv.text.clone());
                self.begin_plan(Key::PlanningGeneric.tr(), h);
            }
            InputKind::NewTag { items } => match rada_core::platform::tags::validate_name(&iv.text)
            {
                Ok(name) => {
                    let h = self.svc.jobs.plan_request(OpRequest::SetTags {
                        items: items.clone(),
                        change: rada_core::ops::TagChange::Add {
                            tag: rada_core::platform::Tag::named(&name),
                        },
                    });
                    self.begin_plan(Key::PlanningGeneric.tr(), h);
                }
                Err(e) => {
                    let mut iv = iv;
                    iv.error = Some(e);
                    self.windows.modal = Some(Modal::Input(iv));
                }
            },
            InputKind::OpenWith { path } => {
                let program = iv.text.trim().to_string();
                if program.is_empty() {
                    self.windows.modal = Some(Modal::Input(iv));
                    return;
                }
                let (path, platform, notes) = (
                    path.clone(),
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
            InputKind::BulkRename { items } => match Pattern::parse(&iv.text) {
                Ok(_) => {
                    let h = self
                        .svc
                        .jobs
                        .plan_bulk_rename(items.clone(), iv.text.clone());
                    self.begin_plan(Key::PlanningRename.tr(), h);
                }
                Err(e) => {
                    let mut iv = iv;
                    iv.error = Some(e);
                    self.windows.modal = Some(Modal::Input(iv));
                }
            },
        }
    }
}
