use std::path::PathBuf;
use std::time::Duration;

use crate::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Virtual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub mount_point: PathBuf,
    pub label: Option<String>,
    pub fs_type: String,
    pub device: String,
    pub kind: VolumeKind,
    /// Windows drive letter (`C`); always `None` elsewhere.
    pub drive_letter: Option<char>,
    pub total: Option<u64>,
    pub available: Option<u64>,
    pub read_only: bool,
    /// False when the mount did not answer in time (hung network share): it is
    /// listed anyway, but nothing is read from it.
    pub responsive: bool,
}

/// Enumerates mounted volumes. May block: only ever call it from a worker thread.
pub trait VolumeLister: Send + Sync {
    fn list(&self) -> Result<Vec<Volume>>;

    /// Wait up to `timeout` for the set of mounts to change; `true` if it did.
    /// The default just sleeps, so callers fall back to periodic polling.
    fn wait_for_change(&self, timeout: Duration) -> bool {
        std::thread::sleep(timeout);
        false
    }
}

/// Which mounts are not disks a person would want to see: virtual and system filesystems,
/// the boot partitions. Entries are matched like this:
///
/// * `name` is a filesystem type (`tmpfs`); a trailing `*` makes it a prefix (`fuse.*`);
/// * `/path` is a mount point; a trailing `/*` makes it a folder and everything below it
///   (`/dev/*`).
///
/// The configuration's `devices.hide` replaces [`DeviceFilter::DEFAULT`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceFilter {
    fs_types: Vec<String>,
    mounts: Vec<String>,
}

impl DeviceFilter {
    pub const DEFAULT: &'static [&'static str] = &[
        // Virtual and system filesystems.
        "binder",
        "binderfs",
        "proc",
        "sysfs",
        "tmpfs",
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
        "overlay",
        "fuse.gvfsd-fuse",
        "fuse.portal",
        "fuse.snapfuse",
        "fuse.lxcfs",
        // The system's own corners.
        "/boot",
        "/boot/efi",
        "/boot/firmware",
        "/efi",
        "/proc/*",
        "/sys/*",
        "/dev/*",
        "/run/user/*",
        "/run/credentials/*",
        "/var/lib/docker/*",
        "/var/lib/snapd/*",
        "/snap/*",
    ];

    pub fn new(entries: &[String]) -> DeviceFilter {
        let (mounts, fs_types) = entries
            .iter()
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .partition(|e: &String| e.starts_with('/'));
        DeviceFilter { fs_types, mounts }
    }

    /// Does the mount of `fs_type` at `mount_point` stay out of the list?
    pub fn hides(&self, fs_type: &str, mount_point: &std::path::Path) -> bool {
        let by_type = |pat: &String| match pat.strip_suffix('*') {
            Some(prefix) => fs_type.starts_with(prefix),
            None => fs_type == pat,
        };
        let by_mount = |pat: &String| match pat.strip_suffix("/*") {
            Some(dir) => mount_point.starts_with(dir),
            None => mount_point == std::path::Path::new(pat),
        };
        self.fs_types.iter().any(by_type) || self.mounts.iter().any(by_mount)
    }
}

impl Default for DeviceFilter {
    fn default() -> Self {
        DeviceFilter::new(
            &Self::DEFAULT
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        )
    }
}
