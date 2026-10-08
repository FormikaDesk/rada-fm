use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Result, pathcodec};

/// Where a trashed item lives, in a backend-specific way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum TrashHandle {
    /// freedesktop.org Trash specification: the stored file and its `.trashinfo`.
    Freedesktop {
        #[serde(with = "pathcodec::path")]
        stored: PathBuf,
        #[serde(with = "pathcodec::path")]
        info: PathBuf,
    },
    /// Windows Recycle Bin / macOS: an opaque identifier owned by the backend.
    Opaque { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashedItem {
    #[serde(with = "pathcodec::path")]
    pub original: PathBuf,
    pub handle: TrashHandle,
}

pub trait TrashBackend: Send + Sync {
    /// Move `path` (not following a final symlink) to the trash.
    fn trash(&self, path: &Path) -> Result<TrashedItem>;
    /// Put the item back at its original location. Never replaces an existing file.
    fn restore(&self, item: &TrashedItem) -> Result<()>;
    /// Is the item still in the trash?
    fn contains(&self, item: &TrashedItem) -> bool;
}
