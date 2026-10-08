//! Filesystem abstraction.
//!
//! Every operation of the engine goes through [`FsEngine`]. The local implementation
//! lives in [`local`]; the indirection exists so that tests can inject faults
//! (unreadable files, `EXDEV`, disk full) and so that remote or archive back-ends can
//! be added later without touching the engine.

pub mod local;
pub mod tree;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use local::LocalFs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpecialKind {
    Fifo,
    Socket,
    BlockDevice,
    CharDevice,
    Unknown,
}

/// Metadata of a path obtained *without* following symlinks (unless from `stat`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsMeta {
    pub kind: FileKind,
    pub special: Option<SpecialKind>,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub atime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
    /// Creation (birth) time, where the filesystem records it.
    pub btime: Option<SystemTime>,
    pub mode: Option<u32>,
    pub dev: Option<u64>,
    pub ino: Option<u64>,
    pub nlink: Option<u64>,
    pub readonly: bool,
    /// Raw Windows file attributes (hidden, system, reparse point, ...). `None` elsewhere.
    pub os_attrs: Option<u32>,
}

impl FsMeta {
    pub fn is_dir(&self) -> bool {
        self.kind == FileKind::Dir
    }
    pub fn is_file(&self) -> bool {
        self.kind == FileKind::File
    }
    pub fn is_symlink(&self) -> bool {
        self.kind == FileKind::Symlink
    }

    pub fn from_std(m: &std::fs::Metadata) -> Self {
        let ft = m.file_type();
        let mut special = None;
        let kind = if ft.is_symlink() {
            FileKind::Symlink
        } else if ft.is_dir() {
            FileKind::Dir
        } else if ft.is_file() {
            FileKind::File
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                special = Some(if ft.is_fifo() {
                    SpecialKind::Fifo
                } else if ft.is_socket() {
                    SpecialKind::Socket
                } else if ft.is_block_device() {
                    SpecialKind::BlockDevice
                } else if ft.is_char_device() {
                    SpecialKind::CharDevice
                } else {
                    SpecialKind::Unknown
                });
            }
            #[cfg(not(unix))]
            {
                special = Some(SpecialKind::Unknown);
            }
            FileKind::Other
        };
        #[cfg(unix)]
        let (mode, dev, ino, nlink, ctime, atime) = {
            use std::os::unix::fs::MetadataExt;
            let ct = stamp_to_time(m.ctime(), m.ctime_nsec() as u32);
            let at = stamp_to_time(m.atime(), m.atime_nsec() as u32);
            (
                Some(m.mode()),
                Some(m.dev()),
                Some(m.ino()),
                Some(m.nlink()),
                ct,
                at,
            )
        };
        #[cfg(not(unix))]
        let (mode, dev, ino, nlink, ctime, atime) =
            (None, None, None, None, None, m.accessed().ok());
        FsMeta {
            kind,
            special,
            size: m.len(),
            mtime: m.modified().ok(),
            atime,
            ctime,
            btime: m.created().ok(),
            mode,
            dev,
            ino,
            nlink,
            readonly: m.permissions().readonly(),
            os_attrs: os_attrs(m),
        }
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            kind: self.kind,
            size: self.size,
            mtime: self.mtime.map(Stamp::from),
            ctime: self.ctime.map(Stamp::from),
        }
    }
}

fn os_attrs(m: &std::fs::Metadata) -> Option<u32> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Some(m.file_attributes())
    }
    #[cfg(not(windows))]
    {
        let _ = m;
        None
    }
}

#[cfg(unix)]
fn stamp_to_time(secs: i64, nanos: u32) -> Option<SystemTime> {
    if secs >= 0 {
        Some(UNIX_EPOCH + Duration::new(secs as u64, nanos))
    } else {
        UNIX_EPOCH
            .checked_sub(Duration::from_secs(secs.unsigned_abs()))
            .map(|t| t + Duration::from_nanos(nanos as u64))
    }
}

/// A point in time that survives JSON round trips without precision loss.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Stamp {
    pub secs: i64,
    pub nanos: u32,
}

impl From<SystemTime> for Stamp {
    fn from(t: SystemTime) -> Self {
        match t.duration_since(UNIX_EPOCH) {
            Ok(d) => Stamp {
                secs: d.as_secs() as i64,
                nanos: d.subsec_nanos(),
            },
            Err(e) => {
                let d = e.duration();
                let (s, n) = (d.as_secs() as i64, d.subsec_nanos());
                if n == 0 {
                    Stamp { secs: -s, nanos: 0 }
                } else {
                    Stamp {
                        secs: -s - 1,
                        nanos: 1_000_000_000 - n,
                    }
                }
            }
        }
    }
}

impl From<Stamp> for SystemTime {
    fn from(s: Stamp) -> Self {
        if s.secs >= 0 {
            UNIX_EPOCH + Duration::new(s.secs as u64, s.nanos)
        } else {
            UNIX_EPOCH - Duration::from_secs(s.secs.unsigned_abs())
                + Duration::from_nanos(s.nanos as u64)
        }
    }
}

/// Identity of a file at a moment in time, used to detect modifications after the fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Fingerprint {
    pub kind: FileKind,
    pub size: u64,
    pub mtime: Option<Stamp>,
    pub ctime: Option<Stamp>,
}

impl Fingerprint {
    /// Describe how `now` differs from `self`, or `None` when unchanged.
    pub fn diff(&self, now: &Fingerprint) -> Option<String> {
        if self.kind != now.kind {
            return Some("its type changed".into());
        }
        if self.size != now.size {
            return Some(format!(
                "size changed from {} to {} bytes",
                self.size, now.size
            ));
        }
        if self.mtime != now.mtime {
            return Some("its modification time changed".into());
        }
        if self.ctime != now.ctime {
            return Some("its metadata changed".into());
        }
        None
    }
}

pub struct DirItem {
    pub name: OsString,
    pub path: PathBuf,
    /// `lstat` result; a failure is kept per entry so one bad entry never hides the rest.
    pub meta: io::Result<FsMeta>,
}

/// Progress/cancellation hook for byte-oriented work.
pub trait CopyControl {
    /// Called with the number of bytes just transferred. Return `false` to cancel.
    fn advance(&mut self, bytes: u64) -> bool;
}

impl<F: FnMut(u64) -> bool> CopyControl for F {
    fn advance(&mut self, bytes: u64) -> bool {
        self(bytes)
    }
}

#[derive(Clone, Debug)]
pub struct CopyRequest {
    pub src: PathBuf,
    /// Created exclusively: fails with `AlreadyExists` if present.
    pub dst: PathBuf,
    /// Final permission bits (only the lower 9 are applied). `None` keeps the default.
    pub mode: Option<u32>,
    pub mtime: Option<SystemTime>,
    pub atime: Option<SystemTime>,
    /// Re-read the destination and compare it with what was read from the source.
    pub verify: bool,
    /// `fsync` before returning (always done when `verify` is set).
    pub sync: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyMethod {
    Empty,
    Reflink,
    KernelCopy,
    ReadWrite,
}

#[derive(Clone, Debug)]
pub struct CopyOutcome {
    pub bytes: u64,
    pub method: CopyMethod,
}

pub trait FsEngine: Send + Sync {
    fn lstat(&self, p: &Path) -> io::Result<FsMeta>;
    fn stat(&self, p: &Path) -> io::Result<FsMeta>;
    fn read_dir(&self, p: &Path) -> io::Result<Vec<DirItem>>;
    fn read_link(&self, p: &Path) -> io::Result<PathBuf>;

    fn create_dir(&self, p: &Path, mode: Option<u32>) -> io::Result<()>;
    fn create_symlink(&self, target: &Path, link: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Rename that fails with `AlreadyExists` instead of replacing the destination.
    fn rename_noreplace(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, p: &Path) -> io::Result<()>;
    fn remove_dir(&self, p: &Path) -> io::Result<()>;
    fn set_mode(&self, p: &Path, mode: u32) -> io::Result<()>;
    fn set_mtime(&self, p: &Path, mtime: SystemTime, atime: Option<SystemTime>) -> io::Result<()>;

    fn copy_file(&self, req: &CopyRequest, ctl: &mut dyn CopyControl) -> io::Result<CopyOutcome>;

    fn available_space(&self, p: &Path) -> io::Result<u64>;
    fn can_read(&self, p: &Path) -> bool;
    fn can_write(&self, p: &Path) -> bool;
}
