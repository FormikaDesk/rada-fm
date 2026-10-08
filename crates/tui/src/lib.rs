//! vela-tui: the terminal interface (ratatui + crossterm).
//!
//! The UI thread never touches the filesystem: it draws state and forwards events from
//! the keyboard and from the workers of `vela-core`.

pub mod app;
pub mod fmt;
pub mod fuzzy;
pub mod icons;
pub mod images;
pub mod palette;
pub mod run;
pub mod services;
pub mod theme;
pub mod ui;

pub use app::Config;
pub use icons::IconSet;
pub use images::{ImageMode, ImageUi};
pub use services::Services;
pub use theme::Theme;
