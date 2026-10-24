//! What comes from `config.toml` and the View menu: the look, the keys, the formats of dates
//! and how the window is laid out. It is read all the time and changes seldom.

use crate::fmt::{self, DateFormat, DateStyle};
use crate::icons::IconSet;
use crate::keymap::Keymap;
use crate::theme::Theme;
use crate::view::LayoutKind;
use std::time::{Duration, SystemTime};

pub struct Settings {
    pub th: Theme,
    pub icons: IconSet,
    pub keymap: Keymap,
    /// Mouse capture on/off (`mouse = false` in the configuration).
    pub mouse: bool,
    /// Key hints in the bottom bar (`hints = false` hides them).
    pub show_hints: bool,
    /// Hidden files are listed (the View menu and Ctrl+H change it).
    pub show_hidden: bool,
    /// How soon a second click on the same row makes a double click.
    pub double_click: Duration,
    pub dates: DateStyle,
    pub date_format: DateFormat,
    /// How much room the sidebar leaves between its entries (`sidebar_density`).
    pub sidebar_density: crate::sidebar::Density,
    /// What the NAV box lists under the places (`nav`).
    pub nav_mode: crate::panels::NavMode,
    /// The basic profile: the soft shapes (rounded ends, shades) give way to plain ones.
    pub basic: bool,
    /// The Tag column of the list is shown (the View menu changes it).
    pub show_tags: bool,
    /// The time zone dates are shown in (the system's; tests pin it).
    pub tz: jiff::tz::TimeZone,
    pub layout: LayoutKind,
    /// The details pane: `None` follows the width of the terminal (the View menu and Alt+P
    /// change it).
    pub details: Option<bool>,
    /// Panels, the classic layout, or whichever fits the window (the View menu sets it).
    pub layout_pref: crate::panels::LayoutPref,
}

impl Settings {
    /// The date of a row of the list, in the style the person chose.
    pub fn explorer_date(&self, t: Option<SystemTime>, now: SystemTime) -> String {
        fmt::explorer_date_in(t, now, self.dates, self.date_format, &self.tz)
    }

    /// "Today, 15:21", "Yesterday, 09:12" or the full date and time, for the details pane.
    pub fn date_with_day(&self, t: Option<SystemTime>, now: SystemTime) -> String {
        fmt::date_with_day(t, now, self.date_format, &self.tz)
    }
}
