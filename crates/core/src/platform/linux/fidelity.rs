//! Linux: owner, group, extended attributes and POSIX ACLs, with plain libc calls.

use std::ffi::{CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use crate::platform::fidelity::{AttrOutcome, DestSupport, Fidelity, Preserve, SourceAttrs};

pub struct LinuxFidelity;

const ACL_PREFIX: &[u8] = b"system.posix_acl_";

fn cpath(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}

/// Names of the extended attributes of `p` (not following a symlink).
fn list(p: &Path) -> io::Result<Vec<OsString>> {
    let c = cpath(p)?;
    loop {
        // SAFETY: a NULL buffer with size 0 asks for the needed size.
        let need = unsafe { libc::llistxattr(c.as_ptr(), std::ptr::null_mut(), 0) };
        if need < 0 {
            return Err(io::Error::last_os_error());
        }
        if need == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; need as usize];
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        let got = unsafe { libc::llistxattr(c.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if got < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ERANGE) {
                continue; // grew in between
            }
            return Err(e);
        }
        buf.truncate(got as usize);
        return Ok(buf
            .split(|&b| b == 0)
            .filter(|n| !n.is_empty())
            .map(|n| OsString::from_vec(n.to_vec()))
            .collect());
    }
}

fn get(p: &Path, name: &OsStr) -> io::Result<Vec<u8>> {
    let c = cpath(p)?;
    let n = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains NUL"))?;
    loop {
        // SAFETY: NULL buffer, size 0: query the size.
        let need = unsafe { libc::lgetxattr(c.as_ptr(), n.as_ptr(), std::ptr::null_mut(), 0) };
        if need < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buf = vec![0u8; need as usize];
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        let got =
            unsafe { libc::lgetxattr(c.as_ptr(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if got < 0 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::ERANGE) {
                continue;
            }
            return Err(e);
        }
        buf.truncate(got as usize);
        return Ok(buf);
    }
}

fn set(p: &Path, name: &OsStr, value: &[u8]) -> io::Result<()> {
    let c = cpath(p)?;
    let n = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains NUL"))?;
    // SAFETY: valid C strings and a valid value buffer.
    let r = unsafe {
        libc::lsetxattr(
            c.as_ptr(),
            n.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn is_acl(name: &OsStr) -> bool {
    name.as_bytes().starts_with(ACL_PREFIX)
}

fn unsupported(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ENOTSUP) | Some(libc::ENOSYS))
}

fn denied(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::EPERM) | Some(libc::EACCES))
}

impl Fidelity for LinuxFidelity {
    fn implemented(&self) -> bool {
        true
    }

    fn destination_support(&self, dir: &Path) -> DestSupport {
        let xattrs = match list(dir) {
            Ok(_) => true,
            Err(e) => !unsupported(&e),
        };
        let acl = xattrs
            && match get(dir, OsStr::new("system.posix_acl_access")) {
                Ok(_) => true,
                // Supported, but this folder has no ACL of its own.
                Err(e) if e.raw_os_error() == Some(libc::ENODATA) => true,
                Err(e) => !unsupported(&e),
            };
        DestSupport { xattrs, acl }
    }

    fn source_attributes(&self, path: &Path) -> SourceAttrs {
        let Ok(names) = list(path) else {
            return SourceAttrs::default();
        };
        SourceAttrs {
            acl: names.iter().any(|n| is_acl(n)),
            xattrs: names.iter().any(|n| !is_acl(n)),
        }
    }

    fn can_set_owner(&self, uid: u32, gid: u32) -> bool {
        // SAFETY: these calls cannot fail (geteuid/getegid) or are bounded (getgroups).
        unsafe {
            if libc::geteuid() == 0 {
                return true;
            }
            if uid != libc::geteuid() {
                return false;
            }
            if gid == libc::getegid() {
                return true;
            }
            let n = libc::getgroups(0, std::ptr::null_mut());
            if n <= 0 {
                return false;
            }
            let mut groups = vec![0 as libc::gid_t; n as usize];
            let got = libc::getgroups(n, groups.as_mut_ptr());
            got > 0 && groups[..got as usize].contains(&gid)
        }
    }

    fn copy_attributes(
        &self,
        src: &Path,
        dst: &Path,
        what: Preserve,
        owner: Option<(u32, u32)>,
    ) -> AttrOutcome {
        let mut out = AttrOutcome::default();

        if what.xattrs {
            match list(src) {
                Ok(names) => {
                    for name in names {
                        let value = match get(src, &name) {
                            Ok(v) => v,
                            // Gone in between, or unreadable (trusted.* without privilege).
                            Err(e) if e.raw_os_error() == Some(libc::ENODATA) => continue,
                            Err(_) => {
                                out.note(describe(&name, "cannot be read"));
                                continue;
                            }
                        };
                        match set(dst, &name, &value) {
                            Ok(()) => {}
                            Err(e) if unsupported(&e) => {
                                out.note(if is_acl(&name) {
                                    "ACLs (not supported by the destination)".to_string()
                                } else {
                                    "extended attributes (not supported by the destination)"
                                        .to_string()
                                });
                            }
                            // A security label follows the destination's own policy.
                            Err(e) if denied(&e) && name.as_bytes() == b"security.selinux" => {}
                            Err(e) if denied(&e) => out.note(describe(&name, "not permitted")),
                            Err(_) => out.note(describe(&name, "could not be set")),
                        }
                    }
                }
                // The source filesystem has none to give.
                Err(e) if unsupported(&e) => {}
                Err(_) => out.note("extended attributes (cannot be read)"),
            }
        }

        if what.owner
            && let Some((uid, gid)) = owner
        {
            if self.can_set_owner(uid, gid) {
                if let Ok(c) = cpath(dst) {
                    // SAFETY: valid C string; AT_SYMLINK_NOFOLLOW keeps a symlink swapped in
                    // after planning from redirecting the change.
                    let r = unsafe {
                        libc::fchownat(
                            libc::AT_FDCWD,
                            c.as_ptr(),
                            uid,
                            gid,
                            libc::AT_SYMLINK_NOFOLLOW,
                        )
                    };
                    if r != 0 {
                        out.note("owner and group (not permitted)");
                    }
                }
            } else {
                out.note("owner and group (the copy belongs to you)");
            }
        }
        out
    }
}

fn describe(name: &OsStr, why: &str) -> String {
    let n = name.to_string_lossy();
    if is_acl(name) {
        format!("ACLs ({why})")
    } else {
        format!("attribute {n} ({why})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        // The disk of the project, not /tmp: user.* attributes need a real filesystem.
        tempfile::Builder::new()
            .prefix("rada-xattr-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap()
    }

    #[test]
    fn extended_attributes_and_the_listing_round_trip() {
        let d = tmp();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "y").unwrap();
        if set(&a, OsStr::new("user.rada.test"), b"hello").is_err() {
            return; // this filesystem has no user attributes: nothing to test here
        }
        assert!(LinuxFidelity.source_attributes(&a).xattrs);
        assert!(!LinuxFidelity.source_attributes(&b).any());
        let out = LinuxFidelity.copy_attributes(&a, &b, Preserve::default(), None);
        assert!(out.not_preserved.is_empty(), "{out:?}");
        assert_eq!(get(&b, OsStr::new("user.rada.test")).unwrap(), b"hello");
    }

    #[test]
    fn the_owner_check_is_honest() {
        let me = unsafe { libc::geteuid() };
        let my_group = unsafe { libc::getegid() };
        assert!(LinuxFidelity.can_set_owner(me, my_group));
        if me != 0 {
            assert!(!LinuxFidelity.can_set_owner(0, 0));
        }
    }
}
