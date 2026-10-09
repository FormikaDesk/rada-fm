//! Volume enumeration from `/proc/self/mountinfo`, with a per-mount probe timeout so a
//! hung network share can never stall the caller (a known failure of other file managers).

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::platform::volumes::{DeviceFilter, Volume, VolumeKind, VolumeLister};
use crate::{Error, Result};

const PROBE_TIMEOUT: Duration = Duration::from_millis(700);
const MOUNTINFO: &str = "/proc/self/mountinfo";

pub struct LinuxVolumes {
    /// Mounts whose last probe is still blocked: never probed twice in parallel.
    stuck: Mutex<HashMap<PathBuf, Arc<AtomicBool>>>,
    filter: DeviceFilter,
}

impl LinuxVolumes {
    pub fn new() -> Self {
        Self::with_filter(DeviceFilter::default())
    }

    pub fn with_filter(filter: DeviceFilter) -> Self {
        LinuxVolumes {
            stuck: Mutex::new(HashMap::new()),
            filter,
        }
    }

    /// `statvfs` on a helper thread, abandoned after [`PROBE_TIMEOUT`]: (total, available).
    fn probe(&self, mount: &Path) -> Option<(u64, u64)> {
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
            let r = crate::fs::local::space_of(&p).ok();
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
        let mut out: Vec<Volume> = Vec::new();
        for m in select_mounts(&text, &self.filter) {
            let space = self.probe(&m.mount_point);
            out.push(Volume {
                label: labels.get(&m.source).cloned(),
                kind: classify(&m),
                drive_letter: None,
                total: space.map(|s| s.0),
                available: space.map(|s| s.1),
                read_only: m.options.split(',').any(|o| o == "ro"),
                responsive: space.is_some(),
                mount_point: m.mount_point.clone(),
                fs_type: m.fstype,
                device: m.source,
            });
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
    /// `major:minor` of the device.
    dev: String,
    /// The part of the filesystem that is mounted here (`/` unless it is a bind mount or a
    /// subvolume).
    root: String,
    mount_point: PathBuf,
    options: String,
    fstype: String,
    source: String,
    super_options: String,
}

/// The mounts worth showing, in order: the filter applied, bind mounts of something that is
/// already listed left out, and a later mount on the same point replacing the earlier one.
fn select_mounts(mountinfo: &[u8], filter: &DeviceFilter) -> Vec<MountLine> {
    let mut seen_root: std::collections::HashSet<(String, String)> = Default::default();
    let mut seen_dev: std::collections::HashSet<String> = Default::default();
    let mut by_point: HashMap<PathBuf, usize> = HashMap::new();
    let mut out: Vec<MountLine> = Vec::new();
    for line in mountinfo.split(|&b| b == b'\n') {
        let Some(m) = parse_line(line) else { continue };
        if filter.hides(&m.fstype, &m.mount_point) {
            continue;
        }
        // The same part of the same device a second time is a bind mount; so is a folder of
        // a device already listed (a btrfs subvolume, which names itself, is a disk of its own).
        let key = (m.dev.clone(), m.root.clone());
        let subvolume = m.super_options.split(',').any(|o| o.starts_with("subvol"));
        let bind_of_listed = m.root != "/" && !subvolume && seen_dev.contains(&m.dev);
        if seen_root.contains(&key) || bind_of_listed {
            continue;
        }
        seen_root.insert(key);
        seen_dev.insert(m.dev.clone());
        // A later mount on the same point hides the earlier one.
        if let Some(&i) = by_point.get(&m.mount_point) {
            out[i] = m;
        } else {
            by_point.insert(m.mount_point.clone(), out.len());
            out.push(m);
        }
    }
    out
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
        dev: String::from_utf8_lossy(fields[2]).into_owned(),
        root: String::from_utf8_lossy(&unescape(fields[3])).into_owned(),
        mount_point: PathBuf::from(OsString::from_vec(unescape(fields[4]))),
        options: String::from_utf8_lossy(fields[5]).into_owned(),
        fstype: String::from_utf8_lossy(fields[dash + 1]).into_owned(),
        source: String::from_utf8_lossy(&unescape(fields[dash + 2])).into_owned(),
        super_options: fields
            .get(dash + 3)
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .unwrap_or_default(),
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
        if b[i] == b'\\'
            && i + 4 <= b.len()
            && b[i + 1] == b'x'
            && let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16)
        {
            out.push(v);
            i += 4;
            continue;
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

    /// A real machine's mountinfo, trimmed: system filesystems, Waydroid's binderfs, the boot
    /// partitions, bind mounts, a btrfs subvolume, a USB stick and a network share.
    const SAMPLE: &str = "\
22 28 0:21 / /sys rw,nosuid,nodev,noexec,relatime shared:7 - sysfs sysfs rw
23 28 0:22 / /proc rw,nosuid,nodev,noexec,relatime shared:13 - proc proc rw
24 28 0:5 / /dev rw,nosuid shared:2 - devtmpfs devtmpfs rw,size=4096k
25 24 0:23 / /dev/pts rw,nosuid,noexec,relatime shared:3 - devpts devpts rw,gid=5,mode=620
26 28 0:24 / /run rw,nosuid,nodev shared:5 - tmpfs tmpfs rw,mode=755
27 22 0:25 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime shared:8 - cgroup2 cgroup2 rw
28 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
29 22 0:30 / /sys/firmware/efi/efivars rw,nosuid,nodev,noexec,relatime shared:9 - efivarfs efivarfs rw
30 28 259:1 / /boot/efi rw,relatime shared:44 - vfat /dev/nvme0n1p1 rw,fmask=0077
31 28 259:3 / /boot rw,relatime shared:45 - ext4 /dev/nvme0n1p3 rw
32 24 0:41 / /dev/binderfs rw,relatime shared:60 - binder binder rw,max=1048576
33 28 0:42 / /tmp rw,nosuid,nodev shared:10 - tmpfs tmpfs rw
34 28 0:43 / /snap/core22/1612 ro,nodev,relatime shared:61 - squashfs /dev/loop0 ro,errors=continue
35 28 0:44 / /run/user/1000/doc rw,nosuid,nodev,relatime shared:62 - fuse.portal portal rw,user_id=1000
36 28 0:45 / /var/lib/docker/overlay2/x/merged rw,relatime shared:63 - overlay overlay rw,lowerdir=/a
37 28 259:2 /var/lib/flatpak /var/lib/flatpak rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
38 28 259:2 / /mnt/bind-of-root rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
39 28 259:4 / /home rw,relatime shared:64 - ext4 /dev/nvme0n1p4 rw
40 28 0:50 /@data /data rw,relatime shared:65 - btrfs /dev/sda2 rw,subvolid=257,subvol=/@data
41 28 0:50 /@backup /backup rw,relatime shared:66 - btrfs /dev/sda2 rw,subvolid=258,subvol=/@backup
42 28 8:17 / /run/media/user/USB\\040STICK rw,nosuid,nodev,relatime shared:70 - vfat /dev/sdb1 rw
43 28 0:60 / /mnt/nas rw,relatime shared:71 - nfs4 nas:/export rw,vers=4.2
";

    fn points(text: &str, filter: &DeviceFilter) -> Vec<String> {
        select_mounts(text.as_bytes(), filter)
            .iter()
            .map(|m| m.mount_point.display().to_string())
            .collect()
    }

    #[test]
    fn only_real_disks_partitions_removables_and_shares_are_listed() {
        assert_eq!(
            points(SAMPLE, &DeviceFilter::default()),
            [
                "/",
                "/home",
                "/data",
                "/backup",
                "/run/media/user/USB STICK",
                "/mnt/nas"
            ],
            "virtual and system filesystems, binderfs, the boot partitions and bind mounts are out"
        );
    }

    #[test]
    fn each_kind_of_noise_is_hidden_for_its_own_reason() {
        let f = DeviceFilter::default();
        let hidden = |fs: &str, at: &str| f.hides(fs, Path::new(at));
        for (fs, at) in [
            ("binder", "/dev/binderfs"),
            ("binderfs", "/mnt/x"),
            ("tmpfs", "/tmp"),
            ("tmpfs", "/run"),
            ("proc", "/proc"),
            ("sysfs", "/sys"),
            ("cgroup2", "/sys/fs/cgroup"),
            ("overlay", "/var/lib/docker/overlay2/x/merged"),
            ("squashfs", "/snap/core22/1612"),
            ("fuse.portal", "/run/user/1000/doc"),
            ("efivarfs", "/sys/firmware/efi/efivars"),
            ("vfat", "/boot/efi"),
            ("ext4", "/boot"),
            ("vfat", "/efi"),
            ("ext4", "/dev/binderfs"),
        ] {
            assert!(hidden(fs, at), "{fs} at {at} should be hidden");
        }
        for (fs, at) in [
            ("ext4", "/"),
            ("ext4", "/home"),
            ("btrfs", "/data"),
            ("vfat", "/run/media/user/USB"),
            ("nfs4", "/mnt/nas"),
            ("ntfs3", "/mnt/windows"),
        ] {
            assert!(!hidden(fs, at), "{fs} at {at} should be shown");
        }
    }

    #[test]
    fn the_hidden_list_is_configurable_and_replaces_the_default() {
        // Someone who wants to see tmpfs mounts and the boot partition, and not the share.
        let custom = DeviceFilter::new(&["nfs4".to_string(), "/proc/*".to_string()]);
        let shown = points(SAMPLE, &custom);
        assert!(shown.contains(&"/tmp".to_string()), "{shown:?}");
        assert!(shown.contains(&"/boot".to_string()), "{shown:?}");
        assert!(!shown.contains(&"/mnt/nas".to_string()), "{shown:?}");
        assert!(!shown.contains(&"/proc".to_string()));
        // Wildcards: a prefix of filesystem types and a folder with everything below it.
        let f = DeviceFilter::new(&["fuse.*".to_string(), "/mnt/*".to_string()]);
        assert!(f.hides("fuse.sshfs", Path::new("/x")));
        assert!(f.hides("ext4", Path::new("/mnt/a/b")));
        assert!(!f.hides("ext4", Path::new("/mntx")));
    }

    #[test]
    fn listing_real_mounts_works_and_is_bounded() {
        let started = std::time::Instant::now();
        let v = LinuxVolumes::new().list().unwrap();
        assert!(!v.is_empty());
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
