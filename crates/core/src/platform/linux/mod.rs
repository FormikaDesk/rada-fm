//! The complete platform: Linux.

mod mounts;
mod open;

use std::sync::Arc;

use super::freedesktop::FreedesktopTrash;
use super::unix::UnixAttributes;
use super::{
    Dirs, FileAttributes, Opener, PathRules, Platform, TrashBackend, UserDirs, VolumeLister,
};
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

    /// Choose which mounts stay out of the disk list.
    pub fn with_device_filter(mut self, filter: super::DeviceFilter) -> Self {
        self.volumes = mounts::LinuxVolumes::with_filter(filter);
        self
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
    fn user_dirs(&self) -> UserDirs {
        // xdg-user-dirs: `$XDG_CONFIG_HOME/user-dirs.dirs`. Without it the system has
        // never been localised, which means the English names.
        match std::fs::read_to_string(self.dirs.config.join("user-dirs.dirs")) {
            Ok(text) => UserDirs::parse_xdg(&text, &self.dirs.home),
            Err(_) => UserDirs::conventional(&self.dirs.home, "Videos"),
        }
    }
}
