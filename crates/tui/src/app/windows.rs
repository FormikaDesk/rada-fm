//! What is put over the explorer: the window that is open, the cards of notifications, the
//! address bar while it is edited, the scroll of the help and the advice to enlarge a small
//! window.

use super::{AddressEdit, Modal, NoticeQueue};

#[derive(Default)]
pub struct Windows {
    /// The window that is open and has the keyboard, if any.
    pub modal: Option<Modal>,
    /// What to tell the person: a card at the bottom right, one at a time (see `notice.rs`).
    pub notices: NoticeQueue,
    /// What the live updates of the folder said last (they are degraded or off).
    pub watch_note: Option<String>,
    pub help_scroll: usize,
    /// The address bar while its text is being edited.
    pub address: Option<AddressEdit>,
    /// The advice to enlarge a small window was closed (for the rest of this session).
    pub size_notice_dismissed: bool,
}

impl Windows {
    pub fn help_down(&mut self, by: usize) {
        self.help_scroll += by;
    }

    pub fn help_up(&mut self, by: usize) {
        self.help_scroll = self.help_scroll.saturating_sub(by);
    }
}
