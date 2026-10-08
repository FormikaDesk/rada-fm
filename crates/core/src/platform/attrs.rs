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
}
