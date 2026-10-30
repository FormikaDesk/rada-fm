//! A rename that never replaces what is already at the destination.
//!
//! Each system has its own atomic way to ask for it; where the filesystem cannot, the last
//! resort looks first and renames after, which leaves a window (see docs/SAFETY.md).

use std::io;
use std::path::Path;

#[cfg(not(windows))]
use std::fs;

pub(super) fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::rename(from, to, false)
    }
    #[cfg(target_os = "macos")]
    {
        macos::rename(from, to)
    }
    #[cfg(windows)]
    {
        windows::rename(from, to)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        check_then_rename(from, to)
    }
}

/// Not atomic: a name created between the look and the rename is replaced.
#[cfg(not(windows))]
fn check_then_rename(from: &Path, to: &Path) -> io::Result<()> {
    match fs::symlink_metadata(to) {
        Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => fs::rename(from, to),
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
fn c_path(p: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    /// `skip_kernel` goes straight to the fallback, which tests cannot otherwise reach on a
    /// filesystem that has `renameat2`.
    pub(super) fn rename(from: &Path, to: &Path, skip_kernel: bool) -> io::Result<()> {
        if !skip_kernel {
            match renameat2_noreplace(from, to) {
                Err(e) if lacks_the_flag(&e) => {}
                done => return done,
            }
        }
        link_then_unlink(from, to)
    }

    fn renameat2_noreplace(from: &Path, to: &Path) -> io::Result<()> {
        let (a, b) = (c_path(from)?, c_path(to)?);
        const RENAME_NOREPLACE: libc::c_uint = 1;
        // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
        let r = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                RENAME_NOREPLACE,
            )
        };
        if r == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// The filesystem (or the kernel) does not know the flag.
    fn lacks_the_flag(e: &io::Error) -> bool {
        matches!(
            e.raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
        )
    }

    fn no_hard_links(e: &io::Error) -> bool {
        matches!(
            e.raw_os_error(),
            Some(libc::EPERM | libc::EOPNOTSUPP | libc::ENOSYS | libc::EMLINK)
        )
    }

    /// `link` fails if the name exists and never replaces it, so for a regular file it is
    /// the atomic check the rename lacks. FAT and exFAT have no links, and a folder cannot
    /// be linked: those two keep the look-then-rename.
    fn link_then_unlink(from: &Path, to: &Path) -> io::Result<()> {
        if !fs::symlink_metadata(from)?.is_file() {
            return check_then_rename(from, to);
        }
        match fs::hard_link(from, to) {
            Ok(()) => {}
            Err(e) if no_hard_links(&e) => return check_then_rename(from, to),
            Err(e) => return Err(e),
        }
        fs::remove_file(from).inspect_err(|_| {
            let _ = fs::remove_file(to);
        })
    }

    #[cfg(test)]
    pub(super) fn rename_by_link(from: &Path, to: &Path) -> io::Result<()> {
        rename(from, to, true)
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    pub(super) fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let (a, b) = (c_path(from)?, c_path(to)?);
        // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
        let r = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) };
        if r == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::EEXIST) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            // A volume that does not support the flag (some network and FAT volumes).
            Some(libc::ENOTSUP | libc::EINVAL) => check_then_rename(from, to),
            _ => Err(err),
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    fn wide(p: &Path) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        p.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    /// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING` refuses an existing name.
    pub(super) fn rename(from: &Path, to: &Path) -> io::Result<()> {
        use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS};
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        let (a, b) = (wide(from), wide(to));
        // SAFETY: both buffers are NUL-terminated and outlive the call.
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0) } != 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error().map(|c| c as u32) {
            Some(ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS) => {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            }
            _ => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn two_files() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let (from, to) = (tmp.path().join("from"), tmp.path().join("to"));
        fs::write(&from, "new").unwrap();
        (tmp, from, to)
    }

    #[test]
    fn a_name_that_is_free_is_taken() {
        let (_tmp, from, to) = two_files();
        rename_noreplace(&from, &to).unwrap();
        assert_eq!(fs::read_to_string(&to).unwrap(), "new");
        assert!(!from.exists());
    }

    #[test]
    fn a_file_that_is_there_is_never_replaced() {
        let (_tmp, from, to) = two_files();
        fs::write(&to, "mine").unwrap();
        let e = rename_noreplace(&from, &to).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&to).unwrap(), "mine");
        assert_eq!(fs::read_to_string(&from).unwrap(), "new");
    }

    #[test]
    fn a_folder_that_is_there_is_never_replaced() {
        let (_tmp, from, to) = two_files();
        fs::create_dir(&to).unwrap();
        let e = rename_noreplace(&from, &to).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert!(to.is_dir());
        assert!(from.exists());
    }

    #[test]
    fn a_folder_is_moved_to_a_free_name_and_not_over_another() {
        let tmp = tempfile::tempdir().unwrap();
        let (from, to) = (tmp.path().join("from"), tmp.path().join("to"));
        fs::create_dir(&from).unwrap();
        fs::write(from.join("f"), "x").unwrap();
        rename_noreplace(&from, &to).unwrap();
        assert!(to.join("f").exists() && !from.exists());
        fs::create_dir(&from).unwrap();
        let e = rename_noreplace(&from, &to).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert!(to.join("f").exists() && from.exists());
    }

    #[test]
    fn a_missing_source_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let e = rename_noreplace(&tmp.path().join("a"), &tmp.path().join("b")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(target_os = "linux")]
    mod by_link {
        use super::*;
        use std::os::unix::fs::MetadataExt;

        #[test]
        fn a_file_moves_by_link_and_unlink_keeping_its_identity() {
            let (_tmp, from, to) = two_files();
            let ino = fs::metadata(&from).unwrap().ino();
            linux::rename_by_link(&from, &to).unwrap();
            let after = fs::metadata(&to).unwrap();
            assert_eq!((after.ino(), after.nlink()), (ino, 1));
            assert!(!from.exists());
        }

        #[test]
        fn a_file_that_is_there_is_never_replaced() {
            let (_tmp, from, to) = two_files();
            fs::write(&to, "mine").unwrap();
            let e = linux::rename_by_link(&from, &to).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read_to_string(&to).unwrap(), "mine");
            assert_eq!(fs::read_to_string(&from).unwrap(), "new");
        }

        #[test]
        fn a_name_that_is_a_dangling_link_counts_as_there() {
            let (_tmp, from, to) = two_files();
            std::os::unix::fs::symlink("nowhere", &to).unwrap();
            let e = linux::rename_by_link(&from, &to).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
            assert!(fs::symlink_metadata(&to).unwrap().is_symlink());
        }

        #[test]
        fn a_folder_keeps_the_look_then_rename() {
            let tmp = tempfile::tempdir().unwrap();
            let (from, to) = (tmp.path().join("from"), tmp.path().join("to"));
            fs::create_dir(&from).unwrap();
            linux::rename_by_link(&from, &to).unwrap();
            assert!(to.is_dir() && !from.exists());
            fs::create_dir(&from).unwrap();
            let e = linux::rename_by_link(&from, &to).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        }

        #[test]
        fn a_source_with_other_names_keeps_them() {
            let (tmp, from, to) = two_files();
            let other = tmp.path().join("other");
            fs::hard_link(&from, &other).unwrap();
            linux::rename_by_link(&from, &to).unwrap();
            assert_eq!(fs::metadata(&to).unwrap().nlink(), 2);
            assert!(other.exists() && !from.exists());
        }
    }
}
