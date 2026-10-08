//! Volume enumeration from `/proc/self/mountinfo`, with a per-mount probe timeout so a
//! hung network share can never stall the caller (a known failure of other file managers).

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::fs::{FsEngine, LocalFs};
use crate::platform::volumes::{Volume, VolumeKind, VolumeLister};
use crate::{Error, Result};

const PROBE_TIMEOUT: Duration = Duration::from_millis(700);
const MOUNTINFO: &str = "/proc/self/mountinfo";

pub struct LinuxVolumes {
    /// Mounts whose last probe is still blocked: never probed twice in parallel.
    stuck: Mutex<HashMap<PathBuf, Arc<AtomicBool>>>,
}

impl LinuxVolumes {
    pub fn new() -> Self {
        LinuxVolumes {
            stuck: Mutex::new(HashMap::new()),
        }
    }

    /// `statvfs` on a helper thread, abandoned after [`PROBE_TIMEOUT`].
    fn probe(&self, mount: &Path) -> Option<u64> {
        let mut stuck = self.stuck.lock().ok()?;
        if let Some(flag) = stuck.get(mount) {
            if !flag.load(Ordering::Acquire) {
                return None; // previous probe never returned
            }
            stuck.remove(mount);
        }
        let done = Arc::new(AtomicBool::new(false));
        stuck.insert(mount.to_path_buf(), done.clone());
        drop(stuck);

        let (tx, rx) = crossbeam_channel::bounded(1);
        let p = mount.to_path_buf();
        std::thread::spawn(move || {
            let r = LocalFs.available_space(&p).ok();
            done.store(true, Ordering::Release);
            let _ = tx.send(r);
        });
        rx.recv_timeout(PROBE_TIMEOUT).ok().flatten()
    }
}

impl Default for LinuxVolumes {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeLister for LinuxVolumes {
    fn list(&self) -> Result<Vec<Volume>> {
        let text = std::fs::read(MOUNTINFO).map_err(|e| Error::io("read", MOUNTINFO, e))?;
        let labels = labels_by_device();
        let mut seen: HashMap<PathBuf, usize> = HashMap::new();
        let mut out: Vec<Volume> = Vec::new();
        for line in text.split(|&b| b == b'\n') {
            let Some(m) = parse_line(line) else { continue };
            if !is_interesting(&m) {
                continue;
            }
            let responsive_space = self.probe(&m.mount_point);
            let vol = Volume {
                label: labels.get(&m.source).cloned(),
                kind: classify(&m),
                drive_letter: None,
                total: None,
                available: responsive_space,
                read_only: m.options.split(',').any(|o| o == "ro"),
                responsive: responsive_space.is_some(),
                mount_point: m.mount_point.clone(),
                fs_type: m.fstype,
                device: m.source,
            };
            // A later mount on the same point hides the earlier one.
            if let Some(&i) = seen.get(&m.mount_point) {
                out[i] = vol;
            } else {
                seen.insert(m.mount_point, out.len());
                out.push(vol);
            }
        }
        Ok(out)
    }

    fn wait_for_change(&self, timeout: Duration) -> bool {
        use std::os::fd::AsRawFd;
        let Ok(f) = std::fs::File::open(MOUNTINFO) else {
            std::thread::sleep(timeout);
            return false;
        };
        let mut pfd = libc::pollfd {
            fd: f.as_raw_fd(),
            events: libc::POLLPRI | libc::POLLERR,
            revents: 0,
        };
        // SAFETY: one valid pollfd, count 1.
        let r = unsafe {
            libc::poll(
                &mut pfd,
                1,
                timeout.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        r > 0 && pfd.revents & (libc::POLLPRI | libc::POLLERR) != 0
    }
}

struct MountLine {
    mount_point: PathBuf,
    options: String,
    fstype: String,
    source: String,
}

fn parse_line(line: &[u8]) -> Option<MountLine> {
    if line.is_empty() {
        return None;
    }
    let fields: Vec<&[u8]> = line.split(|&b| b == b' ').collect();
    let dash = fields.iter().position(|f| *f == b"-")?;
    if dash < 6 || fields.len() < dash + 3 {
        return None;
    }
    Some(MountLine {
        mount_point: PathBuf::from(OsString::from_vec(unescape(fields[4]))),
        options: String::from_utf8_lossy(fields[5]).into_owned(),
        fstype: String::from_utf8_lossy(fields[dash + 1]).into_owned(),
        source: String::from_utf8_lossy(&unescape(fields[dash + 2])).into_owned(),
    })
}

/// mountinfo escapes space, tab, newline and backslash as three octal digits.
fn unescape(f: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(f.len());
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'\\' && i + 4 <= f.len() {
            let oct = &f[i + 1..i + 4];
            if oct.iter().all(|d| (b'0'..=b'7').contains(d)) {
                let v = (oct[0] - b'0') as u16 * 64
                    + (oct[1] - b'0') as u16 * 8
                    + (oct[2] - b'0') as u16;
                out.push(v as u8);
                i += 4;
                continue;
            }
        }
        out.push(f[i]);
        i += 1;
    }
    out
}

const PSEUDO: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "securityfs",
    "debugfs",
    "tracefs",
    "configfs",
    "fusectl",
    "mqueue",
    "hugetlbfs",
    "pstore",
    "bpf",
    "autofs",
    "binfmt_misc",
    "efivarfs",
    "rpc_pipefs",
    "nsfs",
    "squashfs",
    "ramfs",
    "selinuxfs",
    "fuse.gvfsd-fuse",
    "fuse.portal",
    "overlay",
];

fn is_interesting(m: &MountLine) -> bool {
    if PSEUDO.contains(&m.fstype.as_str()) {
        return false;
    }
    let p = &m.mount_point;
    let hidden_roots = [
        "/proc",
        "/sys",
        "/run/user",
        "/run/credentials",
        "/var/lib/docker",
        "/snap",
    ];
    if hidden_roots.iter().any(|r| p.starts_with(r)) {
        return false;
    }
    if m.fstype == "tmpfs" {
        // Real tmpfs mounts users care about; the rest is system plumbing.
        return p == Path::new("/tmp") || p == Path::new("/dev/shm");
    }
    true
}

fn is_network(fstype: &str) -> bool {
    matches!(
        fstype,
        "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "9p" | "ceph" | "afs"
    ) || fstype.starts_with("fuse.sshfs")
}

fn classify(m: &MountLine) -> VolumeKind {
    if is_network(&m.fstype) {
        VolumeKind::Network
    } else if matches!(m.fstype.as_str(), "iso9660" | "udf") {
        VolumeKind::Optical
    } else if m.fstype == "tmpfs" {
        VolumeKind::Virtual
    } else if m.mount_point.starts_with("/run/media") || m.mount_point.starts_with("/media") {
        VolumeKind::Removable
    } else {
        VolumeKind::Fixed
    }
}

/// `/dev/sda1` -> "LABEL", from `/dev/disk/by-label`.
fn labels_by_device() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(rd) = std::fs::read_dir("/dev/disk/by-label") else {
        return map;
    };
    for e in rd.flatten() {
        let Ok(target) = std::fs::canonicalize(e.path()) else {
            continue;
        };
        let label = unescape_label(&e.file_name().to_string_lossy());
        map.insert(target.to_string_lossy().into_owned(), label);
    }
    map
}

/// udev encodes odd characters in label links as `\xNN`.
fn unescape_label(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 4 <= b.len() && b[i + 1] == b'x' {
            if let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_mountinfo_line_with_escapes() {
        let l = b"36 35 98:0 / /mnt/my\\040disk rw,noatime shared:1 - ext4 /dev/sdb1 rw,errors=continue";
        let m = parse_line(l).unwrap();
        assert_eq!(m.mount_point, PathBuf::from("/mnt/my disk"));
        assert_eq!(m.fstype, "ext4");
        assert_eq!(m.source, "/dev/sdb1");
        assert!(m.options.contains("noatime"));
    }

    #[test]
    fn pseudo_filesystems_are_hidden() {
        let m = parse_line(b"20 1 0:5 / /proc rw - proc proc rw").unwrap();
        assert!(!is_interesting(&m));
        let m = parse_line(b"30 1 8:1 / /home rw - ext4 /dev/sda2 rw").unwrap();
        assert!(is_interesting(&m));
    }

    #[test]
    fn listing_real_mounts_works_and_is_bounded() {
        let started = std::time::Instant::now();
        let v = LinuxVolumes::new().list().unwrap();
        assert!(!v.is_empty());
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
