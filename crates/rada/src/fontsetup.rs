//! `rada setup font`: JetBrainsMono Nerd Font Mono for the person's own account, so that the
//! file-type icons draw.
//!
//! The four faces and the licence are downloaded from the Nerd Fonts repository at a fixed
//! release, each checked against a SHA-256 written in this file; nothing is installed unless
//! every file matches. No administrator rights are needed:
//!
//! | System  | Where                                              |
//! |---------|----------------------------------------------------|
//! | Linux   | `~/.local/share/fonts/rada-jetbrainsmono-nerd/`, then `fc-cache` |
//! | macOS   | `~/Library/Fonts/`                                  |
//! | Windows | `%LOCALAPPDATA%\Microsoft\Windows\Fonts\` and the per-user list in the registry (HKCU) |
//!
//! The font is licensed under the SIL Open Font License 1.1; the licence is installed with it.

use rada_core::i18n::{Key, tf};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use rada_core::platform::Dirs;
use rada_core::update::http::Http;
use serde::{Deserialize, Serialize};

/// The release of Nerd Fonts the files are taken from.
pub const RELEASE: &str = "v3.4.0";

/// Where the files are, at that release.
pub const BASE_URL: &str = "https://raw.githubusercontent.com/ryanoasis/nerd-fonts/v3.4.0/patched-fonts/JetBrainsMono/Ligatures";

/// The name to choose in a terminal's font setting.
pub const FAMILY: &str = "JetBrainsMono Nerd Font Mono";

/// One file to download.
#[derive(Clone, Debug)]
pub struct Face {
    /// Where it is under the base address (`Regular/JetBrainsMonoNerdFontMono-Regular.ttf`).
    pub remote: String,
    /// What it is called once installed.
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

/// The four faces rada installs.
pub fn faces() -> Vec<Face> {
    let f = |dir: &str, name: &str, sha: &str, size: u64| Face {
        remote: format!("{dir}/{name}"),
        file: name.to_string(),
        sha256: sha.to_string(),
        size,
    };
    vec![
        f(
            "Regular",
            "JetBrainsMonoNerdFontMono-Regular.ttf",
            "f01031f40e48dc29e1112e6b0b0450a2c6cd097f3f35cfff05c55cb311f8034c",
            2_470_116,
        ),
        f(
            "Bold",
            "JetBrainsMonoNerdFontMono-Bold.ttf",
            "5bdd4a873f3cd32f882d2c55545089123926e27707d5880fc9eaf84eb01b6686",
            2_473_884,
        ),
        f(
            "Italic",
            "JetBrainsMonoNerdFontMono-Italic.ttf",
            "ccd88b36d325e6a905edc8dd3f2522718d9690d9bed3fbb4684c7e746c34f846",
            2_472_232,
        ),
        f(
            "BoldItalic",
            "JetBrainsMonoNerdFontMono-BoldItalic.ttf",
            "d931df2928b3216892d35980cddcad9edade1b9c9cd2e09a6c2937139f474742",
            2_475_144,
        ),
    ]
}

/// The licence of the font (SIL OFL 1.1), installed beside it.
pub fn license() -> Face {
    Face {
        remote: "Regular/OFL.txt".into(),
        file: "JetBrainsMonoNerdFont-OFL.txt".into(),
        sha256: "30f0c136e3c88e422d0791acd97238870f9054a9729bc34cf2ff0d4ed8cac4ad".into(),
        size: 4_399,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// The per-user list of fonts in the Windows registry.
pub trait Registry {
    fn set(&self, name: &str, file: &Path) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
}

/// `reg.exe`, which every Windows has: the registry of the person, no privileges.
pub struct RegExe;

const REG_KEY: &str = r"HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts";

impl Registry for RegExe {
    fn set(&self, name: &str, file: &Path) -> Result<()> {
        run(
            "reg",
            &[
                "add",
                REG_KEY,
                "/v",
                name,
                "/t",
                "REG_SZ",
                "/d",
                &file.to_string_lossy(),
                "/f",
            ],
        )
        .map(|_| ())
    }

    fn remove(&self, name: &str) -> Result<()> {
        run("reg", &["delete", REG_KEY, "/v", name, "/f"]).map(|_| ())
    }
}

/// The name of a face in that list, as Windows writes them.
pub fn registry_name(file: &str) -> String {
    let style = file
        .trim_end_matches(".ttf")
        .rsplit('-')
        .next()
        .unwrap_or("Regular");
    format!("{FAMILY} {style} (TrueType)")
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    files: Vec<PathBuf>,
    registry: Vec<String>,
}

pub struct FontSetup {
    pub os: Os,
    pub fonts_dir: PathBuf,
    pub state_dir: PathBuf,
    pub base_url: String,
    pub faces: Vec<Face>,
    pub license: Face,
    /// Program that refreshes the font cache (Linux); `None` to skip.
    pub fc_cache: Option<String>,
    pub registry: Option<Box<dyn Registry>>,
    pub user_agent: String,
}

impl FontSetup {
    pub fn for_this_system(dirs: &Dirs) -> FontSetup {
        let os = Os::current();
        FontSetup {
            os,
            fonts_dir: fonts_dir(os, dirs),
            state_dir: dirs.rada_state(),
            base_url: BASE_URL.into(),
            faces: faces(),
            license: license(),
            fc_cache: (os == Os::Linux).then(|| "fc-cache".to_string()),
            registry: (os == Os::Windows).then(|| Box::new(RegExe) as Box<dyn Registry>),
            user_agent: format!("rada/{}", env!("CARGO_PKG_VERSION")),
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.state_dir.join("font-install.json")
    }

    fn read_manifest(&self) -> Manifest {
        std::fs::read_to_string(self.manifest_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Whether every face is in place.
    pub fn is_installed(&self) -> bool {
        self.faces
            .iter()
            .all(|f| self.fonts_dir.join(&f.file).is_file())
    }

    /// Download, check and install. `say` is told what is going on.
    pub fn install(&self, say: &mut dyn FnMut(String)) -> Result<()> {
        if self.is_installed() {
            say(tf(
                Key::SayFontAlready,
                &[&FAMILY, &self.fonts_dir.display()],
            ));
            return Ok(());
        }
        let total: u64 = self.faces.iter().map(|f| f.size).sum::<u64>() + self.license.size;
        say(tf(
            Key::SayFontDownloading,
            &[
                &FAMILY,
                &RELEASE,
                &rada_core::i18n::decimal(&format!("{:.1}", total as f64 / 1_000_000.0)),
            ],
        ));
        let work = self
            .state_dir
            .join(format!("font-work-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work)
            .with_context(|| tf(Key::MCannotCreate, &[&work.display()]))?;
        let result = self
            .fetch_all(&work, say)
            .and_then(|()| self.place(&work, say));
        let _ = std::fs::remove_dir_all(&work);
        result
    }

    fn fetch_all(&self, work: &Path, say: &mut dyn FnMut(String)) -> Result<()> {
        let http = Http::new(&self.user_agent, self.base_url.starts_with("http://"));
        for face in self.faces.iter().chain(std::iter::once(&self.license)) {
            say(format!("  {}", face.file));
            let dest = work.join(&face.file);
            let url = format!("{}/{}", self.base_url, face.remote);
            http.download(
                &url,
                &dest,
                face.size.max(1) * 2 + (1 << 20),
                &mut |_, _| true,
            )
            .map_err(anyhow::Error::msg)?;
            let got = rada_core::update::sha256_of(&dest).map_err(anyhow::Error::msg)?;
            if got != face.sha256 {
                bail!(
                    "{}",
                    tf(Key::MFontChecksum, &[&face.file, &face.sha256, &got])
                );
            }
            if face.file.ends_with(".ttf") && !looks_like_a_font(&dest)? {
                bail!("{}", tf(Key::MFontNotAFont, &[&face.file]));
            }
        }
        Ok(())
    }

    fn place(&self, work: &Path, say: &mut dyn FnMut(String)) -> Result<()> {
        std::fs::create_dir_all(&self.fonts_dir)
            .with_context(|| tf(Key::MCannotCreate, &[&self.fonts_dir.display()]))?;
        let mut manifest = self.read_manifest();
        for face in self.faces.iter().chain(std::iter::once(&self.license)) {
            let to = self.fonts_dir.join(&face.file);
            move_file(&work.join(&face.file), &to)?;
            if !manifest.files.contains(&to) {
                manifest.files.push(to.clone());
            }
            if let Some(reg) = &self.registry
                && face.file.ends_with(".ttf")
            {
                let name = registry_name(&face.file);
                reg.set(&name, &to)
                    .with_context(|| tf(Key::MCannotRegisterFont, &[&name]))?;
                if !manifest.registry.contains(&name) {
                    manifest.registry.push(name);
                }
            }
        }
        std::fs::write(
            self.manifest_path(),
            serde_json::to_string_pretty(&manifest)?,
        )
        .with_context(|| tf(Key::MCannotWrite, &[&self.manifest_path().display()]))?;
        if let Some(fc) = &self.fc_cache {
            match run(fc, &["-f", &self.fonts_dir.to_string_lossy()]) {
                Ok(_) => say(Key::SayFontCache.tr().to_string()),
                Err(e) => say(tf(Key::SayFontCacheNote, &[&e])),
            }
        }
        say(tf(
            Key::SayFontInstalled,
            &[&FAMILY, &self.fonts_dir.display()],
        ));
        say(tf(
            Key::SayFontLicence,
            &[&self.fonts_dir.join(&self.license.file).display()],
        ));
        say(tf(Key::SayFontChoose, &[&FAMILY]));
        Ok(())
    }

    /// Take away what `install` put.
    pub fn remove(&self, say: &mut dyn FnMut(String)) -> Result<()> {
        let manifest = self.read_manifest();
        let mut any = false;
        for f in &manifest.files {
            match std::fs::remove_file(f) {
                Ok(()) => {
                    say(tf(Key::SayRemoved, &[&f.display()]));
                    any = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => say(tf(Key::SayCouldNotRemove, &[&f.display(), &e])),
            }
        }
        if let Some(reg) = &self.registry {
            for name in &manifest.registry {
                match reg.remove(name) {
                    Ok(()) => any = true,
                    Err(e) => say(tf(Key::SayFontUnregister, &[&name, &e])),
                }
            }
        }
        // The folder rada made on Linux, if nothing else is in it.
        if self.os == Os::Linux {
            let _ = std::fs::remove_dir(&self.fonts_dir);
            if let Some(fc) = &self.fc_cache {
                let _ = run(fc, &["-f"]);
            }
        }
        let _ = std::fs::remove_file(self.manifest_path());
        if !any {
            say(Key::SayNothingToRemove.tr().to_string());
        }
        Ok(())
    }
}

/// Where the fonts of one person go.
pub fn fonts_dir(os: Os, dirs: &Dirs) -> PathBuf {
    match os {
        Os::Linux => dirs.data.join("fonts/rada-jetbrainsmono-nerd"),
        Os::Mac => dirs.home.join("Library/Fonts"),
        // The folder Windows 10 and 11 give each account for fonts it installed itself.
        Os::Windows => dirs.data.join("Microsoft/Windows/Fonts"),
    }
}

fn looks_like_a_font(path: &Path) -> Result<bool> {
    use std::io::Read;
    let mut head = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .with_context(|| tf(Key::MCannotRead, &[&path.display()]))?;
    // TrueType, OpenType with PostScript outlines, a TrueType collection, old Mac TrueType.
    Ok(matches!(&head, [0, 1, 0, 0] | b"OTTO" | b"ttcf" | b"true"))
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Another filesystem: copy, then remove.
    std::fs::copy(from, to).with_context(|| tf(Key::MCannotWrite, &[&to.display()]))?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

fn run(program: &str, args: &[&str]) -> Result<String> {
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

/// Whether the terminal this process runs in is Ghostty, which ships with JetBrains Mono and a
/// Nerd Font symbols fallback: the icons draw there with no setup.
pub fn running_in_ghostty() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|v| v.eq_ignore_ascii_case("ghostty"))
        || std::env::var("TERM").is_ok_and(|v| v.contains("ghostty"))
        || std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rada_core::testweb::Server;
    use sha2::{Digest, Sha256};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn sha(b: &[u8]) -> String {
        Sha256::digest(b)
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    }

    /// A TrueType-looking file of the given size, different for each name.
    fn ttf(name: &str) -> Vec<u8> {
        let mut v = vec![0, 1, 0, 0];
        v.extend(name.bytes().cycle().take(5000));
        v
    }

    struct World {
        _dir: tempfile::TempDir,
        server: Server,
        setup: FontSetup,
        calls: Rc<RefCell<Vec<String>>>,
    }

    struct FakeRegistry(Rc<RefCell<Vec<String>>>);
    impl Registry for FakeRegistry {
        fn set(&self, name: &str, file: &Path) -> Result<()> {
            self.0
                .borrow_mut()
                .push(format!("set {name} = {}", file.display()));
            Ok(())
        }
        fn remove(&self, name: &str) -> Result<()> {
            self.0.borrow_mut().push(format!("remove {name}"));
            Ok(())
        }
    }

    fn world(os: Os) -> World {
        let dir = tempfile::tempdir().unwrap();
        let server = Server::start();
        let dirs = Dirs::under(dir.path());
        let mut faces_made = Vec::new();
        for (d, name) in [
            ("Regular", "JetBrainsMonoNerdFontMono-Regular.ttf"),
            ("Bold", "JetBrainsMonoNerdFontMono-Bold.ttf"),
            ("Italic", "JetBrainsMonoNerdFontMono-Italic.ttf"),
            ("BoldItalic", "JetBrainsMonoNerdFontMono-BoldItalic.ttf"),
        ] {
            let body = ttf(name);
            server.route(&format!("/{d}/{name}"), 200, body.clone());
            faces_made.push(Face {
                remote: format!("{d}/{name}"),
                file: name.into(),
                sha256: sha(&body),
                size: body.len() as u64,
            });
        }
        let lic = b"SIL OPEN FONT LICENSE Version 1.1".to_vec();
        server.route("/Regular/OFL.txt", 200, lic.clone());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let setup = FontSetup {
            os,
            fonts_dir: fonts_dir(os, &dirs),
            state_dir: dirs.rada_state(),
            base_url: server.base.clone(),
            faces: faces_made,
            license: Face {
                remote: "Regular/OFL.txt".into(),
                file: "JetBrainsMonoNerdFont-OFL.txt".into(),
                sha256: sha(&lic),
                size: lic.len() as u64,
            },
            fc_cache: None,
            registry: (os == Os::Windows)
                .then(|| Box::new(FakeRegistry(calls.clone())) as Box<dyn Registry>),
            user_agent: "rada/test".into(),
        };
        World {
            _dir: dir,
            server,
            setup,
            calls,
        }
    }

    fn install(w: &World) -> (Result<()>, Vec<String>) {
        let mut said = Vec::new();
        let r = w.setup.install(&mut |s| said.push(s));
        (r, said)
    }

    #[test]
    fn the_pinned_files_are_four_faces_and_a_licence_with_real_checksums() {
        let all = faces();
        assert_eq!(all.len(), 4);
        for f in all.iter().chain(std::iter::once(&license())) {
            assert_eq!(f.sha256.len(), 64, "{}", f.file);
            assert!(f.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(f.size > 1000);
        }
        let names: Vec<&str> = all.iter().map(|f| f.file.as_str()).collect();
        assert!(names.contains(&"JetBrainsMonoNerdFontMono-Regular.ttf"));
        assert!(names.contains(&"JetBrainsMonoNerdFontMono-BoldItalic.ttf"));
        // The Mono variant: one width for every glyph, which is what a terminal needs.
        assert!(names.iter().all(|n| n.contains("NerdFontMono")));
        assert!(
            BASE_URL.contains(RELEASE),
            "the release is pinned in the address"
        );
        assert!(BASE_URL.starts_with("https://"));
        assert_eq!(license().file, "JetBrainsMonoNerdFont-OFL.txt");
    }

    #[test]
    fn the_font_goes_where_each_system_looks_for_the_accounts_own_fonts() {
        let dirs = Dirs::under(Path::new("/h"));
        assert!(fonts_dir(Os::Linux, &dirs).ends_with("data/fonts/rada-jetbrainsmono-nerd"));
        assert_eq!(
            fonts_dir(Os::Mac, &dirs),
            Path::new("/h/home/Library/Fonts")
        );
        assert!(fonts_dir(Os::Windows, &dirs).ends_with("Microsoft/Windows/Fonts"));
    }

    #[test]
    fn a_checked_download_installs_the_faces_the_licence_and_a_record() {
        let w = world(Os::Linux);
        let (r, said) = install(&w);
        r.unwrap();
        for f in &w.setup.faces {
            assert_eq!(
                std::fs::read(w.setup.fonts_dir.join(&f.file)).unwrap(),
                ttf(&f.file)
            );
        }
        let lic = std::fs::read_to_string(w.setup.fonts_dir.join("JetBrainsMonoNerdFont-OFL.txt"))
            .unwrap();
        assert!(lic.contains("OPEN FONT LICENSE"));
        assert!(w.setup.is_installed());
        assert!(
            said.iter().any(|l| l.contains("Open Font License")),
            "{said:?}"
        );
        assert!(
            said.iter()
                .any(|l| l.contains("choose “JetBrainsMono Nerd Font Mono”")),
            "{said:?}"
        );
        // The work folder is gone.
        let left: Vec<_> = std::fs::read_dir(&w.setup.state_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left.len(), 1, "only the record is left: {left:?}");
    }

    #[test]
    fn a_file_that_does_not_match_its_checksum_means_nothing_is_installed() {
        let w = world(Os::Linux);
        w.server.route(
            "/Bold/JetBrainsMonoNerdFontMono-Bold.ttf",
            200,
            ttf("tampered"),
        );
        let (r, _) = install(&w);
        let e = r.unwrap_err().to_string();
        assert!(e.contains("does not match its checksum"), "{e}");
        assert!(e.contains("nothing was installed"), "{e}");
        assert!(
            !w.setup.fonts_dir.exists(),
            "not even the faces that were fine"
        );
        assert!(!w.setup.is_installed());
    }

    #[test]
    fn something_that_is_not_a_font_is_refused_even_with_the_right_checksum() {
        let mut w = world(Os::Linux);
        let html = b"<html>an error page</html>".to_vec();
        w.server.route(
            "/Regular/JetBrainsMonoNerdFontMono-Regular.ttf",
            200,
            html.clone(),
        );
        w.setup.faces[0].sha256 = sha(&html);
        let (r, _) = install(&w);
        assert!(r.unwrap_err().to_string().contains("not a font file"));
        assert!(!w.setup.fonts_dir.exists());
    }

    #[test]
    fn a_missing_file_or_server_is_an_error_that_changes_nothing() {
        let mut w = world(Os::Linux);
        w.setup.base_url = format!("{}/nowhere", w.server.base);
        let (r, _) = install(&w);
        assert!(r.unwrap_err().to_string().contains("404"));
        assert!(!w.setup.fonts_dir.exists());
    }

    #[test]
    fn a_second_setup_changes_nothing_and_remove_takes_it_all_away() {
        let w = world(Os::Linux);
        install(&w).0.unwrap();
        let first = w.server.requests().len();
        let (r, said) = install(&w);
        r.unwrap();
        assert!(said[0].contains("already installed"), "{said:?}");
        assert_eq!(w.server.requests().len(), first, "nothing downloaded again");

        let mut said = Vec::new();
        w.setup.remove(&mut |s| said.push(s)).unwrap();
        assert!(
            !w.setup.fonts_dir.exists(),
            "the folder rada made goes with it"
        );
        assert!(!w.setup.manifest_path().exists());
        assert!(said.len() >= 5, "{said:?}");
        let mut again = Vec::new();
        w.setup.remove(&mut |s| again.push(s)).unwrap();
        assert_eq!(again, ["nothing to remove"]);
    }

    #[test]
    fn on_windows_each_face_is_registered_for_the_account_and_unregistered_on_remove() {
        let w = world(Os::Windows);
        install(&w).0.unwrap();
        let calls = w.calls.borrow().clone();
        assert_eq!(calls.len(), 4, "{calls:?}");
        assert!(
            calls[0].starts_with("set JetBrainsMono Nerd Font Mono Regular (TrueType) = "),
            "{calls:?}"
        );
        assert!(
            calls.iter().any(
                |c| c.contains("Bold Italic (TrueType)") || c.contains("BoldItalic (TrueType)")
            ),
            "{calls:?}"
        );
        assert!(calls.iter().all(|c| c.contains("Microsoft/Windows/Fonts")
            || c.contains("Microsoft\\Windows\\Fonts")
            || c.starts_with("set")));
        w.calls.borrow_mut().clear();
        w.setup.remove(&mut |_| {}).unwrap();
        assert_eq!(w.calls.borrow().len(), 4);
        assert!(w.calls.borrow().iter().all(|c| c.starts_with("remove ")));
        // The folder Windows gave the account is not rada's to delete.
        assert!(w.setup.fonts_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn on_linux_the_font_cache_is_refreshed_for_the_folder_and_a_missing_tool_is_only_a_note() {
        use std::os::unix::fs::PermissionsExt;
        let mut w = world(Os::Linux);
        let log = w.setup.state_dir.join("fc.log");
        std::fs::create_dir_all(&w.setup.state_dir).unwrap();
        let fc = w.setup.state_dir.join("fc-cache");
        std::fs::write(
            &fc,
            format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fc, std::fs::Permissions::from_mode(0o755)).unwrap();
        w.setup.fc_cache = Some(fc.to_string_lossy().into_owned());
        let (r, said) = install(&w);
        r.unwrap();
        assert!(
            said.iter().any(|l| l.contains("refreshed the font cache")),
            "{said:?}"
        );
        let args = std::fs::read_to_string(&log).unwrap();
        assert!(
            args.starts_with("-f ") && args.contains("rada-jetbrainsmono-nerd"),
            "{args}"
        );

        let mut w2 = world(Os::Linux);
        w2.setup.fc_cache = Some("no-such-program-anywhere".into());
        let (r, said) = install(&w2);
        r.unwrap();
        assert!(
            said.iter()
                .any(|l| l.contains("could not refresh the font cache")),
            "{said:?}"
        );
        assert!(w2.setup.is_installed());
    }

    #[test]
    fn registry_names_carry_the_style() {
        assert_eq!(
            registry_name("JetBrainsMonoNerdFontMono-Regular.ttf"),
            "JetBrainsMono Nerd Font Mono Regular (TrueType)"
        );
        assert_eq!(
            registry_name("JetBrainsMonoNerdFontMono-BoldItalic.ttf"),
            "JetBrainsMono Nerd Font Mono BoldItalic (TrueType)"
        );
    }
}
