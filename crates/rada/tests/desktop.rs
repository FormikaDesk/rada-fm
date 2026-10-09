//! `rada setup desktop` and `rada --spawn-terminal`, run as the real program inside a
//! sandbox: HOME, every XDG folder and PATH point into a temporary folder, and the helper
//! programs (`xdg-mime`, `update-desktop-database`, terminals) are small fake scripts that
//! record how they were called. Nothing here can touch the real desktop configuration.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Sandbox {
        let root = tempfile::Builder::new()
            .prefix("rada-desktop-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        for d in ["home", "data", "config", "state", "cache", "bin"] {
            fs::create_dir_all(root.path().join(d)).unwrap();
        }
        let sb = Sandbox { root };
        // The fake scripts need a few basic tools; PATH holds nothing else, so a real
        // `xdg-mime` or terminal can never be reached from a test.
        for tool in ["touch", "grep", "cut", "mv", "echo", "cat"] {
            if let Some(real) = ["/usr/bin", "/bin"]
                .iter()
                .map(|d| Path::new(d).join(tool))
                .find(|p| p.exists())
            {
                std::os::unix::fs::symlink(real, sb.p("bin").join(tool)).unwrap();
            }
        }
        sb
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn script(&self, name: &str, body: &str) {
        let f = self.p("bin").join(name);
        fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A stand-in for `xdg-mime` that keeps the default folder handler in mimeapps.list,
    /// like the real one, and logs every call.
    fn fake_xdg_mime(&self) {
        let list = self.p("config/mimeapps.list");
        let log = self.p("xdg-mime.log");
        self.script(
            "xdg-mime",
            &format!(
                r#"echo "$@" >> '{log}'
case "$1" in
  query) grep '^inode/directory=' '{list}' 2>/dev/null | cut -d= -f2 ;;
  default)
    touch '{list}'
    grep -v '^inode/directory=' '{list}' > '{list}.tmp'
    if ! grep -q '^\[Default Applications\]' '{list}.tmp'; then echo '[Default Applications]' >> '{list}.tmp'; fi
    echo "inode/directory=$2" >> '{list}.tmp'
    mv '{list}.tmp' '{list}' ;;
esac"#,
                log = log.display(),
                list = list.display()
            ),
        );
    }

    fn fake_update_db(&self) {
        let log = self.p("update-db.log");
        self.script(
            "update-desktop-database",
            &format!(r#"echo "$@" >> '{}'"#, log.display()),
        );
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_rada"));
        c.args(args)
            .env_clear()
            .env("HOME", self.p("home"))
            .env("XDG_DATA_HOME", self.p("data"))
            .env("XDG_CONFIG_HOME", self.p("config"))
            .env("XDG_STATE_HOME", self.p("state"))
            .env("XDG_CACHE_HOME", self.p("cache"))
            .env("PATH", self.p("bin"));
        c
    }

    fn rada(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn rada_with_terminal_var(&self, args: &[&str], term: &str) -> Output {
        self.command(args).env("TERMINAL", term).output().unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

fn wait_for_file(p: &Path) -> String {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        let t = read(p);
        if !t.is_empty() {
            return t;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{} never appeared", p.display());
}

const ICONS: [&str; 4] = [
    "icons/hicolor/scalable/apps/rada.svg",
    "icons/hicolor/48x48/apps/rada.png",
    "icons/hicolor/128x128/apps/rada.png",
    "icons/hicolor/256x256/apps/rada.png",
];

#[test]
fn setup_installs_the_entry_and_icons_for_the_user_and_says_so() {
    let sb = Sandbox::new();
    sb.fake_update_db();
    sb.fake_xdg_mime();
    let o = sb.rada(&["setup", "desktop"]);
    assert!(o.status.success(), "{}", text(&o));
    let out = text(&o);

    let entry = read(&sb.p("data/applications/rada.desktop"));
    assert!(entry.contains("Name=rada"), "{entry}");
    // A plain setup must not claim folders: the entry would become the fallback handler.
    assert!(!entry.contains("MimeType="), "{entry}");
    // The entry names the program by its full path, so launchers need no PATH.
    let exe = env!("CARGO_BIN_EXE_rada");
    assert!(
        entry.contains(&format!("Exec={exe} --spawn-terminal %F")),
        "{entry}"
    );
    for i in ICONS {
        assert!(sb.p("data").join(i).is_file(), "{i} missing");
        assert!(out.contains(i), "{i} not reported:\n{out}");
    }
    assert!(out.contains("rada.desktop"));
    assert!(out.contains("updated the desktop database"), "{out}");
    assert!(read(&sb.p("update-db.log")).contains("data/applications"));
    // Without --default-file-manager nothing about the default handler is touched.
    assert!(
        !read(&sb.p("xdg-mime.log")).contains("default"),
        "xdg-mime was called"
    );
    assert!(!sb.p("config/mimeapps.list").exists());
    // Nothing outside the sandbox's folders: HOME itself stays empty.
    assert_eq!(fs::read_dir(sb.p("home")).unwrap().count(), 0);
}

#[test]
fn setup_still_works_without_the_desktop_database_tool() {
    let sb = Sandbox::new();
    let o = sb.rada(&["setup", "desktop"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("could not update the desktop database"));
    assert!(sb.p("data/applications/rada.desktop").is_file());
}

#[test]
fn remove_takes_away_exactly_what_was_installed_and_nothing_else() {
    let sb = Sandbox::new();
    sb.fake_update_db();
    fs::create_dir_all(sb.p("data/applications")).unwrap();
    fs::write(sb.p("data/applications/other.desktop"), "keep me").unwrap();
    fs::create_dir_all(sb.p("data/icons/hicolor/48x48/apps")).unwrap();
    fs::write(
        sb.p("data/icons/hicolor/48x48/apps/other.png"),
        "keep me too",
    )
    .unwrap();

    assert!(sb.rada(&["setup", "desktop"]).status.success());
    let o = sb.rada(&["setup", "desktop", "--remove"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!sb.p("data/applications/rada.desktop").exists());
    for i in ICONS {
        assert!(!sb.p("data").join(i).exists(), "{i} left behind");
    }
    assert_eq!(read(&sb.p("data/applications/other.desktop")), "keep me");
    assert_eq!(
        read(&sb.p("data/icons/hicolor/48x48/apps/other.png")),
        "keep me too"
    );
    assert!(!sb.p("state/rada/desktop-install.json").exists());
    // Removing again is harmless.
    let again = sb.rada(&["setup", "desktop", "--remove"]);
    assert!(again.status.success());
    assert!(
        text(&again).contains("nothing to remove"),
        "{}",
        text(&again)
    );
}

#[test]
fn default_file_manager_is_only_set_on_request_and_restored_on_remove() {
    let sb = Sandbox::new();
    sb.fake_update_db();
    sb.fake_xdg_mime();
    fs::write(
        sb.p("config/mimeapps.list"),
        "[Default Applications]\ninode/directory=org.example.Files.desktop\n",
    )
    .unwrap();

    let o = sb.rada(&["setup", "desktop", "--default-file-manager"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("org.example.Files.desktop"),
        "the old handler is reported: {}",
        text(&o)
    );
    assert!(read(&sb.p("config/mimeapps.list")).contains("inode/directory=rada.desktop"));

    // A second run must not remember rada itself as "the previous handler".
    assert!(
        sb.rada(&["setup", "desktop", "--default-file-manager"])
            .status
            .success()
    );

    let o = sb.rada(&["setup", "desktop", "--remove"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        read(&sb.p("config/mimeapps.list")).contains("inode/directory=org.example.Files.desktop")
    );
    assert!(
        text(&o).contains("restored org.example.Files.desktop"),
        "{}",
        text(&o)
    );
}

#[test]
fn a_plain_setup_leaves_the_folder_handler_alone() {
    let sb = Sandbox::new();
    sb.fake_update_db();
    sb.fake_xdg_mime();
    let mimeapps = "[Default Applications]\ninode/directory=org.example.Files.desktop\n";
    fs::write(sb.p("config/mimeapps.list"), mimeapps).unwrap();

    // Twice, as an update of the installed entry is a plain setup again.
    for _ in 0..2 {
        assert!(sb.rada(&["setup", "desktop"]).status.success());
        assert!(!read(&sb.p("data/applications/rada.desktop")).contains("MimeType="));
        assert_eq!(read(&sb.p("config/mimeapps.list")), mimeapps);
        assert!(!read(&sb.p("xdg-mime.log")).contains("default"));
    }
}

#[test]
fn the_entry_claims_folders_only_with_the_default_file_manager_option() {
    let sb = Sandbox::new();
    sb.fake_update_db();
    sb.fake_xdg_mime();
    let entry = || read(&sb.p("data/applications/rada.desktop"));

    assert!(sb.rada(&["setup", "desktop"]).status.success());
    assert!(!entry().contains("MimeType="));

    assert!(
        sb.rada(&["setup", "desktop", "--default-file-manager"])
            .status
            .success()
    );
    assert!(entry().contains("MimeType=inode/directory;"), "{}", entry());
    assert!(read(&sb.p("config/mimeapps.list")).contains("inode/directory=rada.desktop"));

    // Refreshing the entry later keeps it consistent with the handler it was made.
    assert!(sb.rada(&["setup", "desktop"]).status.success());
    assert!(entry().contains("MimeType=inode/directory;"), "{}", entry());

    assert!(sb.rada(&["setup", "desktop", "--remove"]).status.success());
    assert!(!sb.p("data/applications/rada.desktop").exists());
}

#[test]
fn removing_a_default_that_had_no_predecessor_leaves_no_dangling_handler() {
    let sb = Sandbox::new();
    sb.fake_update_db();
    sb.fake_xdg_mime();
    assert!(
        sb.rada(&["setup", "desktop", "--default-file-manager"])
            .status
            .success()
    );
    assert!(read(&sb.p("config/mimeapps.list")).contains("inode/directory=rada.desktop"));
    assert!(sb.rada(&["setup", "desktop", "--remove"]).status.success());
    assert!(!read(&sb.p("config/mimeapps.list")).contains("rada.desktop"));
}

#[test]
fn a_default_changed_by_someone_else_in_the_meantime_is_not_overridden_on_remove() {
    let sb = Sandbox::new();
    sb.fake_update_db();
    sb.fake_xdg_mime();
    fs::write(
        sb.p("config/mimeapps.list"),
        "[Default Applications]\ninode/directory=old.desktop\n",
    )
    .unwrap();
    assert!(
        sb.rada(&["setup", "desktop", "--default-file-manager"])
            .status
            .success()
    );
    fs::write(
        sb.p("config/mimeapps.list"),
        "[Default Applications]\ninode/directory=chosen-later.desktop\n",
    )
    .unwrap();
    let o = sb.rada(&["setup", "desktop", "--remove"]);
    assert!(read(&sb.p("config/mimeapps.list")).contains("chosen-later.desktop"));
    assert!(text(&o).contains("changed since"), "{}", text(&o));
}

#[test]
fn spawn_terminal_prefers_xdg_terminal_exec_and_passes_the_folder() {
    let sb = Sandbox::new();
    let log = sb.p("terminal.log");
    sb.script(
        "xdg-terminal-exec",
        &format!(r#"echo "xdg $@" > '{}'"#, log.display()),
    );
    sb.script(
        "kitty",
        &format!(r#"echo "kitty $@" > '{}.kitty'"#, log.display()),
    );
    let dir = sb.p("home");
    let o = sb.rada_with_terminal_var(&["--spawn-terminal", dir.to_str().unwrap()], "kitty");
    assert!(o.status.success(), "{}", text(&o));
    let got = wait_for_file(&log);
    let exe = env!("CARGO_BIN_EXE_rada");
    assert_eq!(
        got.trim(),
        format!("xdg {exe} {}", fs::canonicalize(&dir).unwrap().display())
    );
    assert!(!sb.p("terminal.log.kitty").exists());
}

#[test]
fn spawn_terminal_falls_back_to_terminal_variable_then_known_terminals() {
    let sb = Sandbox::new();
    let log = sb.p("terminal.log");
    sb.script(
        "myterm",
        &format!(r#"echo "myterm $@" > '{}'"#, log.display()),
    );
    sb.script(
        "ghostty",
        &format!(r#"echo "ghostty $@" > '{}.ghostty'"#, log.display()),
    );
    let o = sb.rada_with_terminal_var(&["--spawn-terminal"], "myterm");
    assert!(o.status.success(), "{}", text(&o));
    let got = wait_for_file(&log);
    assert!(got.starts_with("myterm -e "), "{got}");

    // $TERMINAL pointing at nothing: the fixed list takes over.
    let o = sb.rada_with_terminal_var(&["--spawn-terminal"], "not-installed");
    assert!(o.status.success(), "{}", text(&o));
    let got = wait_for_file(&sb.p("terminal.log.ghostty"));
    assert!(got.starts_with("ghostty -e "), "{got}");
}

#[test]
fn spawn_terminal_explains_when_there_is_no_terminal() {
    let sb = Sandbox::new();
    let o = sb.rada(&["--spawn-terminal"]);
    assert!(!o.status.success());
    assert!(
        text(&o).contains("no terminal emulator found"),
        "{}",
        text(&o)
    );
}

#[test]
fn the_entry_file_in_the_repository_is_a_valid_desktop_file_when_the_validator_exists() {
    let Ok(out) = Command::new("desktop-file-validate")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux/rada.desktop"))
        .output()
    else {
        return;
    };
    let msg = text(&out);
    // Hints are fine; errors and warnings are not.
    assert!(
        out.status.success() && !msg.contains("error:") && !msg.contains("warning:"),
        "{msg}"
    );
}
