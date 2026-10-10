//! A trash that is a folder: items are moved into it under a name that is not taken, and
//! put back from it. No `.trashinfo`, no database: the journal remembers where an item came
//! from, and the handle remembers where it is now.
//!
//! macOS uses it with `~/.Trash` (and `.Trashes/<uid>` on other volumes), which is where
//! the Finder looks; the tests of every system use it with a folder inside their sandbox, so
//! that nothing ever goes to the real Recycle Bin or Trash of the machine running them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::trash::{TrashBackend, TrashHandle, TrashedItem};
use crate::fs::{FsEngine, tree};
use crate::ops::names;
use crate::{Error, Result, display};

/// Where the trash of another volume lives, if the item is on one.
type VolumeTrash = dyn Fn(&Path) -> Option<PathBuf> + Send + Sync;

pub struct DirTrash {
    home: PathBuf,
    fs: Arc<dyn FsEngine>,
    volume_trash: Option<Box<VolumeTrash>>,
}

impl DirTrash {
    pub fn new(home: PathBuf, fs: Arc<dyn FsEngine>) -> DirTrash {
        DirTrash {
            home,
            fs,
            volume_trash: None,
        }
    }

    /// Items on another volume go to that volume's own trash (`f` says which), so that
    /// trashing stays a rename and the Finder finds them.
    pub fn with_volume_trash(
        mut self,
        f: impl Fn(&Path) -> Option<PathBuf> + Send + Sync + 'static,
    ) -> DirTrash {
        self.volume_trash = Some(Box::new(f));
        self
    }

    fn folder_for(&self, abs: &Path) -> PathBuf {
        self.volume_trash
            .as_ref()
            .and_then(|f| f(abs))
            .unwrap_or_else(|| self.home.clone())
    }

    /// A name in `dir` that is free: the item's own, or "name (2).ext", "name (3).ext"…
    fn free_name(&self, dir: &Path, name: &std::ffi::OsStr) -> Result<PathBuf> {
        let first = dir.join(name);
        if self.fs.lstat(&first).is_err() {
            return Ok(first);
        }
        for n in 2..10_000u32 {
            let candidate = dir.join(names::numbered(name, n));
            if self.fs.lstat(&candidate).is_err() {
                return Ok(candidate);
            }
        }
        Err(Error::Trash(
            "the trash has too many items of that name".into(),
        ))
    }
}

impl TrashBackend for DirTrash {
    fn trash(&self, path: &Path) -> Result<TrashedItem> {
        let abs = std::path::absolute(path).map_err(|e| Error::io("read", path, e))?;
        self.fs
            .lstat(&abs)
            .map_err(|e| Error::io("read", &abs, e))?;
        let folder = self.folder_for(&abs);
        if abs.starts_with(&folder) {
            return Err(Error::Trash(format!(
                "{} is already in the trash",
                display::path(&abs)
            )));
        }
        if folder.starts_with(&abs) {
            return Err(Error::Trash(format!(
                "{} contains the trash itself",
                display::path(&abs)
            )));
        }
        self.fs
            .create_dir(&folder, Some(0o700))
            .or_else(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .or_else(|_| std::fs::create_dir_all(&folder))
            .map_err(|e| Error::io("create the trash folder", &folder, e))?;
        let name = abs
            .file_name()
            .ok_or_else(|| Error::Trash("no file name".into()))?;
        let stored = self.free_name(&folder, name)?;
        tree::move_tree(self.fs.as_ref(), &abs, &stored)?;
        Ok(TrashedItem {
            original: abs,
            handle: TrashHandle::Directory { stored },
        })
    }

    fn restore(&self, item: &TrashedItem) -> Result<()> {
        let TrashHandle::Directory { stored } = &item.handle else {
            return Err(Error::Trash("not a trash-folder handle".into()));
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
            std::fs::create_dir_all(parent).map_err(|e| Error::io("create folder", parent, e))?;
        }
        tree::move_tree(self.fs.as_ref(), stored, &item.original)
    }

    fn contains(&self, item: &TrashedItem) -> bool {
        match &item.handle {
            TrashHandle::Directory { stored } => self.fs.lstat(stored).is_ok(),
            _ => false,
        }
    }
}
