//! Ejecting a disk with the system's own tools, and turning what they complain about into a
//! sentence. The tools are `diskutil` (macOS) and `udisksctl` (Linux); both run with the
//! rights of the person, with no password asked.

use std::process::{Command, Stdio};

use crate::{Error, Result};

/// Why `diskutil eject` or `diskutil unmount` refused, from what it printed.
pub fn explain_diskutil(output: &str) -> String {
    // "…failed to unmount: dissented by PID 412 (/System/Library/CoreServices/Finder.app/…)"
    if let Some(rest) = output.split("dissented by PID").nth(1) {
        let pid: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let who = rest
            .split_once('(')
            .and_then(|(_, r)| r.split_once(')'))
            .map(|(p, _)| p)
            .and_then(|p| {
                let p = p.rsplit('/').next().unwrap_or(p);
                // `Finder.app/Contents/MacOS/Finder` → the last part is the program.
                Some(p.trim().to_string()).filter(|s| !s.is_empty())
            });
        return match who {
            Some(w) => format!(
                "the disk is busy: {w} (process {pid}) is still using it. Close it, or what it has open on the disk, and try again."
            ),
            None => format!(
                "the disk is busy: a program (process {pid}) is still using it. Close it and try again."
            ),
        };
    }
    let low = output.to_lowercase();
    if low.contains("unmount failed")
        || low.contains("couldn't unmount")
        || low.contains("failed to unmount")
        || low.contains("busy")
    {
        return "the disk is busy: a program still has something open on it. Close it and try again.".into();
    }
    if low.contains("not permitted") || low.contains("privilege") {
        return "macOS did not allow it; eject the disk from the Finder.".into();
    }
    let first = output.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.is_empty() {
        "the system could not eject it".into()
    } else {
        format!("the system could not eject it: {}", first.trim())
    }
}

/// Why `udisksctl` refused.
pub fn explain_udisks(output: &str) -> String {
    let low = output.to_lowercase();
    if low.contains("devicebusy") || low.contains("target is busy") || low.contains("busy") {
        return "the disk is busy: a program still has a file or a folder on it open. Close it and try again.".into();
    }
    if low.contains("notauthorized") || low.contains("not authorized") {
        return "the system did not allow it without a password; unmount it from your desktop's disk tool.".into();
    }
    if low.contains("not mounted") || low.contains("notmounted") {
        return "the disk was not mounted any more.".into();
    }
    let first = output.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.is_empty() {
        "the system could not eject it".into()
    } else {
        format!("the system could not eject it: {}", first.trim())
    }
}

/// Run `program args…` and return what it printed (both streams), or the explanation.
fn run(
    program: &str,
    args: &[&str],
    explain: fn(&str) -> String,
    missing: &'static str,
) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Invalid(missing.into())
            } else {
                Error::Invalid(format!("cannot start {program}: {e}"))
            }
        })?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        Err(Error::Invalid(explain(&text)))
    }
}

/// macOS: `diskutil eject <mount point>`.
pub fn eject_with_diskutil(mount_point: &std::path::Path) -> Result<()> {
    run(
        "diskutil",
        &["eject", &mount_point.to_string_lossy()],
        explain_diskutil,
        "diskutil was not found",
    )
    .map(|_| ())
}

/// Linux: `udisksctl unmount -b <device>`, then power the drive off when that is possible.
pub fn eject_with_udisks(device: &str) -> Result<()> {
    run(
        "udisksctl",
        &["unmount", "-b", device],
        explain_udisks,
        "udisksctl was not found; install udisks2 to eject disks from rada",
    )?;
    // Powering off is the second half of "eject": best effort, the disk is already safe to
    // unplug if the drive has no way to be switched off.
    if let Some(disk) = parent_disk(device) {
        let _ = Command::new("udisksctl")
            .args(["power-off", "-b", &disk])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}

/// `/dev/sdb1` → `/dev/sdb`, `/dev/nvme0n1p2` → `/dev/nvme0n1`, `/dev/mmcblk0p1` →
/// `/dev/mmcblk0`; `None` for what is not a partition of a disk.
pub fn parent_disk(device: &str) -> Option<String> {
    let name = device.strip_prefix("/dev/")?;
    if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("hd") {
        let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
        return (base.len() < name.len() && !base.is_empty()).then(|| format!("/dev/{base}"));
    }
    if name.starts_with("nvme") || name.starts_with("mmcblk") || name.starts_with("loop") {
        let (base, part) = name.rsplit_once('p')?;
        return (!part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
            .then(|| format!("/dev/{base}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_someone_is_using_names_who() {
        let out = "Volume Backup on disk4s2 failed to unmount: dissented by PID 412 (/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder)\n";
        let e = explain_diskutil(out);
        assert!(e.contains("busy"), "{e}");
        assert!(e.contains("Finder (process 412)"), "{e}");
        assert!(e.contains("try again"), "{e}");
        let e = explain_diskutil("Unmount failed for /dev/disk4s1\n");
        assert!(e.contains("busy"), "{e}");
        let e = explain_diskutil("");
        assert_eq!(e, "the system could not eject it");
        let e = explain_diskutil("Some other complaint\nmore");
        assert!(e.ends_with("Some other complaint"), "{e}");
    }

    #[test]
    fn udisks_complaints_become_sentences() {
        let busy = "Error unmounting /dev/sdb1: GDBus.Error:org.freedesktop.UDisks2.Error.DeviceBusy: target is busy";
        assert!(explain_udisks(busy).contains("busy"));
        assert!(explain_udisks("Error: GDBus.Error:org.freedesktop.UDisks2.Error.NotAuthorizedCanObtain: Not authorized")
            .contains("without a password"));
        assert!(explain_udisks("Error unmounting: NotMounted").contains("not mounted"));
    }

    #[test]
    fn the_whole_disk_is_found_from_a_partition() {
        assert_eq!(parent_disk("/dev/sdb1").as_deref(), Some("/dev/sdb"));
        assert_eq!(parent_disk("/dev/sdc12").as_deref(), Some("/dev/sdc"));
        assert_eq!(
            parent_disk("/dev/nvme0n1p2").as_deref(),
            Some("/dev/nvme0n1")
        );
        assert_eq!(
            parent_disk("/dev/mmcblk0p1").as_deref(),
            Some("/dev/mmcblk0")
        );
        assert_eq!(parent_disk("/dev/sdb"), None);
        assert_eq!(parent_disk("/dev/mapper/crypt"), None);
        assert_eq!(parent_disk("sdb1"), None);
    }
}
