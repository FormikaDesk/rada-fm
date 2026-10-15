//! Linux desktop integration: a launcher entry, icons, and the terminal launcher the
//! entry uses. Everything is installed for the current user only, and the exact list of
//! files written is remembered so that `--remove` takes away precisely those.

use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use rada_core::platform::Dirs;
use serde::{Deserialize, Serialize};

const DESKTOP_ENTRY: &str = include_str!("../assets/linux/rada.desktop");
const ICON_SVG: &[u8] = include_bytes!("../assets/linux/rada.svg");
const ICON_48: &[u8] = include_bytes!("../assets/linux/rada-48.png");
const ICON_128: &[u8] = include_bytes!("../assets/linux/rada-128.png");
const ICON_256: &[u8] = include_bytes!("../assets/linux/rada-256.png");

const DESKTOP_FILE: &str = "rada.desktop";
const DIR_MIME: &str = "inode/directory";

/// What `setup desktop` did, so that `--remove` can undo exactly that.
#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    files: Vec<PathBuf>,
    /// Set when rada was made the default folder handler: what it was before
    /// (`Some("")` means there was none).
    previous_default: Option<String>,
}

/// Where things go, and which helper programs to call (replaceable in tests).
pub struct Setup {
    pub data_home: PathBuf,
    pub config_home: PathBuf,
    pub state_dir: PathBuf,
    /// The program the launcher entry starts.
    pub exe: PathBuf,
    pub xdg_mime: String,
    pub update_db: String,
}

impl Setup {
    pub fn from_env(dirs: &Dirs) -> Result<Setup> {
        Ok(Setup {
            data_home: dirs.data.clone(),
            config_home: dirs.config.clone(),
            state_dir: dirs.rada_state(),
            exe: std::env::current_exe().context(Key::MNoExe.tr())?,
            xdg_mime: "xdg-mime".into(),
            update_db: "update-desktop-database".into(),
        })
    }

    fn manifest_path(&self) -> PathBuf {
        self.state_dir.join("desktop-install.json")
    }

    fn desktop_path(&self) -> PathBuf {
        self.data_home.join("applications").join(DESKTOP_FILE)
    }

    fn icon_files(&self) -> Vec<(PathBuf, &'static [u8])> {
        let hicolor = self.data_home.join("icons/hicolor");
        vec![
            (hicolor.join("scalable/apps/rada.svg"), ICON_SVG),
            (hicolor.join("48x48/apps/rada.png"), ICON_48),
            (hicolor.join("128x128/apps/rada.png"), ICON_128),
            (hicolor.join("256x256/apps/rada.png"), ICON_256),
        ]
    }

    /// Install the launcher entry and icons. Returns the lines to print.
    pub fn install(&self, default_file_manager: bool) -> Result<Vec<String>> {
        let mut say = Vec::new();
        let mut manifest = self.read_manifest();
        let mut written: Vec<PathBuf> = Vec::new();

        // The entry only claims folders when rada is (to be) the folder handler: a
        // `MimeType=inode/directory` line alone would already make it the fallback
        // handler ahead of the system's own file manager.
        let claims_folders = default_file_manager || manifest.previous_default.is_some();
        let desktop = self.desktop_path();
        write_file(
            &desktop,
            launcher_entry(DESKTOP_ENTRY, &self.exe, claims_folders).as_bytes(),
        )?;
        say.push(tf(Key::SayInstalled, &[&desktop.display()]));
        written.push(desktop);
        for (path, bytes) in self.icon_files() {
            write_file(&path, bytes)?;
            say.push(tf(Key::SayInstalled, &[&path.display()]));
            written.push(path);
        }
        for f in written {
            if !manifest.files.contains(&f) {
                manifest.files.push(f);
            }
        }

        match run(
            &self.update_db,
            &[self.data_home.join("applications").as_os_str()],
        ) {
            Ok(_) => say.push(Key::SayDbUpdated.tr().to_string()),
            Err(e) => say.push(tf(Key::SayDbNote, &[&e])),
        }

        if default_file_manager {
            let current = run(
                &self.xdg_mime,
                &["query".as_ref(), "default".as_ref(), DIR_MIME.as_ref()],
            )
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
            if current == DESKTOP_FILE {
                say.push(Key::SayAlreadyDefault.tr().to_string());
            } else {
                // Keep the first previous handler if setup is run twice.
                if manifest.previous_default.is_none() {
                    manifest.previous_default = Some(current.clone());
                }
                self.save_manifest(&manifest)?;
                run(
                    &self.xdg_mime,
                    &["default".as_ref(), DESKTOP_FILE.as_ref(), DIR_MIME.as_ref()],
                )
                .context(Key::MXdgSet.tr())?;
                say.push(match current.as_str() {
                    "" => Key::SayNowDefaultNone.tr().to_string(),
                    prev => tf(Key::SayNowDefaultWas, &[&prev]),
                });
            }
        }
        self.save_manifest(&manifest)?;
        Ok(say)
    }

    /// Remove exactly what `install` wrote and restore the previous default handler.
    pub fn remove(&self) -> Result<Vec<String>> {
        let mut say = Vec::new();
        let manifest = self.read_manifest();
        // Without a record (deleted state folder), fall back to the known locations.
        let mut files = manifest.files.clone();
        for known in std::iter::once(self.desktop_path())
            .chain(self.icon_files().into_iter().map(|(p, _)| p))
        {
            if !files.contains(&known) {
                files.push(known);
            }
        }
        for f in &files {
            match std::fs::remove_file(f) {
                Ok(()) => say.push(tf(Key::SayRemoved, &[&f.display()])),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => say.push(tf(Key::SayCouldNotRemove, &[&f.display(), &e])),
            }
        }
        if let Some(prev) = &manifest.previous_default {
            let now = run(
                &self.xdg_mime,
                &["query".as_ref(), "default".as_ref(), DIR_MIME.as_ref()],
            )
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
            if now == DESKTOP_FILE {
                if prev.is_empty() {
                    self.forget_default_in_mimeapps(&mut say)?;
                } else {
                    run(
                        &self.xdg_mime,
                        &["default".as_ref(), prev.as_ref(), DIR_MIME.as_ref()],
                    )
                    .context(Key::MXdgRestore.tr())?;
                    say.push(tf(Key::SayRestored, &[&prev]));
                }
            } else {
                say.push(Key::SayDefaultChanged.tr().to_string());
            }
        }
        let _ = std::fs::remove_file(self.manifest_path());
        if say.is_empty() {
            say.push("nothing to remove".into());
        } else if run(
            &self.update_db,
            &[self.data_home.join("applications").as_os_str()],
        )
        .is_ok()
        {
            say.push("updated the desktop database".into());
        }
        Ok(say)
    }

    /// `xdg-mime` cannot unset a default: drop our line from `mimeapps.list` ourselves.
    fn forget_default_in_mimeapps(&self, say: &mut Vec<String>) -> Result<()> {
        let path = self.config_home.join("mimeapps.list");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(());
        };
        let wanted = format!("{DIR_MIME}={DESKTOP_FILE}");
        let kept: Vec<&str> = text.lines().filter(|l| l.trim() != wanted).collect();
        if kept.len() != text.lines().count() {
            std::fs::write(&path, kept.join("\n") + "\n")
                .with_context(|| tf(Key::MCannotUpdate, &[&path.display()]))?;
            say.push(tf(Key::SayForgotDefault, &[&path.display()]));
        }
        Ok(())
    }

    fn read_manifest(&self) -> Manifest {
        std::fs::read_to_string(self.manifest_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_manifest(&self, m: &Manifest) -> Result<()> {
        write_file(
            &self.manifest_path(),
            serde_json::to_string_pretty(m)?.as_bytes(),
        )
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| tf(Key::MCannotCreate, &[&dir.display()]))?;
    }
    std::fs::write(path, bytes).with_context(|| tf(Key::MCannotWrite, &[&path.display()]))
}

fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| tf(Key::MNotAvailable, &[&program]))?;
    if !out.status.success() {
        bail!(
            "{}",
            tf(
                Key::MProgramFailed,
                &[&program, &String::from_utf8_lossy(&out.stderr).trim()]
            )
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Quote a program path for an `Exec=` line (Desktop Entry specification).
fn exec_quote(p: &Path) -> String {
    let s = p.to_string_lossy();
    let plain = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    if plain {
        return s.into_owned();
    }
    let mut out = String::from("\"");
    for c in s.chars() {
        if "\"`$\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    // `%` has a meaning of its own in Exec lines.
    out.replace('%', "%%")
}

/// The entry says `Exec=rada …`; launchers often do not have `~/.cargo/bin` in their
/// PATH, so the installed entry names the program by its full path. The `MimeType=` line
/// is dropped unless the entry is to claim folders.
fn launcher_entry(template: &str, exe: &Path, claims_folders: bool) -> String {
    template
        .lines()
        .filter(|l| claims_folders || !l.starts_with("MimeType="))
        .map(|l| match l.strip_prefix("Exec=rada ") {
            Some(rest) => format!("Exec={} {rest}", exec_quote(exe)),
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

// ------------------------------------------------------------------------ terminal launcher

/// The identity rada's window has: the name a bar or a workspace switcher matches to an icon.
pub const APP_ID: &str = "com.formikadesk.rada";

/// One terminal emulator and how it is told to run a command.
struct Terminal {
    program: &'static str,
    /// Arguments right after the program (a subcommand).
    subcommand: &'static [&'static str],
    /// How the window is given its own identity (class on X11, app-id on Wayland); `{}` is
    /// replaced by [`APP_ID`]. Empty for a terminal with no such option that we know of.
    identity: &'static [&'static str],
    /// Arguments that go between the options and the command to run.
    before: &'static [&'static str],
}

const TERMINALS: &[Terminal] = &[
    Terminal {
        program: "ghostty",
        subcommand: &[],
        identity: &["--class={}"],
        before: &["-e"],
    },
    Terminal {
        program: "kitty",
        subcommand: &[],
        identity: &["--class", "{}"],
        before: &[],
    },
    Terminal {
        program: "foot",
        subcommand: &[],
        identity: &["--app-id={}"],
        before: &[],
    },
    Terminal {
        program: "alacritty",
        subcommand: &[],
        identity: &["--class", "{}"],
        before: &["-e"],
    },
    Terminal {
        program: "wezterm",
        subcommand: &["start"],
        identity: &["--class", "{}"],
        before: &["--"],
    },
    Terminal {
        program: "konsole",
        subcommand: &[],
        identity: &[],
        before: &["-e"],
    },
    Terminal {
        program: "gnome-terminal",
        subcommand: &[],
        identity: &[],
        before: &["--"],
    },
    Terminal {
        program: "xterm",
        subcommand: &[],
        identity: &[],
        before: &["-e"],
    },
];

fn identity_args(t: &Terminal) -> Vec<String> {
    t.identity.iter().map(|a| a.replace("{}", APP_ID)).collect()
}

/// The command line that opens `rada` (at `path`) in a new terminal window, chosen in
/// this order: `xdg-terminal-exec`, `$TERMINAL`, then the first installed of a list. The
/// window gets rada's own identity ([`APP_ID`]) where the terminal can be told to.
pub fn terminal_command(
    exe: &Path,
    path: Option<&Path>,
    terminal_var: Option<&str>,
    installed: &dyn Fn(&str) -> bool,
) -> Option<Vec<String>> {
    let mut command: Vec<String> = vec![exe.to_string_lossy().into_owned()];
    if let Some(p) = path {
        command.push(p.to_string_lossy().into_owned());
    }
    let with = |prefix: Vec<String>| -> Vec<String> {
        prefix.into_iter().chain(command.clone()).collect()
    };

    if installed("xdg-terminal-exec") {
        return Some(with(vec!["xdg-terminal-exec".into()]));
    }
    if let Some(t) = terminal_var {
        let mut words = t.split_whitespace().map(str::to_string);
        if let Some(prog) = words.next()
            && installed(&prog)
        {
            // The usual convention: `-e COMMAND…` (kitty and foot take it bare, but
            // also accept `-e`). A terminal we know gets its identity first, so that
            // options the person wrote after it win.
            let known = TERMINALS.iter().find(|k| {
                Path::new(&prog)
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy() == k.program)
            });
            let mut prefix = vec![prog];
            if let Some(k) = known {
                prefix.extend(k.subcommand.iter().map(|s| s.to_string()));
                prefix.extend(identity_args(k));
            }
            prefix.extend(words);
            prefix.push("-e".into());
            return Some(with(prefix));
        }
    }
    TERMINALS.iter().find(|t| installed(t.program)).map(|t| {
        let mut prefix = vec![t.program.to_string()];
        prefix.extend(t.subcommand.iter().map(|s| s.to_string()));
        prefix.extend(identity_args(t));
        prefix.extend(t.before.iter().map(|s| s.to_string()));
        with(prefix)
    })
}

/// Open rada in a new terminal window and return at once.
pub fn spawn_terminal(path: Option<&Path>) -> Result<()> {
    let exe = std::env::current_exe().context(Key::MNoExe.tr())?;
    let path = match path {
        Some(p) => Some(
            rada_core::fs::canonical_path(p)
                .with_context(|| tf(Key::MCannotOpen, &[&p.display()]))?,
        ),
        None => None,
    };
    let var = std::env::var("TERMINAL").ok();
    let installed = rada_core::platform::which::on_path;
    let cmd = terminal_command(&exe, path.as_deref(), var.as_deref(), &installed)
        .context(Key::MNoTerminal.tr())?;
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: closing the window that started us does not end it.
        c.process_group(0);
    }
    c.spawn()
        .with_context(|| tf(Key::MCannotStart, &[&cmd[0]]))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picks(term: Option<&str>, have: &[&str]) -> Option<Vec<String>> {
        let have: Vec<String> = have.iter().map(|s| s.to_string()).collect();
        terminal_command(
            Path::new("/usr/bin/rada"),
            Some(Path::new("/tmp/x")),
            term,
            &|n| have.iter().any(|h| h == n),
        )
    }

    #[test]
    fn the_terminal_is_chosen_in_the_documented_order() {
        let all = ["xdg-terminal-exec", "kitty", "ghostty", "xterm"];
        assert_eq!(picks(Some("kitty"), &all).unwrap()[0], "xdg-terminal-exec");
        let no_xte = ["kitty", "ghostty", "xterm"];
        assert_eq!(
            picks(Some("kitty"), &no_xte).unwrap(),
            [
                "kitty",
                "--class",
                "com.formikadesk.rada",
                "-e",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        // $TERMINAL with its own arguments after rada's identity, so that they win; and one
        // that is not installed.
        assert_eq!(
            picks(Some("alacritty --class mine"), &["alacritty"]).unwrap(),
            [
                "alacritty",
                "--class",
                "com.formikadesk.rada",
                "--class",
                "mine",
                "-e",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        assert_eq!(picks(Some("nothing-here"), &no_xte).unwrap()[0], "ghostty");
        // The fixed list is tried in its order: ghostty before kitty.
        assert_eq!(
            picks(None, &no_xte).unwrap(),
            [
                "ghostty",
                "--class=com.formikadesk.rada",
                "-e",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        assert!(picks(None, &["xterm"]).unwrap().ends_with(&[
            "-e".into(),
            "/usr/bin/rada".into(),
            "/tmp/x".into()
        ]));
        assert!(picks(None, &[]).is_none());
        // No path: just the program.
        let c = terminal_command(Path::new("/r"), None, None, &|n| n == "foot").unwrap();
        assert_eq!(c, ["foot", "--app-id=com.formikadesk.rada", "/r"]);
    }

    #[test]
    fn each_terminal_is_given_rada_s_identity_the_way_it_takes_one() {
        let one = |term: &str| picks(None, &[term]).unwrap();
        assert_eq!(
            one("ghostty"),
            [
                "ghostty",
                "--class=com.formikadesk.rada",
                "-e",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        assert_eq!(
            one("kitty"),
            [
                "kitty",
                "--class",
                "com.formikadesk.rada",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        assert_eq!(
            one("alacritty"),
            [
                "alacritty",
                "--class",
                "com.formikadesk.rada",
                "-e",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        // The class goes to `wezterm start`, before the `--` that ends its options.
        assert_eq!(
            one("wezterm"),
            [
                "wezterm",
                "start",
                "--class",
                "com.formikadesk.rada",
                "--",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        assert_eq!(
            one("foot"),
            [
                "foot",
                "--app-id=com.formikadesk.rada",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        // The others are started as before.
        assert_eq!(one("konsole"), ["konsole", "-e", "/usr/bin/rada", "/tmp/x"]);
        assert_eq!(
            one("gnome-terminal"),
            ["gnome-terminal", "--", "/usr/bin/rada", "/tmp/x"]
        );
        assert_eq!(one("xterm"), ["xterm", "-e", "/usr/bin/rada", "/tmp/x"]);
        // xdg-terminal-exec chooses for itself.
        assert_eq!(
            picks(None, &["xdg-terminal-exec", "foot"]).unwrap(),
            ["xdg-terminal-exec", "/usr/bin/rada", "/tmp/x"]
        );
    }

    #[test]
    fn a_terminal_named_in_the_terminal_variable_by_its_path_is_recognised() {
        let c = picks(Some("/opt/bin/foot --fullscreen"), &["/opt/bin/foot"]).unwrap();
        assert_eq!(
            c,
            [
                "/opt/bin/foot",
                "--app-id=com.formikadesk.rada",
                "--fullscreen",
                "-e",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
    }

    #[test]
    fn exec_lines_are_quoted_when_needed() {
        assert_eq!(
            exec_quote(Path::new("/home/u/.cargo/bin/rada")),
            "/home/u/.cargo/bin/rada"
        );
        assert_eq!(
            exec_quote(Path::new("/opt/my apps/rada")),
            "\"/opt/my apps/rada\""
        );
        assert_eq!(
            exec_quote(Path::new("/opt/a$b/rada")),
            "\"/opt/a\\$b/rada\""
        );
        let e = launcher_entry(DESKTOP_ENTRY, Path::new("/opt/my apps/rada"), false);
        assert!(
            e.contains("Exec=\"/opt/my apps/rada\" --spawn-terminal %F\n"),
            "{e}"
        );
        assert!(e.contains("Name=rada"));
        assert!(!e.contains("MimeType="), "{e}");
        let e = launcher_entry(DESKTOP_ENTRY, Path::new("/opt/rada"), true);
        assert!(e.contains("MimeType=inode/directory;\n"), "{e}");
    }

    #[test]
    fn the_shipped_entry_has_what_the_specification_asks_for() {
        for needle in [
            "Name=rada",
            "GenericName=File Manager",
            "Comment=A safe harbor for your files",
            "Exec=rada --spawn-terminal %F",
            "Icon=rada",
            "StartupWMClass=com.formikadesk.rada",
            "Terminal=false",
            "Categories=System;FileTools;FileManager;Utility;",
            "MimeType=inode/directory;",
            "Keywords=",
            "Keywords[it]=",
        ] {
            assert!(DESKTOP_ENTRY.contains(needle), "{needle}");
        }
    }
}

#[cfg(test)]
mod asset_tests {
    use std::path::Path;

    /// `packaging/linux/` is what distribution packagers look at; the same files are kept
    /// inside the crate so that it builds on its own. They must never drift apart.
    #[test]
    fn the_crate_assets_are_the_packaging_files() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        for name in [
            "rada.desktop",
            "rada.svg",
            "rada-48.png",
            "rada-128.png",
            "rada-256.png",
        ] {
            let a = std::fs::read(here.join("assets/linux").join(name)).unwrap();
            let b = std::fs::read(here.join("../../packaging/linux").join(name)).unwrap();
            assert!(
                a == b,
                "{name} differs between crates/rada/assets/linux and packaging/linux"
            );
        }
    }
}
