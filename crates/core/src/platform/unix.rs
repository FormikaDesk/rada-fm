//! Pieces shared by every Unix-like platform.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use super::attrs::{Attrs, FileAttributes, ReparseKind};
use crate::fs::{FileKind, FsMeta};

pub struct UnixAttributes;

impl FileAttributes for UnixAttributes {
    fn attrs(&self, name: &OsStr, meta: &FsMeta) -> Attrs {
        let mode = meta.mode.unwrap_or(0);
        Attrs {
            hidden: name.as_bytes().first() == Some(&b'.'),
            system: false,
            readonly: mode & 0o222 == 0,
            executable: meta.kind == FileKind::File && mode & 0o111 != 0,
            reparse: (meta.kind == FileKind::Symlink).then_some(ReparseKind::Symlink),
        }
    }
}
