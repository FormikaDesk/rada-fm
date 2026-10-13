//! macOS (also the fallback for other Unix systems).
//!
//! * The trash is `~/.Trash` (and `/Volumes/<name>/.Trashes/<uid>` for items on another
//!   volume), where the Finder looks; putting back is rada's own (undo), the Finder's "Put
//!   Back" metadata is not written.
//! * Volumes are the entries of `/Volumes`, sized with `statfs`; the startup disk is `/`.
//! * Opening goes through `open(1)`: the default program, `open -a <App>` for "Open with…",
//!   `open -R` to reveal, and the terminal the user is in (Terminal, iTerm2, Ghostty).
//! * Names are case-insensitive and Unicode-normalisation-insensitive.
//! * Names that the Finder shows differently (localized folders, the startup disk) come from
//!   `NSFileManager`; packages (`.app`, `.photoslibrary`…) are what the system says they are;
//!   `UF_HIDDEN` folders (`~/Library`) are hidden like in the Finder.
//! * Extended attributes and ACLs are not carried over by a copy yet: the plan says so.

use std::path::Path;
use std::sync::Arc;

use super::attrs::{Attrs, ReparseKind};
use super::dirtrash::DirTrash;
use super::trash::TrashBackend;
use super::volumes::{Volume, VolumeKind, VolumeLister};
use super::{
    AccessDenied, Dirs, FileAttributes, Opener, PathRules, PlaceKind, Platform, UserDirs,
    only_different,
};
use crate::fs::{FileKind, FsMeta, LocalFs};
use crate::{Error, Result};

/// The BSD flag that hides a file from the Finder (`chflags hidden`): `~/Library`, `/usr`…
pub const UF_HIDDEN: u32 = 0x8000;

/// The `kHasBundle` bit of the Finder flags (the high-order bytes of `com.apple.FinderInfo`
/// at offset 8): a folder with it set is a package even without an extension the system
/// knows.
pub fn finder_info_marks_bundle(finder_info: &[u8]) -> bool {
    finder_info
        .get(8..10)
        .is_some_and(|f| u16::from_be_bytes([f[0], f[1]]) & 0x2000 != 0)
}

/// iCloud Drive, where macOS keeps it.
pub fn icloud_drive(home: &Path) -> std::path::PathBuf {
    home.join("Library/Mobile Documents/com~apple~CloudDocs")
}

/// What the Finder hides: dot files (which include `.DS_Store`, `._*` and `.localized`), and
/// anything the system flagged hidden.
pub struct MacAttributes;

impl FileAttributes for MacAttributes {
    fn attrs(&self, name: &std::ffi::OsStr, meta: &FsMeta) -> Attrs {
        let mode = meta.mode.unwrap_or(0);
        Attrs {
            hidden: name.as_encoded_bytes().first() == Some(&b'.')
                || meta.os_attrs.is_some_and(|f| f & UF_HIDDEN != 0),
            system: false,
            readonly: mode & 0o222 == 0,
            executable: meta.kind == FileKind::File && mode & 0o111 != 0,
            reparse: (meta.kind == FileKind::Symlink).then_some(ReparseKind::Symlink),
        }
    }

    fn is_package(&self, path: &Path, meta: &FsMeta) -> bool {
        meta.kind == FileKind::Dir && native::is_package(path)
    }
}

/// The parts that call into the system.
mod native {
    use std::path::Path;

    /// Ask Launch Services whether the folder is a package (`NSURLIsPackageKey`), which knows
    /// every extension that apps declare, and fall back on the Finder's bundle bit.
    #[cfg(target_os = "macos")]
    pub fn is_package(path: &Path) -> bool {
        use objc2_foundation::{NSArray, NSNumber, NSString, NSURL, NSURLIsPackageKey};
        let Some(text) = path.to_str() else {
            return false;
        };
        let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(text), true);
        // SAFETY: NSURLIsPackageKey is a constant string provided by Foundation.
        let key = unsafe { NSURLIsPackageKey };
        let keys = NSArray::from_slice(&[key]);
        if let Ok(values) = url.resourceValuesForKeys_error(&keys)
            && let Some(v) = values.objectForKey(key)
            && let Some(n) = v.downcast_ref::<NSNumber>()
            && n.as_bool()
        {
            return true;
        }
        has_bundle_bit(path)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn is_package(_path: &Path) -> bool {
        false
    }

    #[cfg(target_os = "macos")]
    fn has_bundle_bit(path: &Path) -> bool {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let Ok(p) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let mut info = [0u8; 32];
        // SAFETY: both strings are NUL-terminated and the buffer is 32 bytes long.
        let n = unsafe {
            libc::getxattr(
                p.as_ptr(),
                c"com.apple.FinderInfo".as_ptr(),
                info.as_mut_ptr().cast(),
                info.len(),
                0,
                0,
            )
        };
        n >= 10 && super::finder_info_marks_bundle(&info)
    }

    /// The name the Finder shows for a file or folder.
    #[cfg(target_os = "macos")]
    pub fn display_name(path: &Path) -> Option<String> {
        use objc2_foundation::{NSFileManager, NSString};
        let text = path.to_str()?;
        let fm = NSFileManager::defaultManager();
        let name = fm.displayNameAtPath(&NSString::from_str(text));
        Some(name.to_string())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn display_name(_path: &Path) -> Option<String> {
        None
    }
}

/// What a copy keeps on macOS: the Finder's tags (and nothing else of the extended attributes
/// yet, which the plan says before anything is copied).
pub struct MacFidelity;

impl super::Fidelity for MacFidelity {
    fn implemented(&self) -> bool {
        false
    }
    fn destination_support(&self, dir: &Path) -> super::DestSupport {
        super::fidelity::NotYet.destination_support(dir)
    }
    fn source_attributes(&self, path: &Path) -> super::SourceAttrs {
        super::fidelity::NotYet.source_attributes(path)
    }
    fn can_set_owner(&self, uid: u32, gid: u32) -> bool {
        super::fidelity::NotYet.can_set_owner(uid, gid)
    }
    fn copy_attributes(
        &self,
        src: &Path,
        dst: &Path,
        what: super::Preserve,
        _owner: Option<(u32, u32)>,
    ) -> super::AttrOutcome {
        #[allow(unused_mut)]
        let mut outcome = super::AttrOutcome::default();
        if !what.xattrs {
            return outcome;
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use super::tags::xattr;
            const TAGS: &str = "com.apple.metadata:_kMDItemUserTags";
            match xattr::get(src, TAGS) {
                Ok(Some(value)) => {
                    if xattr::set(dst, TAGS, &value).is_err() {
                        outcome.note("the tags");
                    }
                }
                Ok(None) => {}
                Err(_) => outcome.note("the tags"),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = (src, dst);
        outcome
    }
}

/// The folders and disks worth looking up: the standard places and the few the system names.
pub fn name_candidates(places: &[std::path::PathBuf], home: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = places.to_vec();
    for extra in [
        Path::new("/").to_path_buf(),
        Path::new("/Applications").to_path_buf(),
        home.to_path_buf(),
        home.join("Applications"),
        home.join("Library"),
        home.join("Public"),
        icloud_drive(home),
    ] {
        if !out.contains(&extra) {
            out.push(extra);
        }
    }
    out
}

/// The folder a volume's own trash is, for an item under `/Volumes/<name>/`.
pub fn volume_trash_of(path: &Path, uid: u32) -> Option<std::path::PathBuf> {
    let rest = path.strip_prefix("/Volumes").ok()?;
    let volume = rest.components().next()?;
    // A path that is just the volume (or the trash itself) has no trash of its own to name.
    rest.components().nth(1)?;
    Some(
        Path::new("/Volumes")
            .join(volume)
            .join(".Trashes")
            .join(uid.to_string()),
    )
}

/// The application name `open -a` takes for a terminal program, from `$TERM_PROGRAM`.
pub fn terminal_app(term_program: Option<&str>) -> &'static str {
    match term_program {
        Some("iTerm.app") => "iTerm",
        Some("ghostty") => "Ghostty",
        Some("WezTerm") => "WezTerm",
        Some("Alacritty") => "Alacritty",
        Some("kitty") => "kitty",
        _ => "Terminal",
    }
}

/// How a filesystem type reported by `statfs` is classed as a kind of volume.
pub fn kind_of_fs(fs_type: &str, local: bool) -> VolumeKind {
    match fs_type {
        "smbfs" | "nfs" | "afpfs" | "webdav" | "ftp" | "cifs" => VolumeKind::Network,
        _ if !local => VolumeKind::Network,
        "cd9660" | "udf" => VolumeKind::Optical,
        // What sticks, cards and camera disks are usually formatted as.
        "msdos" | "exfat" | "ntfs" => VolumeKind::Removable,
        "devfs" | "autofs" | "synthfs" => VolumeKind::Virtual,
        _ => VolumeKind::Fixed,
    }
}

struct MacVolumes;

#[cfg(target_os = "macos")]
mod native_volumes {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use super::*;

    struct Stat {
        fs_type: String,
        total: u64,
        available: u64,
        read_only: bool,
        local: bool,
    }

    fn stat(path: &Path) -> Option<Stat> {
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c` is a NUL-terminated path and `st` is a properly sized, zeroed out
        // structure that statfs fills in.
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        // SAFETY: f_fstypename is a NUL-terminated C string inside the structure.
        let fs_type = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let bsize = st.f_bsize as u64;
        Some(Stat {
            fs_type,
            total: st.f_blocks as u64 * bsize,
            available: st.f_bavail as u64 * bsize,
            read_only: st.f_flags & libc::MNT_RDONLY as u32 != 0,
            local: st.f_flags & libc::MNT_LOCAL as u32 != 0,
        })
    }

    fn volume(mount: PathBuf, label: String) -> Option<Volume> {
        let s = stat(&mount)?;
        Some(Volume {
            kind: kind_of_fs(&s.fs_type, s.local),
            mount_point: mount.clone(),
            label: Some(label),
            fs_type: s.fs_type,
            device: mount.to_string_lossy().into_owned(),
            drive_letter: None,
            total: Some(s.total),
            available: Some(s.available),
            read_only: s.read_only,
            responsive: true,
        })
    }

    pub fn list() -> Result<Vec<Volume>> {
        let mut out = Vec::new();
        // The startup disk is "/"; /Volumes holds a link to it, named after it.
        let mut startup = String::from("Macintosh HD");
        let mut others: Vec<(PathBuf, String)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/Volumes") {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let p = e.path();
                match std::fs::canonicalize(&p) {
                    Ok(real) if real == Path::new("/") => startup = name,
                    Ok(_) => others.push((p, name)),
                    Err(_) => {}
                }
            }
        }
        out.extend(volume(PathBuf::from("/"), startup));
        others.sort_by(|a, b| a.1.cmp(&b.1));
        out.extend(others.into_iter().filter_map(|(p, n)| volume(p, n)));
        Ok(out)
    }
}

impl VolumeLister for MacVolumes {
    /// The startup disk and the disks the system never lets go of are not ejectable.
    fn can_eject(&self, v: &Volume) -> bool {
        v.mount_point != Path::new("/") && v.mount_point.starts_with("/Volumes")
    }

    fn eject(&self, v: &Volume) -> Result<()> {
        super::eject::eject_with_diskutil(&v.mount_point)
    }

    #[cfg(target_os = "macos")]
    fn list(&self) -> Result<Vec<Volume>> {
        native_volumes::list()
    }
    #[cfg(not(target_os = "macos"))]
    fn list(&self) -> Result<Vec<Volume>> {
        Err(Error::Unsupported("mounted volumes (statfs)"))
    }
}

struct MacOpener {
    /// Off for the platform tests use: no test starts a window of the system.
    quick_look: bool,
}

#[cfg(unix)]
mod native_open {
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use super::*;
    use crate::platform::split_command;

    fn detach(mut cmd: Command, op: &'static str, subject: &Path) -> Result<()> {
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| Error::io(op, subject, e))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    pub fn open(path: &Path) -> Result<()> {
        let mut c = Command::new("open");
        c.arg(path);
        detach(c, "start open for", path)
    }

    pub fn reveal(path: &Path) -> Result<()> {
        let mut c = Command::new("open");
        c.arg("-R").arg(path);
        detach(c, "start open -R for", path)
    }

    pub fn open_with(path: &Path, program: &str) -> Result<()> {
        let words = split_command(program);
        let Some((first, rest)) = words.split_first() else {
            return Err(Error::Invalid(
                "name an application, for example TextEdit or \"Visual Studio Code\"".into(),
            ));
        };
        let mut c;
        if rest.is_empty() && !first.contains('/') {
            // An application's name, as `open -a` takes it.
            c = Command::new("open");
            c.arg("-a").arg(first).arg(path);
        } else {
            c = Command::new(first);
            c.args(rest).arg(path);
        }
        detach(c, "start the program for", path)
    }

    /// `qlmanage -p`: the preview window of the Finder's Quick Look, for as long as it is open.
    pub fn quick_look(path: &Path) -> Result<()> {
        let mut c = Command::new("qlmanage");
        c.arg("-p").arg(path);
        detach(c, "start Quick Look for", path)
    }

    pub fn open_terminal(dir: &Path) -> Result<()> {
        let app = terminal_app(std::env::var("TERM_PROGRAM").ok().as_deref());
        let mut c = Command::new("open");
        c.arg("-a").arg(app).arg(dir);
        detach(c, "start the terminal in", dir)
    }
}

impl Opener for MacOpener {
    #[cfg(unix)]
    fn open(&self, path: &Path) -> Result<()> {
        native_open::open(path)
    }
    #[cfg(not(unix))]
    fn open(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("open(1)"))
    }
    #[cfg(unix)]
    fn reveal(&self, path: &Path) -> Result<()> {
        native_open::reveal(path)
    }
    #[cfg(not(unix))]
    fn reveal(&self, _: &Path) -> Result<()> {
        Err(Error::Unsupported("open -R"))
    }
    #[cfg(unix)]
    fn open_with(&self, path: &Path, program: &str) -> Result<()> {
        native_open::open_with(path, program)
    }
    #[cfg(unix)]
    fn open_terminal(&self, dir: &Path) -> Result<()> {
        native_open::open_terminal(dir)
    }
    fn can_quick_look(&self) -> bool {
        cfg!(target_os = "macos") && self.quick_look
    }
    #[cfg(unix)]
    fn quick_look(&self, path: &Path) -> Result<()> {
        native_open::quick_look(path)
    }
}

#[cfg(unix)]
type PlatformAttrs = MacAttributes;
#[cfg(not(unix))]
type PlatformAttrs = super::windows::WindowsAttributes;

#[cfg(unix)]
fn attrs() -> PlatformAttrs {
    MacAttributes
}
#[cfg(not(unix))]
fn attrs() -> PlatformAttrs {
    super::windows::WindowsAttributes
}

pub struct MacPlatform {
    dirs: Dirs,
    attrs: PlatformAttrs,
    trash: Box<dyn TrashBackend>,
    volumes: MacVolumes,
    opener: MacOpener,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    tags: super::tags::XattrTags,
    /// Tests: the standard folders come from `user-dirs.dirs` in the sandbox.
    sandbox_user_dirs: bool,
}

impl MacPlatform {
    pub fn new(dirs: Dirs) -> Self {
        let home_trash = dirs.home.join(".Trash");
        #[cfg(unix)]
        let uid = {
            // SAFETY: getuid has no preconditions and cannot fail.
            unsafe { libc::getuid() }
        };
        #[cfg(not(unix))]
        let uid = 0;
        let trash = DirTrash::new(home_trash, Arc::new(LocalFs))
            .with_volume_trash(move |p| volume_trash_of(p, uid));
        Self::with_trash(dirs, Box::new(trash))
    }

    /// A platform whose trash is `trash` (the tests give it a folder of their own).
    pub fn with_trash(dirs: Dirs, trash: Box<dyn TrashBackend>) -> Self {
        MacPlatform {
            dirs,
            attrs: attrs(),
            trash,
            volumes: MacVolumes,
            opener: MacOpener { quick_look: true },
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            tags: super::tags::XattrTags::finder(),
            sandbox_user_dirs: false,
        }
    }

    /// The standard folders are those `user-dirs.dirs` names in the config folder (the
    /// Finder's names without it): for tests.
    pub fn with_sandbox_user_dirs(mut self) -> Self {
        self.sandbox_user_dirs = true;
        self.opener.quick_look = false;
        self
    }
}

/// Alias used by `platform::current` on systems without a dedicated implementation.
pub type GenericUnixStub = MacPlatform;

impl Platform for MacPlatform {
    fn name(&self) -> &'static str {
        "macos"
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
        // Extended attributes, resource forks and ACLs are not carried over yet; the plan
        // says so before anything is copied. The Finder's tags are the exception.
        &MacFidelity
    }
    fn tags(&self) -> Option<&dyn super::TagStore> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Some(&self.tags)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            None
        }
    }
    fn user_dirs(&self) -> UserDirs {
        if self.sandbox_user_dirs {
            return UserDirs::from_xdg_file(&self.dirs.config, &self.dirs.home)
                .unwrap_or_else(|| UserDirs::conventional(&self.dirs.home, "Videos"));
        }
        // The Finder's standard folders: they have fixed names under the home folder.
        UserDirs::conventional(&self.dirs.home, "Movies")
    }
    fn display_names(&self, places: &[std::path::PathBuf]) -> Vec<(std::path::PathBuf, String)> {
        if self.sandbox_user_dirs {
            return Vec::new();
        }
        name_candidates(places, &self.dirs.home)
            .into_iter()
            .filter(|p| p.exists())
            .filter_map(|p| native::display_name(&p).and_then(|n| only_different(&p, n)))
            .collect()
    }
    fn extra_places(&self) -> Vec<(PlaceKind, std::path::PathBuf)> {
        let cloud = icloud_drive(&self.dirs.home);
        if cloud.is_dir() {
            vec![(PlaceKind::ICloud, cloud)]
        } else {
            Vec::new()
        }
    }
    fn explain_denied(&self, path: &Path, err: &std::io::Error) -> Option<AccessDenied> {
        super::access::explain(
            path,
            err.raw_os_error(),
            std::env::var("TERM_PROGRAM").ok().as_deref(),
            &self.dirs.home,
        )
    }
    fn path_rules(&self) -> PathRules {
        PathRules {
            case_insensitive: true,
            normalization_insensitive: true,
            forbidden_chars: &['/', '\0'],
            reserved_names: &[],
            max_name_bytes: 255,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_on_another_volume_use_that_volumes_trash() {
        assert_eq!(
            volume_trash_of(Path::new("/Volumes/Data/photos/a.jpg"), 501),
            Some(Path::new("/Volumes/Data/.Trashes/501").to_path_buf())
        );
        assert_eq!(volume_trash_of(Path::new("/Users/me/a.txt"), 501), None);
        assert_eq!(volume_trash_of(Path::new("/Volumes/Data"), 501), None);
    }

    #[test]
    fn the_terminal_in_use_is_the_one_opened() {
        assert_eq!(terminal_app(Some("iTerm.app")), "iTerm");
        assert_eq!(terminal_app(Some("ghostty")), "Ghostty");
        assert_eq!(terminal_app(Some("Apple_Terminal")), "Terminal");
        assert_eq!(terminal_app(None), "Terminal");
    }

    #[cfg(unix)]
    fn meta_with(kind: FileKind, flags: Option<u32>) -> FsMeta {
        FsMeta {
            kind,
            special: None,
            size: 0,
            mtime: None,
            atime: None,
            ctime: None,
            btime: None,
            mode: Some(0o755),
            dev: None,
            ino: None,
            nlink: None,
            uid: None,
            gid: None,
            blocks: None,
            readonly: false,
            os_attrs: flags,
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_finder_hides_dot_files_and_folders_flagged_hidden() {
        use std::ffi::OsStr;
        let a = MacAttributes;
        let hidden = |name: &str, flags: Option<u32>| {
            a.attrs(OsStr::new(name), &meta_with(FileKind::Dir, flags))
                .hidden
        };
        // The files macOS litters folders with are dot files.
        for name in [
            ".DS_Store",
            "._photo.jpg",
            ".localized",
            ".Trashes",
            ".fseventsd",
        ] {
            assert!(hidden(name, Some(0)), "{name}");
        }
        // ~/Library has an ordinary name and the hidden flag.
        assert!(hidden("Library", Some(UF_HIDDEN | 0x40)));
        assert!(!hidden("Library", Some(0x40)));
        assert!(!hidden("Documents", Some(0)));
        assert!(!hidden("Documents", None));
    }

    #[test]
    fn a_folder_the_finder_marked_as_a_bundle_is_a_package() {
        let mut info = [0u8; 32];
        assert!(!finder_info_marks_bundle(&info));
        info[8] = 0x20; // kHasBundle, high byte of the flags
        assert!(finder_info_marks_bundle(&info));
        info[8] = 0x40; // some other flag
        assert!(!finder_info_marks_bundle(&info));
        assert!(!finder_info_marks_bundle(&[0u8; 4]));
    }

    #[test]
    fn only_names_that_differ_from_the_name_on_disk_are_kept() {
        assert_eq!(
            only_different(Path::new("/Users/a/Desktop"), "Scrivania".into()),
            Some((
                std::path::PathBuf::from("/Users/a/Desktop"),
                "Scrivania".into()
            ))
        );
        assert_eq!(
            only_different(Path::new("/Users/a/Desktop"), "Desktop".into()),
            None
        );
        assert_eq!(
            only_different(Path::new("/"), "Macintosh HD".into())
                .unwrap()
                .1,
            "Macintosh HD"
        );
        assert_eq!(only_different(Path::new("/"), "/".into()), None);
        assert_eq!(only_different(Path::new("/x"), String::new()), None);
    }

    #[test]
    fn the_places_looked_up_include_the_startup_disk_and_icloud() {
        let home = Path::new("/Users/a");
        let got = name_candidates(&[home.join("Desktop"), home.to_path_buf()], home);
        assert!(got.contains(&std::path::PathBuf::from("/")));
        assert!(got.contains(&std::path::PathBuf::from("/Applications")));
        assert!(got.contains(&icloud_drive(home)));
        // No duplicates.
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), got.len());
    }

    #[test]
    fn filesystems_are_classed_as_kinds_of_volume() {
        assert_eq!(kind_of_fs("apfs", true), VolumeKind::Fixed);
        assert_eq!(kind_of_fs("exfat", true), VolumeKind::Removable);
        assert_eq!(kind_of_fs("smbfs", true), VolumeKind::Network);
        assert_eq!(kind_of_fs("apfs", false), VolumeKind::Network);
        assert_eq!(kind_of_fs("cd9660", true), VolumeKind::Optical);
    }
}
