use std::ffi::OsStr;

use crate::fs::FsMeta;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReparseKind {
    Symlink,
    /// NTFS directory junction.
    Junction,
    MountPoint,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs {
    pub hidden: bool,
    pub system: bool,
    pub readonly: bool,
    pub executable: bool,
    pub reparse: Option<ReparseKind>,
}

pub trait FileAttributes: Send + Sync {
    fn attrs(&self, name: &OsStr, meta: &FsMeta) -> Attrs;

    /// The name the system's own file manager shows for this entry when it is not the name on
    /// the disk (the Finder drops `.localized` from a folder's name). `None`: the name as it is.
    fn shown_name(&self, _name: &OsStr, _meta: &FsMeta) -> Option<std::ffi::OsString> {
        None
    }

    /// Whether this folder is one the system treats as a single thing (a macOS package:
    /// `.app`, `.photoslibrary`, `.pages`…). Asked by the system, not guessed from the name.
    /// May read the disk: only called from workers.
    fn is_package(&self, _path: &std::path::Path, _meta: &FsMeta) -> bool {
        false
    }
}
