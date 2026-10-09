//! Archives as part of the file system: listing, browsing, reading and creating them.
//!
//! An archive is a read-only virtual folder. A path **inside** one is the archive's path
//! followed by the member's path (`/home/u/photos.zip/2024/a.jpg`). That path cannot exist on
//! a disk, because `photos.zip` is a file, so [`locate`] tells without ambiguity where an
//! ordinary path ends and an archive begins. Everything else in rada (breadcrumb, history,
//! clipboard, previews) keeps working on paths.
//!
//! Formats are read with pure-Rust libraries wherever one exists (ZIP, tar with gzip, bzip2,
//! xz or zstd, 7z), so the same code builds on every platform. RAR is read only through an
//! external program (`7z`, `7zz` or `unrar`) for licence reasons.

pub mod entry;
pub mod external;
pub mod format;
pub mod index;
pub mod limits;
pub mod reader;
#[cfg(any(test, feature = "testutil"))]
pub mod testkit;
pub mod writer;

use std::io;
use std::path::{Path, PathBuf};

pub use entry::{Entry, EntryKind, NameIssue, link_escapes};
pub use format::{
    ArchiveKind, Compression, Format, archive_stem, name_is_zip_document, name_suggests_archive,
};
pub use index::{Child, Index, ListControl};
pub use limits::ArchiveLimits;
pub use reader::Session;

use crate::fs::FsEngine;

/// Why an archive could not be read.
#[derive(Debug)]
pub enum ArchiveError {
    NotAnArchive,
    /// Truncated or corrupt: the reason, as the reader reports it.
    Damaged(String),
    /// The archive (or its list of names) is protected by a password.
    Encrypted,
    /// A format that needs a program that is not installed.
    ToolMissing(String),
    Unsupported(String),
    Io(io::Error),
    Cancelled,
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::NotAnArchive => write!(f, "this is not an archive rada can read"),
            ArchiveError::Damaged(why) => write!(f, "the archive is damaged or incomplete ({why})"),
            ArchiveError::Encrypted => write!(
                f,
                "the archive is protected by a password, which rada cannot use yet"
            ),
            ArchiveError::ToolMissing(msg) => write!(f, "{msg}"),
            ArchiveError::Unsupported(what) => write!(f, "not supported: {what}"),
            ArchiveError::Io(e) => write!(f, "{e}"),
            ArchiveError::Cancelled => write!(f, "operation cancelled"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<io::Error> for ArchiveError {
    fn from(e: io::Error) -> Self {
        // Decoders report corrupt or cut-off data as plain I/O errors of these kinds.
        match e.kind() {
            io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData => {
                ArchiveError::Damaged(e.to_string())
            }
            io::ErrorKind::Interrupted if e.to_string() == CANCELLED => ArchiveError::Cancelled,
            _ => ArchiveError::Io(e),
        }
    }
}

/// The text of the I/O error a reader returns when asked to stop.
pub(crate) const CANCELLED: &str = "archive read cancelled";

pub(crate) fn cancelled_io() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, CANCELLED)
}

impl ArchiveError {
    pub fn into_error(self, archive: &Path) -> crate::Error {
        match self {
            ArchiveError::Cancelled => crate::Error::Cancelled,
            ArchiveError::Io(e) => crate::Error::io("read archive", archive, e),
            other => crate::Error::Archive {
                path: archive.to_path_buf(),
                message: other.to_string(),
            },
        }
    }
}

/// Where a path is: in an ordinary folder, or inside an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// The archive file.
    pub archive: PathBuf,
    /// The member path inside it; empty for the archive's top.
    pub inner: PathBuf,
}

/// The archive `path` lies in or is, or `None` for an ordinary path.
///
/// An ordinary path costs one `stat`. A path in an archive is found by walking up until a
/// regular file is met.
pub fn locate(fs: &dyn FsEngine, path: &Path) -> Option<Location> {
    for anc in path.ancestors() {
        if anc.as_os_str().is_empty() {
            return None;
        }
        match fs.stat(anc) {
            Ok(m) if m.is_dir() => return None,
            Ok(m) if m.is_file() => {
                let inner = path.strip_prefix(anc).ok()?.to_path_buf();
                return Some(Location {
                    archive: anc.to_path_buf(),
                    inner,
                });
            }
            Ok(_) => return None,
            Err(_) => continue,
        }
    }
    None
}

/// Whether `path` is inside an archive (not the archive file itself).
pub fn is_inside(fs: &dyn FsEngine, path: &Path) -> bool {
    locate(fs, path).is_some_and(|l| !l.inner.as_os_str().is_empty())
}
