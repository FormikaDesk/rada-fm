//! `rada update`, run as the real program against a web server of our own that stands in for
//! GitHub. HOME and every XDG folder point into a temporary folder; the program that gets
//! replaced is a copy made there. Nothing reaches the network or the real installation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use rada_core::testweb::Server;
use sha2::{Digest, Sha256};

const THIS: &str = env!("CARGO_PKG_VERSION");
const NEW: &str = "99.0.0";

fn target() -> String {
    rada_core::update::release::target_for(std::env::consts::OS, std::env::consts::ARCH, false)
        .expect("a system rada has releases for")
}

struct World {
    root: tempfile::TempDir,
    server: Server,
    /// The copy of rada that the tests run (and that an update replaces).
    exe: PathBuf,
}

impl World {
    fn new() -> World {
        let root = tempfile::Builder::new()
            .prefix("rada-update-cli-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        for d in [
            "home",
            "xdg/config/rada",
            "xdg/data",
            "xdg/state",
            "xdg/cache",
            "bin",
        ] {
            fs::create_dir_all(root.path().join(d)).unwrap();
        }
        let exe = root
            .path()
            .join("bin")
            .join(if cfg!(windows) { "rada.exe" } else { "rada" });
        fs::copy(env!("CARGO_BIN_EXE_rada"), &exe).unwrap();
        // The sandbox lies inside rada's own checkout: say that this copy is a downloaded one.
        fs::write(
            root.path().join("xdg/config/rada/config.toml"),
            "[update]\nmethod = \"binary\"\n",
        )
        .unwrap();
        World {
            root,
            server: Server::start(),
            exe,
        }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(&self.exe);
        cmd.args(args)
            .env("HOME", self.p("home"))
            .env("USERPROFILE", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("xdg/config"))
            .env("XDG_DATA_HOME", self.p("xdg/data"))
            .env("XDG_STATE_HOME", self.p("xdg/state"))
            .env("XDG_CACHE_HOME", self.p("xdg/cache"))
            .env("APPDATA", self.p("xdg/config"))
            .env("LOCALAPPDATA", self.p("xdg/data"))
            .env("RADA_UPDATE_API", self.server.url("/releases"))
            .stdin(Stdio::null());
        run_patiently(&mut cmd)
    }

    /// List one release, with a program inside that says its version.
    fn publish(&self, version: &str, program: &[u8]) {
        let name = if cfg!(windows) {
            format!("rada-{version}-{}.zip", target())
        } else {
            format!("rada-{version}-{}.tar.gz", target())
        };
        let inner = format!(
            "rada-{version}-{}/{}",
            target(),
            if cfg!(windows) { "rada.exe" } else { "rada" }
        );
        let archive = if cfg!(windows) {
            use std::io::Write;
            let path = self.p("a.zip");
            let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            z.start_file(&inner, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(program).unwrap();
            z.finish().unwrap();
            fs::read(path).unwrap()
        } else {
            let path = self.p("a.tar.gz");
            let gz = flate2::write::GzEncoder::new(
                fs::File::create(&path).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(program.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            tar.append_data(&mut h, &inner, program).unwrap();
            tar.into_inner().unwrap().finish().unwrap();
            fs::read(path).unwrap()
        };
        let sum: String = Sha256::digest(&archive)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.server.route(&format!("/files/{name}"), 200, archive);
        self.server
            .route("/files/SHA256SUMS", 200, format!("{sum}  {name}\n"));
        self.server.route(
            "/releases",
            200,
            format!(
                r#"[{{"tag_name":"v{version}","prerelease":false,"draft":false,"body":"Tags in Finder colours\n- Faster everything","assets":[
                {{"name":"{name}","browser_download_url":"{}","size":1}},
                {{"name":"SHA256SUMS","browser_download_url":"{}","size":1}}]}}]"#,
                self.server.url(&format!("/files/{name}")),
                self.server.url("/files/SHA256SUMS"),
            ),
        );
    }
}

/// Run a command; a program that another test's copy is still writing is "busy" for a moment
/// (the child of a fork holds the open file), so try again.
fn run_patiently(cmd: &mut Command) -> Output {
    for _ in 0..100 {
        match cmd.output() {
            Ok(o) => return o,
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(100))
            }
            Err(e) => panic!("cannot run {:?}: {e}", cmd.get_program()),
        }
    }
    panic!("{:?} stayed busy", cmd.get_program());
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn check_says_what_is_new_and_changes_nothing() {
    let w = World::new();
    w.publish(NEW, b"whatever");
    let before = fs::read(&w.exe).unwrap();
    let out = w.run(&["update", "--check"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains(&format!("rada {NEW} is available")), "{t}");
    assert!(t.contains("Tags in Finder colours"), "{t}");
    assert!(t.contains("SHA256SUMS"), "what will happen is told: {t}");
    assert_eq!(fs::read(&w.exe).unwrap(), before, "--check never installs");
    // Only the release list was read, once, and with the user agent of this version.
    let seen = w.server.requests();
    assert_eq!(seen.len(), 1, "{seen:?}");
    let ua = seen[0]
        .headers
        .iter()
        .find(|(k, _)| k == "user-agent")
        .unwrap();
    assert_eq!(ua.1, format!("rada/{THIS}"));
}

#[test]
fn nothing_newer_is_said_plainly() {
    let w = World::new();
    w.publish("0.0.1", b"old");
    let out = w.run(&["update", "--check"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("newest version"), "{}", text(&out));
}

#[test]
fn an_unreachable_server_is_an_error_not_a_crash() {
    let w = World::new();
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/releases", dead.local_addr().unwrap());
    drop(dead);
    let out = run_patiently(
        Command::new(&w.exe)
            .args(["update", "--check"])
            .env("HOME", w.p("home"))
            .env("USERPROFILE", w.p("home"))
            .env("XDG_CONFIG_HOME", w.p("xdg/config"))
            .env("XDG_STATE_HOME", w.p("xdg/state"))
            .env("RADA_UPDATE_API", url),
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("cannot reach"), "{}", text(&out));
}

#[test]
fn without_a_terminal_it_will_not_install_unless_told_to() {
    let w = World::new();
    w.publish(NEW, b"whatever");
    let before = fs::read(&w.exe).unwrap();
    let out = w.run(&["update"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("--yes"), "{}", text(&out));
    assert_eq!(fs::read(&w.exe).unwrap(), before);
    assert_eq!(
        w.server.count("/files/SHA256SUMS"),
        0,
        "nothing was downloaded"
    );
}

#[test]
fn there_is_nothing_to_go_back_to_at_first() {
    let w = World::new();
    let out = w.run(&["update", "--rollback"]);
    assert!(!out.status.success());
    assert!(text(&out).contains("no earlier version"), "{}", text(&out));
}

#[cfg_attr(windows, ignore = "the program in the test archive is a shell script")]
#[test]
fn yes_downloads_checks_and_replaces_the_program() {
    let w = World::new();
    w.publish(NEW, format!("#!/bin/sh\necho \"rada {NEW}\"\n").as_bytes());
    let out = w.run(&["update", "--yes"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("downloading"), "{t}");
    assert!(t.contains("Start rada again"), "{t}");
    let version = run_patiently(Command::new(&w.exe).arg("--version"));
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("rada {NEW}")
    );
    // The program it replaced is kept for --rollback.
    assert!(w.p("xdg/state/rada/update/previous").is_dir());
}

#[cfg_attr(windows, ignore = "the program in the test archive is a shell script")]
#[test]
fn a_checksum_that_does_not_match_installs_nothing() {
    let w = World::new();
    w.publish(NEW, format!("#!/bin/sh\necho \"rada {NEW}\"\n").as_bytes());
    let name = format!("rada-{NEW}-{}.tar.gz", target());
    w.server.route(
        "/files/SHA256SUMS",
        200,
        format!("{}  {name}\n", "cd".repeat(32)),
    );
    let before = fs::read(&w.exe).unwrap();
    let out = w.run(&["update", "--yes"]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("does not match its checksum"),
        "{}",
        text(&out)
    );
    assert_eq!(fs::read(&w.exe).unwrap(), before);
}
