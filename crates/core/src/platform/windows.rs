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
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn text(buf: &[u16]) -> String {
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        OsString::from_wide(&buf[..end]).to_string_lossy().into_owned()
    }

    struct Info {
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
            let (label, fs_type, total, available, read_only, responsive) =
                match rx.recv_timeout(ANSWER_WITHIN) {
                    Ok(i) => (i.label, i.fs_type, i.total, i.available, i.read_only, true),
                    Err(_) => (String::new(), String::new(), None, None, false, false),
                };
            // An optical drive with no disc has nothing to show.
            if kind == VolumeKind::Optical && responsive && fs_type.is_empty() {
                continue;
            }
            out.push(Volume {
                mount_point: PathBuf::from(&root),
                label: Some(drive_display_name(&label, kind, letter)),
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
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::*;
    use crate::platform::split_command;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    /// The default program of Windows for this item (what a double click in Explorer does).
    pub fn open(path: &Path) -> Result<()> {
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
        Err(Error::io(
            "open",
            path,
            std::io::Error::other(format!("{why} (code {code})")),
        ))
    }

    fn detach(mut cmd: Command, op: &'static str, subject: &Path, flags: u32) -> Result<()> {
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

    pub fn open_with(path: &Path, program: &str) -> Result<()> {
        if program.trim().is_empty() {
            // Windows' own "Open with" window, with no shell in between: the path is one
            // argument, so quotes and blanks in it are no problem.
            let mut cmd = Command::new("rundll32.exe");
            cmd.arg("shell32.dll,OpenAs_RunDLL").arg(path);
            return detach(cmd, "show the Open with window for", path, DETACHED_PROCESS);
        }
        let words = split_command(program);
        let Some((exe, args)) = words.split_first() else {
            return Err(Error::Invalid("no program given".into()));
        };
        let mut cmd = Command::new(exe);
        cmd.args(args).arg(path);
        detach(
            cmd,
            "start the program for",
            path,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
        )
    }

    pub fn reveal(path: &Path) -> Result<()> {
        // Explorer parses its own command line: `/select,"path"` as one raw argument.
        let mut cmd = Command::new("explorer.exe");
        cmd.raw_arg(format!("/select,\"{}\"", path.display()));
        detach(cmd, "show in Explorer", path, 0)
    }

    pub fn open_terminal(dir: &Path) -> Result<()> {
        // Windows Terminal when there is one, else PowerShell in a console of its own.
        let mut wt = Command::new("wt.exe");
        wt.arg("-d").arg(dir);
        if detach(wt, "start Windows Terminal in", dir, 0).is_ok() {
            return Ok(());
        }
        let mut ps = Command::new("powershell.exe");
        ps.arg("-NoExit").current_dir(dir);
        detach(ps, "start PowerShell in", dir, CREATE_NEW_CONSOLE)
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
    fn open_terminal(&self, dir: &Path) -> Result<()> {
        native_open::open_terminal(dir)
    }
}

pub struct WindowsPlatform {
    dirs: Dirs,
    attrs: WindowsAttributes,
    trash: Box<dyn TrashBackend>,
    volumes: WindowsVolumes,
    opener: WindowsOpener,
}

impl WindowsPlatform {
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
        }
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

    #[test]
    fn drive_types_and_letters() {
        assert_eq!(volume_kind_of(3), Some(VolumeKind::Fixed));
        assert_eq!(volume_kind_of(2), Some(VolumeKind::Removable));
        assert_eq!(volume_kind_of(4), Some(VolumeKind::Network));
        assert_eq!(volume_kind_of(5), Some(VolumeKind::Optical));
        assert_eq!(volume_kind_of(1), None, "no root directory: nothing to list");
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
