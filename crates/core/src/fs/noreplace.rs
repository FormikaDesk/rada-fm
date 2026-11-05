//! A rename that never replaces what is already at the destination.
//!
//! Each system has its own atomic way to ask for it; where the filesystem cannot, the last
//! resort looks first and renames after, which leaves a window (see docs/SAFETY.md). The
//! strict form never takes that last resort: where only it is left, it says `Unsupported`.

use std::io;
use std::path::Path;

#[cfg(not(windows))]
use std::fs;

/// What a rename does where the system has no way to refuse an existing name atomically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(windows, allow(dead_code))]
enum Fallback {
    /// Look, then rename: a name created in between is replaced.
    LookThenRename,
    /// Do not rename at all.
    Refuse,
}

impl Fallback {
    #[cfg(not(windows))]
    fn rename(self, from: &Path, to: &Path) -> io::Result<()> {
        match self {
            Fallback::LookThenRename => check_then_rename(from, to),
            Fallback::Refuse => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this filesystem cannot refuse an existing name atomically",
            )),
        }
    }
}

pub(super) fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    rename_with(from, to, Fallback::LookThenRename)
}

/// Like [`rename_noreplace`], but never looks-then-renames: `Unsupported` where the system
/// has nothing atomic to offer for this item.
pub(super) fn rename_noreplace_strict(from: &Path, to: &Path) -> io::Result<()> {
    rename_with(from, to, Fallback::Refuse)
}

fn rename_with(from: &Path, to: &Path, fallback: Fallback) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::rename(from, to, false, fallback)
    }
    #[cfg(target_os = "macos")]
    {
        macos::rename(from, to, fallback)
    }
    #[cfg(windows)]
    {
        // `MoveFileExW` refuses an existing name by itself: there is no fallback to choose.
        let _ = fallback;
        windows::rename(from, to)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        fallback.rename(from, to)
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
    pub(super) fn rename(
        from: &Path,
        to: &Path,
        skip_kernel: bool,
        fallback: Fallback,
    ) -> io::Result<()> {
        if !skip_kernel {
            match renameat2_noreplace(from, to) {
                Err(e) if lacks_the_flag(&e) => {}
                done => return done,
            }
        }
        // A closure: the function item does not fit "for any lifetime" of the parameter.
        #[allow(clippy::redundant_closure)]
        link_then_unlink(from, to, fallback, |a, b| fs::hard_link(a, b))
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
    /// be linked: those two take the fallback.
    fn link_then_unlink(
        from: &Path,
        to: &Path,
        fallback: Fallback,
        link: impl Fn(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<()> {
        if !fs::symlink_metadata(from)?.is_file() {
            return fallback.rename(from, to);
        }
        match link(from, to) {
            Ok(()) => {}
            Err(e) if no_hard_links(&e) => return fallback.rename(from, to),
            Err(e) => return Err(e),
        }
        fs::remove_file(from).inspect_err(|_| {
            let _ = fs::remove_file(to);
        })
    }

    #[cfg(test)]
    pub(super) fn rename_by_link(from: &Path, to: &Path) -> io::Result<()> {
        rename(from, to, true, Fallback::LookThenRename)
    }

    #[cfg(test)]
    pub(super) fn rename_by_link_strict(from: &Path, to: &Path) -> io::Result<()> {
        rename(from, to, true, Fallback::Refuse)
    }

    /// The same, on a filesystem whose `link` fails with `errno`.
    #[cfg(test)]
    pub(super) fn rename_by_failing_link(
        from: &Path,
        to: &Path,
        errno: i32,
        strict: bool,
    ) -> io::Result<()> {
        let fallback = if strict {
            Fallback::Refuse
        } else {
            Fallback::LookThenRename
        };
        link_then_unlink(from, to, fallback, |_, _| {
            Err(io::Error::from_raw_os_error(errno))
        })
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    pub(super) fn rename(from: &Path, to: &Path, fallback: Fallback) -> io::Result<()> {
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
            Some(libc::ENOTSUP | libc::EINVAL) => fallback.rename(from, to),
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

    #[test]
    fn the_strict_form_takes_a_free_name_and_never_replaces() {
        let (_tmp, from, to) = two_files();
        rename_noreplace_strict(&from, &to).unwrap();
        assert_eq!(fs::read_to_string(&to).unwrap(), "new");
        assert!(!from.exists());

        fs::write(&from, "again").unwrap();
        let e = rename_noreplace_strict(&from, &to).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&to).unwrap(), "new");
        assert_eq!(fs::read_to_string(&from).unwrap(), "again");
    }

    #[test]
    fn the_strict_form_moves_a_folder_where_the_system_can_refuse_a_taken_name() {
        let tmp = tempfile::tempdir().unwrap();
        let (from, to) = (tmp.path().join("from"), tmp.path().join("to"));
        fs::create_dir(&from).unwrap();
        // Whatever the system is, the answer is never "replaced": either it moved, or it says
        // it cannot.
        match rename_noreplace_strict(&from, &to) {
            Ok(()) => {
                fs::create_dir(&from).unwrap();
                let e = rename_noreplace_strict(&from, &to).unwrap_err();
                assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
            }
            Err(e) => assert_eq!(e.kind(), io::ErrorKind::Unsupported),
        }
        assert!(from.exists() || to.exists());
    }

    #[cfg(target_os = "linux")]
    mod strict_by_link {
        use super::*;

        #[test]
        fn a_file_still_moves_by_link_and_is_never_replaced() {
            let (_tmp, from, to) = two_files();
            linux::rename_by_link_strict(&from, &to).unwrap();
            assert_eq!(fs::read_to_string(&to).unwrap(), "new");
            assert!(!from.exists());

            fs::write(&from, "again").unwrap();
            let e = linux::rename_by_link_strict(&from, &to).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read_to_string(&to).unwrap(), "new");
        }

        #[test]
        fn a_folder_is_unsupported_and_stays_where_it_is() {
            let tmp = tempfile::tempdir().unwrap();
            let (from, to) = (tmp.path().join("from"), tmp.path().join("to"));
            fs::create_dir(&from).unwrap();
            let e = linux::rename_by_link_strict(&from, &to).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::Unsupported);
            assert!(from.is_dir() && !to.exists());
        }

        #[test]
        fn a_link_to_something_is_unsupported_too() {
            let (_tmp, from, to) = two_files();
            let link = from.with_file_name("link");
            std::os::unix::fs::symlink(&from, &link).unwrap();
            let e = linux::rename_by_link_strict(&link, &to).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::Unsupported);
            assert!(fs::symlink_metadata(&link).is_ok() && !to.exists());
        }

        /// What FAT does when asked for a hard link (`EPERM`), standing in for the filesystem.
        #[test]
        fn a_file_where_links_are_refused_is_unsupported_in_the_strict_form_only() {
            for errno in [libc::EPERM, libc::EOPNOTSUPP, libc::ENOSYS, libc::EMLINK] {
                let (_tmp, from, to) = two_files();
                let e = linux::rename_by_failing_link(&from, &to, errno, true).unwrap_err();
                assert_eq!(e.kind(), io::ErrorKind::Unsupported, "errno {errno}");
                assert!(from.exists() && !to.exists(), "errno {errno}");
            }
            let (_tmp, from, to) = two_files();
            linux::rename_by_failing_link(&from, &to, libc::EPERM, false).unwrap();
            assert_eq!(fs::read_to_string(&to).unwrap(), "new");
        }

        #[test]
        fn any_other_failure_of_the_link_is_passed_on_not_turned_into_unsupported() {
            let (_tmp, from, to) = two_files();
            let e = linux::rename_by_failing_link(&from, &to, libc::EACCES, true).unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
            assert!(from.exists() && !to.exists());
        }
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
