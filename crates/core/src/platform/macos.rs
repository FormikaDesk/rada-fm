//! macOS (also the fallback for other Unix systems).
//!
//! * The trash is `~/.Trash` (and `/Volumes/<name>/.Trashes/<uid>` for items on another
//!   volume), where the Finder looks; putting back is rada's own (undo), the Finder's "Put
//!   Back" metadata is not written.
//! * Volumes are the entries of `/Volumes`, sized with `statfs`; the startup disk is `/`.
//! * Opening goes through `open(1)`: the default program, `open -a <App>` for "Open with…",
//!   `open -R` to reveal, and the terminal the user is in (Terminal, iTerm2, Ghostty).
//! * Names are case-insensitive and Unicode-normalisation-insensitive.
//! * Extended attributes and ACLs are not carried over by a copy yet: the plan says so.

use std::path::Path;
use std::sync::Arc;

use super::dirtrash::DirTrash;
use super::trash::TrashBackend;
use super::volumes::{Volume, VolumeKind, VolumeLister};
use super::{Dirs, FileAttributes, Opener, PathRules, Platform, UserDirs};
use crate::fs::LocalFs;
use crate::{Error, Result};

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
    #[cfg(target_os = "macos")]
    fn list(&self) -> Result<Vec<Volume>> {
        native_volumes::list()
    }
    #[cfg(not(target_os = "macos"))]
    fn list(&self) -> Result<Vec<Volume>> {
        Err(Error::Unsupported("mounted volumes (statfs)"))
    }
}

struct MacOpener;

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
}

#[cfg(unix)]
type Attrs = super::unix::UnixAttributes;
#[cfg(not(unix))]
type Attrs = super::windows::WindowsAttributes;

#[cfg(unix)]
fn attrs() -> Attrs {
    super::unix::UnixAttributes
}
#[cfg(not(unix))]
fn attrs() -> Attrs {
    super::windows::WindowsAttributes
}

pub struct MacPlatform {
    dirs: Dirs,
    attrs: Attrs,
    trash: Box<dyn TrashBackend>,
    volumes: MacVolumes,
    opener: MacOpener,
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
            opener: MacOpener,
            sandbox_user_dirs: false,
        }
    }

    /// The standard folders are those `user-dirs.dirs` names in the config folder (the
    /// Finder's names without it): for tests.
    pub fn with_sandbox_user_dirs(mut self) -> Self {
        self.sandbox_user_dirs = true;
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
        // says so before anything is copied.
        &super::fidelity::NotYet
    }
    fn user_dirs(&self) -> UserDirs {
        if self.sandbox_user_dirs {
            return UserDirs::from_xdg_file(&self.dirs.config, &self.dirs.home)
                .unwrap_or_else(|| UserDirs::conventional(&self.dirs.home, "Videos"));
        }
        // The Finder's standard folders: they have fixed names under the home folder.
        UserDirs::conventional(&self.dirs.home, "Movies")
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

    #[test]
    fn filesystems_are_classed_as_kinds_of_volume() {
        assert_eq!(kind_of_fs("apfs", true), VolumeKind::Fixed);
        assert_eq!(kind_of_fs("exfat", true), VolumeKind::Removable);
        assert_eq!(kind_of_fs("smbfs", true), VolumeKind::Network);
        assert_eq!(kind_of_fs("apfs", false), VolumeKind::Network);
        assert_eq!(kind_of_fs("cd9660", true), VolumeKind::Optical);
    }
}
