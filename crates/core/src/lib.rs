//! rada-core: filesystem model, plan/execute/undo engine, journal, watcher and
//! platform layer. No dependency on any user interface.

pub mod archive;
pub mod display;
pub mod error;
pub mod events;
pub mod filetype;
pub mod foldersize;
pub mod fs;
pub mod git;
pub mod health;
pub mod i18n;
pub mod jobs;
pub mod journal;
pub mod model;
pub mod oplock;
pub mod ops;
pub mod pathcodec;
pub mod places;
pub mod platform;
pub mod preview;
pub mod recents;
pub mod schema;
pub mod search;
pub mod statefile;
pub mod tagsearch;
pub mod uistate;
pub mod update;
pub mod view;
pub mod views;
pub mod watch;
pub mod workers;
pub mod zoxide;

pub use error::{Error, Result};
pub use view::ViewKind;

#[cfg(any(test, feature = "testutil"))]
pub mod testplatform;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
#[cfg(any(test, feature = "testutil"))]
pub mod testweb;
