//! freedesktop.org Trash specification 1.0.
//!
//! * Items on the same filesystem as the home trash go to `$XDG_DATA_HOME/Trash`.
//! * Items on other filesystems go to `$topdir/.Trash/$uid` (when the admin created
//!   a sticky, non-symlink `.Trash`) or `$topdir/.Trash-$uid`, so that trashing is a
//!   cheap rename on the *same* device. `Path=` is then relative to `$topdir`.
//! * Only if no per-device trash can be created, the item is copied into the home
//!   trash (verified, then the source is removed).
//! * The `.trashinfo` file is reserved with `O_EXCL` before the item moves, and
//!   removed again if the move fails, so no orphan is ever left behind.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::trash::{TrashBackend, TrashHandle, TrashedItem};
use crate::fs::{FsEngine, tree};
use crate::{Error, Result, display};

type TopdirFinder = dyn Fn(&Path) -> io::Result<PathBuf> + Send + Sync;

pub struct FreedesktopTrash {
    home_trash: PathBuf,
    uid: u32,
    fs: Arc<dyn FsEngine>,
    topdir_of: Box<TopdirFinder>,
}

impl FreedesktopTrash {
    pub fn new(home_trash: PathBuf, fs: Arc<dyn FsEngine>) -> Self {
        FreedesktopTrash {
            home_trash,
            // SAFETY: getuid has no preconditions and cannot fail.
            uid: unsafe { libc::getuid() },
            fs,
            topdir_of: Box::new(find_topdir),
        }
    }

    /// Replace the mount-point detection (tests use this to force the fallback path).
    pub fn with_topdir_finder(
        mut self,
        f: impl Fn(&Path) -> io::Result<PathBuf> + Send + Sync + 'static,
    ) -> Self {
        self.topdir_of = Box::new(f);
        self
    }

    /// Choose and prepare the trash directory for `abs`. Returns the trash directory
    /// and, for per-device trashes, the topdir that `Path=` is relative to.
    fn pick_trash(&self, abs: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
        let item_dev = std::fs::symlink_metadata(abs)
            .map_err(|e| Error::io("read", abs, e))?
            .dev();
        let home_dev = nearest_existing_dev(&self.home_trash);
        if home_dev == Some(item_dev) {
            ensure_trash_dir(&self.home_trash)?;
            return Ok((self.home_trash.clone(), None));
        }
        match self.topdir_trash(abs) {
            Ok(Some(found)) => Ok(found),
            Ok(None) => {
                tracing::info!(
                    "no per-device trash for {}; using the home trash with a copy",
                    abs.display()
                );
                ensure_trash_dir(&self.home_trash)?;
                Ok((self.home_trash.clone(), None))
            }
            Err(e) => Err(e),
        }
    }

    fn topdir_trash(&self, abs: &Path) -> Result<Option<(PathBuf, Option<PathBuf>)>> {
        let parent = abs.parent().unwrap_or(abs);
        let topdir =
            (self.topdir_of)(parent).map_err(|e| Error::io("find mount point of", parent, e))?;
        if topdir == abs {
            return Err(Error::Trash(format!(
                "{} is a mount point and cannot be trashed",
                display::path(abs)
            )));
        }
        // 1. $topdir/.Trash/$uid, only if .Trash is a sticky directory and not a symlink.
        let shared = topdir.join(".Trash");
        if let Ok(m) = std::fs::symlink_metadata(&shared) {
            let ok =
                m.is_dir() && !m.file_type().is_symlink() && m.permissions().mode() & 0o1000 != 0;
            if ok {
                let dir = shared.join(self.uid.to_string());
                if ensure_trash_dir(&dir).is_ok() {
                    return Ok(Some((dir, Some(topdir))));
                }
            } else {
                tracing::warn!(
                    "{} is not a valid shared trash (needs sticky bit, not a symlink)",
                    shared.display()
                );
            }
        }
        // 2. $topdir/.Trash-$uid
        let own = topdir.join(format!(".Trash-{}", self.uid));
        if ensure_trash_dir(&own).is_ok() {
            return Ok(Some((own, Some(topdir))));
        }
        Ok(None)
    }
}

impl TrashBackend for FreedesktopTrash {
    fn trash(&self, path: &Path) -> Result<TrashedItem> {
        let abs = absolute_no_follow(path)?;
        if abs.parent().is_none() {
            return Err(Error::Trash("the root directory cannot be trashed".into()));
        }
        self.fs
            .lstat(&abs)
            .map_err(|e| Error::io("read", &abs, e))?;
        if abs.starts_with(&self.home_trash) {
            return Err(Error::Trash(format!(
                "{} is already in the trash",
                display::path(&abs)
            )));
        }
        if self.home_trash.starts_with(&abs) {
            return Err(Error::Trash(format!(
                "{} contains the trash itself",
                display::path(&abs)
            )));
        }

        let (trash_dir, topdir) = self.pick_trash(&abs)?;
        let files = trash_dir.join("files");
        let infos = trash_dir.join("info");
        let name = abs
            .file_name()
            .ok_or_else(|| Error::Trash("no file name".into()))?;
        let recorded: PathBuf = match &topdir {
            Some(t) => abs.strip_prefix(t).unwrap_or(&abs).to_path_buf(),
            None => abs.clone(),
        };
        let content = trashinfo_content(&recorded);

        // Reserve a unique name by creating the .trashinfo exclusively.
        let (stored, info) = reserve_name(&files, &infos, name, content.as_bytes())?;

        match tree::move_tree(self.fs.as_ref(), &abs, &stored) {
            Ok(()) => Ok(TrashedItem {
                original: abs,
                handle: TrashHandle::Freedesktop { stored, info },
            }),
            Err(e) => {
                // Never leave an orphan .trashinfo behind.
                let _ = std::fs::remove_file(&info);
                Err(e)
            }
        }
    }

    fn restore(&self, item: &TrashedItem) -> Result<()> {
        let TrashHandle::Freedesktop { stored, info } = &item.handle else {
            return Err(Error::Trash("not a freedesktop trash handle".into()));
        };
        if self.fs.lstat(stored).is_err() {
            return Err(Error::Trash(format!(
                "{} is no longer in the trash",
                display::path(&item.original)
            )));
        }
        if self.fs.lstat(&item.original).is_ok() {
            return Err(Error::AlreadyExists(item.original.clone()));
        }
        if let Some(parent) = item.original.parent() {
            create_parents(parent).map_err(|e| Error::io("create folder", parent, e))?;
        }
        tree::move_tree(self.fs.as_ref(), stored, &item.original)?;
        match std::fs::remove_file(info) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("could not remove {}: {e}", info.display()),
        }
        Ok(())
    }

    fn contains(&self, item: &TrashedItem) -> bool {
        match &item.handle {
            TrashHandle::Freedesktop { stored, .. } => self.fs.lstat(stored).is_ok(),
            TrashHandle::Opaque { .. } | TrashHandle::Directory { .. } => false,
        }
    }
}

/// Mount point of the filesystem holding `dir`: the highest ancestor with the same device.
fn find_topdir(dir: &Path) -> io::Result<PathBuf> {
    let real = std::fs::canonicalize(dir)?;
    let dev = std::fs::metadata(&real)?.dev();
    let mut top = real.clone();
    for anc in real.ancestors().skip(1) {
        match std::fs::metadata(anc) {
            Ok(m) if m.dev() == dev => top = anc.to_path_buf(),
            _ => break,
        }
    }
    Ok(top)
}

fn nearest_existing_dev(p: &Path) -> Option<u64> {
    p.ancestors()
        .find_map(|a| std::fs::metadata(a).ok().map(|m| m.dev()))
}

/// `p` made absolute, with the parent resolved but the final component left alone
/// (so trashing a symlink trashes the link, never its target).
fn absolute_no_follow(p: &Path) -> Result<PathBuf> {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::io("get current folder", p, e))?
            .join(p)
    };
    let name = abs.file_name().map(OsStr::to_os_string);
    let Some(name) = name else {
        return Ok(abs);
    };
    let parent = abs.parent().unwrap_or(Path::new("/"));
    let parent = std::fs::canonicalize(parent).map_err(|e| Error::io("resolve", parent, e))?;
    Ok(parent.join(name))
}

fn ensure_trash_dir(dir: &Path) -> Result<()> {
    for sub in [dir.join("files"), dir.join("info")] {
        create_private_dirs(&sub).map_err(|e| Error::io("create trash folder", &sub, e))?;
    }
    Ok(())
}

fn create_private_dirs(p: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)
}

fn create_parents(p: &Path) -> io::Result<()> {
    std::fs::create_dir_all(p)
}

fn trashinfo_content(recorded: &Path) -> String {
    let now = jiff::Zoned::now();
    format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode(recorded.as_os_str().as_bytes()),
        now.strftime("%Y-%m-%dT%H:%M:%S")
    )
}

/// Create `info/<name>.trashinfo` exclusively, picking `name`, `name.2`, `name.3`...
/// until neither the info file nor the stored file exists.
fn reserve_name(
    files: &Path,
    infos: &Path,
    name: &OsStr,
    content: &[u8],
) -> Result<(PathBuf, PathBuf)> {
    use std::io::Write;
    for n in 1u32..100_000 {
        let mut cand = OsString::from(name);
        if n > 1 {
            cand.push(format!(".{n}"));
        }
        let stored = files.join(&cand);
        if std::fs::symlink_metadata(&stored).is_ok() {
            continue;
        }
        let mut info_name = cand.clone();
        info_name.push(".trashinfo");
        let info = infos.join(info_name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&info)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(content) {
                    let _ = std::fs::remove_file(&info);
                    return Err(Error::io("write", &info, e));
                }
                return Ok((stored, info));
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::io("create", &info, e)),
        }
    }
    Err(Error::Trash(
        "could not find a free name in the trash".into(),
    ))
}

/// RFC 2396 style: unreserved characters and `/` stay, everything else is `%XX`.
pub fn percent_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn percent_decode(s: &str) -> OsString {
    fn hex(b: u8) -> Option<u8> {
        (b as char).to_digit(16).map(|d| d as u8)
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 3 <= b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    OsString::from_vec(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_round_trips_awkward_names() {
        for raw in [
            &b"/home/a/plain.txt"[..],
            b"/home/a/with space & ampersand%.txt",
            "/home/a/日本語 🎉".as_bytes(),
            b"/home/a/bad\xff\xfebytes",
            b"/home/a/new\nline",
        ] {
            let enc = percent_encode(raw);
            assert!(!enc.contains('\n') && !enc.contains(' '));
            assert_eq!(percent_decode(&enc).as_bytes(), raw);
        }
    }
}
