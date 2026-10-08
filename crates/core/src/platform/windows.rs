//! Windows: compilable stub.
//!
//! The *model* is complete so the rest of the core never needs an `if windows`:
//! drive letters (`Volume::drive_letter`), NTFS junctions and reparse points
//! (`ReparseKind`, superfile #927), hidden/system attributes and case-insensitive
//! names (`PathRules`). The operations that need the Win32 API
//! (`IFileOperation` for the Recycle Bin, `GetLogicalDrives` for volumes,
//! `ShellExecute` for opening) return [`Error::Unsupported`] for now.

use std::ffi::OsStr;
use std::path::Path;

use super::attrs::{Attrs, FileAttributes, ReparseKind};
use super::trash::{TrashBackend, TrashedItem};
use super::volumes::{Volume, VolumeLister};
use super::{Dirs, Opener, PathRules, Platform};
use crate::fs::FsMeta;
use crate::{Error, Result};

const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub struct WindowsAttributes;

impl FileAttributes for WindowsAttributes {
    fn attrs(&self, _name: &OsStr, meta: &FsMeta) -> Attrs {
        let raw = meta.os_attrs.unwrap_or(0);
        let reparse = if raw & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            // Telling a symlink from a junction needs the reparse tag (DeviceIoControl);
            // until that is implemented, a directory reparse point that is not a
            // symlink is the junction case.
            Some(if meta.is_symlink() {
                ReparseKind::Symlink
            } else if meta.is_dir() {
                ReparseKind::Junction
            } else {
                ReparseKind::Other
            })
        } else {
            None
        };
        Attrs {
            hidden: raw & FILE_ATTRIBUTE_HIDDEN != 0,
            system: raw & FILE_ATTRIBUTE_SYSTEM != 0,
            readonly: raw & FILE_ATTRIBUTE_READONLY != 0 || meta.readonly,
            executable: false,
            reparse,
        }
    }
}

struct WindowsTrash;
impl TrashBackend for WindowsTrash {
    fn trash(&self, _: &Path) -> Result<TrashedItem> {
        Err(Error::Unsupported("Windows Recycle Bin (IFileOperation)"))
    }
    fn restore(&self, _: &TrashedItem) -> Result<()> {
        Err(Error::Unsupported("Windows Recycle Bin restore"))
    }
    fn contains(&self, _: &TrashedItem) -> bool {
        false
    }
}

struct WindowsVolumes;
impl VolumeLister for WindowsVolumes {
    fn list(&self) -> Result<Vec<Volume>> {
        Err(Error::Unsupported("drive letters (GetLogicalDrives)"))
    }
}

struct WindowsOpener;
impl Opener for WindowsOpener {
    fn open(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("ShellExecute"))
    }
    fn reveal(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("explorer /select"))
    }
}

pub struct WindowsPlatform {
    dirs: Dirs,
    attrs: WindowsAttributes,
    trash: WindowsTrash,
    volumes: WindowsVolumes,
    opener: WindowsOpener,
}

impl WindowsPlatform {
    pub fn new(dirs: Dirs) -> Self {
        WindowsPlatform {
            dirs,
            attrs: WindowsAttributes,
            trash: WindowsTrash,
            volumes: WindowsVolumes,
            opener: WindowsOpener,
        }
    }
}

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
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
        PathRules {
            case_insensitive: true,
            normalization_insensitive: false,
            forbidden_chars: &['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\0'],
            reserved_names: &[
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ],
            max_name_bytes: 255,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FileKind;

    fn meta(kind: FileKind, attrs: u32) -> FsMeta {
        FsMeta {
            kind,
            special: None,
            size: 0,
            mtime: None,
            atime: None,
            ctime: None,
            btime: None,
            mode: None,
            dev: None,
            ino: None,
            nlink: None,
            readonly: false,
            os_attrs: Some(attrs),
        }
    }

    #[test]
    fn hidden_system_and_junction_attributes_are_modelled() {
        let a = WindowsAttributes.attrs(OsStr::new("x"), &meta(FileKind::File, 0x2 | 0x4));
        assert!(a.hidden && a.system);
        let j = WindowsAttributes.attrs(OsStr::new("link"), &meta(FileKind::Dir, 0x400 | 0x10));
        assert_eq!(j.reparse, Some(ReparseKind::Junction));
    }
}
