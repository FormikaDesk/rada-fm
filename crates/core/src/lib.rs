//! rada-core: filesystem model, plan/execute/undo engine, journal, watcher and
//! platform layer. No dependency on any user interface.

pub mod display;
pub mod error;
pub mod events;
pub mod fs;
pub mod jobs;
pub mod journal;
pub mod model;
pub mod ops;
pub mod pathcodec;
pub mod places;
pub mod platform;
pub mod preview;
pub mod schema;
pub mod uistate;
pub mod watch;
pub mod workers;

pub use error::{Error, Result};

#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
