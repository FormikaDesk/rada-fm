//! How this copy of rada got where it is, and so how it may be updated.

use crate::i18n::{Key, tf};
use std::path::{Path, PathBuf};

use super::Method;

/// What an update does, once the method is settled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A program downloaded from the releases: download, check, swap.
    Binary,
    /// A program built from a git checkout in this folder: pull, build, copy.
    Source { dir: PathBuf },
    /// Installed by a package manager: rada touches nothing and says which tool to use.
    Package { hint: String },
}

/// Folders that belong to a package manager or to the system. A program there is not ours to
/// replace.
fn package_hint(exe: &Path, os_id: Option<&str>) -> Option<String> {
    let p = exe.to_string_lossy().replace('\\', "/");
    let p = p.as_str();
    let has = |s: &str| p.contains(s);
    if has("/Cellar/") || has("/homebrew/") || has("/linuxbrew/") {
        return Some(Key::UpHintBrew.tr().to_string());
    }
    if has("/scoop/apps/") || has("/scoop/shims/") {
        return Some(Key::UpHintScoop.tr().to_string());
    }
    if has("/WindowsApps/") {
        return Some(Key::UpHintStore.tr().to_string());
    }
    if has("/chocolatey/") {
        return Some(Key::UpHintChoco.tr().to_string());
    }
    if has("/nix/store/") {
        return Some(Key::UpHintNix.tr().to_string());
    }
    if has("/snap/") || has("/flatpak/") {
        return Some(Key::UpHintSnap.tr().to_string());
    }
    let system = p.starts_with("/usr/bin/")
        || p.starts_with("/bin/")
        || p.starts_with("/usr/sbin/")
        || p.starts_with("/usr/lib/")
        || p.starts_with("/opt/")
        || p.contains("/Program Files/")
        || p.contains("/Program Files (x86)/");
    if !system {
        return None;
    }
    let tool = match os_id {
        Some("arch" | "manjaro" | "endeavouros" | "cachyos") => Key::UpToolArch.tr(),
        Some("debian" | "ubuntu" | "linuxmint" | "pop" | "raspbian") => {
            "`sudo apt update && sudo apt upgrade`"
        }
        Some("fedora" | "rhel" | "centos" | "rocky" | "almalinux") => "`sudo dnf upgrade`",
        Some("opensuse-tumbleweed" | "opensuse-leap" | "sles") => "`sudo zypper update`",
        Some("alpine") => "`sudo apk upgrade`",
        _ => Key::UpToolPackageManager.tr(),
    };
    Some(tf(
        Key::UpHintSystem,
        &[
            &exe.parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
            &tool,
        ],
    ))
}

/// The git checkout a program was built in: `<checkout>/target/{release,debug}/rada`.
pub fn checkout_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .find(|d| d.join(".git").exists() && d.join("crates/rada/Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

/// The `ID=` of `/etc/os-release` text.
pub fn os_release_id(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("ID="))
        .map(|v| v.trim().trim_matches('"').to_string())
}

/// Settle the mode. `exe` is where the running program is (already followed through links);
/// `source_dir` is the folder the user named.
pub fn detect(
    method: Method,
    exe: &Path,
    source_dir: Option<&Path>,
    os_id: Option<&str>,
) -> Result<Mode, String> {
    let source =
        || -> Result<Mode, String> {
            match source_dir.map(Path::to_path_buf).or_else(|| checkout_of(exe)) {
            Some(dir) => Ok(Mode::Source { dir }),
            None => Err(
                "this rada was not built from a checkout that can be found: set `source_dir` in \
                 the [update] section of config.toml to the folder of the git checkout"
                    .into(),
            ),
        }
        };
    match method {
        Method::Binary => Ok(Mode::Binary),
        Method::Source => source(),
        Method::Package => Ok(Mode::Package {
            hint: package_hint(exe, os_id).unwrap_or_else(|| Key::UpHintDefault.tr().to_string()),
        }),
        Method::Auto => {
            if let Some(hint) = package_hint(exe, os_id) {
                return Ok(Mode::Package { hint });
            }
            if source_dir.is_some() || checkout_of(exe).is_some() {
                return source();
            }
            Ok(Mode::Binary)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(exe: &str, os: Option<&str>) -> Mode {
        detect(Method::Auto, Path::new(exe), None, os).unwrap()
    }

    #[test]
    fn programs_of_a_package_manager_are_left_alone() {
        for (exe, word) in [
            ("/opt/homebrew/Cellar/rada/0.6.0/bin/rada", "brew upgrade"),
            ("/usr/local/Cellar/rada/0.6.0/bin/rada", "brew upgrade"),
            ("/home/linuxbrew/.linuxbrew/bin/rada", "brew upgrade"),
            (
                "C:\\Users\\a\\scoop\\apps\\rada\\current\\rada.exe",
                "scoop update",
            ),
            (
                "C:\\Program Files\\WindowsApps\\x\\rada.exe",
                "Microsoft Store",
            ),
            ("/nix/store/abc-rada/bin/rada", "Nix"),
        ] {
            match auto(exe, None) {
                Mode::Package { hint } => assert!(hint.contains(word), "{exe}: {hint}"),
                other => panic!("{exe}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_system_path_names_the_tool_of_the_distribution() {
        let hint = |os| match auto("/usr/bin/rada", os) {
            Mode::Package { hint } => hint,
            other => panic!("{other:?}"),
        };
        assert!(hint(Some("arch")).contains("pacman"));
        assert!(hint(Some("ubuntu")).contains("apt"));
        assert!(hint(Some("fedora")).contains("dnf"));
        assert!(hint(None).contains("package manager"));
    }

    #[test]
    fn a_downloaded_program_is_updated_by_downloading() {
        assert_eq!(auto("/home/u/.local/bin/rada", None), Mode::Binary);
        assert_eq!(auto("/usr/local/bin/rada", None), Mode::Binary);
        assert_eq!(auto("C:\\Users\\a\\bin\\rada.exe", None), Mode::Binary);
    }

    #[test]
    fn a_program_built_in_a_checkout_is_updated_from_source() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("rada-fm");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("crates/rada")).unwrap();
        std::fs::write(root.join("crates/rada/Cargo.toml"), "").unwrap();
        std::fs::create_dir_all(root.join("target/release")).unwrap();
        let exe = root.join("target/release/rada");
        assert_eq!(checkout_of(&exe), Some(root.clone()));
        assert_eq!(
            detect(Method::Auto, &exe, None, None).unwrap(),
            Mode::Source { dir: root.clone() }
        );
        // The folder the user names wins, wherever the program is.
        let other = d.path().join("elsewhere");
        assert_eq!(
            detect(
                Method::Auto,
                Path::new("/home/u/bin/rada"),
                Some(&other),
                None
            )
            .unwrap(),
            Mode::Source { dir: other }
        );
    }

    #[test]
    fn a_method_chosen_by_hand_is_followed() {
        let exe = Path::new("/usr/bin/rada");
        assert_eq!(
            detect(Method::Binary, exe, None, None).unwrap(),
            Mode::Binary
        );
        assert!(matches!(
            detect(Method::Package, Path::new("/home/u/rada"), None, None).unwrap(),
            Mode::Package { .. }
        ));
        let e = detect(Method::Source, Path::new("/home/u/bin/rada"), None, None).unwrap_err();
        assert!(e.contains("source_dir"), "{e}");
    }

    #[test]
    fn the_distribution_is_read_from_os_release() {
        assert_eq!(
            os_release_id("NAME=\"Arch Linux\"\nID=arch\nID_LIKE=x\n").as_deref(),
            Some("arch")
        );
        assert_eq!(os_release_id("ID=\"ubuntu\"\n").as_deref(), Some("ubuntu"));
        assert_eq!(os_release_id("NAME=x\n"), None);
    }
}
