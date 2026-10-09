//! Error types of the core. Every I/O error carries the operation and the full path.

use std::io;
use std::path::{Path, PathBuf};

use crate::display;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{op} {}: {source}", display::path(.path))]
    Io {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{op} {} -> {}: {source}", display::path(.from), display::path(.to))]
    Io2 {
        op: &'static str,
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{} already exists", display::path(.0))]
    AlreadyExists(PathBuf),

    #[error("{} changed since the operation ({reason}); left untouched", display::path(.path))]
    Modified { path: PathBuf, reason: String },

    #[error("copy of {} could not be verified: {reason}", display::path(.path))]
    VerifyFailed { path: PathBuf, reason: String },

    #[error("{} changed while it was being copied", display::path(.0))]
    SourceChanged(PathBuf),

    #[error("{} is not empty", display::path(.0))]
    NotEmpty(PathBuf),

    #[error("{} and {} are on different filesystems", display::path(.0), display::path(.1))]
    CrossDevice(PathBuf, PathBuf),

    #[error("{}: {message}", display::path(.path))]
    Archive { path: PathBuf, message: String },

    #[error("trash: {0}")]
    Trash(String),

    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),

    #[error("{0}")]
    Invalid(String),

    #[error("journal: {0}")]
    Journal(String),

    #[error("journal data: {0}")]
    Json(#[from] serde_json::Error),

    #[error("operation cancelled")]
    Cancelled,
}

impl Error {
    pub fn io(op: &'static str, path: impl AsRef<Path>, source: io::Error) -> Self {
        Error::Io {
            op,
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    pub fn io2(
        op: &'static str,
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
        source: io::Error,
    ) -> Self {
        Error::Io2 {
            op,
            from: from.as_ref().to_path_buf(),
            to: to.as_ref().to_path_buf(),
            source,
        }
    }

    /// The underlying OS error kind, when there is one.
    pub fn io_kind(&self) -> Option<io::ErrorKind> {
        match self {
            Error::Io { source, .. } | Error::Io2 { source, .. } => Some(source.kind()),
            _ => None,
        }
    }

    pub fn is_cross_device(&self) -> bool {
        match self {
            Error::CrossDevice(..) => true,
            Error::Io { source, .. } | Error::Io2 { source, .. } => is_exdev(source),
            _ => false,
        }
    }
}

/// True for `EXDEV` ("invalid cross-device link").
pub fn is_exdev(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
    }
}

/// True for "permission denied" (`EACCES`, `EPERM`).
pub fn is_denied(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(e.raw_os_error(), Some(libc::EACCES) | Some(libc::EPERM))
    }
    #[cfg(not(unix))]
    {
        e.kind() == io::ErrorKind::PermissionDenied
    }
}

/// True when a hard link could not be made because of where it was asked for (another
/// filesystem, one without links, too many links, no right to link): the file can still be
/// copied.
pub fn is_link_unsupported(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(
            e.raw_os_error(),
            Some(libc::EPERM)
                | Some(libc::EXDEV)
                | Some(libc::EMLINK)
                | Some(libc::ENOTSUP)
                | Some(libc::ENOSYS)
        )
    }
    #[cfg(not(unix))]
    {
        matches!(
            e.kind(),
            io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
        )
    }
}

/// True for `ENOTEMPTY`/`EEXIST` on directory removal.
pub fn is_not_empty(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(e.raw_os_error(), Some(libc::ENOTEMPTY) | Some(libc::EEXIST))
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
    }
}
