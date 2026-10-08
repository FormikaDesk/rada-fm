//! The complete platform: Linux.

mod mounts;
mod open;

use std::sync::Arc;

use super::freedesktop::FreedesktopTrash;
use super::unix::UnixAttributes;
use super::{Dirs, FileAttributes, Opener, PathRules, Platform, TrashBackend, VolumeLister};
use crate::fs::LocalFs;

pub struct LinuxPlatform {
    dirs: Dirs,
    trash: FreedesktopTrash,
    volumes: mounts::LinuxVolumes,
    attrs: UnixAttributes,
    opener: open::XdgOpener,
}

impl LinuxPlatform {
    pub fn new(dirs: Dirs) -> Self {
        let trash = FreedesktopTrash::new(dirs.home_trash(), Arc::new(LocalFs));
        Self::with_trash(dirs, trash)
    }

    /// Use a customised trash backend (tests).
    pub fn with_trash(dirs: Dirs, trash: FreedesktopTrash) -> Self {
        LinuxPlatform {
            dirs,
            trash,
            volumes: mounts::LinuxVolumes::new(),
            attrs: UnixAttributes,
            opener: open::XdgOpener,
        }
    }
}

impl Platform for LinuxPlatform {
    fn name(&self) -> &'static str {
        "linux"
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
    fn path_rules(&self) -> PathRules {
        PathRules::POSIX
    }
}
