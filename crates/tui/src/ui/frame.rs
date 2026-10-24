//! What the last frame drew and the keyboard, the mouse and the tests read afterwards.

use super::DetailsMode;
use crate::hits::Hits;
use crate::panels::Columns;
use crate::sidebar::Mode;
use ratatui::layout::Rect;
use std::path::PathBuf;

pub struct LastFrame {
    /// Size of the area the last frame used, for the keyboard and the tests.
    pub screen: (u16, u16),
    /// Where the field of the bar at the top was drawn (what is offered under it lines up
    /// with it).
    pub bar_field: Rect,
    /// Where popups are drawn this frame (menus, the list of programs, windows): a picture the
    /// terminal draws is left out of these places.
    pub popup_covers: Vec<Rect>,
    /// Screen rectangles of everything clickable, rebuilt every frame.
    pub hits: Hits,
    /// What the last frame showed of the sidebar.
    pub side_mode: Mode,
    /// What the last frame showed of the details pane.
    pub details_shown: DetailsMode,
    /// The last frame was drawn as panels.
    pub panels_shown: bool,
    /// ...with two columns (NAV or the preview, and the folder).
    pub panels_two: bool,
    /// The advice to enlarge a small window was shown by the last frame.
    pub size_notice_visible: bool,
    /// The first row of the NAV box that is shown, where the box was last drawn, and what the
    /// scrolling was last made to show (the folder, the keyboard's row, how many rows there
    /// are): the box scrolls to the folder only when one of those changes, so that the wheel
    /// can move it away.
    pub nav_scroll: usize,
    pub nav_area: Rect,
    pub nav_seen: Option<(PathBuf, Option<usize>, usize, usize)>,
    /// The ring of the SPAZIO box, as it was last made.
    pub ring: Option<crate::ring::Cache>,
}

impl Default for LastFrame {
    fn default() -> LastFrame {
        LastFrame {
            screen: (0, 0),
            bar_field: Rect::default(),
            popup_covers: Vec::new(),
            hits: Hits::default(),
            side_mode: Mode::Hidden,
            details_shown: DetailsMode::Hidden,
            panels_shown: false,
            panels_two: false,
            size_notice_visible: false,
            nav_scroll: 0,
            nav_area: Rect::default(),
            nav_seen: None,
            ring: None,
        }
    }
}

impl LastFrame {
    /// The width of the terminal at the last frame.
    pub fn term_width(&self) -> u16 {
        self.screen.0
    }

    /// A frame of `area` begins: nothing is clickable yet.
    pub fn begin(&mut self, area: Rect) {
        self.hits.clear();
        self.screen = (area.width, area.height);
    }

    /// The frame is drawn as panels with `columns`, or as the classic layout when it is
    /// `None`. The panels have their own sidebar and details, and no advice to enlarge.
    pub fn drawn_as(&mut self, columns: Option<Columns>) {
        self.panels_shown = columns.is_some();
        self.panels_two = columns == Some(Columns::Two);
        if self.panels_shown {
            self.size_notice_visible = false;
            self.side_mode = Mode::Full;
            self.details_shown = DetailsMode::Column;
        }
    }
}
