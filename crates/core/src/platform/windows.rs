//! Windows.
//!
//! The *model* is complete so the rest of the core never needs an `if windows`:
//! drive letters (`Volume::drive_letter`), NTFS junctions and reparse points
//! (`ReparseKind`), hidden/system attributes and case-insensitive
//! names (`PathRules`). The operations that need the Win32 API
//! (the Recycle Bin, `GetLogicalDrives` for volumes, `ShellExecute` for opening, Known
//! Folders) are real on Windows and return [`Error::Unsupported`] elsewhere, so the pure
//! parts (drive names, drive types, attributes) are tested on every system.

use std::ffi::OsStr;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use super::attrs::{Attrs, FileAttributes, ReparseKind};
use super::trash::{TrashBackend, TrashedItem};
use super::volumes::{Volume, VolumeKind, VolumeLister};
use super::{Dirs, Opener, PathRules, Platform, UserDirs};
use crate::fs::FsMeta;
use crate::{Error, Result};

const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub struct WindowsAttributes;

impl FileAttributes for WindowsAttributes {
    fn attrs(&self, _name: &OsStr, meta: &FsMeta) -> Attrs {
        let raw = meta.os_attrs.unwrap_or(0);
        let reparse = if raw & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            // Telling a symlink from a junction needs the reparse tag (DeviceIoControl);
            // until that is implemented, a directory reparse point that is not a
            // symlink is the junction case.
            Some(if meta.is_symlink() {
                ReparseKind::Symlink
            } else if meta.is_dir() {
                ReparseKind::Junction
            } else {
                ReparseKind::Other
            })
        } else {
            None
        };
        Attrs {
            hidden: raw & FILE_ATTRIBUTE_HIDDEN != 0,
            system: raw & FILE_ATTRIBUTE_SYSTEM != 0,
            readonly: raw & FILE_ATTRIBUTE_READONLY != 0 || meta.readonly,
            executable: false,
            reparse,
        }
    }
}

// ----------------------------------------------------------------------------- the Recycle Bin

/// The Recycle Bin of the current user, through the shell (the `trash` crate): moving an item
/// to it, and putting it back from it. An item is remembered by the identifier the bin gives
/// it; the folder it came from is in the journal.
struct WindowsTrash;

#[cfg(windows)]
mod native_trash {
    use super::*;
    use crate::platform::trash::TrashHandle;

    fn fail(op: &'static str, path: &Path, e: impl std::fmt::Display) -> Error {
        Error::io(op, path, std::io::Error::other(e.to_string()))
    }

    /// Windows compares names without regard to case.
    fn same(a: &Path, b: &Path) -> bool {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }

    pub fn trash(path: &Path) -> Result<TrashedItem> {
        // The bin wants an absolute path with backslashes.
        let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        trash::delete(&abs).map_err(|e| fail("move to the Recycle Bin", path, e))?;
        // Find what was just put there: the newest item that came from this very path.
        let items = trash::os_limited::list().map_err(|e| fail("read the Recycle Bin", path, e))?;
        let item = items
            .into_iter()
            .filter(|i| same(&i.original_path(), &abs))
            .max_by_key(|i| i.time_deleted)
            .ok_or_else(|| {
                fail(
                    "find in the Recycle Bin",
                    path,
                    "it was moved, but the bin does not list it",
                )
            })?;
        Ok(TrashedItem {
            original: path.to_path_buf(),
            handle: TrashHandle::Opaque {
                id: item.id.to_string_lossy().into_owned(),
            },
        })
    }

    fn find(item: &TrashedItem) -> Result<Option<trash::TrashItem>> {
        let TrashHandle::Opaque { id } = &item.handle else {
            return Ok(None);
        };
        let items = trash::os_limited::list()
            .map_err(|e| fail("read the Recycle Bin", &item.original, e))?;
        Ok(items
            .into_iter()
            .find(|i| i.id.to_string_lossy() == id.as_str()))
    }

    pub fn restore(item: &TrashedItem) -> Result<()> {
        let Some(found) = find(item)? else {
            return Err(Error::Invalid(format!(
                "{} is no longer in the Recycle Bin",
                crate::display::path(&item.original)
            )));
        };
        if item.original.exists() {
            return Err(Error::AlreadyExists(item.original.clone()));
        }
        trash::os_limited::restore_all([found])
            .map_err(|e| fail("restore from the Recycle Bin", &item.original, e))
    }

    pub fn contains(item: &TrashedItem) -> bool {
        matches!(find(item), Ok(Some(_)))
    }
}

impl TrashBackend for WindowsTrash {
    #[cfg(windows)]
    fn trash(&self, path: &Path) -> Result<TrashedItem> {
        native_trash::trash(path)
    }
    #[cfg(not(windows))]
    fn trash(&self, _: &Path) -> Result<TrashedItem> {
        Err(Error::Unsupported("the Windows Recycle Bin"))
    }
    #[cfg(windows)]
    fn restore(&self, item: &TrashedItem) -> Result<()> {
        native_trash::restore(item)
    }
    #[cfg(not(windows))]
    fn restore(&self, _: &TrashedItem) -> Result<()> {
        Err(Error::Unsupported("the Windows Recycle Bin"))
    }
    #[cfg(windows)]
    fn contains(&self, item: &TrashedItem) -> bool {
        native_trash::contains(item)
    }
    #[cfg(not(windows))]
    fn contains(&self, _: &TrashedItem) -> bool {
        false
    }
}

// ------------------------------------------------------------------------------------ drives

/// What kind of drive Windows says a root is (`GetDriveTypeW`).
pub fn volume_kind_of(drive_type: u32) -> Option<VolumeKind> {
    // DRIVE_UNKNOWN 0, DRIVE_NO_ROOT_DIR 1, REMOVABLE 2, FIXED 3, REMOTE 4, CDROM 5, RAMDISK 6
    match drive_type {
        2 => Some(VolumeKind::Removable),
        3 => Some(VolumeKind::Fixed),
        4 => Some(VolumeKind::Network),
        5 => Some(VolumeKind::Optical),
        6 => Some(VolumeKind::Virtual),
        _ => None,
    }
}

/// The name shown for a drive, as Explorer writes it: "Windows (C:)", or "Local Disk (C:)"
/// when the volume has no label.
pub fn drive_display_name(label: &str, kind: VolumeKind, letter: char) -> String {
    let base = if label.trim().is_empty() {
        match kind {
            VolumeKind::Removable => "Removable Disk",
            VolumeKind::Network => "Network Drive",
            VolumeKind::Optical => "CD Drive",
            VolumeKind::Virtual => "RAM Disk",
            VolumeKind::Fixed => "Local Disk",
            // Windows lists its own devices as drives; this kind only comes from Linux.
            VolumeKind::Device => "Device",
        }
    } else {
        label.trim()
    };
    format!("{base} ({letter}:)")
}

/// The drive letters in a `GetLogicalDrives` bit mask, in order.
pub fn letters_in(mask: u32) -> Vec<char> {
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| (b'A' + i) as char)
        .collect()
}

/// The names Explorer shows (the shell's display names), which are the localized ones:
/// "Documenti", "Disco locale (C:)".
#[cfg(windows)]
pub mod shell_names {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::UI::Shell::{SHFILEINFOW, SHGFI_DISPLAYNAME, SHGetFileInfoW};

    /// What Explorer calls this folder or drive, or `None` if the shell does not say.
    pub fn display_name(path: &Path) -> Option<String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: SHFILEINFOW is plain data for which all-zero is a valid empty value.
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        // SAFETY: `wide` is NUL-terminated and outlives the call; `info` is the size passed.
        let got = unsafe {
            SHGetFileInfoW(
                wide.as_ptr(),
                0,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_DISPLAYNAME,
            )
        };
        if got == 0 {
            return None;
        }
        let end = info
            .szDisplayName
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(info.szDisplayName.len());
        let name = String::from_utf16_lossy(&info.szDisplayName[..end]);
        (!name.is_empty()).then_some(name)
    }
}

/// Safely removing a drive without administrator rights: lock the volume (it fails if a
/// program has a file open on it), dismount it, and ask the drive to let the media go.
#[cfg(windows)]
mod eject_drive {
    use std::os::windows::ffi::OsStrExt;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{
        FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA,
        IOCTL_STORAGE_MEDIA_REMOVAL, PREVENT_MEDIA_REMOVAL,
    };

    use crate::{Error, Result};

    fn control(
        handle: *mut std::ffi::c_void,
        code: u32,
        input: Option<&PREVENT_MEDIA_REMOVAL>,
    ) -> bool {
        let mut returned = 0u32;
        let (ptr, len) = match input {
            Some(i) => (
                i as *const PREVENT_MEDIA_REMOVAL as *const std::ffi::c_void,
                std::mem::size_of::<PREVENT_MEDIA_REMOVAL>() as u32,
            ),
            None => (std::ptr::null(), 0),
        };
        // SAFETY: `handle` is an open volume handle; the input is either null with length 0
        // or points to a live PREVENT_MEDIA_REMOVAL of the length given; there is no output.
        unsafe {
            DeviceIoControl(
                handle,
                code,
                ptr,
                len,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            ) != 0
        }
    }

    pub fn eject(letter: char) -> Result<()> {
        let path: Vec<u16> = std::ffi::OsStr::new(&format!(r"\\.\{letter}:"))
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: a NUL-terminated wide path; the other arguments are plain values or null.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(Error::Invalid(format!(
                "Windows would not let rada open drive {letter}: ({}). Use \"Safely Remove Hardware\" in the taskbar.",
                std::io::Error::last_os_error()
            )));
        }
        let result = (|| {
            // A program with a file open on the drive makes the lock fail: try a few times,
            // since a background scan often lets go within a second.
            let mut locked = false;
            for _ in 0..4 {
                if control(handle, FSCTL_LOCK_VOLUME, None) {
                    locked = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(400));
            }
            if !locked {
                return Err(Error::Invalid(format!(
                    "the drive is busy: a program still has a file or a folder on {letter}: open. Close it and try again."
                )));
            }
            if !control(handle, FSCTL_DISMOUNT_VOLUME, None) {
                return Err(Error::Invalid(format!(
                    "Windows could not dismount {letter}: ({})",
                    std::io::Error::last_os_error()
                )));
            }
            let allow = PREVENT_MEDIA_REMOVAL {
                PreventMediaRemoval: false,
            };
            control(handle, IOCTL_STORAGE_MEDIA_REMOVAL, Some(&allow));
            // A memory stick has nothing to eject: the dismount is what makes it safe.
            control(handle, IOCTL_STORAGE_EJECT_MEDIA, None);
            Ok(())
        })();
        // SAFETY: closing the handle opened above, once.
        unsafe { CloseHandle(handle) };
        result
    }
}

struct WindowsVolumes;

#[cfg(windows)]
mod native_volumes {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::sync::mpsc;
    use std::time::Duration;

    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    use super::*;

    /// How long a drive may take to answer before it is listed as not responding (a network
    /// share that went away can hold a call for half a minute).
    const ANSWER_WITHIN: Duration = Duration::from_secs(3);

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    fn text(buf: &[u16]) -> String {
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        OsString::from_wide(&buf[..end])
            .to_string_lossy()
            .into_owned()
    }

    struct Info {
        /// What Explorer calls the drive ("Local Disk (C:)", localized).
        shell_name: Option<String>,
        label: String,
        fs_type: String,
        total: Option<u64>,
        available: Option<u64>,
        read_only: bool,
    }

    fn query(root: &str) -> Info {
        let w = wide(root);
        let (mut label, mut fs) = ([0u16; 261], [0u16; 261]);
        let mut flags = 0u32;
        // SAFETY: the buffers are as long as the sizes passed with them, and the root is a
        // NUL-terminated wide string that outlives the call.
        let ok = unsafe {
            GetVolumeInformationW(
                w.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        } != 0;
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: three valid out-pointers and a NUL-terminated wide string.
        let sized =
            unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) } != 0;
        Info {
            shell_name: if ok {
                super::shell_names::display_name(Path::new(root))
            } else {
                None
            },
            label: if ok { text(&label) } else { String::new() },
            fs_type: if ok { text(&fs) } else { String::new() },
            total: sized.then_some(total),
            available: sized.then_some(avail),
            // FILE_READ_ONLY_VOLUME
            read_only: ok && flags & 0x0008_0000 != 0,
        }
    }

    pub fn list() -> Result<Vec<Volume>> {
        // SAFETY: no arguments.
        let mask = unsafe { GetLogicalDrives() };
        if mask == 0 {
            return Err(Error::io(
                "list the drives",
                Path::new(""),
                std::io::Error::last_os_error(),
            ));
        }
        let mut out = Vec::new();
        for letter in letters_in(mask) {
            let root = format!("{letter}:\\");
            let w = wide(&root);
            // SAFETY: a NUL-terminated wide string.
            let kind = volume_kind_of(unsafe { GetDriveTypeW(w.as_ptr()) });
            let Some(kind) = kind else { continue };
            // Ask on a thread of its own: a dead share must not hold the list.
            let (tx, rx) = mpsc::channel();
            let r = root.clone();
            let _ = std::thread::Builder::new()
                .name("rada-drive".into())
                .spawn(move || {
                    let _ = tx.send(query(&r));
                });
            let (shell_name, label, fs_type, total, available, read_only, responsive) =
                match rx.recv_timeout(ANSWER_WITHIN) {
                    Ok(i) => (
                        i.shell_name,
                        i.label,
                        i.fs_type,
                        i.total,
                        i.available,
                        i.read_only,
                        true,
                    ),
                    Err(_) => (None, String::new(), String::new(), None, None, false, false),
                };
            // An optical drive with no disc has nothing to show.
            if kind == VolumeKind::Optical && responsive && fs_type.is_empty() {
                continue;
            }
            out.push(Volume {
                mount_point: PathBuf::from(&root),
                // Explorer's own words when the shell has them; else made the same way.
                label: Some(shell_name.unwrap_or_else(|| drive_display_name(&label, kind, letter))),
                fs_type,
                device: format!("{letter}:"),
                kind,
                drive_letter: Some(letter),
                total,
                available,
                read_only,
                responsive,
            });
        }
        Ok(out)
    }
}

impl VolumeLister for WindowsVolumes {
    fn can_eject(&self, v: &Volume) -> bool {
        cfg!(windows)
            && v.drive_letter.is_some()
            && matches!(v.kind, VolumeKind::Removable | VolumeKind::Optical)
    }

    fn eject(&self, v: &Volume) -> Result<()> {
        #[cfg(windows)]
        {
            match v.drive_letter {
                Some(letter) => eject_drive::eject(letter),
                None => Err(Error::Unsupported("eject")),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = v;
            Err(Error::Unsupported("eject"))
        }
    }

    #[cfg(windows)]
    fn list(&self) -> Result<Vec<Volume>> {
        native_volumes::list()
    }
    #[cfg(not(windows))]
    fn list(&self) -> Result<Vec<Volume>> {
        Err(Error::Unsupported("drive letters"))
    }
}

// ---------------------------------------------------------------------------- Known Folders

#[cfg(windows)]
pub mod known_folders {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music,
        FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath,
    };
    use windows_sys::core::GUID;

    use crate::platform::UserDirs;

    fn folder(id: &GUID) -> Option<PathBuf> {
        let mut raw: *mut u16 = std::ptr::null_mut();
        // SAFETY: `id` is a valid GUID, the token is the current user (null), and `raw` is
        // an out-pointer; on success it points to a NUL-terminated string that the shell
        // allocated and that is freed below.
        let hr = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw) };
        if hr < 0 || raw.is_null() {
            return None;
        }
        // SAFETY: `raw` is NUL-terminated (see above), so scanning up to the NUL stays
        // inside the allocation.
        let path = unsafe {
            let mut len = 0;
            while *raw.add(len) != 0 {
                len += 1;
            }
            PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(raw, len)))
        };
        // SAFETY: allocated by the shell with CoTaskMemAlloc, freed once.
        unsafe { CoTaskMemFree(raw as *const _) };
        Some(path)
    }

    /// The user's standard folders as Windows knows them (they may have been moved to
    /// another drive or to OneDrive).
    pub fn user_dirs() -> UserDirs {
        UserDirs {
            desktop: folder(&FOLDERID_Desktop),
            documents: folder(&FOLDERID_Documents),
            downloads: folder(&FOLDERID_Downloads),
            music: folder(&FOLDERID_Music),
            pictures: folder(&FOLDERID_Pictures),
            videos: folder(&FOLDERID_Videos),
        }
    }
}

// ------------------------------------------------------------------------------- opening files

struct WindowsOpener;

#[cfg(windows)]
mod native_open {
    use std::io::Read;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::*;
    use crate::platform::launch::{self, Spawn, Stream, Task};
    use crate::platform::spawn::operand;
    use crate::platform::split_command;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    fn stdio(s: Stream) -> Stdio {
        match s {
            Stream::Null => Stdio::null(),
            Stream::Piped => Stdio::piped(),
        }
    }

    /// The process a [`Spawn`] describes, not yet started.
    fn command_of(spawn: &Spawn) -> Command {
        let mut cmd = Command::new(&spawn.program);
        cmd.args(&spawn.args)
            .stdin(stdio(spawn.stdin))
            .stdout(stdio(spawn.stdout))
            .stderr(stdio(spawn.stderr))
            .creation_flags(spawn.creation_flags);
        cmd
    }

    /// Start a program and leave it: nobody waits for it but a thread that collects it.
    fn detach(cmd: Command, op: &'static str, subject: &Path, flags: u32) -> Result<()> {
        let mut cmd = cmd;
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
            .map_err(|e| Error::io(op, subject, e))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    /// The task, done by a helper process that has no console (see [`launch`]): the shell is
    /// called from there, so that what it starts has no console of rada's to attach to. The
    /// helper is waited for (it ends as soon as the shell has answered), and its standard
    /// output, which holds the reason when it failed, becomes the error.
    pub fn via_helper(task: &Task, op: &'static str, subject: &Path) -> Result<()> {
        let exe = std::env::current_exe().map_err(|e| Error::io(op, subject, e))?;
        let spawn = launch::helper_spawn(&exe, task);
        let mut child = command_of(&spawn)
            .spawn()
            .map_err(|e| Error::io(op, subject, e))?;
        let mut out = child.stdout.take();
        let end = Instant::now() + Duration::from_secs(30);
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break st,
                Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(15)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::io(
                        op,
                        subject,
                        std::io::Error::other("Windows did not answer"),
                    ));
                }
                Err(e) => return Err(Error::io(op, subject, e)),
            }
        };
        if status.success() {
            return Ok(());
        }
        let mut why = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut why);
        }
        let why = why.trim();
        Err(Error::io(
            op,
            subject,
            std::io::Error::other(if why.is_empty() {
                "Windows could not open it".to_string()
            } else {
                why.to_string()
            }),
        ))
    }

    /// The default program of Windows for this item (what a double click in Explorer does).
    pub fn open(path: &Path) -> Result<()> {
        via_helper(&Task::Open(path.to_path_buf()), "open", path)
    }

    /// What the helper does for [`open`]: the call to the shell itself. `Err` is the reason.
    pub fn shell_open(path: &Path) -> std::result::Result<(), String> {
        let verb = wide(std::ffi::OsStr::new("open"));
        let file = wide(path.as_os_str());
        // SAFETY: NUL-terminated wide strings that live through the call; no window, no
        // parameters, no working folder.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Success is anything above 32; below that is an error code.
        let code = result as isize;
        if code > 32 {
            return Ok(());
        }
        let why = match code {
            2 | 3 => "it was not found",
            5 => "access was denied",
            31 => "no program is set to open this kind of file (try Open with…)",
            _ => "Windows could not open it",
        };
        Err(format!("{why} (code {code})"))
    }

    pub fn open_with(path: &Path, program: &str) -> Result<()> {
        if program.trim().is_empty() {
            // Windows' own "Open with" window, with no shell in between: the path is one
            // argument, so quotes and blanks in it are no problem.
            let mut cmd = Command::new("rundll32.exe");
            cmd.arg("shell32.dll,OpenAs_RunDLL").arg(operand(path));
            return detach(
                cmd,
                "show the Open with window for",
                path,
                launch::NO_CONSOLE,
            );
        }
        let words = split_command(program);
        let Some((exe, args)) = words.split_first() else {
            return Err(Error::Invalid("no program given".into()));
        };
        let mut cmd = Command::new(exe);
        cmd.args(args).arg(operand(path));
        detach(
            cmd,
            "start the program for",
            path,
            launch::NO_CONSOLE | launch::CREATE_NEW_PROCESS_GROUP,
        )
    }

    pub fn reveal(path: &Path) -> Result<()> {
        // Explorer parses its own command line: `/select,"path"` as one raw argument.
        let mut cmd = Command::new("explorer.exe");
        cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        detach(cmd, "show in Explorer", path, launch::NO_CONSOLE)
    }

    pub fn open_terminal(dir: &Path) -> Result<()> {
        // Windows Terminal when there is one, else PowerShell in a console of its own.
        let mut wt = Command::new("wt.exe");
        wt.arg("-d").arg(dir);
        if detach(wt, "start Windows Terminal in", dir, launch::NO_CONSOLE).is_ok() {
            return Ok(());
        }
        let mut ps = Command::new("powershell.exe");
        ps.arg("-NoExit").current_dir(dir);
        detach(ps, "start PowerShell in", dir, launch::CREATE_NEW_CONSOLE)
    }
}

/// The programs the shell associates with a file's extension (what Explorer's "Open with"
/// lists), through `SHAssocEnumHandlers`; and starting a file with one of them, as Explorer
/// does, through the handler's invoker.
#[cfg(windows)]
mod native_apps {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx,
        IDataObject,
    };
    use windows::Win32::UI::Shell::{
        ASSOC_FILTER, ASSOC_FILTER_NONE, ASSOC_FILTER_RECOMMENDED, ASSOCF_NONE,
        ASSOCSTR_EXECUTABLE, AssocQueryStringW, BHID_DataObject, IAssocHandler, IShellItem,
        SHAssocEnumHandlers, SHCreateItemFromParsingName,
    };
    use windows::core::{HSTRING, PWSTR};

    use super::*;
    use crate::platform::{OpenApp, OpenChoices};

    /// Run `f` with COM started on this thread (the shell's objects want an apartment).
    fn with_com<T>(f: impl FnOnce() -> T) -> T {
        // SAFETY: balanced with `CoUninitialize` below when it succeeded.
        let started = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let out = f();
        if started {
            // SAFETY: this thread started COM above.
            unsafe { CoUninitialize() };
        }
        out
    }

    /// A string the shell allocated, copied and freed.
    fn take(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: a NUL-terminated string from the shell, freed once with the task allocator.
        let s = unsafe { p.to_string() }.unwrap_or_default();
        unsafe { CoTaskMemFree(Some(p.0 as *const core::ffi::c_void)) };
        s
    }

    fn handlers(ext: &str, filter: ASSOC_FILTER) -> Vec<IAssocHandler> {
        // SAFETY: plain calls into the shell with a valid wide string.
        let Ok(list) = (unsafe { SHAssocEnumHandlers(&HSTRING::from(ext), filter) }) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for _ in 0..200 {
            let mut slot: [Option<IAssocHandler>; 1] = [None];
            let mut got = 0u32;
            // SAFETY: one slot, and a place for the count.
            let r = unsafe { list.Next(&mut slot, Some(&mut got)) };
            match slot[0].take() {
                Some(h) if r.is_ok() && got > 0 => out.push(h),
                _ => break,
            }
        }
        out
    }

    /// The program Windows opens this extension with (its path), if it says.
    fn default_program(ext: &str) -> Option<String> {
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: a buffer of `len` wide characters.
        let r = unsafe {
            AssocQueryStringW(
                ASSOCF_NONE,
                ASSOCSTR_EXECUTABLE,
                &HSTRING::from(ext),
                &HSTRING::from("open"),
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
            )
        };
        if r.is_err() || len == 0 {
            return None;
        }
        let n = (len as usize).min(buf.len());
        Some(
            String::from_utf16_lossy(&buf[..n])
                .trim_end_matches('\0')
                .to_string(),
        )
    }

    pub fn apps_for(path: &Path) -> OpenChoices {
        let Some(ext) = path.extension() else {
            return OpenChoices::default();
        };
        let ext = format!(".{}", ext.to_string_lossy());
        let apps = with_com(|| {
            let default = default_program(&ext)
                .and_then(|p| {
                    Path::new(&p)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_lowercase())
                })
                .unwrap_or_default();
            let mut apps: Vec<OpenApp> = Vec::new();
            for h in handlers(&ext, ASSOC_FILTER_RECOMMENDED) {
                // SAFETY: the handler is alive; the strings are taken and freed.
                let (id, name) = unsafe {
                    (
                        take(h.GetName().unwrap_or_default()),
                        take(h.GetUIName().unwrap_or_default()),
                    )
                };
                if id.is_empty() || apps.iter().any(|a| a.id == id) {
                    continue;
                }
                let is_default = !default.is_empty() && id.to_lowercase().ends_with(&default);
                apps.push(OpenApp {
                    name: if name.is_empty() { id.clone() } else { name },
                    exec: id.clone(),
                    id,
                    terminal: false,
                    is_default,
                    source: None,
                    icon: None,
                });
            }
            apps
        });
        OpenChoices {
            kind: Some(ext.to_lowercase()),
            apps,
        }
    }

    pub fn open_with_app(path: &Path, app: &OpenApp) -> Result<()> {
        super::native_open::via_helper(
            &crate::platform::launch::Task::OpenWith {
                path: path.to_path_buf(),
                app_id: app.id.clone(),
            },
            "open",
            path,
        )
    }

    /// What the helper does for [`open_with_app`]: asks the handler of the shell to open the
    /// item. `Err` is the reason.
    pub fn handler_open(path: &Path, app_id: &str) -> std::result::Result<(), String> {
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        with_com(|| {
            let handler = handlers(&ext, ASSOC_FILTER_NONE)
                .into_iter()
                // SAFETY: the handler is alive; the string is taken and freed.
                .find(|h| unsafe { take(h.GetName().unwrap_or_default()) } == app_id)
                .ok_or_else(|| {
                    format!("{app_id} is not associated with this kind of file any more")
                })?;
            // SAFETY: the item and the data object are COM objects owned here.
            unsafe {
                let item: IShellItem = SHCreateItemFromParsingName(
                    &HSTRING::from(path.as_os_str()),
                    None::<&IBindCtx>,
                )
                .map_err(|e| e.message())?;
                let data: IDataObject = item
                    .BindToHandler(None::<&IBindCtx>, &BHID_DataObject)
                    .map_err(|e| e.message())?;
                let invoker = handler.CreateInvoker(&data).map_err(|e| e.message())?;
                invoker.Invoke().map_err(|e| e.message())
            }
        })
    }
}

impl Opener for WindowsOpener {
    #[cfg(windows)]
    fn open(&self, path: &Path) -> Result<()> {
        native_open::open(path)
    }
    #[cfg(not(windows))]
    fn open(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("ShellExecute"))
    }
    #[cfg(windows)]
    fn reveal(&self, path: &Path) -> Result<()> {
        native_open::reveal(path)
    }
    #[cfg(not(windows))]
    fn reveal(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("explorer /select"))
    }
    #[cfg(windows)]
    fn open_with(&self, path: &Path, program: &str) -> Result<()> {
        native_open::open_with(path, program)
    }
    #[cfg(windows)]
    fn apps_for(&self, path: &Path) -> crate::platform::OpenChoices {
        native_apps::apps_for(path)
    }
    #[cfg(windows)]
    fn open_with_app(&self, path: &Path, app: &crate::platform::OpenApp) -> Result<()> {
        native_apps::open_with_app(path, app)
    }
    #[cfg(windows)]
    fn open_terminal(&self, dir: &Path) -> Result<()> {
        native_open::open_terminal(dir)
    }
}

/// What rada does when it was started as the launch helper (see [`crate::platform::launch`]):
/// the one call to the shell, from a process that has no console. The reason of a failure is
/// written to standard output, which the parent reads. The exit code is 0 or 1.
#[cfg(windows)]
pub fn run_launch_helper(task: &crate::platform::launch::Task) -> i32 {
    use crate::platform::launch::Task;
    let result = match task {
        Task::Open(path) => native_open::shell_open(path),
        Task::OpenWith { path, app_id } => native_apps::handler_open(path, app_id),
    };
    match result {
        Ok(()) => 0,
        Err(why) => {
            use std::io::Write;
            let _ = write!(std::io::stdout(), "{why}");
            1
        }
    }
}

#[cfg(not(windows))]
pub fn run_launch_helper(_: &crate::platform::launch::Task) -> i32 {
    2
}

pub struct WindowsPlatform {
    dirs: Dirs,
    attrs: WindowsAttributes,
    trash: Box<dyn TrashBackend>,
    volumes: WindowsVolumes,
    opener: WindowsOpener,
    /// Tests: the standard folders come from `user-dirs.dirs` in the sandbox, not from the
    /// shell of the machine running them.
    sandbox_user_dirs: bool,
}

impl WindowsPlatform {
    /// The user's Recent folder, where Windows keeps a shortcut to everything opened. Not in
    /// the sandbox: tests do not read the real one.
    fn recent_dir(&self) -> Option<std::path::PathBuf> {
        if self.sandbox_user_dirs {
            return None;
        }
        let appdata = std::env::var_os("APPDATA")?;
        Some(std::path::PathBuf::from(appdata).join(r"Microsoft\Windows\Recent"))
    }

    pub fn new(dirs: Dirs) -> Self {
        Self::with_trash(dirs, Box::new(WindowsTrash))
    }

    /// A platform whose trash is `trash` (the tests give it a folder of their own, so that
    /// nothing goes to the real Recycle Bin of the machine running them).
    pub fn with_trash(dirs: Dirs, trash: Box<dyn TrashBackend>) -> Self {
        WindowsPlatform {
            dirs,
            attrs: WindowsAttributes,
            trash,
            volumes: WindowsVolumes,
            opener: WindowsOpener,
            sandbox_user_dirs: false,
        }
    }

    /// The standard folders are those `user-dirs.dirs` names in the config folder (the
    /// English ones without it), whatever the real Known Folders are: for tests.
    pub fn with_sandbox_user_dirs(mut self) -> Self {
        self.sandbox_user_dirs = true;
        self
    }
}

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
    }
    fn dirs(&self) -> &Dirs {
        &self.dirs
    }
    fn trash(&self) -> &dyn TrashBackend {
        self.trash.as_ref()
    }
    fn volumes(&self) -> &dyn VolumeLister {
        &self.volumes
    }
    fn attributes(&self) -> &dyn FileAttributes {
        &self.attrs
    }
    fn opener(&self) -> &dyn Opener {
        &self.opener
    }
    fn fidelity(&self) -> &dyn super::Fidelity {
        // Windows: NTFS ACLs and alternate data streams are for the platform phase (2A).
        &super::fidelity::NotYet
    }
    fn user_dirs(&self) -> UserDirs {
        if self.sandbox_user_dirs {
            return UserDirs::from_xdg_file(&self.dirs.config, &self.dirs.home)
                .unwrap_or_else(|| UserDirs::conventional(&self.dirs.home, "Videos"));
        }
        // The Known Folders, as the shell has them (they may have been moved). Anything the
        // shell does not answer for falls back to the names of a default profile.
        #[cfg(windows)]
        {
            let known = known_folders::user_dirs();
            let fallback = UserDirs::conventional(&self.dirs.home, "Videos");
            UserDirs {
                desktop: known.desktop.or(fallback.desktop),
                documents: known.documents.or(fallback.documents),
                downloads: known.downloads.or(fallback.downloads),
                music: known.music.or(fallback.music),
                pictures: known.pictures.or(fallback.pictures),
                videos: known.videos.or(fallback.videos),
            }
        }
        #[cfg(not(windows))]
        UserDirs::conventional(&self.dirs.home, "Videos")
    }
    fn recent_available(&self) -> bool {
        self.recent_dir().is_some_and(|d| d.is_dir())
    }
    fn recent_files(
        &self,
        _cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<(std::path::PathBuf, std::time::SystemTime)>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(self.recent_dir()?).ok()?.flatten() {
            let path = e.path();
            if !path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("lnk"))
            {
                continue;
            }
            // The shortcut was last touched when the item was last opened.
            let Some(at) = e.metadata().ok().and_then(|m| m.modified().ok()) else {
                continue;
            };
            // A shortcut is a few hundred bytes; a huge one is not one.
            if e.metadata().is_ok_and(|m| m.len() > 64 * 1024) {
                continue;
            }
            if let Some(target) = std::fs::read(&path)
                .ok()
                .and_then(|b| crate::recents::lnk_target(&b))
            {
                out.push((target, at));
            }
        }
        Some(out)
    }
    fn display_names(&self, places: &[std::path::PathBuf]) -> Vec<(std::path::PathBuf, String)> {
        if self.sandbox_user_dirs {
            return Vec::new();
        }
        #[cfg(windows)]
        {
            // The Known Folders and the drives, as Explorer names them.
            let mut paths: Vec<std::path::PathBuf> = places.to_vec();
            paths.push(self.dirs.home.clone());
            if let Ok(vols) = native_volumes::list() {
                paths.extend(vols.into_iter().map(|v| v.mount_point));
            }
            paths.sort();
            paths.dedup();
            paths
                .into_iter()
                .filter_map(|p| {
                    shell_names::display_name(&p).and_then(|n| super::only_different(&p, n))
                })
                .collect()
        }
        #[cfg(not(windows))]
        {
            let _ = places;
            Vec::new()
        }
    }
    fn path_rules(&self) -> PathRules {
        PathRules {
            case_insensitive: true,
            normalization_insensitive: false,
            forbidden_chars: &['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\0'],
            reserved_names: &[
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ],
            max_name_bytes: 255,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FileKind;

    fn meta(kind: FileKind, attrs: u32) -> FsMeta {
        FsMeta {
            kind,
            special: None,
            size: 0,
            mtime: None,
            atime: None,
            ctime: None,
            btime: None,
            mode: None,
            dev: None,
            ino: None,
            nlink: None,
            uid: None,
            gid: None,
            blocks: None,
            readonly: false,
            os_attrs: Some(attrs),
        }
    }

    #[test]
    fn drives_are_named_the_way_explorer_names_them() {
        assert_eq!(
            drive_display_name("Windows", VolumeKind::Fixed, 'C'),
            "Windows (C:)"
        );
        assert_eq!(
            drive_display_name("", VolumeKind::Fixed, 'D'),
            "Local Disk (D:)"
        );
        assert_eq!(
            drive_display_name("  ", VolumeKind::Removable, 'E'),
            "Removable Disk (E:)"
        );
        assert_eq!(
            drive_display_name("", VolumeKind::Network, 'Z'),
            "Network Drive (Z:)"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_shell_names_the_system_drive_the_way_explorer_does() {
        let name = shell_names::display_name(Path::new("C:\\")).expect("the shell names C:");
        assert!(name.ends_with("(C:)"), "{name}");
        // A folder with nothing localized about it keeps its name.
        let tmp = std::env::temp_dir();
        let shown = shell_names::display_name(&tmp).unwrap_or_default();
        assert!(!shown.is_empty());
        // The drive list carries the same words.
        let vols = native_volumes::list().unwrap();
        let c = vols
            .iter()
            .find(|v| v.drive_letter == Some('C'))
            .expect("C:");
        assert_eq!(c.label.as_deref(), Some(name.as_str()));
        // And the names asked for a place are only the ones that differ from the disk's.
        let got = WindowsPlatform::new(Dirs::under(&tmp)).display_names(&[]);
        assert!(
            got.iter()
                .any(|(p, n)| p == Path::new("C:\\") && *n == name),
            "{got:?}"
        );
        assert!(
            got.iter()
                .all(|(p, n)| p.file_name().is_none_or(|f| f.to_string_lossy() != *n))
        );
    }

    #[test]
    fn no_removable_means_no_eject_entry_and_a_stick_has_one() {
        let v = |kind, letter| Volume {
            mount_point: std::path::PathBuf::from("E:\\"),
            label: None,
            fs_type: "FAT32".into(),
            device: "E:".into(),
            kind,
            drive_letter: letter,
            total: None,
            available: None,
            read_only: false,
            responsive: true,
        };
        let l = WindowsVolumes;
        assert_eq!(
            l.can_eject(&v(VolumeKind::Removable, Some('E'))),
            cfg!(windows)
        );
        assert!(!l.can_eject(&v(VolumeKind::Fixed, Some('C'))));
        assert!(!l.can_eject(&v(VolumeKind::Network, Some('Z'))));
        assert!(!l.can_eject(&v(VolumeKind::Removable, None)));
    }

    #[test]
    fn drive_types_and_letters() {
        assert_eq!(volume_kind_of(3), Some(VolumeKind::Fixed));
        assert_eq!(volume_kind_of(2), Some(VolumeKind::Removable));
        assert_eq!(volume_kind_of(4), Some(VolumeKind::Network));
        assert_eq!(volume_kind_of(5), Some(VolumeKind::Optical));
        assert_eq!(
            volume_kind_of(1),
            None,
            "no root directory: nothing to list"
        );
        assert_eq!(volume_kind_of(0), None);
        assert_eq!(letters_in(0b101), vec!['A', 'C']);
        assert_eq!(letters_in(1 << 25), vec!['Z']);
        assert!(letters_in(0).is_empty());
    }

    #[test]
    fn hidden_system_and_junction_attributes_are_modelled() {
        let a = WindowsAttributes.attrs(OsStr::new("x"), &meta(FileKind::File, 0x2 | 0x4));
        assert!(a.hidden && a.system);
        let j = WindowsAttributes.attrs(OsStr::new("link"), &meta(FileKind::Dir, 0x400 | 0x10));
        assert_eq!(j.reparse, Some(ReparseKind::Junction));
    }
}
