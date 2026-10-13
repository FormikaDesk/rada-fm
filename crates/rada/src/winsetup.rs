//! Windows: shortcuts and a Windows Terminal profile for rada.
//!
//! * A shortcut in the Start menu (and with `--desktop` on the Desktop) with rada's icon.
//! * With Windows Terminal installed: a profile "rada" written as a *fragment*
//!   (`%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\rada\rada.json`, the way
//!   Windows Terminal asks programs to add profiles, with no edit of the person's own
//!   settings), and the shortcut opens that profile. The profile names the icon, the font
//!   (when the Nerd Font is installed: a missing font makes Windows Terminal complain) and
//!   its size.
//! * Without it: the shortcut starts `rada.exe` directly, whose icon is built into the program.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const ICON: &[u8] = include_bytes!("../assets/windows/rada.ico");

/// The profile's identity in Windows Terminal (fixed, so that a second setup updates it).
pub const PROFILE_GUID: &str = "{b4e5a2f0-5d3c-5c61-9a07-2f6d1e8c0b43}";

/// The Nerd Font rada asks Windows Terminal for when it is installed.
pub const FONT_FACE: &str = "JetBrainsMono Nerd Font Mono";

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    paths: Vec<PathBuf>,
}

pub struct WinSetup {
    /// `%APPDATA%\Microsoft\Windows\Start Menu\Programs`.
    pub start_menu: PathBuf,
    pub desktop_dir: PathBuf,
    /// `%LOCALAPPDATA%`.
    pub local_app_data: PathBuf,
    /// `rada.exe`.
    pub exe: PathBuf,
    pub state_dir: PathBuf,
    /// `wt.exe`, when Windows Terminal is installed.
    pub windows_terminal: Option<PathBuf>,
    /// The Nerd Font has been installed by `rada setup font`.
    pub font_installed: bool,
}

/// Where Windows Terminal is, under a local application data folder: the alias Windows puts in
/// `WindowsApps` for the Store version.
pub fn find_windows_terminal(local_app_data: &Path) -> Option<PathBuf> {
    let alias = local_app_data.join("Microsoft/WindowsApps/wt.exe");
    alias.is_file().then_some(alias)
}

impl WinSetup {
    fn icon_path(&self) -> PathBuf {
        self.local_app_data.join("rada").join("rada.ico")
    }

    fn fragment_path(&self) -> PathBuf {
        self.local_app_data
            .join("Microsoft/Windows Terminal/Fragments/rada/rada.json")
    }

    fn shortcut_in(&self, dir: &Path) -> PathBuf {
        dir.join("rada.lnk")
    }

    fn manifest_path(&self) -> PathBuf {
        self.state_dir.join("windows-install.json")
    }

    /// What the shortcut starts and with what arguments, and the icon it shows.
    pub fn shortcut_plan(&self) -> ShortcutPlan {
        match &self.windows_terminal {
            Some(wt) => ShortcutPlan {
                target: wt.clone(),
                arguments: "-p \"rada\"".into(),
                icon: self.icon_path(),
                icon_index: 0,
            },
            None => ShortcutPlan {
                target: self.exe.clone(),
                arguments: String::new(),
                icon: self.exe.clone(),
                icon_index: 0,
            },
        }
    }

    pub fn install(&self, desktop: bool) -> Result<Vec<String>> {
        let mut say = Vec::new();
        let mut written: Vec<PathBuf> = Vec::new();
        write(&self.icon_path(), ICON)?;
        written.push(self.icon_path());

        if self.windows_terminal.is_some() {
            let frag = self.fragment_path();
            write(
                &frag,
                fragment_json(&self.exe, &self.icon_path(), self.font_installed).as_bytes(),
            )?;
            say.push(format!(
                "added the Windows Terminal profile \"rada\": {}",
                frag.display()
            ));
            written.push(frag);
        } else {
            say.push(
                "Windows Terminal is not installed: the shortcut starts rada.exe directly".into(),
            );
        }

        let plan = self.shortcut_plan();
        let mut dirs = vec![self.start_menu.clone()];
        if desktop {
            dirs.push(self.desktop_dir.clone());
        }
        for dir in dirs {
            let lnk = self.shortcut_in(&dir);
            make_shortcut(&lnk, &plan, &self.exe)?;
            say.push(format!("made the shortcut {}", lnk.display()));
            written.push(lnk);
        }
        if self.windows_terminal.is_some() && !self.font_installed {
            say.push(format!(
                "for the icons, run `rada setup font` and then `rada setup desktop` again: the profile then asks for {FONT_FACE}"
            ));
        }
        let mut manifest = self.read_manifest();
        for p in written {
            if !manifest.paths.contains(&p) {
                manifest.paths.push(p);
            }
        }
        self.save_manifest(&manifest)?;
        Ok(say)
    }

    pub fn remove(&self) -> Result<Vec<String>> {
        let mut say = Vec::new();
        let mut paths = self.read_manifest().paths;
        for known in [
            self.shortcut_in(&self.start_menu),
            self.shortcut_in(&self.desktop_dir),
            self.fragment_path(),
            self.icon_path(),
        ] {
            if !paths.contains(&known) {
                paths.push(known);
            }
        }
        for p in paths {
            match std::fs::remove_file(&p) {
                Ok(()) => say.push(format!("removed {}", p.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => say.push(format!("could not remove {}: {e}", p.display())),
            }
        }
        // The folders rada made, if nothing else is in them.
        for dir in [
            self.fragment_path().parent().map(Path::to_path_buf),
            self.icon_path().parent().map(Path::to_path_buf),
        ]
        .into_iter()
        .flatten()
        {
            let _ = std::fs::remove_dir(dir);
        }
        let _ = std::fs::remove_file(self.manifest_path());
        if say.is_empty() {
            say.push("nothing to remove".into());
        }
        Ok(say)
    }

    fn read_manifest(&self) -> Manifest {
        std::fs::read_to_string(self.manifest_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_manifest(&self, m: &Manifest) -> Result<()> {
        write(
            &self.manifest_path(),
            serde_json::to_string_pretty(m)?.as_bytes(),
        )
    }
}

/// What a shortcut points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutPlan {
    pub target: PathBuf,
    pub arguments: String,
    pub icon: PathBuf,
    pub icon_index: i32,
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

/// The fragment that adds the profile.
pub fn fragment_json(exe: &Path, icon: &Path, font_installed: bool) -> String {
    let mut profile = serde_json::json!({
        "guid": PROFILE_GUID,
        "name": "rada",
        "commandline": format!("\"{}\"", exe.display()),
        "icon": icon.display().to_string(),
        "startingDirectory": "%USERPROFILE%",
        "tabTitle": "rada",
        "suppressApplicationTitle": true,
        "closeOnExit": "graceful",
    });
    if font_installed {
        profile["font"] = serde_json::json!({ "face": FONT_FACE, "size": 12 });
    }
    serde_json::to_string_pretty(&serde_json::json!({ "profiles": [profile] }))
        .expect("a value built here serialises")
        + "\n"
}

#[cfg(windows)]
fn make_shortcut(lnk: &Path, plan: &ShortcutPlan, exe: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface, PCWSTR};

    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() };
    // SAFETY: COM is initialised for this thread and released at the end; every string
    // passed lives until the call returns.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<()> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(PCWSTR(wide(&plan.target).as_ptr()))?;
            link.SetArguments(&HSTRING::from(plan.arguments.as_str()))?;
            link.SetIconLocation(PCWSTR(wide(&plan.icon).as_ptr()), plan.icon_index)?;
            link.SetDescription(&HSTRING::from("rada: a safe harbor for your files"))?;
            if let Some(dir) = exe.parent() {
                link.SetWorkingDirectory(PCWSTR(wide(dir).as_ptr()))?;
            }
            let file: IPersistFile = link.cast()?;
            file.Save(PCWSTR(wide(lnk).as_ptr()), true)?;
            Ok(())
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result.map_err(|e| anyhow::anyhow!("cannot make {}: {e}", lnk.display()))
    }
}

#[cfg(not(windows))]
fn make_shortcut(lnk: &Path, _plan: &ShortcutPlan, _exe: &Path) -> Result<()> {
    anyhow::bail!("shortcuts are made on Windows only ({})", lnk.display())
}

/// Read a shortcut back: what it starts, with what arguments (tests).
#[cfg(all(windows, test))]
pub fn read_shortcut(lnk: &Path) -> Result<ShortcutPlan> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IPersistFile, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{Interface, PCWSTR};

    let wide: Vec<u16> = lnk.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: as in `make_shortcut`; the buffers are as long as the sizes passed with them.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = (|| -> windows::core::Result<ShortcutPlan> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            let file: IPersistFile = link.cast()?;
            file.Load(PCWSTR(wide.as_ptr()), STGM_READ)?;
            let mut path = [0u16; 520];
            let mut find = WIN32_FIND_DATAW::default();
            link.GetPath(&mut path, &mut find, 0)?;
            let mut args = [0u16; 520];
            link.GetArguments(&mut args)?;
            let mut icon = [0u16; 520];
            let mut index = 0i32;
            link.GetIconLocation(&mut icon, &mut index)?;
            let text = |b: &[u16]| {
                let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
                std::ffi::OsString::from_wide(&b[..end])
            };
            Ok(ShortcutPlan {
                target: PathBuf::from(text(&path)),
                arguments: text(&args).to_string_lossy().into_owned(),
                icon: PathBuf::from(text(&icon)),
                icon_index: index,
            })
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result.map_err(|e| anyhow::anyhow!("cannot read {}: {e}", lnk.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(root: &Path, wt: bool, font: bool) -> WinSetup {
        let local = root.join("Local");
        if wt {
            std::fs::create_dir_all(local.join("Microsoft/WindowsApps")).unwrap();
            std::fs::write(local.join("Microsoft/WindowsApps/wt.exe"), "").unwrap();
        }
        WinSetup {
            start_menu: root.join("Programs"),
            desktop_dir: root.join("Desktop"),
            windows_terminal: find_windows_terminal(&local),
            local_app_data: local,
            exe: PathBuf::from("C:\\Tools\\rada.exe"),
            state_dir: root.join("state"),
            font_installed: font,
        }
    }

    #[test]
    fn the_fragment_is_a_profile_with_icon_and_font_only_when_the_font_is_there() {
        let exe = Path::new("C:\\Tools\\rada.exe");
        let icon = Path::new("C:\\Users\\me\\AppData\\Local\\rada\\rada.ico");
        let v: serde_json::Value =
            serde_json::from_str(&fragment_json(exe, icon, true)).expect("valid JSON");
        let p = &v["profiles"][0];
        assert_eq!(p["name"], "rada");
        assert_eq!(p["guid"], PROFILE_GUID);
        assert_eq!(p["commandline"], "\"C:\\Tools\\rada.exe\"");
        assert_eq!(p["icon"], icon.display().to_string());
        assert_eq!(p["font"]["face"], "JetBrainsMono Nerd Font Mono");
        assert_eq!(p["font"]["size"], 12);
        assert_eq!(p["startingDirectory"], "%USERPROFILE%");
        // No font installed: no font named, so Windows Terminal has nothing to complain about.
        let v: serde_json::Value = serde_json::from_str(&fragment_json(exe, icon, false)).unwrap();
        assert!(v["profiles"][0].get("font").is_none());
        // The GUID is in the braces form Windows Terminal wants.
        assert!(
            PROFILE_GUID.starts_with('{')
                && PROFILE_GUID.ends_with('}')
                && PROFILE_GUID.len() == 38
        );
    }

    #[test]
    fn with_windows_terminal_the_shortcut_opens_the_profile() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path(), true, false);
        let plan = s.shortcut_plan();
        assert_eq!(
            plan.target,
            d.path().join("Local/Microsoft/WindowsApps/wt.exe")
        );
        assert_eq!(plan.arguments, "-p \"rada\"");
        assert_eq!(plan.icon, d.path().join("Local/rada/rada.ico"));
    }

    #[test]
    fn without_windows_terminal_the_shortcut_starts_rada_with_the_icon_built_into_it() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path(), false, false);
        let plan = s.shortcut_plan();
        assert_eq!(plan.target, Path::new("C:\\Tools\\rada.exe"));
        assert_eq!(plan.arguments, "");
        assert_eq!(plan.icon, Path::new("C:\\Tools\\rada.exe"));
        assert!(find_windows_terminal(&d.path().join("Local")).is_none());
    }

    #[test]
    fn the_embedded_icon_is_the_logo_at_every_size() {
        let entries = crate::icons::ico_entries(ICON).expect("a well-formed icon");
        assert_eq!(entries.len(), crate::icons::ICO_SIZES.len());
    }

    /// The part of setup that is files only, tried where there are no shortcuts to make.
    #[cfg(not(windows))]
    #[test]
    fn shortcuts_are_a_windows_thing_and_other_systems_say_so() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path(), true, true);
        let err = s.install(true).unwrap_err().to_string();
        assert!(err.contains("Windows only"), "{err}");
        // The profile and the icon were written before that; remove takes them away.
        assert!(
            d.path()
                .join("Local/Microsoft/Windows Terminal/Fragments/rada/rada.json")
                .is_file()
        );
        assert!(d.path().join("Local/rada/rada.ico").is_file());
        let say = s.remove().unwrap();
        assert!(say.iter().any(|l| l.contains("rada.json")), "{say:?}");
        assert!(
            !d.path()
                .join("Local/Microsoft/Windows Terminal/Fragments/rada")
                .exists()
        );
        assert!(!d.path().join("Local/rada").exists());
        assert_eq!(s.remove().unwrap(), ["nothing to remove"]);
    }

    #[cfg(windows)]
    #[test]
    fn setup_makes_shortcuts_a_profile_and_remove_takes_them_away() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path(), true, true);
        let say = s.install(true).unwrap();
        assert!(say.iter().any(|l| l.contains("rada.lnk")), "{say:?}");
        let lnk = d.path().join("Programs/rada.lnk");
        let back = read_shortcut(&lnk).unwrap();
        assert_eq!(back, s.shortcut_plan());
        assert!(d.path().join("Desktop/rada.lnk").is_file());
        let frag = std::fs::read_to_string(
            d.path()
                .join("Local/Microsoft/Windows Terminal/Fragments/rada/rada.json"),
        )
        .unwrap();
        assert!(frag.contains("JetBrainsMono Nerd Font Mono"));
        s.remove().unwrap();
        assert!(!lnk.exists());
        assert!(!d.path().join("Desktop/rada.lnk").exists());
        assert!(!d.path().join("Local/rada").exists());
    }

    #[cfg(windows)]
    #[test]
    fn without_windows_terminal_the_shortcut_points_at_the_program_itself() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path(), false, false);
        s.install(false).unwrap();
        let back = read_shortcut(&d.path().join("Programs/rada.lnk")).unwrap();
        assert_eq!(back.target, Path::new("C:\\Tools\\rada.exe"));
        assert!(!d.path().join("Local/Microsoft").exists());
    }
}
