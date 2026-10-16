//! `rada setup file-picker`: make rada the file chooser of the desktop (Linux), through
//! xdg-desktop-portal-termfilechooser.
//!
//! Three files are involved, all in the user's own configuration folder: the script the portal
//! runs (`rada-wrapper.sh`), the portal's configuration (`cmd=` points at the script) and the
//! file that tells xdg-desktop-portal which backend answers for file choosers. Nothing is
//! written before the person has seen what and where and said yes; a file that exists is
//! copied aside first, and `--remove` puts everything back. The program asks for no rights, and
//! installs no package: when the portal itself is missing it says which one to install.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rada_core::i18n::{Key, tf};
use rada_core::platform::Dirs;
use serde::{Deserialize, Serialize};

/// The script the portal runs: `contrib/rada-wrapper.sh` of the source.
const WRAPPER: &str = include_str!("../../../contrib/rada-wrapper.sh");
const WRAPPER_NAME: &str = "rada-wrapper.sh";
const PORTAL_DIR: &str = "xdg-desktop-portal-termfilechooser";
const FILE_CHOOSER: &str = "org.freedesktop.impl.portal.FileChooser";
const MANIFEST: &str = "file-picker-install.json";

/// One file as it was written, so that `--remove` can tell whether it is still that one.
#[derive(Clone, Serialize, Deserialize)]
struct Written {
    path: PathBuf,
    /// The copy of what was there before; `None` when the file was made by the setup.
    backup: Option<PathBuf>,
    content: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    files: Vec<Written>,
}

/// Where things go (replaceable in tests).
pub struct FilePicker {
    pub config_home: PathBuf,
    pub state_dir: PathBuf,
    /// The program the script starts.
    pub exe: PathBuf,
    /// `XDG_CURRENT_DESKTOP`.
    pub desktops: String,
    /// Where the system keeps its portal files and defaults (`/usr/share/xdg-desktop-portal`…).
    pub system_dirs: Vec<PathBuf>,
}

/// What one file would become.
struct Planned {
    path: PathBuf,
    old: Option<String>,
    new: String,
    executable: bool,
    /// Show the whole text (the script is long and is in the source).
    show_text: bool,
}

impl Planned {
    fn changes(&self) -> bool {
        self.old.as_deref() != Some(self.new.as_str())
    }
}

impl FilePicker {
    pub fn from_env(dirs: &Dirs) -> Result<FilePicker> {
        let share: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into())
            .split(':')
            .filter(|d| Path::new(d).is_absolute())
            .map(PathBuf::from)
            .collect();
        let mut system_dirs: Vec<PathBuf> = share.into_iter().collect();
        system_dirs.insert(0, PathBuf::from("/etc/xdg"));
        Ok(FilePicker {
            config_home: dirs.config.clone(),
            state_dir: dirs.rada_state(),
            exe: std::env::current_exe().context(Key::MNoExe.tr())?,
            desktops: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            system_dirs,
        })
    }

    fn wrapper_path(&self) -> PathBuf {
        self.config_home.join(PORTAL_DIR).join(WRAPPER_NAME)
    }

    fn config_path(&self) -> PathBuf {
        self.config_home.join(PORTAL_DIR).join("config")
    }

    /// The first desktop named in `XDG_CURRENT_DESKTOP`, in lower case, or none.
    fn desktop(&self) -> Option<String> {
        self.desktops
            .split(':')
            .map(|d| d.trim().to_lowercase())
            .find(|d| {
                !d.is_empty()
                    && d.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            })
    }

    fn portals_conf_name(&self) -> String {
        match self.desktop() {
            Some(d) => format!("{d}-portals.conf"),
            None => "portals.conf".into(),
        }
    }

    fn portals_conf_path(&self) -> PathBuf {
        self.config_home
            .join("xdg-desktop-portal")
            .join(self.portals_conf_name())
    }

    /// What the system itself says for this desktop: a file of ours only replaces it, so it is
    /// where ours starts when there is none of the user's own.
    fn system_portals_conf(&self) -> Option<String> {
        let name = self.portals_conf_name();
        self.system_dirs.iter().find_map(|d| {
            [d.join("xdg-desktop-portal").join(&name), d.join(&name)]
                .iter()
                .find_map(|p| std::fs::read_to_string(p).ok())
        })
    }

    /// Whether the portal itself is installed: its `termfilechooser.portal` is somewhere.
    pub fn portal_installed(&self) -> bool {
        self.system_dirs.iter().any(|d| {
            d.join("xdg-desktop-portal/portals/termfilechooser.portal")
                .is_file()
        })
    }

    fn read(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn plan(&self) -> Vec<Planned> {
        let wrapper = self.wrapper_path();
        let config = self.config_path();
        let portals = self.portals_conf_path();
        let config_old = Self::read(&config);
        let config_new = merge_ini(
            config_old.as_deref().unwrap_or(""),
            "filechooser",
            &[
                Want::key("cmd", &wrapper.to_string_lossy()),
                Want::key("create_help_file", "0"),
                Want::env("RADA_BIN", &self.exe.to_string_lossy()),
            ],
        );
        let portals_old = Self::read(&portals);
        let portals_base = portals_old
            .clone()
            .or_else(|| self.system_portals_conf())
            .unwrap_or_default();
        let portals_new = merge_ini(
            &portals_base,
            "preferred",
            &[Want::key(FILE_CHOOSER, "termfilechooser")],
        );
        vec![
            Planned {
                old: Self::read(&wrapper),
                new: WRAPPER.to_string(),
                path: wrapper,
                executable: true,
                show_text: false,
            },
            Planned {
                path: config,
                old: config_old,
                new: config_new,
                executable: false,
                show_text: true,
            },
            Planned {
                path: portals,
                old: portals_old,
                new: portals_new,
                executable: false,
                show_text: true,
            },
        ]
    }

    /// Where a copy of `path` goes: the first name that is free.
    fn backup_name(path: &Path) -> PathBuf {
        let base = format!("{}.rada-backup", path.display());
        let mut candidate = PathBuf::from(&base);
        let mut n = 1;
        while candidate.exists() {
            candidate = PathBuf::from(format!("{base}.{n}"));
            n += 1;
        }
        candidate
    }

    fn manifest_path(&self) -> PathBuf {
        self.state_dir.join(MANIFEST)
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
            false,
        )
    }

    /// What would be written, for the person to read.
    fn describe(&self, plan: &[Planned]) -> Vec<String> {
        let mut out = vec![Key::FpIntro.tr().to_string(), String::new()];
        for p in plan {
            if !p.changes() {
                out.push(tf(Key::FpUnchanged, &[&p.path.display()]));
                continue;
            }
            match &p.old {
                None => out.push(tf(Key::FpCreate, &[&p.path.display()])),
                Some(_) => out.push(tf(
                    Key::FpChange,
                    &[&p.path.display(), &Self::backup_name(&p.path).display()],
                )),
            }
            if p.show_text {
                let old: Vec<&str> = p.old.as_deref().unwrap_or("").lines().collect();
                for line in p.new.lines() {
                    let mark = if old.contains(&line) { ' ' } else { '+' };
                    out.push(format!("  {mark} {line}"));
                }
            } else {
                out.push(format!("    ({} lines)", p.new.lines().count()));
            }
            out.push(String::new());
        }
        out.push(Key::FpNoSudo.tr().to_string());
        if !self.portal_installed() {
            out.push(Key::FpPortalMissing.tr().to_string());
        }
        out
    }

    /// Write what was planned: a copy of each file that exists and changes, then the file.
    fn install(&self, plan: &[Planned]) -> Result<Vec<String>> {
        let mut say = Vec::new();
        let mut manifest = self.read_manifest();
        for p in plan.iter().filter(|p| p.changes()) {
            let backup = match &p.old {
                Some(old) => {
                    let to = Self::backup_name(&p.path);
                    write_file(&to, old.as_bytes(), false)?;
                    say.push(tf(Key::FpKeptCopy, &[&to.display()]));
                    Some(to)
                }
                None => None,
            };
            write_file(&p.path, p.new.as_bytes(), p.executable)?;
            say.push(tf(Key::FpWrote, &[&p.path.display()]));
            // Setting up again keeps the first copy: that is what `--remove` goes back to.
            match manifest.files.iter_mut().find(|w| w.path == p.path) {
                Some(w) => {
                    w.content = p.new.clone();
                    if w.backup.is_none() && backup.is_some() {
                        w.backup = backup;
                    }
                }
                None => manifest.files.push(Written {
                    path: p.path.clone(),
                    backup,
                    content: p.new.clone(),
                }),
            }
        }
        self.save_manifest(&manifest)?;
        say.push(Key::FpNext.tr().to_string());
        Ok(say)
    }

    /// Put everything back: the copies where there were any, nothing where the setup made the
    /// file. A file that was changed since is left alone.
    pub fn remove(&self) -> Result<Vec<String>> {
        let manifest = self.read_manifest();
        if manifest.files.is_empty() {
            return Ok(vec![Key::FpNothingToRemove.tr().to_string()]);
        }
        let mut say = Vec::new();
        for w in manifest.files.iter().rev() {
            match std::fs::read_to_string(&w.path) {
                Ok(now) if now == w.content => match &w.backup {
                    Some(b) => {
                        let old = std::fs::read(b)
                            .with_context(|| tf(Key::MCannotRead, &[&b.display()]))?;
                        write_file(&w.path, &old, is_executable(&w.path))?;
                        let _ = std::fs::remove_file(b);
                        say.push(tf(Key::FpRestored, &[&w.path.display(), &b.display()]));
                    }
                    None => {
                        std::fs::remove_file(&w.path)
                            .with_context(|| tf(Key::MCannotWrite, &[&w.path.display()]))?;
                        say.push(tf(Key::SayRemoved, &[&w.path.display()]));
                    }
                },
                Ok(_) => say.push(tf(Key::FpChangedSince, &[&w.path.display()])),
                // Already gone: nothing to take away, and the copy (if any) stays where it is.
                Err(_) => {}
            }
        }
        let _ = std::fs::remove_file(self.manifest_path());
        say.push(Key::FpAfterRemove.tr().to_string());
        Ok(say)
    }
}

/// The command: show, ask, write; or with `remove`, go back. `input` is where the answer is
/// read from.
pub fn run(dirs: &Dirs, remove: bool, yes: bool, input: &mut dyn BufRead) -> Result<Vec<String>> {
    if cfg!(any(windows, target_os = "macos")) {
        return Ok(vec![Key::FpLinuxOnly.tr().to_string()]);
    }
    let setup = FilePicker::from_env(dirs)?;
    if remove {
        return setup.remove();
    }
    let plan = setup.plan();
    for line in setup.describe(&plan) {
        println!("{line}");
    }
    if plan.iter().all(|p| !p.changes()) {
        return Ok(vec![Key::FpNext.tr().to_string()]);
    }
    if !yes {
        print!("{}", Key::FpAsk.tr());
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut answer = String::new();
        input.read_line(&mut answer).ok();
        if !is_yes(&answer) {
            return Ok(vec![String::new(), Key::FpAborted.tr().to_string()]);
        }
    }
    setup.install(&plan)
}

/// `y`, `yes`, `s`, `si`, `sì`.
fn is_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "yes" | "s" | "si" | "sì"
    )
}

/// A line to put in a section of an ini file.
struct Want<'a> {
    key: &'a str,
    value: String,
    /// For `env=NAME=value`: the name that tells which of several `env=` lines is ours.
    env_name: Option<&'a str>,
}

impl<'a> Want<'a> {
    fn key(key: &'a str, value: &str) -> Want<'a> {
        Want {
            key,
            value: value.to_string(),
            env_name: None,
        }
    }

    fn env(name: &'a str, value: &str) -> Want<'a> {
        Want {
            key: "env",
            value: format!("{name}={value}"),
            env_name: Some(name),
        }
    }

    /// Is this line (of the section) the one this stands for?
    fn is_line(&self, line: &str) -> bool {
        let t = line.trim();
        if t.starts_with('#') || t.starts_with(';') {
            return false;
        }
        let Some((k, v)) = t.split_once('=') else {
            return false;
        };
        k.trim() == self.key
            && match self.env_name {
                Some(name) => v
                    .trim()
                    .split_once('=')
                    .is_some_and(|(n, _)| n.trim() == name),
                None => true,
            }
    }

    fn line(&self) -> String {
        format!("{}={}", self.key, self.value)
    }
}

/// `text` with each wanted line in `section`: replaced where the file has it (one copy of it),
/// added at the end of the section where it does not, and the section made if it is missing.
/// Everything else (other keys, comments, other sections) stays as it was.
fn merge_ini(text: &str, section: &str, wants: &[Want]) -> String {
    let header = format!("[{section}]");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let start = lines.iter().position(|l| l.trim() == header);
    let Some(start) = start else {
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(header);
        lines.extend(wants.iter().map(Want::line));
        return lines.join("\n") + "\n";
    };
    // The section runs to the next header.
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);
    let mut body: Vec<String> = lines[start + 1..end].to_vec();
    for w in wants {
        let mut first = true;
        body = body
            .into_iter()
            .filter_map(|l| {
                if !w.is_line(&l) {
                    Some(l)
                } else if first {
                    first = false;
                    Some(w.line())
                } else {
                    None
                }
            })
            .collect();
        if first {
            // Not there: after the last line that says something.
            let at = body
                .iter()
                .rposition(|l| !l.trim().is_empty())
                .map_or(0, |i| i + 1);
            body.insert(at, w.line());
        }
    }
    let mut out: Vec<String> = lines[..=start].to_vec();
    out.extend(body);
    out.extend(lines[end..].iter().cloned());
    out.join("\n") + "\n"
}

fn write_file(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| tf(Key::MCannotCreate, &[&dir.display()]))?;
    }
    std::fs::write(path, bytes).with_context(|| tf(Key::MCannotWrite, &[&path.display()]))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .with_context(|| tf(Key::MCannotWrite, &[&path.display()]))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(root: &Path, desktops: &str) -> FilePicker {
        FilePicker {
            config_home: root.join("config"),
            state_dir: root.join("state"),
            exe: PathBuf::from("/opt/rada/bin/rada"),
            desktops: desktops.into(),
            system_dirs: vec![root.join("system")],
        }
    }

    fn tmp() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("rada-fp-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap()
    }

    fn all(setup: &FilePicker) -> Vec<String> {
        let plan = setup.plan();
        setup.install(&plan).unwrap()
    }

    #[test]
    fn a_missing_file_is_made_with_the_lines_wanted() {
        let out = merge_ini("", "filechooser", &[Want::key("cmd", "/x/w.sh")]);
        assert_eq!(out, "[filechooser]\ncmd=/x/w.sh\n");
    }

    #[test]
    fn a_line_is_replaced_once_and_the_rest_is_kept() {
        let old = "# mine\n[filechooser]\ncmd=yazi-wrapper.sh\ndefault_dir=$HOME\n; create_help_file=0\ncmd=again\n\n[other]\ncmd=keep\n";
        let out = merge_ini(
            old,
            "filechooser",
            &[
                Want::key("cmd", "/x/w.sh"),
                Want::key("create_help_file", "0"),
            ],
        );
        assert_eq!(
            out,
            "# mine\n[filechooser]\ncmd=/x/w.sh\ndefault_dir=$HOME\n; create_help_file=0\ncreate_help_file=0\n\n[other]\ncmd=keep\n"
        );
        // Run again, nothing moves.
        let again = merge_ini(
            &out,
            "filechooser",
            &[
                Want::key("cmd", "/x/w.sh"),
                Want::key("create_help_file", "0"),
            ],
        );
        assert_eq!(again, out);
    }

    #[test]
    fn only_our_env_line_is_replaced() {
        let old = "[filechooser]\nenv=TERMCMD=foot\nenv=RADA_BIN=/old/rada\n";
        let out = merge_ini(old, "filechooser", &[Want::env("RADA_BIN", "/new/rada")]);
        assert_eq!(
            out,
            "[filechooser]\nenv=TERMCMD=foot\nenv=RADA_BIN=/new/rada\n"
        );
    }

    #[test]
    fn a_section_that_is_missing_is_added_at_the_end() {
        let out = merge_ini(
            "[preferred]\ndefault=hyprland;gtk",
            "preferred",
            &[Want::key(FILE_CHOOSER, "termfilechooser")],
        );
        assert_eq!(
            out,
            "[preferred]\ndefault=hyprland;gtk\norg.freedesktop.impl.portal.FileChooser=termfilechooser\n"
        );
        let out = merge_ini("[x]\na=1\n", "preferred", &[Want::key("k", "v")]);
        assert_eq!(out, "[x]\na=1\n\n[preferred]\nk=v\n");
    }

    #[test]
    fn the_desktop_names_the_file() {
        let t = tmp();
        assert_eq!(
            picker(t.path(), "Hyprland").portals_conf_name(),
            "hyprland-portals.conf"
        );
        assert_eq!(
            picker(t.path(), "Budgie:GNOME").portals_conf_name(),
            "budgie-portals.conf"
        );
        assert_eq!(picker(t.path(), "").portals_conf_name(), "portals.conf");
        // Nothing that is not a name makes a path.
        assert_eq!(picker(t.path(), "../x").portals_conf_name(), "portals.conf");
    }

    #[test]
    fn the_setup_writes_three_files_and_starts_from_the_systems_defaults() {
        let t = tmp();
        std::fs::create_dir_all(t.path().join("system/xdg-desktop-portal")).unwrap();
        std::fs::write(
            t.path()
                .join("system/xdg-desktop-portal/hyprland-portals.conf"),
            "[preferred]\ndefault=hyprland;gtk",
        )
        .unwrap();
        let s = picker(t.path(), "Hyprland");
        let say = all(&s);
        assert!(say.last().unwrap().contains("systemctl --user restart"));
        let wrapper = t
            .path()
            .join("config/xdg-desktop-portal-termfilechooser/rada-wrapper.sh");
        assert_eq!(std::fs::read_to_string(&wrapper).unwrap(), WRAPPER);
        #[cfg(unix)]
        assert!(is_executable(&wrapper));
        let config = std::fs::read_to_string(
            t.path()
                .join("config/xdg-desktop-portal-termfilechooser/config"),
        )
        .unwrap();
        assert!(
            config.contains(&format!("cmd={}", wrapper.display())),
            "{config}"
        );
        assert!(config.contains("create_help_file=0"));
        assert!(config.contains("env=RADA_BIN=/opt/rada/bin/rada"));
        let portals = std::fs::read_to_string(
            t.path()
                .join("config/xdg-desktop-portal/hyprland-portals.conf"),
        )
        .unwrap();
        assert_eq!(
            portals,
            "[preferred]\ndefault=hyprland;gtk\norg.freedesktop.impl.portal.FileChooser=termfilechooser\n"
        );
    }

    #[test]
    fn what_was_there_is_copied_first_and_comes_back() {
        let t = tmp();
        let s = picker(t.path(), "Hyprland");
        let config = s.config_path();
        let portals = s.portals_conf_path();
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(portals.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            "[filechooser]\ncmd=yazi-wrapper.sh\nenv=TERMCMD=foot\n",
        )
        .unwrap();
        std::fs::write(&portals, "[preferred]\ndefault=gtk\n").unwrap();
        let say = all(&s);
        assert_eq!(say.iter().filter(|l| l.starts_with("copy kept")).count(), 2);
        let kept = PathBuf::from(format!("{}.rada-backup", config.display()));
        assert_eq!(
            std::fs::read_to_string(&kept).unwrap(),
            "[filechooser]\ncmd=yazi-wrapper.sh\nenv=TERMCMD=foot\n"
        );
        // The user's own lines stay in the new file.
        assert!(
            std::fs::read_to_string(&config)
                .unwrap()
                .contains("env=TERMCMD=foot")
        );

        // Again: nothing changes, nothing is copied.
        let plan = s.plan();
        assert!(plan.iter().all(|p| !p.changes()));
        s.install(&plan).unwrap();
        assert!(!PathBuf::from(format!("{}.rada-backup.1", config.display())).exists());

        let say = s.remove().unwrap();
        assert!(say.iter().any(|l| l.starts_with("restored")), "{say:?}");
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "[filechooser]\ncmd=yazi-wrapper.sh\nenv=TERMCMD=foot\n"
        );
        assert_eq!(
            std::fs::read_to_string(&portals).unwrap(),
            "[preferred]\ndefault=gtk\n"
        );
        assert!(!kept.exists());
        assert!(
            !s.wrapper_path().exists(),
            "the script was made by the setup"
        );
        assert!(!s.manifest_path().exists());
    }

    #[test]
    fn a_file_changed_since_is_left_as_it_is() {
        let t = tmp();
        let s = picker(t.path(), "");
        all(&s);
        std::fs::write(s.config_path(), "[filechooser]\ncmd=mine\n").unwrap();
        let say = s.remove().unwrap();
        assert!(
            say.iter()
                .any(|l| l.contains("was changed after the setup")),
            "{say:?}"
        );
        assert_eq!(
            std::fs::read_to_string(s.config_path()).unwrap(),
            "[filechooser]\ncmd=mine\n"
        );
        assert!(!s.wrapper_path().exists());
    }

    #[test]
    fn removing_what_was_never_set_up_says_so() {
        let t = tmp();
        let s = picker(t.path(), "");
        assert!(s.remove().unwrap()[0].contains("nothing to remove"));
    }

    #[test]
    fn the_description_shows_the_new_lines_and_where_they_go() {
        let t = tmp();
        let s = picker(t.path(), "Hyprland");
        let text = s.describe(&s.plan()).join("\n");
        assert!(text.contains("create "), "{text}");
        assert!(text.contains("hyprland-portals.conf"), "{text}");
        assert!(text.contains("+ org.freedesktop.impl.portal.FileChooser=termfilechooser"));
        assert!(
            text.contains("Nothing needs administrator rights"),
            "{text}"
        );
        // The portal is not there in this sandbox: the line says which one to install.
        assert!(text.contains("xdg-desktop-portal-termfilechooser-hunkyburrito-git"));
        // Nothing has been written by describing.
        assert!(!s.config_path().exists());
    }

    #[test]
    fn an_installed_portal_is_not_mentioned() {
        let t = tmp();
        let s = picker(t.path(), "");
        let d = t.path().join("system/xdg-desktop-portal/portals");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("termfilechooser.portal"), "[portal]\n").unwrap();
        assert!(s.portal_installed());
        assert!(!s.describe(&s.plan()).join("\n").contains("hunkyburrito"));
    }

    #[test]
    fn the_answers_that_mean_yes() {
        for y in ["y", "Y\n", "yes", "s", "sì", "si", " SI "] {
            assert!(is_yes(y), "{y:?}");
        }
        for n in ["", "n", "no", "\n", "maybe"] {
            assert!(!is_yes(n), "{n:?}");
        }
    }
}
