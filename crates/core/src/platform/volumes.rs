use std::path::PathBuf;
use std::time::Duration;

use crate::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Virtual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub mount_point: PathBuf,
    pub label: Option<String>,
    pub fs_type: String,
    pub device: String,
    pub kind: VolumeKind,
    /// Windows drive letter (`C`); always `None` elsewhere.
    pub drive_letter: Option<char>,
    pub total: Option<u64>,
    pub available: Option<u64>,
    pub read_only: bool,
    /// False when the mount did not answer in time (hung network share): it is
    /// listed anyway, but nothing is read from it.
    pub responsive: bool,
}

/// Enumerates mounted volumes. May block: only ever call it from a worker thread.
pub trait VolumeLister: Send + Sync {
    fn list(&self) -> Result<Vec<Volume>>;

    /// Wait up to `timeout` for the set of mounts to change; `true` if it did.
    /// The default just sleeps, so callers fall back to periodic polling.
    fn wait_for_change(&self, timeout: Duration) -> bool {
        std::thread::sleep(timeout);
        false
    }
}
