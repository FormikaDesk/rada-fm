//! When the system says no because the person has not said yes yet.
//!
//! macOS asks the person, once per program, before a terminal may read the Desktop,
//! Documents, Downloads, iCloud Drive, or an external or network volume. Until they answer in
//! System Settings the folders fail with "Operation not permitted", which looks like a
//! broken disk. This is the explanation to give instead.

use std::path::{Path, PathBuf};

/// A refusal that has a fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessDenied {
    /// The program the system has not given access to: the terminal rada runs in.
    pub who: String,
    /// The protected folder or volume (the folder itself, not the deeper one that was opened).
    pub folder: PathBuf,
    /// Its name as the system's list of protected places calls it ("Documents").
    pub folder_name: String,
    /// The page of the system settings where the person can allow it.
    pub settings_url: &'static str,
    /// The page for "Full Disk Access", which allows everything at once.
    pub full_disk_url: &'static str,
}

/// System Settings → Privacy & Security → Files & Folders.
pub const FILES_AND_FOLDERS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";
/// System Settings → Privacy & Security → Full Disk Access.
pub const FULL_DISK_ACCESS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

impl AccessDenied {
    /// "macOS has not given Ghostty access to Documents".
    pub fn message(&self) -> String {
        format!(
            "macOS has not given {} access to {}",
            self.who, self.folder_name
        )
    }

    /// What to do about it, in a few lines.
    pub fn explanation(&self) -> Vec<String> {
        vec![
            format!(
                "Open System Settings → Privacy & Security → Files & Folders and switch {} on under {}.",
                self.who, self.folder_name
            ),
            format!(
                "Or give {} Full Disk Access (System Settings → Privacy & Security → Full Disk Access), which covers every folder at once.",
                self.who
            ),
            "Then quit and start the terminal again.".into(),
        ]
    }
}

/// The program the person knows the terminal as, from `$TERM_PROGRAM`.
pub fn terminal_name(term_program: Option<&str>) -> String {
    match term_program {
        Some("ghostty") => "Ghostty",
        Some("Apple_Terminal") => "Terminal",
        Some("iTerm.app") => "iTerm2",
        Some("WezTerm") => "WezTerm",
        Some("alacritty" | "Alacritty") => "Alacritty",
        Some("kitty") => "kitty",
        Some("vscode") => "Visual Studio Code",
        Some("WarpTerminal") => "Warp",
        Some("Hyper") => "Hyper",
        Some("tmux") | None | Some("") => "your terminal",
        Some(other) => return other.to_string(),
    }
    .to_string()
}

/// The protected place that holds `path`, and the name macOS gives it in its settings.
pub fn protected_place(path: &Path, home: &Path) -> Option<(PathBuf, String)> {
    for name in ["Desktop", "Documents", "Downloads"] {
        let p = home.join(name);
        if path.starts_with(&p) {
            return Some((p, name.to_string()));
        }
    }
    let cloud = home.join("Library/Mobile Documents");
    if path.starts_with(&cloud) {
        return Some((cloud, "iCloud Drive".to_string()));
    }
    // An external disk, a disk image or a network share: /Volumes/<name>.
    let rest = path.strip_prefix("/Volumes").ok()?;
    let volume = rest.components().next()?;
    Some((
        Path::new("/Volumes").join(volume),
        volume.as_os_str().to_string_lossy().into_owned(),
    ))
}

/// Whether failing to open `path` with `os_error` is the system waiting for the person's
/// consent, and if so what to tell them. `EPERM` is what that looks like ("Operation not
/// permitted"); an ordinary missing right is `EACCES` and is not explained this way.
pub fn explain(
    path: &Path,
    os_error: Option<i32>,
    term_program: Option<&str>,
    home: &Path,
) -> Option<AccessDenied> {
    const EPERM: i32 = 1;
    if os_error != Some(EPERM) {
        return None;
    }
    let (folder, folder_name) = protected_place(path, home)?;
    Some(AccessDenied {
        who: terminal_name(term_program),
        folder,
        folder_name,
        settings_url: FILES_AND_FOLDERS_URL,
        full_disk_url: FULL_DISK_ACCESS_URL,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/ale")
    }

    #[test]
    fn the_protected_folders_are_the_ones_macos_asks_about() {
        let p = |s: &str| protected_place(Path::new(s), &home());
        assert_eq!(
            p("/Users/ale/Documents/work/a.txt"),
            Some((PathBuf::from("/Users/ale/Documents"), "Documents".into()))
        );
        assert_eq!(p("/Users/ale/Desktop").unwrap().1, "Desktop");
        assert_eq!(p("/Users/ale/Downloads/x").unwrap().1, "Downloads");
        assert_eq!(
            p("/Users/ale/Library/Mobile Documents/com~apple~CloudDocs/a")
                .unwrap()
                .1,
            "iCloud Drive"
        );
        assert_eq!(
            p("/Volumes/Backup/photos"),
            Some((PathBuf::from("/Volumes/Backup"), "Backup".into()))
        );
        assert_eq!(p("/Users/ale/Pictures"), None);
        assert_eq!(p("/Users/ale"), None);
        assert_eq!(p("/Volumes"), None);
        assert_eq!(p("/tmp/x"), None);
        // Another user's Documents is not this person's to ask about.
        assert_eq!(p("/Users/bob/Documents"), None);
    }

    #[test]
    fn only_operation_not_permitted_is_explained() {
        let d = Path::new("/Users/ale/Documents");
        let got = explain(d, Some(1), Some("ghostty"), &home()).unwrap();
        assert_eq!(
            got.message(),
            "macOS has not given Ghostty access to Documents"
        );
        assert!(got.settings_url.contains("Privacy_FilesAndFolders"));
        assert!(got.full_disk_url.contains("Privacy_AllFiles"));
        // A file mode that says no (EACCES), a missing folder (ENOENT) and unrelated places
        // are not the system waiting for consent.
        assert_eq!(explain(d, Some(13), Some("ghostty"), &home()), None);
        assert_eq!(explain(d, Some(2), Some("ghostty"), &home()), None);
        assert_eq!(explain(d, None, Some("ghostty"), &home()), None);
        assert_eq!(
            explain(Path::new("/etc/hosts"), Some(1), Some("ghostty"), &home()),
            None
        );
    }

    #[test]
    fn the_terminal_is_named_the_way_its_people_name_it() {
        assert_eq!(terminal_name(Some("Apple_Terminal")), "Terminal");
        assert_eq!(terminal_name(Some("iTerm.app")), "iTerm2");
        assert_eq!(terminal_name(Some("vscode")), "Visual Studio Code");
        assert_eq!(terminal_name(None), "your terminal");
        assert_eq!(terminal_name(Some("tmux")), "your terminal");
        assert_eq!(terminal_name(Some("SomethingNew")), "SomethingNew");
        let d = explain(Path::new("/Volumes/USB/x"), Some(1), None, &home()).unwrap();
        assert_eq!(
            d.message(),
            "macOS has not given your terminal access to USB"
        );
        assert_eq!(d.explanation().len(), 3);
    }
}
