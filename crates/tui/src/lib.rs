//! rada-tui: the terminal interface (ratatui + crossterm).
//!
//! The UI thread never touches the filesystem: it draws state and forwards events from
//! the keyboard and from the workers of `rada-core`.

pub mod app;
pub mod caps;
pub mod chooser;
pub mod clipboard;
pub mod compat;
pub mod fmt;
pub mod fuzzy;
pub mod hits;
pub mod icons;
pub mod images;
pub mod input;
pub mod keymap;
pub mod nav;
pub mod palette;
pub mod panels;
pub mod progicons;
pub mod ring;
pub mod run;
pub mod services;
pub mod sidebar;
pub mod termbg;
pub mod termtheme;
pub mod theme;
pub mod ui;
pub mod view;

pub use app::{Config, FontOffer};
pub use caps::{Profile, TermCaps};
pub use icons::IconSet;
pub use images::{ImageMode, ImageUi};
pub use panels::LayoutPref;
pub use services::Services;
pub use theme::Theme;
pub use view::{LayoutKind, ViewMode};
