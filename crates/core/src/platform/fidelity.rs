//! What a copy keeps besides the bytes: owner and group, extended attributes (including
//! POSIX ACLs and other labels). Linux does all of it; Windows (NTFS ACLs, alternate streams)
//! and macOS (extended attributes, resource forks) are documented stubs for the platform phase.

use std::path::Path;

/// What to carry over from the source to the copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preserve {
    pub owner: bool,
    pub xattrs: bool,
}

impl Default for Preserve {
    fn default() -> Self {
        Preserve {
            owner: true,
            xattrs: true,
        }
    }
}

/// What could not be carried over, in words, for the report after the operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttrOutcome {
    pub not_preserved: Vec<String>,
}

impl AttrOutcome {
    pub fn note(&mut self, what: impl Into<String>) {
        let what = what.into();
        if !self.not_preserved.contains(&what) {
            self.not_preserved.push(what);
        }
    }
}

/// What the filesystem of a destination folder accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DestSupport {
    pub xattrs: bool,
    pub acl: bool,
}

/// What a source item carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceAttrs {
    /// Extended attributes other than ACLs.
    pub xattrs: bool,
    pub acl: bool,
}

impl SourceAttrs {
    pub fn any(&self) -> bool {
        self.xattrs || self.acl
    }
}

pub trait Fidelity: Send + Sync {
    /// Is this implemented on this platform at all? When not, the plan says so once.
    fn implemented(&self) -> bool;

    /// What the destination folder's filesystem accepts. Reads only (a plan modifies nothing).
    fn destination_support(&self, dir: &Path) -> DestSupport;

    /// Extended attributes and ACLs of an item. Reads only.
    fn source_attributes(&self, path: &Path) -> SourceAttrs;

    /// Could this process give a file to `uid`:`gid`?
    fn can_set_owner(&self, uid: u32, gid: u32) -> bool;

    /// Carry owner/group and extended attributes from `src` to the existing item `dst`.
    /// Best effort: whatever fails is reported in the outcome, never as an error.
    fn copy_attributes(
        &self,
        src: &Path,
        dst: &Path,
        what: Preserve,
        owner: Option<(u32, u32)>,
    ) -> AttrOutcome;
}

/// Platforms where this is not done yet: nothing is carried over, and `implemented` says so.
pub struct NotYet;

impl Fidelity for NotYet {
    fn implemented(&self) -> bool {
        false
    }
    fn destination_support(&self, _dir: &Path) -> DestSupport {
        DestSupport {
            xattrs: true,
            acl: true,
        }
    }
    fn source_attributes(&self, _path: &Path) -> SourceAttrs {
        SourceAttrs::default()
    }
    fn can_set_owner(&self, _uid: u32, _gid: u32) -> bool {
        false
    }
    fn copy_attributes(
        &self,
        _s: &Path,
        _d: &Path,
        _w: Preserve,
        _o: Option<(u32, u32)>,
    ) -> AttrOutcome {
        AttrOutcome::default()
    }
}
