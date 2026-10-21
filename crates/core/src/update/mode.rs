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
    /// Installed by a package manager (or `cargo install rada`): rada touches nothing and says
    /// which tool to use.
    Package { hint: String },
}

/// Whether `cargo install rada` put this program where it is. Cargo writes what it installed
/// in `.crates.toml` next to the `bin` folder (`~/.cargo`, or the `--root` that was given), one
/// line per package: `"rada 0.6.0 (registry+https://...)" = ["rada"]`. A `registry+` source is
/// the crate from crates.io; `path+` and `git+` (an install from a checkout) are not, and a
/// program that was only copied into `~/.cargo/bin` has no line at all: those go on as before.
fn installed_by_cargo(exe: &Path) -> bool {
    let Some(root) = exe.parent().and_then(Path::parent) else {
        return false;
    };
    if exe.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new("bin")) {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(root.join(".crates.toml")) else {
        return false;
    };
    text.lines()
        .map(str::trim_start)
        .any(|l| l.starts_with("\"rada ") && l.contains(" (registry+"))
}

/// The command that updates a program installed with cargo. Cargo does not pick a pre-release
/// by itself, so while the newer version is one (it has a `-`) the command names it; a stable
/// version, or none known, needs no `--version`.
fn cargo_command(newer: Option<&str>) -> String {
    match newer {
        Some(v) if v.contains('-') => format!("cargo install rada --locked --version {v}"),
        _ => "cargo install rada --locked".to_string(),
    }
}

/// Folders that belong to a package manager or to the system. A program there is not ours to
/// replace. `newer` is the version the update is about, when one is known.
fn package_hint(exe: &Path, os_id: Option<&str>, newer: Option<&str>) -> Option<String> {
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
    if installed_by_cargo(exe) {
        return Some(tf(Key::UpHintCargo, &[&cargo_command(newer)]));
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
/// `source_dir` is the folder the user named; `newer` is the version being updated to, when
/// one is known (a copy installed with cargo is told to name it while it is a pre-release).
pub fn detect(
    method: Method,
    exe: &Path,
    source_dir: Option<&Path>,
    os_id: Option<&str>,
    newer: Option<&str>,
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
            hint: package_hint(exe, os_id, newer)
                .unwrap_or_else(|| Key::UpHintDefault.tr().to_string()),
        }),
        Method::Auto => {
            if let Some(hint) = package_hint(exe, os_id, newer) {
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
        detect(Method::Auto, Path::new(exe), None, os, None).unwrap()
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

    /// A cargo home in a temporary folder with `.crates.toml` saying `lines`, and the path of
    /// the `rada` in its `bin`.
    fn cargo_home(lines: &str) -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join(".cargo/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            d.path().join(".cargo/.crates.toml"),
            format!("[v1]\n{lines}"),
        )
        .unwrap();
        let exe = bin.join("rada");
        (d, exe)
    }

    #[test]
    fn a_program_installed_with_cargo_is_left_to_cargo() {
        let (_d, exe) = cargo_home(
            "\"rada 0.6.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rada\"]\n",
        );
        match auto(exe.to_str().unwrap(), None) {
            Mode::Package { hint } => {
                assert!(hint.contains("cargo install rada --locked"), "{hint}")
            }
            other => panic!("{other:?}"),
        }
        // Chosen by hand as a package, it says the same.
        match detect(Method::Package, &exe, None, None, None).unwrap() {
            Mode::Package { hint } => assert!(hint.contains("cargo install"), "{hint}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cargo_is_told_the_version_only_while_it_is_a_pre_release() {
        let (_d, exe) = cargo_home(
            "\"rada 0.6.0-rc.12 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rada\"]\n",
        );
        let hint =
            |newer: Option<&str>| match detect(Method::Auto, &exe, None, None, newer).unwrap() {
                Mode::Package { hint } => hint,
                other => panic!("{other:?}"),
            };
        // A pre-release: the command names it, in both languages.
        for lang in [crate::i18n::Lang::En, crate::i18n::Lang::It] {
            let h = crate::i18n::with_lang(lang, || hint(Some("0.6.0-rc.13")));
            assert!(
                h.contains("`cargo install rada --locked --version 0.6.0-rc.13`"),
                "{h}"
            );
        }
        // A stable version, or none known: the plain command.
        for newer in [Some("0.6.0"), Some("1.2.3"), None] {
            let h = hint(newer);
            assert!(h.contains("`cargo install rada --locked`"), "{h}");
            assert!(!h.contains("--version"), "{h}");
        }
    }

    #[test]
    fn a_program_cargo_did_not_get_from_crates_io_is_not_cargos() {
        // Built from a checkout (`cargo install --path`), from git, another crate with a name
        // that starts the same, and a program only copied there: all go on as downloaded ones.
        for lines in [
            "\"rada 0.6.0 (path+file:///home/u/rada/crates/rada)\" = [\"rada\"]\n",
            "\"rada 0.6.0 (git+https://example.org/rada#abc)\" = [\"rada\"]\n",
            "\"rada-tools 0.1.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rada-tools\"]\n",
            "",
        ] {
            let (_d, exe) = cargo_home(lines);
            assert_eq!(auto(exe.to_str().unwrap(), None), Mode::Binary, "{lines}");
        }
        // No `.crates.toml` at all.
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        assert_eq!(auto(bin.join("rada").to_str().unwrap(), None), Mode::Binary);
    }

    #[test]
    fn the_cargo_line_counts_only_beside_a_bin_folder() {
        // The file is there, but the program is not in `<root>/bin`.
        let (d, _) = cargo_home(
            "\"rada 0.6.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"rada\"]\n",
        );
        let elsewhere = d.path().join(".cargo/other");
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert_eq!(
            auto(elsewhere.join("rada").to_str().unwrap(), None),
            Mode::Binary
        );
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
            detect(Method::Auto, &exe, None, None, None).unwrap(),
            Mode::Source { dir: root.clone() }
        );
        // The folder the user names wins, wherever the program is.
        let other = d.path().join("elsewhere");
        assert_eq!(
            detect(
                Method::Auto,
                Path::new("/home/u/bin/rada"),
                Some(&other),
                None,
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
            detect(Method::Binary, exe, None, None, None).unwrap(),
            Mode::Binary
        );
        assert!(matches!(
            detect(Method::Package, Path::new("/home/u/rada"), None, None, None).unwrap(),
            Mode::Package { .. }
        ));
        let e = detect(
            Method::Source,
            Path::new("/home/u/bin/rada"),
            None,
            None,
            None,
        )
        .unwrap_err();
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
