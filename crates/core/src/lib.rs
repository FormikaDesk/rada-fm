//! vela-core: filesystem model, plan/execute/undo engine, journal, watcher and
//! platform layer. No dependency on any user interface.

pub mod display;
pub mod error;
pub mod fs;
pub mod journal;
pub mod ops;
pub mod pathcodec;
pub mod platform;

pub use error::{Error, Result};

#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
