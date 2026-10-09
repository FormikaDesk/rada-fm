//! Linux desktop integration: a launcher entry, icons, and the terminal launcher the
//! entry uses. Everything is installed for the current user only, and the exact list of
//! files written is remembered so that `--remove` takes away precisely those.

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
            exe: std::env::current_exe().context("cannot find the rada executable")?,
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
        say.push(format!("installed {}", desktop.display()));
        written.push(desktop);
        for (path, bytes) in self.icon_files() {
            write_file(&path, bytes)?;
            say.push(format!("installed {}", path.display()));
            written.push(path);
        }
        for f in written {
            if !manifest.files.contains(&f) {
                manifest.files.push(f);
            }
        }

        match run(&self.update_db, &[self.data_home.join("applications").as_os_str()]) {
            Ok(_) => say.push("updated the desktop database".into()),
            Err(e) => say.push(format!(
                "note: could not update the desktop database ({e}); the entry appears after the next login"
            )),
        }

        if default_file_manager {
            let current = run(
                &self.xdg_mime,
                &["query".as_ref(), "default".as_ref(), DIR_MIME.as_ref()],
            )
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
            if current == DESKTOP_FILE {
                say.push("rada is already the default file manager".into());
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
                .context("xdg-mime could not set the default file manager")?;
                say.push(match current.as_str() {
                    "" => {
                        "rada is now the default file manager (there was none before)".to_string()
                    }
                    prev => format!("rada is now the default file manager (it was {prev})"),
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
                Ok(()) => say.push(format!("removed {}", f.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => say.push(format!("could not remove {}: {e}", f.display())),
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
                    .context("xdg-mime could not restore the previous file manager")?;
                    say.push(format!("restored {prev} as the default file manager"));
                }
            } else {
                say.push("the default file manager was changed since; left as it is".into());
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
                .with_context(|| format!("cannot update {}", path.display()))?;
            say.push(format!(
                "removed rada as the default file manager from {}",
                path.display()
            ));
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
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("{program} is not available"))?;
    if !out.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
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

/// One terminal emulator and how it is told to run a command.
struct Terminal {
    program: &'static str,
    /// Arguments that go between the program and the command to run.
    before: &'static [&'static str],
}

const TERMINALS: &[Terminal] = &[
    Terminal {
        program: "ghostty",
        before: &["-e"],
    },
    Terminal {
        program: "kitty",
        before: &[],
    },
    Terminal {
        program: "foot",
        before: &[],
    },
    Terminal {
        program: "alacritty",
        before: &["-e"],
    },
    Terminal {
        program: "wezterm",
        before: &["start", "--"],
    },
    Terminal {
        program: "konsole",
        before: &["-e"],
    },
    Terminal {
        program: "gnome-terminal",
        before: &["--"],
    },
    Terminal {
        program: "xterm",
        before: &["-e"],
    },
];

/// The command line that opens `rada` (at `path`) in a new terminal window, chosen in
/// this order: `xdg-terminal-exec`, `$TERMINAL`, then the first installed of a list.
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
            // also accept `-e`).
            let mut prefix = vec![prog];
            prefix.extend(words);
            prefix.push("-e".into());
            return Some(with(prefix));
        }
    }
    TERMINALS.iter().find(|t| installed(t.program)).map(|t| {
        let mut prefix = vec![t.program.to_string()];
        prefix.extend(t.before.iter().map(|s| s.to_string()));
        with(prefix)
    })
}

/// Open rada in a new terminal window and return at once.
pub fn spawn_terminal(path: Option<&Path>) -> Result<()> {
    let exe = std::env::current_exe().context("cannot find the rada executable")?;
    let path = match path {
        Some(p) => {
            Some(std::fs::canonicalize(p).with_context(|| format!("cannot open {}", p.display()))?)
        }
        None => None,
    };
    let var = std::env::var("TERMINAL").ok();
    let installed = |name: &str| which(name).is_some();
    let cmd = terminal_command(&exe, path.as_deref(), var.as_deref(), &installed).context(
        "no terminal emulator found: install one, or set $TERMINAL (for example TERMINAL=kitty)",
    )?;
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
        .with_context(|| format!("cannot start {}", cmd[0]))?;
    Ok(())
}

fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| p.is_file() && is_executable(p))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    true
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
            ["kitty", "-e", "/usr/bin/rada", "/tmp/x"]
        );
        // $TERMINAL with its own arguments, and one that is not installed.
        assert_eq!(
            picks(Some("alacritty --class rada"), &["alacritty"]).unwrap(),
            [
                "alacritty",
                "--class",
                "rada",
                "-e",
                "/usr/bin/rada",
                "/tmp/x"
            ]
        );
        assert_eq!(picks(Some("nothing-here"), &no_xte).unwrap()[0], "ghostty");
        // The fixed list is tried in its order: ghostty before kitty.
        assert_eq!(
            picks(None, &no_xte).unwrap(),
            ["ghostty", "-e", "/usr/bin/rada", "/tmp/x"]
        );
        assert_eq!(picks(None, &["xterm"]).unwrap()[0], "xterm");
        assert_eq!(
            picks(None, &["foot"]).unwrap(),
            ["foot", "/usr/bin/rada", "/tmp/x"]
        );
        assert_eq!(
            picks(None, &["wezterm"]).unwrap(),
            ["wezterm", "start", "--", "/usr/bin/rada", "/tmp/x"]
        );
        assert!(picks(None, &[]).is_none());
        // No path: just the program.
        let c = terminal_command(Path::new("/r"), None, None, &|n| n == "foot").unwrap();
        assert_eq!(c, ["foot", "/r"]);
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
