use std::sync::Arc;

use crate::fs::{FsEngine, LocalFs};
use crate::platform::Platform;

/// Everything an operation needs from the outside world.
#[derive(Clone)]
pub struct Engine {
    pub fs: Arc<dyn FsEngine>,
    pub platform: Arc<dyn Platform>,
}

impl Engine {
    pub fn new(fs: Arc<dyn FsEngine>, platform: Arc<dyn Platform>) -> Self {
        Engine { fs, platform }
    }

    /// The real local filesystem with the given platform.
    pub fn local(platform: Arc<dyn Platform>) -> Self {
        Engine {
            fs: Arc::new(LocalFs),
            platform,
        }
    }
}
