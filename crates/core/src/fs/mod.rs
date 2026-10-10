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

/// [`std::fs::canonicalize`] with the path written the way people write it. On Windows the
/// standard function answers `\\?\C:\Users\me`; that form is for the system, and showing it
/// in an address bar or comparing it with a plain `C:\Users\me` goes wrong. Everywhere else
/// this is `canonicalize`.
pub fn canonical_path(p: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(p).map(plain_path)
}

/// `\\?\C:\x` to `C:\x`, and `\\?\UNC\server\share` to `\\server\share`; a verbatim path
/// that has no plain form (a very long one) is left as it is.
pub fn plain_path(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let Some(Component::Prefix(prefix)) = p.components().next() else {
            return p;
        };
        let text = p.to_string_lossy().into_owned();
        let plain = match prefix.kind() {
            Prefix::VerbatimDisk(_) => text.strip_prefix(r"\\?\").map(str::to_owned),
            Prefix::VerbatimUNC(..) => text
                .strip_prefix(r"\\?\UNC\")
                .map(|rest| format!(r"\\{rest}")),
            _ => None,
        };
        match plain {
            // Past MAX_PATH only the verbatim form works.
            Some(s) if s.len() < 248 => PathBuf::from(s),
            _ => p,
        }
    }
    #[cfg(not(windows))]
    p
}

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
    /// Owner and group (Unix).
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// 512-byte blocks really allocated (Unix); less than `size` means a sparse file.
    pub blocks: Option<u64>,
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
        let (mode, dev, ino, nlink, ctime, atime, uid, gid, blocks) = {
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
                Some(m.uid()),
                Some(m.gid()),
                Some(m.blocks()),
            )
        };
        #[cfg(not(unix))]
        let (mode, dev, ino, nlink, ctime, atime, uid, gid, blocks) = (
            None,
            None,
            None,
            None,
            None,
            m.accessed().ok(),
            None,
            None,
            None,
        );
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
            uid,
            gid,
            blocks,
            readonly: m.permissions().readonly(),
            os_attrs: os_attrs(m),
        }
    }

    /// Bytes really stored: less than `size` for a sparse file.
    pub fn stored_bytes(&self) -> u64 {
        match (self.kind, self.blocks) {
            (FileKind::File, Some(b)) => b.saturating_mul(512).min(self.size),
            _ => self.size,
        }
    }

    pub fn is_sparse(&self) -> bool {
        self.kind == FileKind::File
            && self.size > 0
            && self
                .blocks
                .is_some_and(|b| b.saturating_mul(512) < self.size)
    }

    pub fn fingerprint(&self) -> Fingerprint {
        // Creating or removing a hard link changes the status-change time of every name of
        // the file: for a file with several names it says nothing about its contents.
        let shared = self.kind == FileKind::File && self.nlink.unwrap_or(1) > 1;
        Fingerprint {
            kind: self.kind,
            size: self.size,
            mtime: self.mtime.map(Stamp::from),
            ctime: if shared {
                None
            } else {
                self.ctime.map(Stamp::from)
            },
        }
    }

    /// Like [`fingerprint`](Self::fingerprint) without the status-change time: for a file
    /// that is about to get more names.
    pub fn fingerprint_without_ctime(&self) -> Fingerprint {
        Fingerprint {
            ctime: None,
            ..self.fingerprint()
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
        // A missing time on either side means "not comparable" (a file with several names).
        if let (Some(a), Some(b)) = (self.ctime, now.ctime)
            && a != b
        {
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
    /// Only the data regions of a sparse file were copied; the holes stay holes.
    Sparse,
}

#[derive(Clone, Debug)]
pub struct CopyOutcome {
    /// Bytes actually transferred: for a sparse file, the data without the holes.
    pub bytes: u64,
    pub method: CopyMethod,
}

/// A new file being written (and, for formats that go back to patch a header, seeked).
pub trait FileSink: std::io::Write + std::io::Seek + Send {}
impl<T: std::io::Write + std::io::Seek + Send> FileSink for T {}

pub trait FsEngine: Send + Sync {
    fn lstat(&self, p: &Path) -> io::Result<FsMeta>;
    fn stat(&self, p: &Path) -> io::Result<FsMeta>;
    fn read_dir(&self, p: &Path) -> io::Result<Vec<DirItem>>;
    fn read_link(&self, p: &Path) -> io::Result<PathBuf>;

    fn create_dir(&self, p: &Path, mode: Option<u32>) -> io::Result<()>;
    fn create_symlink(&self, target: &Path, link: &Path) -> io::Result<()>;
    /// A new, empty file opened for writing; fails with `AlreadyExists` if the name is taken.
    /// `mode` is the permission bits it is created with (the process's umask still applies).
    fn create_file(&self, p: &Path, mode: u32) -> io::Result<Box<dyn FileSink>>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Rename that fails with `AlreadyExists` instead of replacing the destination.
    fn rename_noreplace(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, p: &Path) -> io::Result<()>;
    fn remove_dir(&self, p: &Path) -> io::Result<()>;
    fn set_mode(&self, p: &Path, mode: u32) -> io::Result<()>;
    fn set_mtime(&self, p: &Path, mtime: SystemTime, atime: Option<SystemTime>) -> io::Result<()>;
    /// A new name `link` for the existing file `existing` (a hard link). Fails with
    /// `AlreadyExists` if `link` is taken.
    fn hard_link(&self, existing: &Path, link: &Path) -> io::Result<()>;

    fn copy_file(&self, req: &CopyRequest, ctl: &mut dyn CopyControl) -> io::Result<CopyOutcome>;

    fn available_space(&self, p: &Path) -> io::Result<u64>;
    fn can_read(&self, p: &Path) -> bool;
    fn can_write(&self, p: &Path) -> bool;
}
