//! Everything that depends on the operating system lives behind [`Platform`].
//!
//! The rest of the core never branches on the OS: it asks the platform. Linux is
//! complete; Windows and macOS are compilable stubs that already model the concepts
//! those systems need (drive letters, NTFS junctions and reparse points,
//! hidden/system attributes, the Windows Recycle Bin, Finder trash).

pub mod attrs;
pub mod dirs;
#[cfg(unix)]
pub mod freedesktop;
pub mod macos;
pub mod trash;
pub mod userdirs;
pub mod volumes;
pub mod windows;

#[cfg(unix)]
pub mod unix;

#[cfg(target_os = "linux")]
pub mod linux;

use std::path::Path;
use std::sync::Arc;

pub use attrs::{Attrs, FileAttributes, ReparseKind};
pub use dirs::Dirs;
pub use trash::{TrashBackend, TrashHandle, TrashedItem};
pub use userdirs::{PlaceKind, UserDirs};
pub use volumes::{Volume, VolumeKind, VolumeLister};

use crate::Result;

/// How names behave on this platform's usual filesystems.
#[derive(Clone, Debug)]
pub struct PathRules {
    /// `Foo` and `foo` are the same file (Windows, default macOS).
    pub case_insensitive: bool,
    /// Canonically equivalent Unicode names are the same file (APFS/HFS+ normalise).
    pub normalization_insensitive: bool,
    pub forbidden_chars: &'static [char],
    pub reserved_names: &'static [&'static str],
    pub max_name_bytes: usize,
}

impl PathRules {
    pub const POSIX: PathRules = PathRules {
        case_insensitive: false,
        normalization_insensitive: false,
        forbidden_chars: &['/', '\0'],
        reserved_names: &[],
        max_name_bytes: 255,
    };
}

pub trait Opener: Send + Sync {
    /// Open with the default application, detached from the terminal.
    fn open(&self, path: &Path) -> Result<()>;
    /// Show the item in the system file manager.
    fn reveal(&self, path: &Path) -> Result<()>;
}

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;
    fn dirs(&self) -> &Dirs;
    fn trash(&self) -> &dyn TrashBackend;
    fn volumes(&self) -> &dyn VolumeLister;
    fn attributes(&self) -> &dyn FileAttributes;
    fn opener(&self) -> &dyn Opener;
    fn path_rules(&self) -> PathRules;
    /// The user's standard folders under their real names. May read files: call it from
    /// a worker, not from the interface thread.
    fn user_dirs(&self) -> UserDirs;
}

/// The one place where the operating system is selected.
pub fn current(dirs: Dirs) -> Arc<dyn Platform> {
    #[cfg(target_os = "linux")]
    {
        Arc::new(linux::LinuxPlatform::new(dirs))
    }
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsPlatform::new(dirs))
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(macos::MacPlatform::new(dirs))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Arc::new(macos::GenericUnixStub::new(dirs))
    }
}
