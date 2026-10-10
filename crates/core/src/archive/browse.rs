//! An archive's folders as a listing the interface can show like any other folder.

use std::path::PathBuf;

use super::index::{ListControl, open_cached};
use super::{EntryKind, Location};
use crate::display;
use crate::fs::{FileKind, FsMeta};
use crate::model::{Entry, LinkInfo};
use crate::ops::LinkState;
use crate::platform::Platform;
use crate::{Error, Result};

/// What the interface needs to know about the archive it is looking into.
#[derive(Clone, Debug)]
pub struct ArchiveView {
    pub archive: PathBuf,
    /// The folder inside it that is shown (empty for the top).
    pub inner: PathBuf,
    pub format: String,
    pub files: u64,
    pub dirs: u64,
    /// Uncompressed size of the files.
    pub bytes: u64,
    /// Size of the archive file.
    pub packed: u64,
    /// Set when the list is not the whole archive, with the reason.
    pub note: Option<String>,
    /// Members protected by a password.
    pub encrypted: u64,
}

fn synthetic_meta(
    kind: EntryKind,
    size: u64,
    mtime: Option<std::time::SystemTime>,
    mode: Option<u32>,
) -> FsMeta {
    FsMeta {
        kind: match kind {
            EntryKind::Dir => FileKind::Dir,
            EntryKind::Symlink => FileKind::Symlink,
            EntryKind::Special => FileKind::Other,
            _ => FileKind::File,
        },
        special: None,
        size,
        mtime,
        atime: None,
        ctime: None,
        btime: None,
        mode,
        dev: None,
        ino: None,
        nlink: None,
        uid: None,
        gid: None,
        blocks: None,
        readonly: true,
        os_attrs: None,
    }
}

/// The folder `loc` of an archive, as listing entries. `cancel` is asked now and then while a
/// big stream is read.
pub fn read_dir(
    platform: &dyn Platform,
    loc: &Location,
    cancel: &dyn Fn() -> bool,
) -> Result<(Vec<Entry>, ArchiveView)> {
    let mut lc = ListControl::new(cancel);
    let ix = open_cached(&loc.archive, &mut lc).map_err(|e| e.into_error(&loc.archive))?;
    if !ix.is_folder(&loc.inner) {
        let shown = loc.archive.join(&loc.inner);
        return Err(match ix.find(&loc.inner) {
            Some(_) => Error::Invalid(format!("{} is not a folder", display::path(&shown))),
            None => Error::Invalid(format!("{} is not in the archive", display::path(&shown))),
        });
    }
    let here = loc.archive.join(&loc.inner);
    let entries = ix
        .children(&loc.inner)
        .into_iter()
        .map(|c| {
            let meta = synthetic_meta(c.kind, c.size, c.mtime, c.mode);
            let attrs = platform.attributes().attrs(&c.name, &meta);
            let shown = display::name(&c.name);
            let link = (c.kind == EntryKind::Symlink).then(|| LinkInfo {
                target: c.link.clone().unwrap_or_default(),
                // Never followed, so never known: shown as an ordinary link.
                state: LinkState::ToFile,
            });
            let executable = c.kind == EntryKind::File && c.mode.is_some_and(|m| m & 0o111 != 0);
            let type_label =
                crate::filetype::label(&shown, meta.kind, c.kind == EntryKind::Dir, executable);
            Entry {
                sort_name: shown.to_lowercase(),
                display: shown,
                name: c.name.clone(),
                path: here.join(&c.name),
                kind: meta.kind,
                link,
                size: if c.kind == EntryKind::File { c.size } else { 0 },
                mtime: c.mtime,
                created: None,
                type_label,
                mode: c.mode,
                hidden: attrs.hidden,
                readonly: true,
                executable,
                reparse: None,
                error: None,
            }
        })
        .collect();
    let view = ArchiveView {
        archive: loc.archive.clone(),
        inner: loc.inner.clone(),
        format: ix.format.label(),
        files: ix.files,
        dirs: ix.dirs,
        bytes: ix.bytes,
        packed: ix.packed,
        note: ix.note.clone(),
        encrypted: ix.encrypted_members,
    };
    Ok((entries, view))
}
