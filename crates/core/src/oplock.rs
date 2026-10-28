//! Locks between rada processes, one file each in the `locks` folder next to the journal.
//!
//! A lock is held with the system's own advisory file lock (`flock` on Unix, `LockFileEx` on
//! Windows, through [`File::try_lock`]), so it is released by the system when the process
//! ends in any way, `kill -9` and a power cut included: nothing has to be cleaned up for the
//! next process to find the lock free. The file itself is only a name to lock; it is removed
//! when the lock is let go, and one that is left behind by a dead process is harmless.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// A lock held by this process. Dropping it lets go of the lock and leaves the file.
#[derive(Debug)]
pub struct OpLock {
    file: File,
    path: PathBuf,
    /// The file did not exist: nobody had ever locked this name.
    pub created: bool,
}

impl OpLock {
    /// Take the lock called `name` in `dir`, or `None` when another holder has it (a process
    /// that is running, or another part of this one).
    pub fn try_take(dir: &Path, name: &str) -> io::Result<Option<OpLock>> {
        crate::statefile::create_private_dir(dir)?;
        let path = dir.join(name);
        // The file may be removed by its last holder between the two calls: try again.
        for _ in 0..8 {
            let (file, created) = match open_new(&path) {
                Ok(f) => (f, true),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match open_existing(&path) {
                    Ok(f) => (f, false),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                },
                Err(e) => return Err(e),
            };
            return match file.try_lock() {
                Ok(()) => Ok(Some(OpLock {
                    file,
                    path,
                    created,
                })),
                Err(TryLockError::WouldBlock) => Ok(None),
                Err(TryLockError::Error(e)) => Err(e),
            };
        }
        Err(io::Error::other("the lock file keeps disappearing"))
    }

    /// Let go of the lock and remove its file.
    pub fn release(self) {
        let _ = std::fs::remove_file(&self.path);
        drop(self.file);
    }
}

fn open_new(path: &Path) -> io::Result<File> {
    crate::statefile::private_file_options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

fn open_existing(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_is_exclusive_until_it_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let a = OpLock::try_take(dir.path(), "x").unwrap().unwrap();
        assert!(a.created);
        assert!(OpLock::try_take(dir.path(), "x").unwrap().is_none());
        assert!(OpLock::try_take(dir.path(), "y").unwrap().is_some());
        a.release();
        let again = OpLock::try_take(dir.path(), "x").unwrap().unwrap();
        assert!(again.created, "the file went with the lock");
    }

    #[test]
    fn a_file_left_behind_does_not_hold_anything() {
        let dir = tempfile::tempdir().unwrap();
        let a = OpLock::try_take(dir.path(), "x").unwrap().unwrap();
        drop(a); // what a dead process leaves: the file, not the lock
        let b = OpLock::try_take(dir.path(), "x").unwrap().unwrap();
        assert!(!b.created);
    }
}
