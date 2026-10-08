//! Small recursive helpers used outside the plan/execute engine (trash fallback,
//! restore). They never follow symlinks and clean up after themselves on failure.
//! User-facing copies and moves go through the planner, which has per-file error
//! handling; these are all-or-nothing.

use std::io;
use std::path::Path;

use super::{CopyRequest, FileKind, FsEngine};
use crate::{Error, Result};

/// Remove a file, symlink or whole directory tree without following symlinks.
pub fn remove_tree(fs: &dyn FsEngine, p: &Path) -> io::Result<()> {
    let meta = fs.lstat(p)?;
    if meta.kind == FileKind::Dir {
        for item in fs.read_dir(p)? {
            remove_tree(fs, &item.path)?;
        }
        fs.remove_dir(p)
    } else {
        fs.remove_file(p)
    }
}

/// Copy a tree preserving symlinks, permissions and times. Fails if `dst` exists.
pub fn copy_tree(fs: &dyn FsEngine, src: &Path, dst: &Path, verify: bool) -> Result<()> {
    let meta = fs.lstat(src).map_err(|e| Error::io("read", src, e))?;
    match meta.kind {
        FileKind::Dir => {
            fs.create_dir(dst, Some(0o700))
                .map_err(|e| Error::io("create folder", dst, e))?;
            let items = fs
                .read_dir(src)
                .map_err(|e| Error::io("read folder", src, e))?;
            for item in items {
                copy_tree(fs, &item.path, &dst.join(&item.name), verify)?;
            }
            if let Some(m) = meta.mode {
                fs.set_mode(dst, m)
                    .map_err(|e| Error::io("set permissions", dst, e))?;
            }
            if let Some(mt) = meta.mtime {
                let _ = fs.set_mtime(dst, mt, meta.atime);
            }
            Ok(())
        }
        FileKind::Symlink => {
            let target = fs
                .read_link(src)
                .map_err(|e| Error::io("read link", src, e))?;
            fs.create_symlink(&target, dst)
                .map_err(|e| Error::io("create link", dst, e))
        }
        FileKind::File => {
            let req = CopyRequest {
                src: src.to_path_buf(),
                dst: dst.to_path_buf(),
                mode: meta.mode,
                mtime: meta.mtime,
                atime: meta.atime,
                verify,
                sync: verify,
            };
            fs.copy_file(&req, &mut |_| true)
                .map(|_| ())
                .map_err(|e| Error::io2("copy", src, dst, e))
        }
        FileKind::Other => Err(Error::Invalid(format!(
            "{} is a special file and cannot be copied",
            crate::display::path(src)
        ))),
    }
}

/// Move across filesystems: copy, verify, and only then remove the source.
/// On any failure the partial destination is removed and the source is untouched.
pub fn move_tree(fs: &dyn FsEngine, src: &Path, dst: &Path) -> Result<()> {
    match fs.rename_noreplace(src, dst) {
        Ok(()) => return Ok(()),
        Err(e) if crate::error::is_exdev(&e) => {}
        Err(e) => return Err(Error::io2("move", src, dst, e)),
    }
    if let Err(e) = copy_tree(fs, src, dst, true) {
        let _ = remove_tree(fs, dst);
        return Err(e);
    }
    remove_tree(fs, src).map_err(|e| Error::io("remove source after copy", src, e))
}
