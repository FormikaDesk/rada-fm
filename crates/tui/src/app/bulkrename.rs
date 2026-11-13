//! The preview of a bulk rename: asked to the background worker each time the pattern
//! changes, never made while the window is drawn. A pattern with fields of the files'
//! metadata reads them first, and the window says how far it is.

use std::path::PathBuf;
use std::sync::Arc;

use rada_core::i18n::{Key, tf};
use rada_core::ops::{Pattern, Preview, RenamePreview};

use super::prompt::{InputKind, InputView};
use super::{App, Modal};

/// What the window knows of the preview.
#[derive(Default)]
pub struct BulkPreview {
    /// The question asked last, and the pattern it was asked for.
    epoch: u64,
    asked: Option<String>,
    /// How far the reading of the files is: done, of total.
    pub reading: Option<(usize, usize)>,
    /// The last rows that came, and the pattern they are for: they stay on screen while the
    /// rows of a newer pattern are made, so the window does not flicker at each key.
    pub rows: Vec<Preview>,
    rows_for: Option<String>,
}

impl BulkPreview {
    /// The items that cannot be renamed with the pattern now in the field, once it is known.
    fn refused(&self, text: &str) -> Option<usize> {
        let current = self.rows_for.as_deref() == Some(text);
        current.then(|| self.rows.iter().filter(|r| r.to.is_err()).count())
    }

    /// Whether the rows shown are those of `text`, all of them.
    pub fn ready_for(&self, text: &str) -> bool {
        self.reading.is_none() && self.rows_for.as_deref() == Some(text)
    }

    /// How many items cannot be renamed, when the rows shown are those of `text`.
    pub fn refused_now(&self, text: &str) -> usize {
        self.refused(text).unwrap_or(0)
    }
}

impl App {
    /// Ask for the preview of what is in the field, unless it was already asked.
    pub(super) fn ask_bulk_preview(&self, iv: &mut InputView) {
        let InputKind::BulkRename {
            items,
            rules,
            preview,
        } = &mut iv.kind
        else {
            return;
        };
        if preview.asked.as_deref() == Some(iv.text.as_str()) {
            return;
        }
        preview.asked = Some(iv.text.clone());
        preview.reading = None;
        match Pattern::parse(&iv.text) {
            Ok(p) => {
                if p.needs_meta() {
                    preview.reading = Some((0, items.len()));
                }
                preview.epoch = self.svc.renames.ask(p, items.clone(), rules.clone());
            }
            // What is wrong with the pattern is said under the field.
            Err(_) => self.svc.renames.stop(),
        }
    }

    pub(super) fn on_rename_preview(&mut self, ev: RenamePreview) {
        let Some(Modal::Input(InputView {
            kind: InputKind::BulkRename { preview, .. },
            ..
        })) = &mut self.windows.modal
        else {
            return;
        };
        match ev {
            RenamePreview::Reading { epoch, done, total } if epoch == preview.epoch => {
                preview.reading = Some((done, total));
            }
            RenamePreview::Ready { epoch, rows } if epoch == preview.epoch => {
                preview.reading = None;
                preview.rows = rows;
                preview.rows_for = preview.asked.clone();
            }
            _ => {}
        }
    }

    /// Enter on a bulk rename: refused while an item has no name, with what can be done.
    pub(super) fn bulk_refusal(iv: &InputView) -> Option<String> {
        let InputKind::BulkRename { preview, .. } = &iv.kind else {
            return None;
        };
        match preview.refused(&iv.text) {
            Some(n) if n > 0 => Some(tf(Key::InBulkRefused, &[&n])),
            _ => None,
        }
    }

    /// Take the items that cannot be renamed out of the rename (`Ctrl+X`), and ask again.
    pub(super) fn drop_refused(&self, iv: &mut InputView) {
        let InputKind::BulkRename { items, preview, .. } = &mut iv.kind else {
            return;
        };
        if preview.refused(&iv.text).unwrap_or(0) == 0 {
            return;
        }
        let keep: Vec<PathBuf> = preview
            .rows
            .iter()
            .filter(|r| r.to.is_ok())
            .map(|r| r.from.clone())
            .collect();
        if keep.is_empty() {
            iv.error = Some(Key::InBulkNoneLeft.tr().to_string());
            return;
        }
        *items = Arc::new(keep);
        // The rows left are those of the items kept, with the counter starting again.
        *preview = BulkPreview::default();
        iv.error = None;
        self.ask_bulk_preview(iv);
    }
}
