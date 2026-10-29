//! What the pointer is over, for the light highlight of whatever can be clicked.

use std::path::PathBuf;

use super::App;
use crate::hits::Target;

/// The pointer, and what is highlighted under it. One for the whole screen: the map of clicks
/// says what is under the pointer, and the frame paints it (`ui::hover`).
#[derive(Default)]
pub struct Hover {
    /// Where the pointer was last seen. Empty until it moves, and always with the mouse off.
    pub(crate) at: Option<(u16, u16)>,
    /// What is highlighted there.
    pub(crate) over: Option<Target>,
}

impl Hover {
    /// The pointer moved to `(x, y)`, where the last frame had `under`. True when what is
    /// highlighted is not what it was: only then does the screen need drawing again.
    pub(super) fn moved(&mut self, x: u16, y: u16, under: Option<Target>) -> bool {
        self.at = Some((x, y));
        let changed = under != self.over;
        self.over = under;
        changed
    }
}

impl App {
    /// The place of the sidebar, the navigation pane or the columns that the pointer is over.
    pub(super) fn hovered_place(&self) -> Option<&PathBuf> {
        match &self.hover.over {
            Some(Target::Place(p)) => Some(p),
            _ => None,
        }
    }
}
