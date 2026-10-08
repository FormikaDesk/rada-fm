//! The real filesystem.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use xxhash_rust::xxh3::Xxh3;

use super::{CopyControl, CopyMethod, CopyOutcome, CopyRequest, DirItem, FsEngine, FsMeta};

/// Failures of [`FsEngine::copy_file`] that are not plain OS errors. They are carried
/// inside an `io::Error` and recovered with [`copy_fault`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CopyFault {
    #[error("cancelled")]
    Cancelled,
    #[error("the source changed while it was being copied")]
    SourceChanged,
    #[error("{0}")]
    VerifyMismatch(String),
}

pub fn copy_fault(e: &io::Error) -> Option<&CopyFault> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<CopyFault>())
}

fn fault(f: CopyFault) -> io::Error {
    io::Error::other(f)
}

const RW_CHUNK: usize = 1 << 20;
#[cfg(target_os = "linux")]
const CFR_CHUNK: usize = 8 << 20;

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFs;

impl LocalFs {
    pub fn new() -> Self {
        LocalFs
    }
}

impl FsEngine for LocalFs {
    fn lstat(&self, p: &Path) -> io::Result<FsMeta> {
        fs::symlink_metadata(p).map(|m| FsMeta::from_std(&m))
    }

    fn stat(&self, p: &Path) -> io::Result<FsMeta> {
        fs::metadata(p).map(|m| FsMeta::from_std(&m))
    }

    fn read_dir(&self, p: &Path) -> io::Result<Vec<DirItem>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(p)? {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("reading {}: {e}", p.display());
                    continue;
                }
            };
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map(|m| FsMeta::from_std(&m));
            out.push(DirItem {
                name: entry.file_name(),
                path,
                meta,
            });
        }
        Ok(out)
    }

    fn read_link(&self, p: &Path) -> io::Result<PathBuf> {
        fs::read_link(p)
    }

    fn create_dir(&self, p: &Path, mode: Option<u32>) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut b = fs::DirBuilder::new();
            if let Some(m) = mode {
                b.mode(m & 0o7777);
            }
            b.create(p)
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            fs::create_dir(p)
        }
    }

    fn create_symlink(&self, target: &Path, link: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(windows)]
        {
            let resolved = link
                .parent()
                .map(|d| d.join(target))
                .unwrap_or_else(|| target.to_path_buf());
            if fs::metadata(&resolved).map(|m| m.is_dir()).unwrap_or(false) {
                std::os::windows::fs::symlink_dir(target, link)
            } else {
                std::os::windows::fs::symlink_file(target, link)
            }
        }
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn rename_noreplace(&self, from: &Path, to: &Path) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            let a = CString::new(from.as_os_str().as_bytes())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
            let b = CString::new(to.as_os_str().as_bytes())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
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
                return Ok(());
            }
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                // The filesystem (or kernel) does not know the flag: fall back below.
                Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::EOPNOTSUPP) => {}
                _ => return Err(err),
            }
        }
        // Best effort where an atomic no-replace rename is not available.
        match fs::symlink_metadata(to) {
            Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => fs::rename(from, to),
            Err(e) => Err(e),
        }
    }

    fn remove_file(&self, p: &Path) -> io::Result<()> {
        fs::remove_file(p)
    }

    fn remove_dir(&self, p: &Path) -> io::Result<()> {
        fs::remove_dir(p)
    }

    fn set_mode(&self, p: &Path, mode: u32) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(mode & 0o7777))
        }
        #[cfg(not(unix))]
        {
            let _ = (p, mode);
            Ok(())
        }
    }

    fn set_mtime(&self, p: &Path, mtime: SystemTime, atime: Option<SystemTime>) -> io::Result<()> {
        let f = File::open(p).or_else(|_| OpenOptions::new().write(true).open(p))?;
        let mut t = fs::FileTimes::new().set_modified(mtime);
        if let Some(a) = atime {
            t = t.set_accessed(a);
        }
        f.set_times(t)
    }

    fn copy_file(&self, req: &CopyRequest, ctl: &mut dyn CopyControl) -> io::Result<CopyOutcome> {
        let mut src = open_source(&req.src)?;
        let before = src.metadata()?;
        if !before.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        // Private until the very end: a partial copy of a private file is never exposed.
        let mut dst = create_dest(&req.dst)?;
        let result = copy_body(&mut src, &mut dst, req, ctl).and_then(|outcome| {
            finish_dest(&dst, req)?;
            if req.verify {
                // Same guard as `cp`/`rsync`: a source that moved under us is not a copy.
                let after = src.metadata()?;
                if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
                    return Err(fault(CopyFault::SourceChanged));
                }
            }
            Ok(outcome)
        });
        match result {
            Ok(o) => Ok(o),
            Err(e) => {
                drop(dst);
                let _ = fs::remove_file(&req.dst);
                Err(e)
            }
        }
    }

    fn available_space(&self, p: &Path) -> io::Result<u64> {
        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            let c = CString::new(p.as_os_str().as_bytes())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
            let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
            // SAFETY: `c` is a valid C string and `st` is a valid out-pointer.
            let r = unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) };
            if r != 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: statvfs returned success, so the struct is initialised.
            let st = unsafe { st.assume_init() };
            // The field types differ between Unix systems (u32 on macOS).
            #[allow(clippy::unnecessary_cast)]
            Ok((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
        }
        #[cfg(not(unix))]
        {
            let _ = p;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "free space is not available",
            ))
        }
    }

    fn can_read(&self, p: &Path) -> bool {
        access(p, true, false)
    }

    fn can_write(&self, p: &Path) -> bool {
        access(p, false, true)
    }
}

fn access(p: &Path, read: bool, write: bool) -> bool {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = CString::new(p.as_os_str().as_bytes()) else {
            return false;
        };
        let mut mode = 0;
        if read {
            mode |= libc::R_OK;
        }
        if write {
            mode |= libc::W_OK;
        }
        // SAFETY: `c` is a valid NUL-terminated string.
        unsafe { libc::access(c.as_ptr(), mode) == 0 }
    }
    #[cfg(not(unix))]
    {
        match fs::metadata(p) {
            Ok(m) => {
                !(write && m.permissions().readonly())
                    && (!read || fs::File::open(p).is_ok() || m.is_dir())
            }
            Err(_) => false,
        }
    }
}

/// Open a source file. Never follows a symlink swapped in after planning and never
/// blocks on a FIFO swapped in after planning.
fn open_source(p: &Path) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    o.open(p)
}

fn create_dest(p: &Path) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(p)
}

fn finish_dest(dst: &File, req: &CopyRequest) -> io::Result<()> {
    #[cfg(unix)]
    if let Some(m) = req.mode {
        use std::os::unix::fs::PermissionsExt;
        dst.set_permissions(fs::Permissions::from_mode(m & 0o777))?;
    }
    if let Some(mt) = req.mtime {
        let mut t = fs::FileTimes::new().set_modified(mt);
        if let Some(a) = req.atime {
            t = t.set_accessed(a);
        }
        dst.set_times(t)?;
    }
    if req.sync || req.verify {
        dst.sync_all()?;
    }
    Ok(())
}

fn copy_body(
    src: &mut File,
    dst: &mut File,
    req: &CopyRequest,
    ctl: &mut dyn CopyControl,
) -> io::Result<CopyOutcome> {
    let size = src.metadata()?.len();
    if size == 0 {
        return Ok(CopyOutcome {
            bytes: 0,
            method: CopyMethod::Empty,
        });
    }

    if req.verify {
        return copy_rw(src, dst, ctl, true, &req.dst);
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // 1) Reflink: instant and space-free where the filesystem supports it.
        // SAFETY: both descriptors are open for the duration of the call.
        let cloned =
            unsafe { libc::ioctl(dst.as_raw_fd(), libc::FICLONE as _, src.as_raw_fd()) } == 0;
        if cloned {
            if !ctl.advance(size) {
                return Err(fault(CopyFault::Cancelled));
            }
            return Ok(CopyOutcome {
                bytes: size,
                method: CopyMethod::Reflink,
            });
        }
        // 2) copy_file_range in chunks, so progress and cancellation stay responsive.
        let mut total = 0u64;
        loop {
            // SAFETY: null offsets make the kernel use and advance the file offsets.
            let n = unsafe {
                libc::copy_file_range(
                    src.as_raw_fd(),
                    std::ptr::null_mut(),
                    dst.as_raw_fd(),
                    std::ptr::null_mut(),
                    CFR_CHUNK,
                    0,
                )
            };
            if n < 0 {
                let e = io::Error::last_os_error();
                match e.raw_os_error() {
                    Some(libc::EINTR) => continue,
                    Some(
                        libc::EXDEV
                        | libc::ENOSYS
                        | libc::EINVAL
                        | libc::EOPNOTSUPP
                        | libc::EPERM
                        | libc::EBADF
                        | libc::ETXTBSY
                        | libc::EISDIR,
                    ) => {
                        // Not supported here (cross-fs on old kernels, special fs): read/write.
                        let rest = copy_rw(src, dst, ctl, false, &req.dst)?;
                        return Ok(CopyOutcome {
                            bytes: total + rest.bytes,
                            method: if total == 0 {
                                CopyMethod::ReadWrite
                            } else {
                                CopyMethod::KernelCopy
                            },
                        });
                    }
                    _ => return Err(e),
                }
            }
            if n == 0 {
                break;
            }
            total += n as u64;
            if !ctl.advance(n as u64) {
                return Err(fault(CopyFault::Cancelled));
            }
        }
        Ok(CopyOutcome {
            bytes: total,
            method: CopyMethod::KernelCopy,
        })
    }

    #[cfg(not(target_os = "linux"))]
    copy_rw(src, dst, ctl, false, &req.dst)
}

fn copy_rw(
    src: &mut File,
    dst: &mut File,
    ctl: &mut dyn CopyControl,
    verify: bool,
    dst_path: &Path,
) -> io::Result<CopyOutcome> {
    let mut buf = vec![0u8; RW_CHUNK];
    let mut hasher = verify.then(Xxh3::new);
    let mut total = 0u64;
    loop {
        let n = match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        dst.write_all(&buf[..n])?;
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..n]);
        }
        total += n as u64;
        if !ctl.advance(n as u64) {
            return Err(fault(CopyFault::Cancelled));
        }
    }
    if let Some(h) = hasher {
        dst.sync_all()?;
        let want = h.digest128();
        let (len, got) = hash_path(dst_path, ctl)?;
        if len != total {
            return Err(fault(CopyFault::VerifyMismatch(format!(
                "copy has {len} bytes, expected {total}"
            ))));
        }
        if got != want {
            return Err(fault(CopyFault::VerifyMismatch(
                "copy differs from the source (checksum mismatch)".into(),
            )));
        }
    }
    Ok(CopyOutcome {
        bytes: total,
        method: CopyMethod::ReadWrite,
    })
}

/// Hash a file from disk (dropping cached pages first where possible, so that the
/// bytes really come back from the medium).
fn hash_path(p: &Path, ctl: &mut dyn CopyControl) -> io::Result<(u64, u128)> {
    let mut f = File::open(p)?;
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: plain advisory call on an open descriptor.
        unsafe {
            libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
        }
    }
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; RW_CHUNK];
    let mut total = 0u64;
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        h.update(&buf[..n]);
        total += n as u64;
        if !ctl.advance(0) {
            return Err(fault(CopyFault::Cancelled));
        }
    }
    Ok((total, h.digest128()))
}
