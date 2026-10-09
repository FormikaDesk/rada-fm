//! macOS: compilable stub (also used as the fallback for other Unix systems).
//!
//! Planned: `NSFileManager trashItem` for the trash (with "Put Back" metadata),
//! `getfsstat` for volumes (always from a worker, with a timeout),
//! `clonefile`/`copyfile` for fast copies, NFD-insensitive and case-insensitive names.

use std::path::Path;

use super::trash::{TrashBackend, TrashedItem};
use super::volumes::{Volume, VolumeLister};
use super::{Dirs, FileAttributes, Opener, PathRules, Platform, UserDirs};
use crate::{Error, Result};

struct MacTrash;
impl TrashBackend for MacTrash {
    fn trash(&self, _: &Path) -> Result<TrashedItem> {
        Err(Error::Unsupported("macOS Trash (NSFileManager trashItem)"))
    }
    fn restore(&self, _: &TrashedItem) -> Result<()> {
        Err(Error::Unsupported("macOS Trash restore"))
    }
    fn contains(&self, _: &TrashedItem) -> bool {
        false
    }
}

struct MacVolumes;
impl VolumeLister for MacVolumes {
    fn list(&self) -> Result<Vec<Volume>> {
        Err(Error::Unsupported("mounted volumes (getfsstat)"))
    }
}

struct MacOpener;
impl Opener for MacOpener {
    fn open(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("open(1)"))
    }
    fn reveal(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("open -R"))
    }
}

#[cfg(unix)]
type Attrs = super::unix::UnixAttributes;
#[cfg(not(unix))]
type Attrs = super::windows::WindowsAttributes;

#[cfg(unix)]
fn attrs() -> Attrs {
    super::unix::UnixAttributes
}
#[cfg(not(unix))]
fn attrs() -> Attrs {
    super::windows::WindowsAttributes
}

pub struct MacPlatform {
    dirs: Dirs,
    attrs: Attrs,
    trash: MacTrash,
    volumes: MacVolumes,
    opener: MacOpener,
}

impl MacPlatform {
    pub fn new(dirs: Dirs) -> Self {
        MacPlatform {
            dirs,
            attrs: attrs(),
            trash: MacTrash,
            volumes: MacVolumes,
            opener: MacOpener,
        }
    }
}

/// Alias used by `platform::current` on systems without a dedicated implementation.
pub type GenericUnixStub = MacPlatform;

impl Platform for MacPlatform {
    fn name(&self) -> &'static str {
        "macos"
    }
    fn dirs(&self) -> &Dirs {
        &self.dirs
    }
    fn trash(&self) -> &dyn TrashBackend {
        &self.trash
    }
    fn volumes(&self) -> &dyn VolumeLister {
        &self.volumes
    }
    fn attributes(&self) -> &dyn FileAttributes {
        &self.attrs
    }
    fn opener(&self) -> &dyn Opener {
        &self.opener
    }
    fn user_dirs(&self) -> UserDirs {
        // The Finder's standard folders; the system API comes in a later phase.
        UserDirs::conventional(&self.dirs.home, "Movies")
    }
    fn path_rules(&self) -> PathRules {
        PathRules {
            case_insensitive: true,
            normalization_insensitive: true,
            forbidden_chars: &['/', '\0'],
            reserved_names: &[],
            max_name_bytes: 255,
        }
    }
}
