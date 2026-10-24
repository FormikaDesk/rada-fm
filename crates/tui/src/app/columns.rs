//! What the Columns view needs besides the folder itself: the folder above it, read by a
//! worker of its own, and the keys that move between the columns.
//!
//! The three columns are the folder above (with the one you are in marked), the folder you
//! are in, and what the item under the cursor holds (or its preview, for a file). Left goes to
//! the folder above, Right goes into the folder under the cursor; the drawing is in
//! `ui/columns.rs`.

use rada_core::model::{DirListing, Entry};

use super::App;
use crate::view::ViewMode;
use std::path::{Path, PathBuf};

/// Generations at or above this are answers about the folder above, not about the shown one.
pub const PARENT_GEN: u64 = 1 << 62;

/// The folder above the shown one, as the first column lists it.
pub struct ParentColumn {
    pub path: PathBuf,
    pub listing: DirListing,
}

/// The state of the Columns view: its first column and the question that fills it.
#[derive(Default)]
pub struct ColumnsView {
    /// The folder above the one shown.
    pub columns_parent: Option<ParentColumn>,
    parent_gen: u64,
    parent_asked: Option<PathBuf>,
}

impl App {
    /// Whether the Columns view is the one drawn.
    pub fn columns_on(&self) -> bool {
        self.view == ViewMode::Columns
    }

    /// Ask for the folder above the shown one if the first column does not have it yet.
    pub fn want_parent_column(&mut self) {
        if self.view != ViewMode::Columns || self.archive.is_some() || self.tag_view().is_some() {
            return;
        }
        let Some(parent) = self.cwd.parent().map(Path::to_path_buf) else {
            self.columns.columns_parent = None;
            return;
        };
        if self
            .columns
            .columns_parent
            .as_ref()
            .is_some_and(|c| c.path == parent)
            || self.columns.parent_asked.as_ref() == Some(&parent)
        {
            return;
        }
        self.columns.parent_gen += 1;
        self.columns.parent_asked = Some(parent.clone());
        self.svc
            .parent_loader
            .load(parent, PARENT_GEN | self.columns.parent_gen);
    }

    /// The answer about the folder above arrived.
    pub(super) fn on_parent_loaded(
        &mut self,
        generation: u64,
        path: PathBuf,
        result: Result<Vec<Entry>, String>,
    ) {
        if generation != PARENT_GEN | self.columns.parent_gen {
            return; // a question that was replaced by a newer one
        }
        self.columns.parent_asked = None;
        match result {
            Ok(entries) => {
                self.columns.columns_parent = Some(ParentColumn {
                    listing: DirListing::new(path.clone(), entries, self.sort),
                    path,
                });
            }
            Err(_) => self.columns.columns_parent = None,
        }
        self.redraw.dirty = true;
    }

    /// The entries of the folder above in the order of the first column: what the list would
    /// show of it (hidden items only when hidden files are on).
    pub fn parent_entries(&self) -> Vec<&Entry> {
        let Some(c) = &self.columns.columns_parent else {
            return Vec::new();
        };
        c.listing
            .all()
            .iter()
            .filter(|e| self.settings.show_hidden || !e.hidden)
            .collect()
    }

    /// Right in the Columns view: into the folder under the cursor. A file stays where it is
    /// (Enter opens it; Right only moves between columns).
    pub(super) fn columns_right(&mut self) {
        let Some(e) = self.current() else { return };
        if e.is_navigable_dir() {
            let target = e.path.clone();
            self.open_dir(target);
        }
    }
}
