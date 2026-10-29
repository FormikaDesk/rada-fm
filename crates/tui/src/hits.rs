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
    /// The arrow of a folder of the NAV tree (panels view): opens or closes it.
    TreeToggle(PathBuf),
    /// The button of a plan window that runs it, and the one that cancels it.
    PlanRun,
    PlanCancel,
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
    /// The checkbox of a row (index into the visible entries).
    Check(usize),
    /// A tab, its close mark, the new-tab button and the arrows of the compact tab bar.
    Tab(usize),
    TabClose(usize),
    TabNew,
    TabPrev,
    TabNext,
    /// The address bar outside its segments: a click turns it into a text field.
    Address,
    /// A suggestion under the address field.
    Suggest(usize),
    /// A command or a place offered under the bar while words are typed in it.
    BarHit(usize),
    /// A dropdown button of the command bar.
    Menu(crate::app::MenuKind),
    /// The Details / Icons buttons of the status bar.
    ViewMode(crate::view::ViewMode),
    /// The button of the details pane that shows the preview over the whole screen.
    FullPreview,
    /// A program in the "Open with" window (index into the rows shown).
    OpenWithRow(usize),
    /// The "always use this program for this kind of file" box of that window.
    OpenWithAlways,
    /// The version in the status bar: look for a newer one now.
    CheckUpdates,
    /// Something on a notification card (the id of the card, 0 for the running operation).
    Notice(u64, NoticeHit),
    /// Inside a window but on nothing in particular.
    Window,
}

impl Target {
    /// What lights up when the pointer is over this: the thing itself, the row a checkbox
    /// belongs to, the place a tree arrow belongs to. Nothing for what only holds other
    /// things (the list, a window, the preview) or answers a click anywhere (the address bar
    /// outside its segments, a card outside its buttons).
    pub fn lit(&self) -> Option<Target> {
        match self {
            Target::List
            | Target::Preview
            | Target::Address
            | Target::Window
            | Target::Notice(_, NoticeHit::Card) => None,
            Target::Check(i) => Some(Target::Row(*i)),
            Target::TreeToggle(p) => Some(Target::Place(p.clone())),
            other => Some(other.clone()),
        }
    }
}

/// The parts of a notification card that can be clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeHit {
    /// Take back what was done.
    Undo,
    /// Do again what was taken back.
    Redo,
    /// Go to where the result is.
    Show,
    /// Close an error.
    Close,
    /// Anywhere else on the card: it goes away.
    Card,
    /// Stop the operation that is running.
    CancelRun,
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
        self.rect_at(x, y).map(|(_, t)| t)
    }

    /// [`at`](Self::at), with the rectangle it was registered with.
    pub fn rect_at(&self, x: u16, y: u16) -> Option<(Rect, &Target)> {
        self.items
            .iter()
            .rev()
            .find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .map(|(r, t)| (*r, t))
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
