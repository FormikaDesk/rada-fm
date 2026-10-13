//! Everything that depends on the operating system lives behind [`Platform`].
//!
//! The rest of the core never branches on the OS: it asks the platform. Linux is
//! complete; Windows and macOS are compilable stubs that already model the concepts
//! those systems need (drive letters, NTFS junctions and reparse points,
//! hidden/system attributes, the Windows Recycle Bin, Finder trash).

pub mod access;
pub mod attrs;
pub mod bplist;
pub mod dirs;
pub mod dirtrash;
pub mod eject;
pub mod fidelity;
#[cfg(unix)]
pub mod freedesktop;
pub mod macos;
pub mod tags;
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

pub use access::AccessDenied;
pub use attrs::{Attrs, FileAttributes, ReparseKind};
pub use dirs::Dirs;
pub use fidelity::{AttrOutcome, DestSupport, Fidelity, NotYet, Preserve, SourceAttrs};
pub use tags::{Tag, TagColor, TagStore};
pub use trash::{TrashBackend, TrashHandle, TrashedItem};
pub use userdirs::{PlaceKind, UserDirs};
pub use volumes::{DeviceFilter, Volume, VolumeKind, VolumeLister};

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
    /// Open with a program the user names (a command line, which may carry options), detached
    /// from the terminal. An empty `program` asks the system to offer its own choice, where
    /// it has one.
    fn open_with(&self, _path: &Path, _program: &str) -> Result<()> {
        Err(crate::Error::Unsupported("open with"))
    }
    /// Open a new terminal window in `dir`.
    fn open_terminal(&self, _dir: &Path) -> Result<()> {
        Err(crate::Error::Unsupported("open a terminal"))
    }
    /// Open an address (a page of the system settings: `x-apple.systempreferences:…`) with
    /// the program that handles it.
    fn open_url(&self, url: &str) -> Result<()> {
        self.open(Path::new(url))
    }
    /// Whether this system has a quick look (macOS) for [`quick_look`](Self::quick_look).
    fn can_quick_look(&self) -> bool {
        false
    }
    /// Show the system's quick preview of the item, in a window of its own.
    fn quick_look(&self, _path: &Path) -> Result<()> {
        Err(crate::Error::Unsupported("quick look"))
    }
}

/// Split a command line the way a shell would for the simple cases: words separated by
/// blanks, with single or double quotes keeping blanks inside a word.
pub fn split_command(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        words.push(cur);
    }
    words
}

#[cfg(test)]
mod command_tests {
    use super::split_command;

    #[test]
    fn a_command_line_splits_like_a_shell_would_for_simple_cases() {
        assert_eq!(split_command("vlc --fullscreen"), ["vlc", "--fullscreen"]);
        assert_eq!(
            split_command(r#"  code  "my folder" 'a b' c"#),
            ["code", "my folder", "a b", "c"]
        );
        assert_eq!(split_command("x ''"), ["x", ""]);
        assert!(split_command("   ").is_empty());
    }
}

pub trait Platform: Send + Sync {
    fn name(&self) -> &'static str;
    fn dirs(&self) -> &Dirs;
    fn trash(&self) -> &dyn TrashBackend;
    fn volumes(&self) -> &dyn VolumeLister;
    fn attributes(&self) -> &dyn FileAttributes;
    fn opener(&self) -> &dyn Opener;
    fn path_rules(&self) -> PathRules;
    /// Owner, extended attributes and ACLs: what a copy keeps besides the bytes.
    fn fidelity(&self) -> &dyn Fidelity;
    /// The user's standard folders under their real names. May read files: call it from
    /// a worker, not from the interface thread.
    fn user_dirs(&self) -> UserDirs;
    /// The names the system's own file manager shows for these folders and drives when they
    /// differ from the name on disk ("Scrivania" for `Desktop`, "Macintosh HD" for `/`). Only
    /// the ones that differ are returned. The platform adds the places it knows are worth
    /// naming. May talk to the system: call it from a worker.
    fn display_names(&self, _places: &[std::path::PathBuf]) -> Vec<(std::path::PathBuf, String)> {
        Vec::new()
    }
    /// Places of this system that the standard folders do not cover (iCloud Drive on macOS),
    /// those that exist.
    fn extra_places(&self) -> Vec<(PlaceKind, std::path::PathBuf)> {
        Vec::new()
    }
    /// Where tags are kept on this system; `None` where they are not supported (Windows).
    fn tags(&self) -> Option<&dyn TagStore> {
        None
    }
    /// When opening `path` failed with `err`: whether the system is waiting for the person's
    /// consent, and what to tell them.
    fn explain_denied(&self, _path: &Path, _err: &std::io::Error) -> Option<AccessDenied> {
        None
    }
}

/// Keep the name the system shows for `path` only if it is not the name on disk.
pub fn only_different(path: &Path, shown: String) -> Option<(std::path::PathBuf, String)> {
    let on_disk = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    (!shown.is_empty() && shown != on_disk).then(|| (path.to_path_buf(), shown))
}

/// Choices of the user that reach the platform layer.
#[derive(Clone, Debug, Default)]
pub struct PlatformOptions {
    /// Replaces the built-in list of mounts kept out of the disk list (`devices.hide`).
    /// Only the Linux platform lists mounts for now.
    pub hide_devices: Option<Vec<String>>,
}

/// The platform of this system with its trash kept inside `dirs` (a folder that is `dirs`'s
/// own): for tests, which must never put anything in the real trash of the machine. On Linux
/// the trash already follows `XDG_DATA_HOME`, so this is [`current`].
pub fn sandboxed(dirs: Dirs) -> Arc<dyn Platform> {
    #[cfg(target_os = "windows")]
    {
        let trash = dirtrash::DirTrash::new(dirs.home_trash(), Arc::new(crate::fs::LocalFs));
        Arc::new(
            windows::WindowsPlatform::with_trash(dirs, Box::new(trash)).with_sandbox_user_dirs(),
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(not(target_os = "linux"))]
        {
            let trash = dirtrash::DirTrash::new(dirs.home_trash(), Arc::new(crate::fs::LocalFs));
            Arc::new(macos::MacPlatform::with_trash(dirs, Box::new(trash)).with_sandbox_user_dirs())
        }
        #[cfg(target_os = "linux")]
        current(dirs)
    }
}

/// The one place where the operating system is selected.
pub fn current(dirs: Dirs) -> Arc<dyn Platform> {
    current_with(dirs, PlatformOptions::default())
}

/// [`current`], with the user's choices.
pub fn current_with(dirs: Dirs, options: PlatformOptions) -> Arc<dyn Platform> {
    #[cfg(target_os = "linux")]
    {
        let filter = options
            .hide_devices
            .as_deref()
            .map(DeviceFilter::new)
            .unwrap_or_default();
        Arc::new(linux::LinuxPlatform::new(dirs).with_device_filter(filter))
    }
    #[cfg(not(target_os = "linux"))]
    let _ = options;
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
