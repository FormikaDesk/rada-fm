use std::sync::Arc;

use super::programs::Programs;
use crate::archive::ArchiveLimits;
use crate::fs::{FsEngine, LocalFs};
use crate::platform::Platform;

/// Everything an operation needs from the outside world.
#[derive(Clone)]
pub struct Engine {
    pub fs: Arc<dyn FsEngine>,
    pub platform: Arc<dyn Platform>,
    /// When an extraction needs an explicit confirmation.
    pub archive_limits: ArchiveLimits,
    /// The programs the configuration names for actions, over the built-in table.
    pub programs: Programs,
}

impl Engine {
    pub fn new(fs: Arc<dyn FsEngine>, platform: Arc<dyn Platform>) -> Self {
        Engine {
            fs,
            platform,
            archive_limits: ArchiveLimits::default(),
            programs: Programs::default(),
        }
    }

    /// The real local filesystem with the given platform.
    pub fn local(platform: Arc<dyn Platform>) -> Self {
        Engine {
            fs: Arc::new(LocalFs),
            platform,
            archive_limits: ArchiveLimits::default(),
            programs: Programs::default(),
        }
    }

    pub fn with_archive_limits(mut self, limits: ArchiveLimits) -> Self {
        self.archive_limits = limits;
        self
    }

    pub fn with_programs(mut self, programs: Programs) -> Self {
        self.programs = programs;
        self
    }
}
