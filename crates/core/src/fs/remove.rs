//! Removing a file only if it is still the one that was expected, with as little time as the
//! system allows between the look and the removal ([`FsEngine::remove_file_if`]).
//!
//! * Unix: the folder is opened once, and the file is looked at (`fstatat`, never following a
//!   link) and removed (`unlinkat`) through that open folder, so a folder swapped for a link in
//!   the meantime is not followed. What stays open is a file that takes the name between the
//!   look and the removal: POSIX has no call that removes a name only if it still points at the
//!   same file.
//! * Windows: the file is opened with a share mode that lets nobody else write, rename or
//!   delete it, looked at through that handle, and deleted through the same handle, so it
//!   cannot be replaced in between.

use std::io;
use std::path::Path;

use super::{Fingerprint, FsEngine, Removed};

/// The way that needs no more than the plain operations: look, compare, remove.
pub(super) fn remove_if_by_path(
    fs: &(impl FsEngine + ?Sized),
    p: &Path,
    expect: &Fingerprint,
) -> io::Result<Removed> {
    let now = match fs.lstat(p) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Removed::Gone),
        Err(e) => return Err(e),
    };
    match expect.diff(&now.fingerprint()) {
        Some(why) => Ok(Removed::Changed(why)),
        None => fs.remove_file(p).map(|()| Removed::Done),
    }
}

#[cfg(unix)]
pub(super) use unix::remove_if;
#[cfg(windows)]
pub(super) use windows::remove_if;

#[cfg(unix)]
mod unix {
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    use super::super::{FileKind, Fingerprint, LocalFs, Removed, stamp_to_time};
    use super::remove_if_by_path;

    pub(in super::super) fn remove_if(p: &Path, expect: &Fingerprint) -> io::Result<Removed> {
        remove_if_with(p, expect, || {})
    }

    /// `after_open` runs once the folder is open: the place for a test to change the folder.
    pub(in super::super) fn remove_if_with(
        p: &Path,
        expect: &Fingerprint,
        after_open: impl FnOnce(),
    ) -> io::Result<Removed> {
        let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else {
            return remove_if_by_path(&LocalFs, p, expect);
        };
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        let dir = match open_dir(parent) {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Removed::Gone),
            // A folder one may write in but not read: no handle to work from.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                return remove_if_by_path(&LocalFs, p, expect);
            }
            Err(e) => return Err(e),
        };
        after_open();
        let name = CString::new(name.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in the name"))?;
        let now = match fingerprint_at(&dir, &name) {
            Ok(now) => now,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Removed::Gone),
            Err(e) => return Err(e),
        };
        if let Some(why) = expect.diff(&now) {
            return Ok(Removed::Changed(why));
        }
        // SAFETY: `dir` is an open folder and `name` is NUL-terminated; flag 0 removes a name
        // that is not a folder.
        let r = unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) };
        match r {
            0 => Ok(Removed::Done),
            _ => match io::Error::last_os_error() {
                e if e.kind() == io::ErrorKind::NotFound => Ok(Removed::Gone),
                e => Err(e),
            },
        }
    }

    fn open_dir(p: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(p)
    }

    /// The fingerprint of `name` in `dir`, a link looked at itself.
    fn fingerprint_at(dir: &File, name: &CString) -> io::Result<Fingerprint> {
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: `dir` is open, `name` is NUL-terminated, and `st` is filled by the call.
        let r = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                st.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if r != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded, so it wrote the whole structure.
        let st = unsafe { st.assume_init() };
        let kind = match st.st_mode & libc::S_IFMT {
            libc::S_IFREG => FileKind::File,
            libc::S_IFDIR => FileKind::Dir,
            libc::S_IFLNK => FileKind::Symlink,
            _ => FileKind::Other,
        };
        // The width of the link count depends on the system.
        #[allow(clippy::useless_conversion)]
        let nlink = u64::from(st.st_nlink);
        Ok(Fingerprint::new(
            kind,
            st.st_size as u64,
            stamp_to_time(st.st_mtime, st.st_mtime_nsec as u32),
            stamp_to_time(st.st_ctime, st.st_ctime_nsec as u32),
            nlink,
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::fs::FsEngine;

        fn fingerprint(p: &Path) -> Fingerprint {
            LocalFs.lstat(p).unwrap().fingerprint()
        }

        #[test]
        fn a_folder_swapped_for_a_link_after_it_was_opened_is_not_followed() {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path();
            std::fs::create_dir_all(root.join("real")).unwrap();
            std::fs::create_dir_all(root.join("trap")).unwrap();
            std::fs::write(root.join("real/f"), "mine").unwrap();
            std::fs::write(root.join("trap/f"), "mine").unwrap();
            let expect = fingerprint(&root.join("real/f"));

            let out = remove_if_with(&root.join("real/f"), &expect, || {
                std::fs::rename(root.join("real"), root.join("moved")).unwrap();
                std::os::unix::fs::symlink(root.join("trap"), root.join("real")).unwrap();
            })
            .unwrap();

            assert_eq!(out, Removed::Done);
            assert!(!root.join("moved/f").exists(), "the file that was meant");
            assert!(root.join("trap/f").exists(), "what the link points to");
        }

        #[test]
        fn a_file_that_changed_or_a_link_in_its_place_is_left() {
            let tmp = tempfile::tempdir().unwrap();
            let f = tmp.path().join("f");
            std::fs::write(&f, "one").unwrap();
            let expect = fingerprint(&f);
            std::fs::write(&f, "longer now").unwrap();
            assert!(matches!(
                remove_if(&f, &expect).unwrap(),
                Removed::Changed(_)
            ));
            std::fs::remove_file(&f).unwrap();
            std::os::unix::fs::symlink(tmp.path(), &f).unwrap();
            assert!(matches!(
                remove_if(&f, &expect).unwrap(),
                Removed::Changed(_)
            ));
            assert!(f.symlink_metadata().is_ok());
            std::fs::remove_file(&f).unwrap();
            assert_eq!(remove_if(&f, &expect).unwrap(), Removed::Gone);
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::fs::OpenOptions;
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FileDispositionInfo,
        FileDispositionInfoEx, SetFileInformationByHandle,
    };

    use super::super::{Fingerprint, FsMeta, Removed};

    pub(in super::super) fn remove_if(p: &Path, expect: &Fingerprint) -> io::Result<Removed> {
        // Nobody else may write, rename or delete the file while this handle is open, and a
        // link is looked at itself.
        let f = match OpenOptions::new()
            .access_mode(DELETE | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(p)
        {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Removed::Gone),
            Err(e) => return Err(e),
        };
        let now = FsMeta::from_std(&f.metadata()?).fingerprint();
        if let Some(why) = expect.diff(&now) {
            return Ok(Removed::Changed(why));
        }
        delete_on_handle(f.as_raw_handle() as HANDLE)?;
        Ok(Removed::Done)
    }

    /// Delete the file the handle is open on: the name goes at once, whatever is still open
    /// on it, and a read-only file goes too. Older systems only know the plain form.
    fn delete_on_handle(h: HANDLE) -> io::Result<()> {
        let ex = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        };
        // SAFETY: `h` is a valid handle opened with DELETE access, and the buffer is the
        // structure the class asks for, with its size.
        let ok = unsafe {
            SetFileInformationByHandle(
                h,
                FileDispositionInfoEx,
                (&raw const ex).cast(),
                size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        };
        if ok != 0 {
            return Ok(());
        }
        let plain = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: as above.
        let ok = unsafe {
            SetFileInformationByHandle(
                h,
                FileDispositionInfo,
                (&raw const plain).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
