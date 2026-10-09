//! What is under the mouse.
//!
//! The renderer records, every frame, the screen rectangles of everything that can be
//! clicked. The mouse handler asks this map what a click landed on, so drawing and
//! hit-testing can never disagree.

use std::path::PathBuf;

use crossterm::event::KeyCode;
use rada_core::ops::ConflictPolicy;
use ratatui::layout::Rect;

use crate::keymap::Action;

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A row of the file list (index into the visible entries).
    Row(usize),
    /// The list area (wheel scrolls it; a click on empty space clears the selection).
    List,
    /// A column label: sorts by it.
    SortBy(rada_core::model::SortKey),
    /// A breadcrumb segment: go to that folder.
    Crumb(PathBuf),
    /// The "…" of a long path: the folders it stands for, to choose from.
    CrumbMore(Vec<PathBuf>),
    /// An item of the sidebar: the folder it leads to.
    Place(PathBuf),
    /// An item of the hint bar or a header button: runs the action.
    Act(Action),
    Preview,
    /// A button in a window: behaves like pressing that key.
    Key(KeyCode),
    /// A row of the jump palette.
    PaletteRow(usize),
    /// A choice of the conflict control in the Plan window.
    Policy(ConflictPolicy),
    /// An entry of the context menu.
    MenuItem(usize),
    /// A row of the history window.
    HistoryRow(usize),
    /// Inside a window but on nothing in particular.
    Window,
}

#[derive(Default, Clone)]
pub struct Hits {
    items: Vec<(Rect, Target)>,
}

impl Hits {
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn add(&mut self, area: Rect, target: Target) {
        if area.width > 0 && area.height > 0 {
            self.items.push((area, target));
        }
    }

    /// The topmost thing at a cell: later registrations are drawn over earlier ones.
    pub fn at(&self, x: u16, y: u16) -> Option<&Target> {
        self.items
            .iter()
            .rev()
            .find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .map(|(_, t)| t)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Everything registered, in drawing order.
    pub fn all(&self) -> impl Iterator<Item = &(Rect, Target)> {
        self.items.iter()
    }

    pub fn find(&self, t: &Target) -> Option<Rect> {
        self.items
            .iter()
            .rev()
            .find(|(_, x)| x == t)
            .map(|(r, _)| *r)
    }
}
