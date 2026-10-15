//! Phones, cameras and USB sticks: finding the ones that are plugged in, mounting them and
//! letting them go.
//!
//! Three sources, none of them linked in: the folders the desktop's virtual filesystem
//! (gvfs) keeps for the devices it has opened, `gio mount -li` for their names and for the
//! phones it has seen and not opened, and `lsblk` for the sticks that are not mounted.
//! Every program runs with the C locale (its words are read here), with a time limit (a
//! phone that does not answer must never hold the list), and a program that is not
//! installed is an empty answer, not an error.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::i18n::{Key, t, tf};
use crate::platform::volumes::{Unmounted, Volume, VolumeKind};
use crate::platform::which::{which, which_in};
use crate::{Error, Result};

/// The kinds of device the virtual filesystem names `scheme:host=…`, and that rada opens.
const SCHEMES: &[&str] = &["mtp", "gphoto2"];

/// What a listing may take before it is given up on.
const LIST_LIMIT: Duration = Duration::from_secs(4);
/// What mounting a phone may take (it waits for the phone to answer).
const MOUNT_LIMIT: Duration = Duration::from_secs(60);
const UNMOUNT_LIMIT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------- programs

/// What a program printed and whether it succeeded.
#[derive(Debug)]
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Both streams, for looking for a word in what the program complained about.
    pub fn text(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ToolError {
    Missing,
    TimedOut,
    Io(String),
}

/// Where the helper programs are looked for: the `PATH` of the process, or one that is given
/// (the tests put stand-ins in a folder of their own).
#[derive(Clone, Debug, Default)]
pub struct Tools {
    path: Option<OsString>,
}

impl Tools {
    pub fn system() -> Tools {
        Tools::default()
    }

    pub fn with_path(path: OsString) -> Tools {
        Tools { path: Some(path) }
    }

    pub fn find(&self, name: &str) -> Option<PathBuf> {
        match &self.path {
            Some(p) => which_in(p, name),
            None => which(name),
        }
    }

    /// Run `name args…` for at most `limit`. A program that does not finish is killed.
    pub fn run(
        &self,
        name: &str,
        args: &[&str],
        limit: Duration,
    ) -> std::result::Result<Output, ToolError> {
        let exe = self.find(name).ok_or(ToolError::Missing)?;
        let child = Command::new(exe)
            .args(args)
            // Their words are read here: the same words whatever the language of the person.
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env_remove("LANGUAGE")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::Io(e.to_string()))?;
        let pid = child.id();
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        match rx.recv_timeout(limit) {
            Ok(Ok(o)) => Ok(Output {
                ok: o.status.success(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            }),
            Ok(Err(e)) => Err(ToolError::Io(e.to_string())),
            Err(_) => {
                // SAFETY: `kill` on a process id this function started; the thread that waits
                // for it reaps it.
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
                Err(ToolError::TimedOut)
            }
        }
    }
}

// ---------------------------------------------------------------------- the gvfs folder

/// Where the virtual filesystem keeps the devices it has opened. A test build can point it
/// at a folder of its own with `RADA_TEST_GVFS_DIR`; an empty value means there is none.
pub fn gvfs_dir() -> Option<PathBuf> {
    #[cfg(feature = "testutil")]
    if let Some(dir) = std::env::var_os("RADA_TEST_GVFS_DIR") {
        return (!dir.is_empty()).then(|| PathBuf::from(dir));
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        // SAFETY: getuid has no preconditions and cannot fail.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    Some(runtime.join("gvfs"))
}

/// The scheme of a folder name like `mtp:host=Pixel_6_1A2B3C`, if it is a device rada opens.
fn device_scheme(folder: &str) -> Option<&'static str> {
    let (scheme, rest) = folder.split_once(':')?;
    if !rest.starts_with("host=") {
        return None;
    }
    SCHEMES.iter().copied().find(|s| *s == scheme)
}

/// The devices that are open: the folders of `dir` named like one. Only the names are read;
/// nothing is opened inside them.
fn open_devices(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            device_scheme(&name).map(|_| (name, e.path()))
        })
        .collect();
    out.sort();
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 3 <= b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `mtp://Pixel_6_1A2B3C/` → `mtp:host=Pixel_6_1A2B3C`: the name of its folder.
fn folder_name_of(uri: &str) -> Option<String> {
    let (scheme, rest) = uri.split_once("://")?;
    let host = percent_decode(rest.split('/').next()?);
    (!host.is_empty()).then(|| format!("{scheme}:host={host}"))
}

/// The name of a device from the name of its folder, for when `gio` is not there to say:
/// `mtp:host=Motorola_moto_g_8__ZY22ABCD12` → "Motorola moto g 8". The serial number the
/// virtual filesystem puts at the end is left out.
pub fn name_from_folder(folder: &str) -> String {
    let host = percent_decode(folder.split_once("host=").map_or(folder, |(_, h)| h));
    let mut words: Vec<&str> = host.split(['_', ' ']).filter(|w| !w.is_empty()).collect();
    let serial = |w: &str| {
        w.len() >= 8
            && w.chars().any(|c| c.is_ascii_digit())
            && w.chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
    };
    if words.len() > 1 && words.last().is_some_and(|w| serial(w)) {
        words.pop();
    }
    if words.is_empty() {
        host
    } else {
        words.join(" ")
    }
}

// --------------------------------------------------------------------------- gio output

#[derive(Debug, PartialEq, Eq)]
pub struct GioDevice {
    pub name: String,
    /// `mtp://host/`
    pub uri: String,
    pub mounted: bool,
}

fn is_device_uri(uri: &str) -> bool {
    SCHEMES
        .iter()
        .any(|s| uri.strip_prefix(s).is_some_and(|r| r.starts_with("://")))
}

fn same_uri(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// What `gio mount -li` printed, line by line, before anything is decided about it.
#[derive(Debug, Default)]
pub struct GioScan {
    /// `Mount(n): name -> uri`
    pub mounts: Vec<(String, String)>,
    /// `Volume(n): name`, with the `activation_root=` that followed it.
    pub volumes: Vec<(String, Option<String>)>,
}

pub fn scan_gio(text: &str) -> GioScan {
    let mut scan = GioScan::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Mount(")
            && let Some((_, rest)) = rest.split_once("): ")
            && let Some((name, uri)) = rest.rsplit_once(" -> ")
        {
            scan.mounts
                .push((name.trim().to_string(), uri.trim().to_string()));
        } else if let Some(rest) = line.strip_prefix("Volume(")
            && let Some((_, name)) = rest.split_once("): ")
        {
            scan.volumes.push((name.trim().to_string(), None));
        } else if let Some(root) = line.strip_prefix("activation_root=")
            && let Some(last) = scan.volumes.last_mut()
        {
            last.1 = Some(root.trim().to_string());
        }
    }
    scan
}

/// The phones and cameras in the output of `gio mount -li`: the mounted ones (a `Mount(n): name
/// -> uri` line) and the ones that are only plugged in (a `Volume(n): name` with an
/// `activation_root=uri`, and no mount of that uri).
pub fn parse_gio(text: &str) -> Vec<GioDevice> {
    let scan = scan_gio(text);
    let mut out: Vec<GioDevice> = Vec::new();
    for (name, uri) in &scan.mounts {
        if is_device_uri(uri) && !out.iter().any(|d| same_uri(&d.uri, uri)) {
            out.push(GioDevice {
                name: name.clone(),
                uri: uri.clone(),
                mounted: true,
            });
        }
    }
    for (name, root) in scan.volumes {
        let Some(root) = root.filter(|r| is_device_uri(r)) else {
            continue;
        };
        if !out.iter().any(|d| same_uri(&d.uri, &root)) {
            out.push(GioDevice {
                name,
                uri: root,
                mounted: false,
            });
        }
    }
    out
}

/// What `gio mount -li` says, or nothing when `gio` is missing, slow or fails. What is wrong is
/// written to the log (and `rada debug devices` says it again): a device is never left out
/// without a reason being kept.
fn gio_devices(tools: &Tools) -> Vec<GioDevice> {
    match tools.run("gio", &["mount", "-li"], LIST_LIMIT) {
        Ok(o) if o.ok => {
            let devices = parse_gio(&o.stdout);
            let scan = scan_gio(&o.stdout);
            if devices.is_empty()
                && !o.stdout.trim().is_empty()
                && scan.volumes.is_empty()
                && scan.mounts.is_empty()
            {
                tracing::warn!(
                    "gio mount -li printed something rada does not recognise: {}",
                    first_line(&o.stdout)
                );
            }
            devices
        }
        Ok(o) => {
            tracing::warn!("gio mount -li failed: {}", first_line(&o.text()));
            Vec::new()
        }
        Err(ToolError::Missing) => {
            tracing::debug!("gio is not installed: no phones or cameras are listed");
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("gio mount -li: {e:?}");
            Vec::new()
        }
    }
}

// ----------------------------------------------------------------------------- mounted

/// The phones and cameras that are open, as entries of the disk list. Nothing is read from
/// inside them: no free space (the answer can take long and means little), no scan.
pub fn open_device_volumes(dir: &Path, tools: &Tools) -> Vec<Volume> {
    let folders = open_devices(dir);
    if folders.is_empty() {
        return Vec::new();
    }
    // The name the device gives itself, by the folder its uri stands for.
    let names: HashMap<String, GioDevice> = gio_devices(tools)
        .into_iter()
        .filter_map(|d| folder_name_of(&d.uri).map(|f| (f, d)))
        .collect();
    folders
        .into_iter()
        .map(|(folder, path)| {
            let decoded = percent_decode(&folder);
            let known = names.get(&decoded);
            let scheme = device_scheme(&folder).unwrap_or("mtp");
            Volume {
                label: Some(
                    known
                        .map(|d| d.name.clone())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| name_from_folder(&folder)),
                ),
                fs_type: scheme.to_string(),
                device: known
                    .map(|d| d.uri.clone())
                    .unwrap_or_else(|| uri_of_folder(&decoded)),
                kind: VolumeKind::Device,
                drive_letter: None,
                total: None,
                available: None,
                read_only: false,
                responsive: true,
                mount_point: path,
            }
        })
        .collect()
}

/// `mtp:host=Pixel_6` → `mtp://Pixel_6/`
fn uri_of_folder(folder: &str) -> String {
    match folder.split_once(":host=") {
        Some((scheme, host)) => format!("{scheme}://{host}/"),
        None => folder.to_string(),
    }
}

// --------------------------------------------------------------------------- unmounted

/// The phones and cameras that are plugged in and not open.
pub fn unmounted_devices(tools: &Tools) -> Vec<Unmounted> {
    gio_devices(tools)
        .into_iter()
        .filter(|d| !d.mounted)
        .map(|d| Unmounted {
            name: if d.name.is_empty() {
                name_from_folder(&d.uri)
            } else {
                d.name
            },
            id: d.uri,
            kind: VolumeKind::Device,
            size: None,
        })
        .collect()
}

/// The filesystems a stick is offered for: the ones the desktop mounts without asking for
/// anything. Encrypted volumes, RAID and LVM members are not disks to open.
const MOUNTABLE: &[&str] = &[
    "vfat", "exfat", "ntfs", "ntfs3", "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "udf",
    "iso9660", "hfsplus", "jfs",
];

fn truthy(v: Option<&serde_json::Value>) -> bool {
    match v {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) != 0,
        Some(serde_json::Value::String(s)) => s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn text_of(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn size_of(v: Option<&serde_json::Value>) -> Option<u64> {
    match v? {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The columns asked of `lsblk`, in the form of its current versions; older ones call the
/// mount column `MOUNTPOINT`.
const LSBLK_COLUMNS: [&str; 2] = [
    "NAME,PATH,LABEL,FSTYPE,MOUNTPOINTS,HOTPLUG,TRAN,TYPE,SIZE,MODEL",
    "NAME,PATH,LABEL,FSTYPE,MOUNTPOINT,HOTPLUG,TRAN,TYPE,SIZE,MODEL",
];

/// The partitions (or whole disks without a partition table) of removable drives that hold a
/// filesystem and are not mounted, from `lsblk -J` output.
pub fn parse_lsblk(json: &str) -> Vec<Unmounted> {
    judge_all_lsblk(json).0
}

/// The sticks found in `lsblk -J` output, and what was said of every node, found or not.
#[allow(clippy::type_complexity)]
fn judge_all_lsblk(json: &str) -> (Vec<Unmounted>, Vec<(String, Result<(), String>)>) {
    let mut out = Vec::new();
    let mut judged = Vec::new();
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        judged.push(("(output)".into(), Err("not JSON".into())));
        return (out, judged);
    };
    match root.get("blockdevices").and_then(|v| v.as_array()) {
        Some(list) => {
            for node in list {
                walk_lsblk(node, false, &mut out, &mut judged);
            }
        }
        None => judged.push(("(output)".into(), Err("no \"blockdevices\" list".into()))),
    }
    (out, judged)
}

/// Is this node of `lsblk` a stick that can be mounted? Either it is (with its name), or why
/// not, in words.
fn judge_lsblk(
    node: &serde_json::Value,
    removable: bool,
    no_children: bool,
) -> Result<Unmounted, String> {
    let kind = text_of(node.get("type")).unwrap_or_default();
    let fstype = text_of(node.get("fstype"));
    let mounted = node
        .get("mountpoints")
        .and_then(|m| m.as_array())
        .is_some_and(|m| m.iter().any(|p| p.as_str().is_some_and(|s| !s.is_empty())))
        || text_of(node.get("mountpoint")).is_some();
    if !removable {
        return Err("not a removable drive (no hotplug flag, not on the USB bus)".into());
    }
    if kind != "part" && !(kind == "disk" && no_children) {
        return Err(format!(
            "type {kind:?}: neither a partition nor a disk without partitions"
        ));
    }
    match &fstype {
        None => return Err("no filesystem".into()),
        Some(f) if !MOUNTABLE.contains(&f.as_str()) => {
            return Err(format!("filesystem {f:?} is not one rada offers to mount"));
        }
        _ => {}
    }
    if mounted {
        return Err("already mounted".into());
    }
    let Some(path) = text_of(node.get("path")) else {
        return Err("no device path in the output (the PATH column is missing)".into());
    };
    let name = text_of(node.get("label"))
        .or_else(|| text_of(node.get("model")))
        .unwrap_or_else(|| path.clone());
    Ok(Unmounted {
        id: path,
        name,
        kind: VolumeKind::Removable,
        size: size_of(node.get("size")),
    })
}

fn walk_lsblk(
    node: &serde_json::Value,
    inherited: bool,
    out: &mut Vec<Unmounted>,
    judged: &mut Vec<(String, Result<(), String>)>,
) {
    let removable = inherited
        || truthy(node.get("hotplug"))
        || truthy(node.get("rm"))
        || text_of(node.get("tran")).is_some_and(|t| t == "usb");
    let children = node
        .get("children")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    let label = text_of(node.get("name")).unwrap_or_else(|| "?".into());
    match judge_lsblk(node, removable, children.is_empty()) {
        Ok(u) => {
            judged.push((label, Ok(())));
            out.push(u);
        }
        Err(why) => judged.push((label, Err(why))),
    }
    for c in &children {
        walk_lsblk(c, removable, out, judged);
    }
}

/// The sticks that are plugged in and not mounted. A failure is written to the log.
pub fn unmounted_disks(tools: &Tools) -> Vec<Unmounted> {
    for columns in LSBLK_COLUMNS {
        match tools.run("lsblk", &["-J", "-b", "-o", columns], LIST_LIMIT) {
            Ok(o) if o.ok => return parse_lsblk(&o.stdout),
            // The column list was refused by an older `lsblk`: try its spelling.
            Ok(o) => tracing::debug!("lsblk -o {columns}: {}", first_line(&o.text())),
            Err(ToolError::Missing) => {
                tracing::debug!("lsblk is not installed: no sticks are listed");
                break;
            }
            Err(e) => {
                tracing::warn!("lsblk: {e:?}");
                break;
            }
        }
    }
    Vec::new()
}

/// Everything that is plugged in and not mounted, sticks first, with no name twice listed
/// for the same thing.
pub fn unmounted(tools: &Tools) -> Vec<Unmounted> {
    let mut out = unmounted_disks(tools);
    let seen: HashSet<String> = out.iter().map(|u| u.id.clone()).collect();
    out.extend(
        unmounted_devices(tools)
            .into_iter()
            .filter(|u| !seen.contains(&u.id)),
    );
    out
}

// ------------------------------------------------------------------ mount and unmount

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Why `udisksctl mount` refused, in a sentence; `None` when it was already mounted (which
/// is what was asked for).
fn explain_udisks_mount(text: &str) -> Option<String> {
    let low = text.to_lowercase();
    if low.contains("alreadymounted") || low.contains("already mounted") {
        return None;
    }
    Some(
        if low.contains("notauthorized") || low.contains("not authorized") {
            t(Key::DevNotAuthorized).to_string()
        } else if low.contains("devicebusy") || low.contains("busy") {
            t(Key::DevBusy).to_string()
        } else {
            tf(Key::DevMountFailed, &[&first_line(text)])
        },
    )
}

/// Why `gio mount` refused; `None` when the device was already open.
fn explain_gio_mount(text: &str) -> Option<String> {
    let low = text.to_lowercase();
    if low.contains("already mounted") {
        return None;
    }
    Some(
        if low.contains("doesn't implement mount")
            || low.contains("not supported")
            || low.contains("no such interface")
            || low.contains("invalid mount spec")
        {
            t(Key::DevNoMtp).to_string()
        } else if low.contains("busy") || low.contains("claim") {
            t(Key::DevBusy).to_string()
        } else if low.contains("unable to open") || low.contains("no mtp") {
            t(Key::DevLocked).to_string()
        } else {
            tf(Key::DevMountFailed, &[&first_line(text)])
        },
    )
}

fn explain_gio_unmount(text: &str) -> String {
    let low = text.to_lowercase();
    if low.contains("busy") {
        t(Key::DevUnmountBusy).to_string()
    } else {
        tf(Key::DevUnmountFailed, &[&first_line(text)])
    }
}

fn tool_error(name: &str, e: ToolError) -> Error {
    Error::Invalid(match e {
        ToolError::Missing if name == "gio" => t(Key::DevMissingGio).to_string(),
        ToolError::Missing => t(Key::DevMissingUdisks).to_string(),
        ToolError::TimedOut => t(Key::DevTimedOut).to_string(),
        ToolError::Io(e) => tf(Key::DevMountFailed, &[&e]),
    })
}

pub fn mount(tools: &Tools, device: &Unmounted) -> Result<()> {
    if device.id.starts_with("/dev/") {
        let out = tools
            .run(
                "udisksctl",
                &["mount", "-b", &device.id, "--no-user-interaction"],
                MOUNT_LIMIT,
            )
            .map_err(|e| tool_error("udisksctl", e))?;
        return match (out.ok, explain_udisks_mount(&out.text())) {
            (true, _) | (_, None) => Ok(()),
            (false, Some(why)) => Err(Error::Invalid(why)),
        };
    }
    let out = tools
        .run("gio", &["mount", &device.id], MOUNT_LIMIT)
        .map_err(|e| tool_error("gio", e))?;
    match (out.ok, explain_gio_mount(&out.text())) {
        (true, _) | (_, None) => Ok(()),
        (false, Some(why)) => Err(Error::Invalid(why)),
    }
}

/// Let go of an open phone or camera.
pub fn unmount_device(tools: &Tools, uri: &str) -> Result<()> {
    let out = tools
        .run("gio", &["mount", "-u", uri], UNMOUNT_LIMIT)
        .map_err(|e| tool_error("gio", e))?;
    if out.ok {
        Ok(())
    } else {
        Err(Error::Invalid(explain_gio_unmount(&out.text())))
    }
}

// ---------------------------------------------------------------- removable by sysfs

/// Is the block device `/dev/sdb1` on a drive that is removable or on the USB bus? Read from
/// `sys` (`/sys`): the drive's `removable` flag, and a `usb` in where the drive is plugged.
/// A stick mounted by hand somewhere unusual is still a stick.
pub fn removable_drive(sys: &Path, device: &str) -> bool {
    let Some(name) = device.strip_prefix("/dev/").filter(|n| !n.contains('/')) else {
        return false;
    };
    let Ok(real) = std::fs::canonicalize(sys.join("class/block").join(name)) else {
        return false;
    };
    // A partition is a folder inside its drive's.
    let disk = if real.join("partition").exists() {
        real.parent().map(Path::to_path_buf)
    } else {
        Some(real.clone())
    };
    let flag = disk
        .and_then(|d| std::fs::read_to_string(d.join("removable")).ok())
        .is_some_and(|s| s.trim() == "1");
    flag || real
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with("usb"))
}

// ----------------------------------------------------------------------------- diagnosis

fn tool_state(tools: &Tools, name: &str) -> String {
    match tools.find(name) {
        Some(p) => format!("{name}: {}", p.display()),
        None => format!("{name}: NOT FOUND in PATH"),
    }
}

/// What the device code sees, line by line, for `rada debug devices`: the folder of the
/// virtual filesystem and every entry in it, what `gio` and `lsblk` print and what became of
/// each thing in it, and the final lists. Nothing is mounted, unmounted or written.
pub fn diagnose(gvfs: Option<&Path>, tools: &Tools, sys: &Path) -> Vec<String> {
    let mut out = Vec::new();
    out.push("[tools]".to_string());
    for t in ["gio", "lsblk", "udisksctl"] {
        out.push(format!("  {}", tool_state(tools, t)));
    }

    out.push("[gvfs folder]".to_string());
    match gvfs {
        None => out.push("  none (RADA_TEST_GVFS_DIR is empty)".into()),
        Some(dir) => match std::fs::read_dir(dir) {
            Err(e) => out.push(format!("  {}: cannot be read: {e}", dir.display())),
            Ok(rd) => {
                let mut names: Vec<String> = rd
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                out.push(format!("  {}: {} entries", dir.display(), names.len()));
                for n in &names {
                    out.push(match device_scheme(n) {
                        Some(s) => format!("  entry {n:?} -> device ({s})"),
                        None => {
                            format!("  entry {n:?} -> dropped: not a {SCHEMES:?} device folder")
                        }
                    });
                }
            }
        },
    }

    out.push("[gio mount -li]".to_string());
    let gio = match tools.run("gio", &["mount", "-li"], LIST_LIMIT) {
        Err(e) => {
            out.push(format!("  could not run: {e:?}"));
            None
        }
        Ok(o) => {
            out.push(format!(
                "  exit {}, {} lines of output, {} of errors",
                if o.ok { "ok" } else { "FAILED" },
                o.stdout.lines().count(),
                o.stderr.lines().count()
            ));
            for l in o.stderr.lines().take(5) {
                out.push(format!("  stderr: {l}"));
            }
            o.ok.then_some(o.stdout)
        }
    };
    if let Some(text) = &gio {
        let scan = scan_gio(text);
        out.push(format!(
            "  read {} Mount lines and {} Volume lines",
            scan.mounts.len(),
            scan.volumes.len()
        ));
        for (name, uri) in &scan.mounts {
            out.push(if is_device_uri(uri) {
                format!("  Mount {name:?} -> {uri}: device (open)")
            } else {
                format!("  Mount {name:?} -> {uri}: dropped: not an mtp:// or gphoto2:// address")
            });
        }
        for (name, root) in &scan.volumes {
            out.push(match root {
                None => format!("  Volume {name:?}: dropped: no activation_root line"),
                Some(r) if !is_device_uri(r) => {
                    format!(
                        "  Volume {name:?} root {r}: dropped: not an mtp:// or gphoto2:// address"
                    )
                }
                Some(r) if scan.mounts.iter().any(|(_, u)| same_uri(u, r)) => {
                    format!("  Volume {name:?} root {r}: already open (listed as a Mount)")
                }
                Some(r) => format!("  Volume {name:?} root {r}: device (plugged in, not open)"),
            });
        }
        if scan.mounts.is_empty() && scan.volumes.is_empty() && !text.trim().is_empty() {
            out.push(
                "  UNEXPECTED FORMAT: output has lines but no Mount( or Volume( line; first line:"
                    .into(),
            );
            out.push(format!("    {}", first_line(text)));
        }
    }

    out.push("[lsblk]".to_string());
    let mut shown = false;
    for columns in LSBLK_COLUMNS {
        match tools.run("lsblk", &["-J", "-b", "-o", columns], LIST_LIMIT) {
            Err(e) => {
                out.push(format!("  could not run: {e:?}"));
                shown = true;
                break;
            }
            Ok(o) if !o.ok => {
                out.push(format!("  -o {columns}: FAILED: {}", first_line(&o.text())))
            }
            Ok(o) => {
                let (_, judged) = judge_all_lsblk(&o.stdout);
                out.push(format!("  -o {columns}: ok, {} nodes", judged.len()));
                for (name, verdict) in judged {
                    out.push(match verdict {
                        Ok(()) => {
                            format!("  {name}: OFFERED (removable, not mounted, has a filesystem)")
                        }
                        Err(why) => format!("  {name}: dropped: {why}"),
                    });
                }
                shown = true;
                break;
            }
        }
    }
    if !shown {
        out.push("  no column list was accepted".into());
    }

    out.push("[result]".to_string());
    if let Some(dir) = gvfs {
        let open = open_device_volumes(dir, tools);
        out.push(format!("  open devices: {}", open.len()));
        for v in &open {
            out.push(format!(
                "    {:?} at {} ({})",
                v.label.as_deref().unwrap_or(""),
                v.mount_point.display(),
                v.device
            ));
        }
    }
    let pending = unmounted(tools);
    out.push(format!("  plugged in and not mounted: {}", pending.len()));
    for u in &pending {
        out.push(format!("    {:?} id {} ({:?})", u.name, u.id, u.kind));
    }
    let _ = sys;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `gio mount -li` prints, written from its documented format: a phone that is open,
    /// a second one that is only plugged in, a stick the volume monitor of the disks knows,
    /// and a network share.
    const GIO: &str = "\
Drive(0): KBG40ZNS256G BG4A KIOXIA
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/nvme0n1'
Volume(0): Motorola moto g(8) power
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  ids:
   unix-device: '/dev/bus/usb/001/013'
   class: 'device'
  themed icons:  [phone]  [phone-symbolic]
  can_mount=1
  can_eject=0
  should_automount=1
  activation_root=mtp://Motorola_moto_g_8__power_ZY22ABCD12/
  Mount(0): Motorola moto g(8) power -> mtp://Motorola_moto_g_8__power_ZY22ABCD12/
    Type: GDaemonMount
    default_location=mtp://Motorola_moto_g_8__power_ZY22ABCD12/
    can_unmount=1
    can_eject=0
Volume(1): Canon EOS
  Type: GProxyVolume (GProxyVolumeMonitorGPhoto2)
  can_mount=1
  activation_root=gphoto2://Canon_Inc._Canon_Digital_Camera/
Volume(2): STICK
  Type: GProxyVolume (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/sdb1'
  can_mount=1
  activation_root=file:///run/media/ale/STICK
Mount(1): files on nas -> smb://nas/files/
  Type: GDaemonMount
";

    /// What `gio mount -li` printed on a real machine with a phone plugged in and not opened
    /// (the serial number is made up): the internal drive first, then the phone as a volume.
    const GIO_REAL_UNMOUNTED: &str = "\
Drive(0): KBG40ZNS256G BG4A KIOXIA
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/nvme0n1'
  themed icons:  [drive-harddisk-solidstate]  [drive-harddisk]  [drive]  [drive-harddisk-solidstate-symbolic]  [drive-harddisk-symbolic]  [drive-symbolic]
  symbolic themed icons:  [drive-harddisk-solidstate-symbolic]  [drive-harddisk-symbolic]  [drive-symbolic]  [drive-harddisk-solidstate]  [drive-harddisk]  [drive]
  is_removable=0
  is_media_removable=0
  has_media=1
  is_media_check_automatic=1
  can_poll_for_media=0
  can_eject=0
  can_start=0
  can_stop=0
  start_stop_type=shutdown
  sort_key=00coldplug/00fixed/nvme0
Volume(0): motorola edge 40
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  ids:
   unix-device: '/dev/bus/usb/003/030'
  activation_root=mtp://motorola_motorola_edge_40_ZY22FAKE0001/
  themed icons:  [phone]
  symbolic themed icons:  [phone-symbolic]  [phone]
  can_mount=1
  can_eject=0
  should_automount=1
";

    /// The same phone after the desktop has opened it (Nautilus, say), as gio prints it: the
    /// volume with its mount below. Written from the documented format, as the first
    /// fixture was; the real one has to be checked on a phone that is open.
    const GIO_MOUNTED_BY_THE_DESKTOP: &str = "\
Volume(0): motorola edge 40
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  ids:
   unix-device: '/dev/bus/usb/003/030'
  activation_root=mtp://motorola_motorola_edge_40_ZY22FAKE0001/
  can_mount=1
  can_eject=0
  should_automount=1
  Mount(0): motorola edge 40 -> mtp://motorola_motorola_edge_40_ZY22FAKE0001/
    Type: GDaemonMount
    default_location=mtp://motorola_motorola_edge_40_ZY22FAKE0001/
    themed icons:  [phone]
    can_unmount=1
    can_eject=0
    is_shadowed=0
";

    #[test]
    fn the_phone_that_a_real_gio_printed_is_found_as_plugged_in_and_not_open() {
        let d = parse_gio(GIO_REAL_UNMOUNTED);
        assert_eq!(
            d,
            [GioDevice {
                name: "motorola edge 40".into(),
                uri: "mtp://motorola_motorola_edge_40_ZY22FAKE0001/".into(),
                mounted: false
            }],
            "the internal drive is not a phone; the volume is"
        );
    }

    #[test]
    fn a_phone_the_desktop_has_opened_is_found_as_open_and_only_once() {
        let d = parse_gio(GIO_MOUNTED_BY_THE_DESKTOP);
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].mounted);
        assert_eq!(d[0].name, "motorola edge 40");
        // Its folder in the virtual filesystem gets that name.
        let sb = Sandbox::new();
        let bin = Bin::new(&sb);
        bin.gio(GIO_MOUNTED_BY_THE_DESKTOP, "exit 0");
        let gvfs = sb.mkdir("gvfs");
        std::fs::create_dir_all(gvfs.join("mtp:host=motorola_motorola_edge_40_ZY22FAKE0001"))
            .unwrap();
        let v = open_device_volumes(&gvfs, &bin.tools());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].label.as_deref(), Some("motorola edge 40"));
        assert_eq!(v[0].device, "mtp://motorola_motorola_edge_40_ZY22FAKE0001/");
        // And it is not offered a second time as "not mounted".
        assert!(unmounted_devices(&bin.tools()).is_empty());
    }

    #[test]
    fn the_diagnosis_says_what_became_of_everything_and_what_went_wrong() {
        let sb = Sandbox::new();
        let bin = Bin::new(&sb);
        bin.gio(GIO_REAL_UNMOUNTED, "exit 0");
        let json = sb.root.join("lsblk.json");
        std::fs::write(&json, LSBLK).unwrap();
        bin.tool("lsblk", &format!("cat '{}'", json.display()));
        let gvfs = sb.mkdir("gvfs");
        std::fs::create_dir_all(gvfs.join("smb-share:server=nas,share=x")).unwrap();
        let text = diagnose(Some(&gvfs), &bin.tools(), &sb.path("sys")).join("\n");
        for want in [
            "gio: ",
            "udisksctl: NOT FOUND",
            "entry \"smb-share:server=nas,share=x\" -> dropped",
            "Volume \"motorola edge 40\" root mtp://motorola_motorola_edge_40_ZY22FAKE0001/: device (plugged in, not open)",
            "sdb3: dropped: already mounted",
            "nvme0n1p2: dropped: not a removable drive",
            "sdd: dropped: not a removable drive",
            "sdb1: OFFERED",
            "sdb2: dropped: filesystem \"crypto_LUKS\"",
            "plugged in and not mounted: 3",
        ] {
            assert!(text.contains(want), "{want:?} missing from:\n{text}");
        }

        // A command that fails, and one that prints something nobody expected, are said.
        bin.gio("", "exit 0");
        bin.tool(
            "gio",
            "echo 'Error: the volume monitor is not running' >&2; exit 1",
        );
        let text = diagnose(Some(&gvfs), &bin.tools(), &sb.path("sys")).join("\n");
        assert!(
            text.contains("exit FAILED") && text.contains("volume monitor is not running"),
            "{text}"
        );
        bin.tool("gio", "echo 'something else entirely'");
        let text = diagnose(Some(&gvfs), &bin.tools(), &sb.path("sys")).join("\n");
        assert!(text.contains("UNEXPECTED FORMAT"), "{text}");
        let none = Tools::with_path(sb.mkdir("empty-bin").into_os_string());
        let text = diagnose(None, &none, &sb.path("sys")).join("\n");
        assert!(
            text.contains("gio: NOT FOUND") && text.contains("could not run: Missing"),
            "{text}"
        );
    }

    #[test]
    fn gio_output_gives_the_open_and_the_plugged_in_devices_and_nothing_else() {
        let d = parse_gio(GIO);
        assert_eq!(
            d,
            [
                GioDevice {
                    name: "Motorola moto g(8) power".into(),
                    uri: "mtp://Motorola_moto_g_8__power_ZY22ABCD12/".into(),
                    mounted: true
                },
                GioDevice {
                    name: "Canon EOS".into(),
                    uri: "gphoto2://Canon_Inc._Canon_Digital_Camera/".into(),
                    mounted: false
                },
            ],
            "the stick is the disk tool's, the share is not a device, a volume that is open is listed once"
        );
        assert!(parse_gio("").is_empty());
        assert!(parse_gio("garbage\n  activation_root=mtp://x/\n").is_empty());
    }

    #[test]
    fn a_name_is_read_from_a_folder_when_gio_cannot_give_it() {
        assert_eq!(
            name_from_folder("mtp:host=Motorola_moto_g_8__ZY22ABCD12"),
            "Motorola moto g 8"
        );
        assert_eq!(name_from_folder("mtp:host=Pixel_6"), "Pixel 6");
        assert_eq!(name_from_folder("gphoto2:host=Canon_EOS_R"), "Canon EOS R");
        assert_eq!(
            name_from_folder("mtp:host=My%20Phone_1A2B3C4D5E"),
            "My Phone"
        );
        // A name that is all serial stays as it is.
        assert_eq!(name_from_folder("mtp:host=ABCD1234"), "ABCD1234");
    }

    #[test]
    fn folders_and_uris_name_each_other() {
        assert_eq!(
            folder_name_of("mtp://Pixel_6_1A/").as_deref(),
            Some("mtp:host=Pixel_6_1A")
        );
        assert_eq!(
            folder_name_of("mtp://My%20Phone/").as_deref(),
            Some("mtp:host=My Phone")
        );
        assert_eq!(folder_name_of("not a uri"), None);
        assert_eq!(uri_of_folder("mtp:host=Pixel_6"), "mtp://Pixel_6/");
        assert_eq!(device_scheme("mtp:host=x"), Some("mtp"));
        assert_eq!(device_scheme("gphoto2:host=x"), Some("gphoto2"));
        assert_eq!(device_scheme("smb-share:server=nas,share=files"), None);
        assert_eq!(device_scheme("mtp"), None);
    }

    const LSBLK: &str = r#"{"blockdevices":[
      {"name":"nvme0n1","path":"/dev/nvme0n1","label":null,"fstype":null,"mountpoints":[null],"hotplug":false,"tran":"nvme","type":"disk","size":256060514304,"model":"KBG40ZNS256G",
       "children":[
         {"name":"nvme0n1p1","path":"/dev/nvme0n1p1","label":null,"fstype":"vfat","mountpoints":["/boot/efi"],"hotplug":false,"tran":null,"type":"part","size":536870912,"model":null},
         {"name":"nvme0n1p2","path":"/dev/nvme0n1p2","label":null,"fstype":"ext4","mountpoints":["/"],"hotplug":false,"tran":null,"type":"part","size":1000,"model":null}]},
      {"name":"sdb","path":"/dev/sdb","label":null,"fstype":null,"mountpoints":[null],"hotplug":true,"tran":"usb","type":"disk","size":15931539456,"model":"Cruzer Blade",
       "children":[
         {"name":"sdb1","path":"/dev/sdb1","label":"BLUE STICK","fstype":"vfat","mountpoints":[null],"hotplug":true,"tran":null,"type":"part","size":15930000000,"model":null},
         {"name":"sdb2","path":"/dev/sdb2","label":null,"fstype":"crypto_LUKS","mountpoints":[null],"hotplug":true,"tran":null,"type":"part","size":1000,"model":null},
         {"name":"sdb3","path":"/dev/sdb3","label":"IN USE","fstype":"ext4","mountpoints":["/run/media/ale/IN USE"],"hotplug":true,"tran":null,"type":"part","size":1000,"model":null}]},
      {"name":"sdc","path":"/dev/sdc","label":null,"fstype":"exfat","mountpoints":[null],"hotplug":"1","tran":"usb","type":"disk","size":"64000000000","model":"Card Reader"},
      {"name":"sdd","path":"/dev/sdd","label":"INTERNAL","fstype":"ext4","mountpoints":[null],"hotplug":false,"tran":"sata","type":"disk","size":1000,"model":"Data disk"},
      {"name":"loop0","path":"/dev/loop0","label":null,"fstype":"squashfs","mountpoints":["/snap/x"],"hotplug":false,"tran":null,"type":"loop","size":1000,"model":null}
    ]}"#;

    #[test]
    fn only_removable_unmounted_disks_with_a_filesystem_are_offered() {
        let d = parse_lsblk(LSBLK);
        let ids: Vec<&str> = d.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(
            ids,
            ["/dev/sdb1", "/dev/sdc"],
            "not the internal disks, the boot partition, the encrypted or the mounted partition, the loop device"
        );
        assert_eq!(d[0].name, "BLUE STICK");
        assert_eq!(d[0].size, Some(15930000000));
        assert_eq!(d[0].kind, VolumeKind::Removable);
        // No label: the model of the drive stands in; flags and sizes written as text work.
        assert_eq!(d[1].name, "Card Reader");
        assert_eq!(d[1].size, Some(64_000_000_000));
        assert!(parse_lsblk("not json").is_empty());
        assert!(parse_lsblk("{}").is_empty());
        // The spelling of older versions: one mount point, not a list.
        let old = r#"{"blockdevices":[{"name":"sdb1","path":"/dev/sdb1","label":"X","fstype":"vfat","mountpoint":null,"hotplug":"1","tran":"usb","type":"part","size":"10"}]}"#;
        assert_eq!(parse_lsblk(old).len(), 1);
        let mounted = old.replace("\"mountpoint\":null", "\"mountpoint\":\"/mnt/x\"");
        assert!(parse_lsblk(&mounted).is_empty());
    }

    #[test]
    fn a_tool_that_asked_for_the_wrong_thing_is_explained() {
        assert!(explain_udisks_mount("Error: AlreadyMounted").is_none());
        assert!(
            explain_udisks_mount(
                "GDBus.Error:org.freedesktop.UDisks2.Error.NotAuthorizedCanObtain"
            )
            .unwrap()
            .contains("password")
        );
        assert!(
            explain_udisks_mount("target is busy")
                .unwrap()
                .contains("another program")
        );
        assert!(
            explain_udisks_mount("Error mounting /dev/sdb1: wrong fs type")
                .unwrap()
                .contains("wrong fs type")
        );
        assert!(explain_gio_mount("Error mounting location: already mounted").is_none());
        assert!(
            explain_gio_mount("gio: mtp://x/: volume doesn't implement mount")
                .unwrap()
                .contains("gvfs-mtp")
        );
        assert!(
            explain_gio_mount("Error mounting location: Unable to open MTP device 'x'")
                .unwrap()
                .contains("unlock")
        );
        assert!(
            explain_gio_mount(
                "Error mounting location: Could not claim interface 0: Device or resource busy"
            )
            .unwrap()
            .contains("another program")
        );
        assert!(explain_gio_unmount("Error unmounting mount: busy").contains("busy"));
        assert!(explain_gio_unmount("something else\nmore").ends_with("something else"));
    }

    // ------------------------------------------------- with stand-in programs and folders

    use crate::testutil::Sandbox;

    /// A folder of stand-in programs: each is a script that adds its arguments to `calls` and
    /// then does `body`.
    struct Bin {
        dir: PathBuf,
        calls: PathBuf,
    }

    impl Bin {
        fn new(sb: &Sandbox) -> Bin {
            let dir = sb.root.join("device-tools");
            std::fs::create_dir_all(&dir).unwrap();
            Bin {
                dir,
                calls: sb.root.join("device-calls.log"),
            }
        }

        fn tool(&self, name: &str, body: &str) {
            use std::os::unix::fs::PermissionsExt;
            let exe = self.dir.join(name);
            std::fs::write(
                &exe,
                format!(
                    "#!/bin/sh\necho \"{name} $*\" >> '{}'\n{body}\n",
                    self.calls.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        /// `gio` that answers `mount -li` with `listing` and everything else with `rest`.
        fn gio(&self, listing: &str, rest: &str) {
            let file = self.dir.join("gio-listing.txt");
            std::fs::write(&file, listing).unwrap();
            self.tool(
                "gio",
                &format!(
                    "if [ \"$1 $2\" = \"mount -li\" ]; then cat '{}'; exit 0; fi\n{rest}",
                    file.display()
                ),
            );
        }

        fn tools(&self) -> Tools {
            Tools::with_path(self.dir.clone().into_os_string())
        }

        fn calls(&self) -> Vec<String> {
            std::fs::read_to_string(&self.calls)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    #[test]
    fn an_open_phone_is_listed_with_its_own_name_and_nothing_is_read_inside_it() {
        let sb = Sandbox::new();
        let bin = Bin::new(&sb);
        bin.gio(GIO, "exit 0");
        let gvfs = sb.mkdir("gvfs");
        for folder in [
            "mtp:host=Motorola_moto_g_8__power_ZY22ABCD12",
            "gphoto2:host=Canon_Inc._Canon_Digital_Camera",
            // Not devices: a share, a stray file and a folder of another kind.
            "smb-share:server=nas,share=files",
            "sftp:host=example.org",
            "something",
        ] {
            std::fs::create_dir_all(gvfs.join(folder)).unwrap();
        }
        // Whatever is inside the phone is not touched: a folder that cannot be read stays quiet.
        std::fs::create_dir_all(gvfs.join("mtp:host=Motorola_moto_g_8__power_ZY22ABCD12/Internal"))
            .unwrap();

        let v = open_device_volumes(&gvfs, &bin.tools());
        let shown: Vec<(String, VolumeKind, String)> = v
            .iter()
            .map(|v| (v.label.clone().unwrap(), v.kind, v.fs_type.clone()))
            .collect();
        assert_eq!(
            shown,
            [
                ("Canon EOS".into(), VolumeKind::Device, "gphoto2".into()),
                (
                    "Motorola moto g(8) power".into(),
                    VolumeKind::Device,
                    "mtp".into()
                ),
            ],
            "the share, the link and the stray folder are out; both have the name gio gave them"
        );
        let phone = &v[1];
        assert_eq!(phone.device, "mtp://Motorola_moto_g_8__power_ZY22ABCD12/");
        assert_eq!(
            phone.mount_point,
            gvfs.join("mtp:host=Motorola_moto_g_8__power_ZY22ABCD12")
        );
        assert_eq!((phone.total, phone.available), (None, None));
        assert!(phone.responsive && !phone.read_only);
        assert_eq!(
            bin.calls(),
            ["gio mount -li"],
            "one question to gio for all the devices"
        );
    }

    #[test]
    fn without_gio_a_phone_is_still_listed_with_a_name_from_its_folder() {
        let sb = Sandbox::new();
        let gvfs = sb.mkdir("gvfs");
        std::fs::create_dir_all(gvfs.join("mtp:host=Pixel_6_1A2B3C4D")).unwrap();
        let none = Tools::with_path(sb.mkdir("empty-bin").into_os_string());
        let v = open_device_volumes(&gvfs, &none);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].label.as_deref(), Some("Pixel 6"));
        assert_eq!(v[0].device, "mtp://Pixel_6_1A2B3C4D/");
        // No folder for the devices at all: an empty list, not an error.
        assert!(open_device_volumes(&sb.path("nowhere"), &none).is_empty());
    }

    #[test]
    fn the_unmounted_list_joins_sticks_and_phones_and_a_missing_tool_is_an_empty_list() {
        let sb = Sandbox::new();
        let bin = Bin::new(&sb);
        bin.gio(GIO, "exit 0");
        let json = sb.root.join("lsblk.json");
        std::fs::write(&json, LSBLK).unwrap();
        bin.tool("lsblk", &format!("cat '{}'", json.display()));
        let all = unmounted(&bin.tools());
        let ids: Vec<&str> = all.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "/dev/sdb1",
                "/dev/sdc",
                "gphoto2://Canon_Inc._Canon_Digital_Camera/"
            ],
            "sticks first, then the camera that is plugged in; the phone that is open is not here"
        );
        assert_eq!(all[2].kind, VolumeKind::Device);
        assert_eq!(all[2].name, "Canon EOS");

        let none = Tools::with_path(sb.mkdir("empty-bin").into_os_string());
        assert!(unmounted(&none).is_empty());
        // An `lsblk` that does not know the column list is asked again the way old ones spell it.
        let old = Bin {
            dir: sb.mkdir("old-bin"),
            calls: sb.root.join("old-calls.log"),
        };
        std::fs::write(old.dir.join("old.json"), LSBLK).unwrap();
        old.tool(
            "lsblk",
            &format!(
                "case \"$*\" in *MOUNTPOINTS*) exit 1;; esac\ncat '{}'",
                old.dir.join("old.json").display()
            ),
        );
        assert!(!unmounted_disks(&old.tools()).is_empty());
    }

    #[test]
    fn mounting_a_stick_and_a_phone_asks_the_right_program() {
        let sb = Sandbox::new();
        let bin = Bin::new(&sb);
        bin.tool(
            "udisksctl",
            "echo 'Mounted /dev/sdb1 at /run/media/ale/STICK.'",
        );
        bin.gio(GIO, "exit 0");
        let stick = Unmounted {
            id: "/dev/sdb1".into(),
            name: "STICK".into(),
            kind: VolumeKind::Removable,
            size: None,
        };
        let phone = Unmounted {
            id: "mtp://Pixel_6/".into(),
            name: "Pixel 6".into(),
            kind: VolumeKind::Device,
            size: None,
        };
        mount(&bin.tools(), &stick).unwrap();
        mount(&bin.tools(), &phone).unwrap();
        assert_eq!(
            bin.calls(),
            [
                "udisksctl mount -b /dev/sdb1 --no-user-interaction",
                "gio mount mtp://Pixel_6/"
            ]
        );
        unmount_device(&bin.tools(), "mtp://Pixel_6/").unwrap();
        assert_eq!(bin.calls().last().unwrap(), "gio mount -u mtp://Pixel_6/");
    }

    #[test]
    fn a_missing_tool_is_a_sentence_about_what_to_install_not_a_crash() {
        let sb = Sandbox::new();
        let none = Tools::with_path(sb.mkdir("empty-bin").into_os_string());
        let phone = Unmounted {
            id: "mtp://Pixel_6/".into(),
            name: "Pixel 6".into(),
            kind: VolumeKind::Device,
            size: None,
        };
        let e = mount(&none, &phone).unwrap_err().to_string();
        assert!(
            e.contains("gio was not found") && e.contains("gvfs-mtp"),
            "{e}"
        );
        let stick = Unmounted {
            id: "/dev/sdb1".into(),
            ..phone.clone()
        };
        let e = mount(&none, &stick).unwrap_err().to_string();
        assert!(e.contains("udisks2"), "{e}");
        let e = unmount_device(&none, "mtp://Pixel_6/")
            .unwrap_err()
            .to_string();
        assert!(e.contains("gvfs"), "{e}");
    }

    #[test]
    fn a_refusal_is_explained_and_a_device_that_is_already_open_is_not_an_error() {
        let sb = Sandbox::new();
        let bin = Bin::new(&sb);
        let phone = Unmounted {
            id: "mtp://Pixel_6/".into(),
            name: "Pixel 6".into(),
            kind: VolumeKind::Device,
            size: None,
        };
        bin.gio(
            "",
            "echo 'Error mounting location: Unable to open MTP device' >&2; exit 2",
        );
        let e = mount(&bin.tools(), &phone).unwrap_err().to_string();
        assert!(e.contains("unlock"), "{e}");
        bin.gio(
            "",
            "echo 'Error mounting location: Location is already mounted' >&2; exit 2",
        );
        mount(&bin.tools(), &phone).unwrap();
        bin.gio(
            "",
            "echo 'Error unmounting mount: target is busy' >&2; exit 1",
        );
        let e = unmount_device(&bin.tools(), &phone.id)
            .unwrap_err()
            .to_string();
        assert!(e.contains("busy"), "{e}");
    }

    #[test]
    fn a_program_that_never_answers_is_stopped() {
        let sb = Sandbox::new();
        let bin = Bin::new(&sb);
        bin.tool("slow", "exec sleep 20");
        let started = std::time::Instant::now();
        let r = bin.tools().run("slow", &[], Duration::from_millis(300));
        assert_eq!(r.unwrap_err(), ToolError::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            bin.tools()
                .run("absent", &[], Duration::from_secs(1))
                .unwrap_err(),
            ToolError::Missing
        );
    }

    #[test]
    fn a_drive_on_the_usb_bus_or_flagged_removable_is_removable() {
        let d = tempfile::tempdir().unwrap();
        let sys = d.path();
        let make = |bus: &str, disk: &str, part: Option<&str>, removable: &str| {
            let disk_dir = sys.join("devices/pci0").join(bus).join("block").join(disk);
            std::fs::create_dir_all(&disk_dir).unwrap();
            std::fs::write(disk_dir.join("removable"), removable).unwrap();
            std::fs::create_dir_all(sys.join("class/block")).unwrap();
            let mut name = disk.to_string();
            let mut target = disk_dir.clone();
            if let Some(p) = part {
                target = disk_dir.join(p);
                std::fs::create_dir_all(&target).unwrap();
                std::fs::write(target.join("partition"), "1").unwrap();
                name = p.to_string();
            }
            std::os::unix::fs::symlink(&target, sys.join("class/block").join(name)).unwrap();
        };
        make("usb1/1-1", "sdb", Some("sdb1"), "0\n");
        make("ata1", "sda", Some("sda1"), "0\n");
        make("ata2", "sdc", Some("sdc1"), "1\n");
        make("ata3", "sdd", None, "0\n");
        assert!(removable_drive(sys, "/dev/sdb1"), "on the usb bus");
        assert!(!removable_drive(sys, "/dev/sda1"), "an internal disk");
        assert!(removable_drive(sys, "/dev/sdc1"), "flagged removable");
        assert!(!removable_drive(sys, "/dev/sdd"), "a whole internal disk");
        assert!(!removable_drive(sys, "/dev/nope1"));
        assert!(!removable_drive(sys, "/dev/mapper/x"));
        assert!(!removable_drive(sys, "tmpfs"));
    }
}
