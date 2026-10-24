//! macOS: `~/Applications/Rada.app`, a small application that opens rada in a terminal window
//! with a click. It is made of files only (no compiled code, no signature to go stale) and
//! needs no administrator rights:
//!
//! ```text
//! Rada.app/Contents/Info.plist
//! Rada.app/Contents/MacOS/Rada              a shell script that picks the terminal
//! Rada.app/Contents/Resources/rada.icns     the logo, at every size
//! Rada.app/Contents/Resources/rada-ghostty.conf   settings for the Ghostty window only
//! Rada.app/Contents/Resources/rada.command  what iTerm2 and Terminal run
//! ```
//!
//! The terminal is Ghostty if it is installed (a window of 180×45 with the Cmd keys let
//! through, see [`ghostty_config`]), else iTerm2, else Terminal.
//!
//! Ghostty is never given rada as an argument or as a document. Its macOS application asks
//! "Allow Ghostty to execute …?" for every file it is asked to open as a document (Finder,
//! the Dock, or a path among the arguments that AppKit takes for a document: that is what
//! `-e <path>` did on older versions of Ghostty, besides starting a second tab), and the
//! maintainers keep that question on purpose (it is a security measure, not a setting). The
//! command is the `initial-command` of a settings file instead, which Ghostty reads as
//! configuration: no question, one window, one tab. `window-save-state = never` makes sure
//! that the windows of the last run are not given back besides it. The Dock shows the
//! terminal's icon while rada runs, not rada's: a window belongs to the program that draws it.

use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const ICON: &[u8] = include_bytes!("../assets/macos/rada.icns");

pub const BUNDLE_ID: &str = "com.formikadesk.rada";

/// Window size, in cells, of the Ghostty window.
pub const WINDOW_COLUMNS: u32 = 180;
pub const WINDOW_ROWS: u32 = 45;

/// What was written, so that `--remove` takes away exactly that.
#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    paths: Vec<PathBuf>,
}

pub struct MacSetup {
    /// `~/Applications`.
    pub apps_dir: PathBuf,
    /// `~/Desktop`, for the alias.
    pub desktop_dir: PathBuf,
    /// The program the application starts.
    pub exe: PathBuf,
    pub state_dir: PathBuf,
    pub version: String,
}

impl MacSetup {
    fn app(&self) -> PathBuf {
        self.apps_dir.join("Rada.app")
    }

    fn alias(&self) -> PathBuf {
        self.desktop_dir.join("Rada")
    }

    fn manifest_path(&self) -> PathBuf {
        self.state_dir.join("macos-install.json")
    }

    /// Make the application (and, with `desktop`, a link to it on the Desktop).
    pub fn install(&self, desktop: bool) -> Result<Vec<String>> {
        let mut say = Vec::new();
        let app = self.app();
        let contents = app.join("Contents");
        let mut written: Vec<PathBuf> = vec![app.clone()];
        // A fresh bundle each time: nothing of an older version is left inside.
        if app.exists() {
            std::fs::remove_dir_all(&app)
                .with_context(|| tf(Key::MCannotReplace, &[&app.display()]))?;
        }
        write(
            &contents.join("Info.plist"),
            info_plist(&self.version).as_bytes(),
            false,
        )?;
        write(
            &contents.join("MacOS/Rada"),
            launcher_script(&self.exe).as_bytes(),
            true,
        )?;
        write(&contents.join("Resources/rada.icns"), ICON, false)?;
        write(
            &contents.join("Resources/rada-ghostty.conf"),
            ghostty_config(&self.exe).as_bytes(),
            false,
        )?;
        write(
            &contents.join("Resources/rada.command"),
            command_script(&self.exe).as_bytes(),
            true,
        )?;
        say.push(tf(Key::SayMade, &[&app.display()]));
        refresh_launch_services(&app);

        if desktop {
            let alias = self.alias();
            make_alias(&app, &alias)?;
            say.push(tf(Key::SayMacLink, &[&alias.display()]));
            written.push(alias);
        }
        let mut manifest = self.read_manifest();
        for p in written {
            if !manifest.paths.contains(&p) {
                manifest.paths.push(p);
            }
        }
        self.save_manifest(&manifest)?;
        say.push(Key::SayMacGhostty.tr().to_string());
        say.push(tf(Key::SayMacWindow, &[&WINDOW_COLUMNS, &WINDOW_ROWS]));
        Ok(say)
    }

    /// Take away what `install` made.
    pub fn remove(&self) -> Result<Vec<String>> {
        let mut say = Vec::new();
        let mut paths = self.read_manifest().paths;
        // Without a record, the known places.
        for known in [self.app(), self.alias()] {
            if !paths.contains(&known) {
                paths.push(known);
            }
        }
        for p in paths {
            // The alias is a link to the application: it is removed as a link, and only if
            // it still points there.
            let meta = std::fs::symlink_metadata(&p);
            match meta {
                Ok(m) if m.file_type().is_symlink() => {
                    if std::fs::read_link(&p).is_ok_and(|t| t == self.app()) {
                        std::fs::remove_file(&p)
                            .with_context(|| tf(Key::MCannotRemove, &[&p.display()]))?;
                        say.push(tf(Key::SayRemoved, &[&p.display()]));
                    }
                }
                Ok(m) if m.is_dir() && p == self.app() => {
                    std::fs::remove_dir_all(&p)
                        .with_context(|| tf(Key::MCannotRemove, &[&p.display()]))?;
                    say.push(tf(Key::SayRemoved, &[&p.display()]));
                }
                _ => {}
            }
        }
        let _ = std::fs::remove_file(self.manifest_path());
        if say.is_empty() {
            say.push(Key::SayNothingToRemove.tr().to_string());
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
            false,
        )
    }
}

fn write(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| tf(Key::MCannotCreate, &[&dir.display()]))?;
    }
    std::fs::write(path, bytes).with_context(|| tf(Key::MCannotWrite, &[&path.display()]))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .with_context(|| tf(Key::MCannotRunnable, &[&path.display()]))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

/// The Desktop entry is a symbolic link: the Finder shows it with the application's icon and
/// opens the application when it is double-clicked.
fn make_alias(app: &Path, alias: &Path) -> Result<()> {
    if let Some(dir) = alias.parent() {
        std::fs::create_dir_all(dir).with_context(|| tf(Key::MCannotCreate, &[&dir.display()]))?;
    }
    if let Ok(m) = std::fs::symlink_metadata(alias) {
        if m.file_type().is_symlink() {
            std::fs::remove_file(alias)
                .with_context(|| tf(Key::MCannotReplace, &[&alias.display()]))?;
        } else {
            anyhow::bail!("{}", tf(Key::MMacNotLink, &[&alias.display()]));
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(app, alias)
            .with_context(|| tf(Key::MCannotCreate, &[&alias.display()]))
    }
    #[cfg(not(unix))]
    {
        let _ = app;
        anyhow::bail!("{}", Key::MMacLinkOnly.tr())
    }
}

/// Tell the system a new application is there, so that the icon shows at once. Best effort.
fn refresh_launch_services(app: &Path) {
    #[cfg(target_os = "macos")]
    {
        let lsregister = "/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister";
        if Path::new(lsregister).is_file() {
            let _ = std::process::Command::new(lsregister)
                .arg("-f")
                .arg(app)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        let _ = std::process::Command::new("touch").arg(app).status();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// `Info.plist` of the application. `LSUIElement` keeps the launcher itself out of the Dock:
/// it lives for a moment, only to start the terminal.
pub fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>Rada</string>
	<key>CFBundleDisplayName</key>
	<string>Rada</string>
	<key>CFBundleIdentifier</key>
	<string>{BUNDLE_ID}</string>
	<key>CFBundleExecutable</key>
	<string>Rada</string>
	<key>CFBundleIconFile</key>
	<string>rada</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSMinimumSystemVersion</key>
	<string>10.13</string>
	<key>LSUIElement</key>
	<true/>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.utilities</string>
</dict>
</plist>
"#
    )
}

/// A path as one word for a shell: in single quotes.
fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

/// What `Contents/MacOS/Rada` runs. The terminal is looked for when the application is
/// opened, not when it is made, so that installing Ghostty later is enough.
/// `RADA_APP_DIRS` (folders separated by `:`) replaces where applications are looked for:
/// for tests.
pub fn launcher_script(exe: &Path) -> String {
    format!(
        r#"#!/bin/sh
# Rada.app: opens rada in Ghostty (with settings of its own), else in iTerm2, else in Terminal.
RADA={rada}
HERE="$(cd "$(dirname "$0")" && pwd)"
RES="$(cd "$HERE/../Resources" && pwd)"
APPS="${{RADA_APP_DIRS:-/Applications:$HOME/Applications:/System/Applications/Utilities}}"
found() (
  IFS=:
  for d in $APPS; do
    [ -d "$d/$1.app" ] && return 0
  done
  return 1
)
if found Ghostty; then
  exec open -n -a Ghostty --args --config-file="$RES/rada-ghostty.conf"
elif found iTerm; then
  exec open -a iTerm "$RES/rada.command"
else
  exec open -a Terminal "$RES/rada.command"
fi
"#,
        rada = sh_quote(exe)
    )
}

/// What iTerm2 and Terminal run: they are given this file to open.
pub fn command_script(exe: &Path) -> String {
    format!("#!/bin/sh\nRADA_FROM_APP=1 exec {}\n", sh_quote(exe))
}

/// The settings of the Ghostty window Rada.app opens (and of no other window).
///
/// Ghostty keeps some Cmd shortcuts for itself (copy, paste, undo, "jump to prompt", split
/// navigation…): they never reach the program in the window. Rada's classic scheme uses the
/// same keys the way the Finder does, so the ones Ghostty binds by default are unbound here;
/// `unbind` hands the key back to the program (documented in Ghostty's keybind reference).
/// Keys Ghostty does not bind (Cmd+X, Cmd+I, Cmd+O, Cmd+Y, Cmd+Shift+N, Cmd+.) need nothing.
///
/// The window's one command is `initial-command` (only the first terminal Ghostty opens runs
/// it, once), written as `shell:` and a quoted path so that any folder name works. The program
/// is started with `RADA_FROM_APP=1`, which tells it that the window is its own and closes
/// with it: an error is shown and waits for a key instead of vanishing. Ghostty
/// restores the windows of its last run on macOS unless told not to (`window-save-state`):
/// a restored tab would stand beside the one that runs rada, so it is told not to.
pub fn ghostty_config(exe: &Path) -> String {
    let mut out = format!(
        "# Written by `rada setup desktop`. Ghostty reads it only for the window Rada.app opens.\n\
         initial-command = shell:env RADA_FROM_APP=1 {}\n\
         window-save-state = never\n\
         window-width = {WINDOW_COLUMNS}\n\
         window-height = {WINDOW_ROWS}\n\
         # The margin around the grid takes the terminal's background, which rada sets to its\n\
         # theme's (see docs/themes.md), and what is left over is shared on every side.\n\
         window-padding-color = background\n\
         window-padding-balance = true\n\
         # This window is rada's: closing it ends this Ghostty.\n\
         quit-after-last-window-closed = true\n\
         \n\
         # The Mac shortcuts of rada's classic keys, given back to the program:\n",
        sh_quote(exe)
    );
    for (key, what) in GHOSTTY_UNBIND {
        out.push_str(&format!("keybind = {key}=unbind\n"));
        let _ = what;
    }
    out
}

/// Cmd shortcuts that Ghostty binds by default and rada uses, with what Ghostty did with them.
pub const GHOSTTY_UNBIND: &[(&str, &str)] = &[
    ("super+c", "copy_to_clipboard"),
    ("super+v", "paste_from_clipboard"),
    ("super+z", "undo"),
    ("super+shift+z", "redo"),
    ("super+a", "select_all"),
    ("super+backspace", "text:\\x15"),
    ("super+arrow_up", "jump_to_prompt:-1"),
    ("super+bracket_left", "goto_split:previous"),
    ("super+bracket_right", "goto_split:next"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_names_the_application_and_its_icon() {
        let p = info_plist("0.6.0-rc.2");
        for needle in [
            "<key>CFBundleIdentifier</key>\n\t<string>com.formikadesk.rada</string>",
            "<key>CFBundleExecutable</key>\n\t<string>Rada</string>",
            "<key>CFBundleIconFile</key>\n\t<string>rada</string>",
            "<key>CFBundleShortVersionString</key>\n\t<string>0.6.0-rc.2</string>",
            "<key>CFBundlePackageType</key>\n\t<string>APPL</string>",
            "<key>LSUIElement</key>\n\t<true/>",
        ] {
            assert!(p.contains(needle), "{needle}\n{p}");
        }
        assert!(p.starts_with("<?xml"));
        assert!(p.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn the_ghostty_settings_free_the_keys_rada_uses() {
        let c = ghostty_config(Path::new("/Users/me/.cargo/bin/rada"));
        assert!(
            c.contains("window-width = 180\nwindow-height = 45\n"),
            "{c}"
        );
        assert!(c.contains("quit-after-last-window-closed = true"), "{c}");
        assert!(
            c.contains("window-padding-color = background\nwindow-padding-balance = true\n"),
            "{c}"
        );
        for key in [
            "super+c",
            "super+v",
            "super+z",
            "super+shift+z",
            "super+a",
            "super+backspace",
            "super+arrow_up",
            "super+bracket_left",
            "super+bracket_right",
        ] {
            assert!(
                c.contains(&format!("keybind = {key}=unbind\n")),
                "{key}\n{c}"
            );
        }
        // Only unbinding: nothing that takes keys away from the terminal.
        assert!(
            c.lines()
                .filter(|l| l.starts_with("keybind"))
                .all(|l| l.ends_with("=unbind"))
        );
        assert!(!c.contains("clear"));
    }

    #[test]
    fn ghostty_gets_rada_once_through_its_settings_and_not_with_dash_e() {
        // `-e` makes Ghostty ask for a confirmation on every launch and, through `open`,
        // run the command twice; `initial-command` runs it once in the first window.
        let exe = Path::new("/Users/Jo O'Neil/bin/rada");
        let c = ghostty_config(exe);
        assert_eq!(
            c.lines()
                .filter(|l| l.starts_with("initial-command"))
                .count(),
            1,
            "{c}"
        );
        assert!(
            c.contains(
                r"initial-command = shell:env RADA_FROM_APP=1 '/Users/Jo O'\''Neil/bin/rada'"
            ),
            "{c}"
        );
        let launcher = launcher_script(exe);
        let ghostty_line = launcher.lines().find(|l| l.contains("-a Ghostty")).unwrap();
        assert!(!ghostty_line.contains(" -e "), "{ghostty_line}");
        assert!(!ghostty_line.contains("initial-command"), "{ghostty_line}");
    }

    #[test]
    fn ghostty_is_given_options_only_never_a_path_that_it_could_take_for_a_document() {
        // AppKit reports a path among the arguments as a document to open, and Ghostty asks
        // "Allow Ghostty to execute …?" for it and opens a tab of its own. Everything after
        // `--args` is an option (`--name=value`); the program and the folders are in the file.
        let exe = Path::new("/Users/utente/.cargo/bin/rada");
        let launcher = launcher_script(exe);
        let line = launcher.lines().find(|l| l.contains("-a Ghostty")).unwrap();
        let after = line.split("--args").nth(1).expect("--args");
        for word in after.split_whitespace() {
            assert!(word.starts_with("--"), "{word} in {line}");
        }
        assert!(
            !launcher
                .lines()
                .filter(|l| l.contains("Ghostty"))
                .any(|l| l.contains("$RADA")),
            "the program is not passed to Ghostty: {launcher}"
        );
        // The window of the last run is not given back beside the one that runs rada.
        let c = ghostty_config(exe);
        assert!(c.contains("window-save-state = never\n"), "{c}");
        // One command, in the first window only.
        assert_eq!(c.matches("initial-command").count(), 1, "{c}");
        assert!(!c.lines().any(|l| l.starts_with("command ")), "{c}");
    }

    #[test]
    fn every_key_the_ghostty_file_frees_is_one_rada_uses() {
        use rada_tui::keymap::{Chord, Keymap, Preset};
        let km = Keymap::new_for(Preset::VimClassic, &Default::default(), true);
        for (key, _) in GHOSTTY_UNBIND {
            // Ghostty's `super+bracket_left` is rada's `cmd+[`; `arrow_up` is `up`.
            let rada = key
                .replace("super", "cmd")
                .replace("bracket_left", "[")
                .replace("bracket_right", "]")
                .replace("arrow_up", "up");
            let chord = Chord::parse(&rada).unwrap_or_else(|e| panic!("{key}: {e}"));
            let ev = crossterm::event::KeyEvent::new(chord.code, chord.mods);
            assert!(
                km.action_for(&ev).is_some(),
                "{key} ({rada}) means nothing to rada"
            );
        }
    }

    fn setup(root: &Path) -> MacSetup {
        MacSetup {
            apps_dir: root.join("Applications"),
            desktop_dir: root.join("Desktop"),
            exe: PathBuf::from("/Users/me/.cargo/bin/rada"),
            state_dir: root.join("state"),
            version: "0.6.0-rc.2".into(),
        }
    }

    #[test]
    fn the_application_is_made_of_the_files_a_bundle_needs() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path());
        let say = s.install(false).unwrap();
        assert!(say[0].contains("Rada.app"), "{say:?}");
        let app = d.path().join("Applications/Rada.app/Contents");
        for f in [
            "Info.plist",
            "MacOS/Rada",
            "Resources/rada.icns",
            "Resources/rada-ghostty.conf",
            "Resources/rada.command",
        ] {
            assert!(app.join(f).is_file(), "{f}");
        }
        // The icon is the real one, at every size.
        let icns = std::fs::read(app.join("Resources/rada.icns")).unwrap();
        assert_eq!(
            crate::icons::icns_entries(&icns).unwrap().len(),
            crate::icons::ICNS_ENTRIES.len()
        );
        // The launcher and the command file can be run.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in ["MacOS/Rada", "Resources/rada.command"] {
                let m = std::fs::metadata(app.join(f)).unwrap().permissions().mode();
                assert_eq!(m & 0o111, 0o111, "{f}");
            }
        }
        // It says what the Dock will show.
        assert!(say.iter().any(|l| l.contains("Dock")), "{say:?}");
        // No Desktop link without asking.
        assert!(!d.path().join("Desktop").exists());
    }

    #[test]
    fn a_second_setup_replaces_the_application_whole() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path());
        s.install(false).unwrap();
        let stale = d
            .path()
            .join("Applications/Rada.app/Contents/Resources/old-thing");
        std::fs::write(&stale, "x").unwrap();
        s.install(false).unwrap();
        assert!(!stale.exists());
        assert!(
            d.path()
                .join("Applications/Rada.app/Contents/MacOS/Rada")
                .is_file()
        );
    }

    #[cfg(unix)]
    #[test]
    fn desktop_adds_a_link_and_remove_takes_everything_away() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path());
        let say = s.install(true).unwrap();
        assert!(say.iter().any(|l| l.contains("Desktop")), "{say:?}");
        let link = d.path().join("Desktop/Rada");
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            d.path().join("Applications/Rada.app")
        );
        // Something else with that name is not ours to replace.
        let other = tempfile::tempdir().unwrap();
        let s2 = setup(other.path());
        std::fs::create_dir_all(other.path().join("Desktop")).unwrap();
        std::fs::write(other.path().join("Desktop/Rada"), "mine").unwrap();
        let err = s2.install(true).unwrap_err().to_string();
        assert!(err.contains("left alone"), "{err}");
        assert_eq!(
            std::fs::read_to_string(other.path().join("Desktop/Rada")).unwrap(),
            "mine"
        );

        let say = s.remove().unwrap();
        assert!(say.iter().any(|l| l.contains("Rada.app")), "{say:?}");
        assert!(!d.path().join("Applications/Rada.app").exists());
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert!(!d.path().join("state/macos-install.json").exists());
        assert_eq!(s.remove().unwrap(), ["nothing to remove"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_now_points_elsewhere_is_left_when_removing() {
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path());
        s.install(true).unwrap();
        let link = d.path().join("Desktop/Rada");
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink("/somewhere/else", &link).unwrap();
        s.remove().unwrap();
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            Path::new("/somewhere/else")
        );
    }

    /// Run the launcher of a made application, with `open` and the application folders
    /// replaced by ours, and say what it asked `open` to do.
    #[cfg(unix)]
    fn launch(installed: &[&str]) -> Vec<String> {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let s = setup(d.path());
        s.install(false).unwrap();
        let apps = d.path().join("apps");
        for a in installed {
            std::fs::create_dir_all(apps.join(format!("{a}.app"))).unwrap();
        }
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let log = d.path().join("open.log");
        let open = bin.join("open");
        std::fs::write(
            &open,
            format!(
                "#!/bin/sh
for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status = std::process::Command::new("/bin/sh")
            .arg(d.path().join("Applications/Rada.app/Contents/MacOS/Rada"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("HOME", d.path().join("home"))
            .env("RADA_APP_DIRS", apps.display().to_string())
            .status()
            .unwrap();
        assert!(status.success());
        let res = d.path().join("Applications/Rada.app/Contents/Resources");
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|l| l.replace(&res.display().to_string(), "<Resources>"))
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn the_launcher_prefers_ghostty_then_iterm_then_terminal() {
        assert_eq!(
            launch(&["Ghostty", "iTerm", "Terminal"]),
            [
                "-n",
                "-a",
                "Ghostty",
                "--args",
                "--config-file=<Resources>/rada-ghostty.conf",
            ]
        );
        assert_eq!(
            launch(&["iTerm", "Terminal"]),
            ["-a", "iTerm", "<Resources>/rada.command"]
        );
        assert_eq!(launch(&[]), ["-a", "Terminal", "<Resources>/rada.command"]);
    }

    /// Both ways rada is started from the application tell it so, whatever the path looks like.
    #[cfg(unix)]
    #[test]
    fn the_program_is_told_it_was_started_from_the_application() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("it's a rada");
        std::fs::write(&exe, "#!/bin/sh\nprintf '%s' \"$RADA_FROM_APP\"\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("/bin/sh")
                .args(args)
                .env_remove("RADA_FROM_APP")
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        // iTerm2 and Terminal: the file is run.
        let command = d.path().join("rada.command");
        std::fs::write(&command, command_script(&exe)).unwrap();
        assert_eq!(run(&[command.to_str().unwrap()]), "1");
        // Ghostty: the line after `shell:` is run by a shell.
        let conf = ghostty_config(&exe);
        let line = conf
            .lines()
            .find_map(|l| l.strip_prefix("initial-command = shell:"))
            .unwrap();
        assert_eq!(run(&["-c", line]), "1");
    }

    #[test]
    fn paths_with_spaces_and_quotes_are_one_word_for_the_shell() {
        let s = launcher_script(Path::new("/Users/Jo O'Neil/bin/rada"));
        assert!(s.contains(r"RADA='/Users/Jo O'\''Neil/bin/rada'"), "{s}");
        assert_eq!(
            command_script(Path::new("/a b/rada")),
            "#!/bin/sh\nRADA_FROM_APP=1 exec '/a b/rada'\n"
        );
    }
}
