//! `rada setup …` on macOS and Windows, run as the real program with HOME and the application
//! data folders inside a temporary folder. Nothing here touches the real Applications folder,
//! Start menu, Desktop, fonts or registry, and nothing is downloaded.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Sandbox {
        let root = tempfile::Builder::new()
            .prefix("rada-setup-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        for d in [
            "home", "data", "config", "state", "cache", "appdata", "local",
        ] {
            fs::create_dir_all(root.path().join(d)).unwrap();
        }
        Sandbox { root }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn rada(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rada"))
            .args(args)
            .env("LC_ALL", "en_US.UTF-8")
            .env("HOME", self.p("home"))
            .env("USERPROFILE", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("config"))
            .env("XDG_DATA_HOME", self.p("data"))
            .env("XDG_STATE_HOME", self.p("state"))
            .env("XDG_CACHE_HOME", self.p("cache"))
            .env("APPDATA", self.p("appdata"))
            .env("LOCALAPPDATA", self.p("local"))
            .output()
            .unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn removing_a_font_that_was_never_installed_is_a_quiet_nothing() {
    let sb = Sandbox::new();
    let out = sb.rada(&["setup", "font", "--remove"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("nothing to remove"), "{}", text(&out));
}

#[cfg(target_os = "macos")]
#[test]
fn setup_desktop_makes_rada_app_in_the_users_applications_and_remove_takes_it_away() {
    let sb = Sandbox::new();
    let out = sb.rada(&["setup", "desktop"]);
    assert!(out.status.success(), "{}", text(&out));
    let app = sb.p("home/Applications/Rada.app/Contents");
    for f in [
        "Info.plist",
        "MacOS/Rada",
        "Resources/rada.icns",
        "Resources/rada-ghostty.conf",
    ] {
        assert!(app.join(f).is_file(), "{f}");
    }
    // The launcher points at the program that made it.
    let launcher = fs::read_to_string(app.join("MacOS/Rada")).unwrap();
    assert!(launcher.contains(env!("CARGO_BIN_EXE_rada")), "{launcher}");
    assert!(text(&out).contains("Dock"), "{}", text(&out));
    assert!(
        !sb.p("home/Desktop").exists(),
        "no Desktop link unless asked"
    );

    let out = sb.rada(&["setup", "desktop", "--desktop"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(fs::symlink_metadata(sb.p("home/Desktop/Rada")).is_ok());

    let out = sb.rada(&["setup", "desktop", "--remove"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!sb.p("home/Applications/Rada.app").exists());
    assert!(fs::symlink_metadata(sb.p("home/Desktop/Rada")).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn on_a_mac_there_is_no_default_file_manager_to_set() {
    let sb = Sandbox::new();
    let out = sb.rada(&["setup", "desktop", "--default-file-manager"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("Linux and Windows"), "{}", text(&out));
}

#[cfg(windows)]
#[test]
fn setup_desktop_makes_a_start_menu_shortcut_and_remove_takes_it_away() {
    let sb = Sandbox::new();
    let out = sb.rada(&["setup", "desktop"]);
    assert!(out.status.success(), "{}", text(&out));
    let lnk = sb.p("appdata/Microsoft/Windows/Start Menu/Programs/rada.lnk");
    assert!(lnk.is_file(), "{}", text(&out));
    // No Windows Terminal in the sandbox: the shortcut starts the program itself, and no
    // profile is written.
    assert!(
        text(&out).contains("Windows Terminal is not installed"),
        "{}",
        text(&out)
    );
    assert!(!sb.p("local/Microsoft/Windows Terminal").exists());
    assert!(sb.p("local/rada/rada.ico").is_file());

    let out = sb.rada(&["setup", "desktop", "--remove"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(!lnk.exists());
    assert!(!sb.p("local/rada").exists());
}
