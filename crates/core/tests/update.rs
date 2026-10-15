//! Updates, against a web server of our own that stands in for GitHub: nothing here ever
//! reaches the network, and every file written lives in the sandbox.

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::unbounded;
use minisign::KeyPair;
use rada_core::events::CoreEvent;
use rada_core::testutil::Sandbox;
use rada_core::testweb::Server;
use rada_core::update::{
    self, Channel, Check, CheckState, Ctx, Method, Mode, Settings, UpdateEvent, Updater, Version,
    release,
};
use sha2::{Digest, Sha256};

// ------------------------------------------------------------------------ release files

const NEW: &str = "0.7.0";
/// A plausible "now" (seconds since the epoch).
const T0: u64 = 1_700_000_000;

fn tar_gz(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let path = dir.join(name);
    let gz = flate2::write::GzEncoder::new(
        fs::File::create(&path).unwrap(),
        flate2::Compression::fast(),
    );
    let mut tar = tar::Builder::new(gz);
    for (n, data) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        tar.append_data(&mut h, n, *data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
    fs::read(path).unwrap()
}

fn zip_bytes(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    for (n, data) in files {
        zip.start_file(*n, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    fs::read(path).unwrap()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The program inside a release: a script that says its version (on a system that can run
/// one), or just bytes (where the run check is off).
fn program(version: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho \"rada {version}\"\n").into_bytes()
}

/// Like [`program`], and it leaves a mark in the folder it is started in: a test that finds no
/// mark knows that the new program was never run.
fn marking_program(version: &str) -> Vec<u8> {
    format!("#!/bin/sh\ntouch \"$(dirname \"$0\")/was-run\"\necho \"rada {version}\"\n")
        .into_bytes()
}

/// A key pair made for a test (never a real key), and what it signs.
struct Signer {
    pair: KeyPair,
}

impl Signer {
    fn new() -> Signer {
        Signer {
            pair: KeyPair::generate_unencrypted_keypair().expect("a test key pair"),
        }
    }

    /// The public key as the program wants it in its list.
    fn public(&self) -> String {
        self.pair.pk.to_base64()
    }

    /// A signature file for `data` with the trusted comment `comment`.
    fn sign(&self, data: &[u8], comment: &str) -> String {
        minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            std::io::Cursor::new(data),
            Some(comment),
            Some("signature from a test key"),
        )
        .expect("a signature")
        .into_string()
    }
}

struct World {
    sb: Sandbox,
    server: Server,
    ctx: Ctx,
    exe: PathBuf,
    target: &'static str,
    /// The key the releases are signed with, and the only one the program trusts.
    signer: Signer,
}

fn target() -> &'static str {
    if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-darwin"
    } else {
        "x86_64-unknown-linux-musl"
    }
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "rada.exe" } else { "rada" }
}

/// A sandbox with an installed "rada 0.6.0-rc.1" and a server listing the releases given.
fn world(current: &str, mut settings: Settings) -> World {
    // The sandbox sits inside the checkout of rada itself, which the automatic choice would
    // take for the program's own: tests that care say what they want.
    if settings.method == Method::Auto {
        settings.method = Method::Binary;
    }
    let sb = Sandbox::new();
    let server = Server::start();
    let bin = sb.mkdir("install/bin");
    let exe = bin.join(exe_name());
    fs::write(&exe, b"the old program").unwrap();
    let signer = Signer::new();
    let ctx = Ctx {
        current: Version::parse(current).unwrap(),
        settings,
        api_url: server.url("/releases"),
        exe: exe.clone(),
        state_dir: sb.mkdir("state/rada"),
        home: sb.path("home"),
        target: Some(target().to_string()),
        user_agent: format!("rada/{current}"),
        os_id: None,
        git: "git".into(),
        cargo: "cargo".into(),
        // Only a system that can run a shell script checks the new program by running it.
        run_check: cfg!(unix),
        signing_keys: vec![signer.public()],
        extra_env: Vec::new(),
    };
    World {
        sb,
        server,
        ctx,
        exe,
        target: target(),
        signer,
    }
}

impl World {
    /// Publish release `tag` with the usual files; `program` is what the archive holds.
    fn publish(&self, tag: &str, _prerelease: bool, program: &[u8]) -> (String, String) {
        let version = Version::parse(tag).unwrap();
        let archive = release::archive_name(&version, self.target);
        let inner = release::inner_path(&version, self.target);
        let work = self.sb.mkdir("serverfiles");
        let bytes = if archive.ends_with(".zip") {
            zip_bytes(
                &work,
                &archive,
                &[(&inner, program), ("x/README.md", b"read me")],
            )
        } else {
            tar_gz(
                &work,
                &archive,
                &[(&inner, program), ("x/README.md", b"read me")],
            )
        };
        let sums = format!(
            "{}  {archive}\n{}  other-file\n",
            sha(&bytes),
            "0".repeat(64)
        );
        self.server
            .route(&format!("/files/{tag}/{archive}"), 200, bytes);
        self.set_sums(tag, &sums);
        (archive, sums)
    }

    /// Serve `sums` as the SHA256SUMS of `tag`, signed as the author signs a release.
    fn set_sums(&self, tag: &str, sums: &str) {
        self.set_sums_signed_by(tag, sums, &self.signer, &format!("rada {tag}"));
    }

    /// Serve `sums`, signed by `signer` with the trusted comment `comment`.
    fn set_sums_signed_by(&self, tag: &str, sums: &str, signer: &Signer, comment: &str) {
        self.server
            .route(&format!("/files/{tag}/SHA256SUMS"), 200, sums.to_string());
        self.server.route(
            &format!("/files/{tag}/SHA256SUMS.minisig"),
            200,
            signer.sign(sums.as_bytes(), comment),
        );
    }

    fn list(&self, entries: &[(&str, bool)]) {
        let items: Vec<String> = entries
            .iter()
            .map(|(tag, pre)| {
                let v = Version::parse(tag).unwrap();
                let a = release::archive_name(&v, self.target);
                format!(
                    r#"{{"tag_name":"{tag}","prerelease":{pre},"draft":false,"body":"Notes for {tag}","assets":[
                    {{"name":"{a}","browser_download_url":"{}","size":1}},
                    {{"name":"SHA256SUMS","browser_download_url":"{}","size":1}},
                    {{"name":"SHA256SUMS.minisig","browser_download_url":"{}","size":1}}]}}"#,
                    self.server.url(&format!("/files/{tag}/{a}")),
                    self.server.url(&format!("/files/{tag}/SHA256SUMS")),
                    self.server.url(&format!("/files/{tag}/SHA256SUMS.minisig")),
                )
            })
            .collect();
        self.server
            .route("/releases", 200, format!("[{}]", items.join(",")));
    }

    fn newest(&self) -> release::Release {
        update::newest_release(&self.ctx)
            .unwrap()
            .expect("a newer release")
    }

    fn leftovers(&self) -> Vec<String> {
        fs::read_dir(self.exe.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != exe_name())
            // Windows keeps the replaced program beside the new one until it is no longer
            // running; it is removed at the next start.
            .filter(|n| !(cfg!(windows) && n.starts_with(".rada-old-")))
            .collect()
    }
}

fn quiet(_: String) {}

// ------------------------------------------------------------------------------ checking

#[test]
fn a_check_reads_the_list_and_sends_nothing_but_who_asks() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.list(&[("v0.6.0-rc.2", true), ("v0.5.0", false)]);
    let got = update::check(&w.ctx, true, T0);
    let Check::Newer(n) = got else {
        panic!("{got:?}")
    };
    assert_eq!(n.version, "0.6.0-rc.2");
    assert_eq!(n.notes, "Notes for v0.6.0-rc.2");
    assert!(n.prerelease);

    let seen = w.server.requests();
    assert_eq!(seen.len(), 1);
    let names: Vec<&str> = seen[0].headers.iter().map(|(k, _)| k.as_str()).collect();
    let ua = &seen[0]
        .headers
        .iter()
        .find(|(k, _)| k == "user-agent")
        .unwrap()
        .1;
    assert_eq!(ua, "rada/0.6.0-rc.1");
    // Only the request's own headers: no cookies, no tokens, no identifiers.
    for n in names {
        assert!(
            ["host", "user-agent", "accept", "accept-encoding"].contains(&n),
            "unexpected header {n}"
        );
    }
}

#[test]
fn a_stable_version_is_not_shown_prereleases_and_a_prerelease_is_shown_later_ones() {
    let w = world("0.5.0", Settings::default());
    w.list(&[("v0.6.0-rc.2", true), ("v0.5.1", false)]);
    let Check::Newer(n) = update::check(&w.ctx, true, T0) else {
        panic!()
    };
    assert_eq!(n.version, "0.5.1");

    let mut w = world("0.6.0-rc.1", Settings::default());
    w.list(&[("v0.6.0-rc.2", true), ("v0.5.1", false)]);
    let Check::Newer(n) = update::check(&w.ctx, true, T0) else {
        panic!()
    };
    assert_eq!(n.version, "0.6.0-rc.2");
    // The choice in the configuration wins over what the running version suggests.
    w.ctx.settings.channel = Channel::Stable;
    assert_eq!(update::check(&w.ctx, true, T0), Check::UpToDate);
}

#[test]
fn the_network_is_asked_once_a_day() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.list(&[("v0.6.0-rc.2", true)]);
    let day = 24 * 60 * 60;
    assert!(matches!(update::check(&w.ctx, false, T0), Check::Newer(_)));
    assert_eq!(
        w.server.count("/releases?per_page=30") + w.server.count("/releases"),
        1
    );

    // Later the same day: answered from what was found, without a request.
    assert!(matches!(
        update::check(&w.ctx, false, T0 + day - 1),
        Check::Newer(_)
    ));
    assert!(matches!(
        update::check(&w.ctx, false, T0 + 3_600),
        Check::Newer(_)
    ));
    assert_eq!(w.server.requests().len(), 1);

    // A day later: asked again.
    assert!(matches!(
        update::check(&w.ctx, false, T0 + day),
        Check::Newer(_)
    ));
    assert_eq!(w.server.requests().len(), 2);

    // Somebody asking by hand always asks.
    update::check(&w.ctx, true, T0 + day + 5);
    assert_eq!(w.server.requests().len(), 3);
}

#[test]
fn what_was_found_is_forgotten_once_it_is_no_longer_newer() {
    let mut w = world("0.6.0-rc.1", Settings::default());
    w.list(&[("v0.6.0-rc.2", true)]);
    assert!(matches!(update::check(&w.ctx, false, T0), Check::Newer(_)));
    // The user got the new version by other means.
    w.ctx.current = Version::parse("0.6.0-rc.2").unwrap();
    assert_eq!(update::check(&w.ctx, false, T0 + 2_000), Check::UpToDate);
    assert_eq!(w.server.requests().len(), 1);
}

#[test]
fn no_network_is_a_quiet_failure_that_is_retried_in_an_hour_not_at_once() {
    let w = world("0.6.0-rc.1", Settings::default());
    // Nothing listens here.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/releases", dead.local_addr().unwrap());
    drop(dead);
    let mut ctx = w.ctx.clone();
    ctx.api_url = url;
    let got = update::check(&ctx, false, T0);
    assert!(matches!(got, Check::Failed(_)), "{got:?}");
    let st = CheckState::load(&ctx);
    assert_eq!(st.last_attempt, T0);
    assert_eq!(st.last_check, 0);
    assert!(!st.due(T0 + 60));
    assert!(st.due(T0 + 3_600));
}

#[test]
fn http_errors_and_nonsense_are_failures_with_a_reason() {
    let w = world("0.6.0-rc.1", Settings::default());
    let Check::Failed(e) = update::check(&w.ctx, true, T0) else {
        panic!()
    };
    assert!(e.contains("404"), "{e}");
    w.server.route("/releases", 403, "rate limited");
    let Check::Failed(e) = update::check(&w.ctx, true, T0) else {
        panic!()
    };
    assert!(e.contains("403"), "{e}");
    w.server.route("/releases", 200, "<html>not json</html>");
    let Check::Failed(e) = update::check(&w.ctx, true, T0) else {
        panic!()
    };
    assert!(e.contains("not a list of releases"), "{e}");
    // A failed check never changes what is installed.
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
}

// -------------------------------------------------------------------------- binary mode

#[test]
fn a_binary_update_downloads_checks_and_swaps_the_program() {
    let w = world("0.6.0-rc.1", Settings::default());
    let new = program(NEW);
    w.publish("v0.7.0", false, &new);
    w.list(&[("v0.7.0", false)]);
    assert_eq!(w.ctx.mode().unwrap(), Mode::Binary);

    let mut steps = Vec::new();
    let done = update::apply(&w.ctx, &w.newest(), &mut |s| steps.push(s)).unwrap();
    assert_eq!(done.version, NEW);
    assert!(done.restart);
    assert_eq!(fs::read(&w.exe).unwrap(), new);
    assert!(
        steps.iter().any(|s| s.starts_with("downloading")),
        "{steps:?}"
    );
    assert!(
        steps.iter().any(|s| s == "checking the download"),
        "{steps:?}"
    );
    // Nothing is left beside the program.
    assert_eq!(w.leftovers(), Vec::<String>::new());
    // The old program is kept, with its version.
    let (kept, info) = update::install::read_backup(&w.ctx.update_dir().join("previous")).unwrap();
    assert_eq!(info.version, "0.6.0-rc.1");
    assert_eq!(fs::read(kept).unwrap(), b"the old program");
    // The work folder is gone, the log is kept.
    assert!(w.ctx.log_path().is_file());
    assert!(
        !w.ctx
            .update_dir()
            .join(format!("work-{}", std::process::id()))
            .exists()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&w.exe).unwrap().permissions().mode() & 0o111,
            0
        );
    }
}

#[test]
fn a_wrong_checksum_installs_nothing() {
    let w = world("0.6.0-rc.1", Settings::default());
    let (archive, _) = w.publish("v0.7.0", false, &program(NEW));
    // Signed by the author, and still wrong: only the signature is not enough.
    w.set_sums("v0.7.0", &format!("{}  {archive}\n", "ab".repeat(32)));
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("does not match its checksum"), "{err}");
    assert!(err.contains("nothing was installed"), "{err}");
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
    assert_eq!(w.leftovers(), Vec::<String>::new());
}

#[test]
fn an_archive_that_is_not_one_is_refused_even_with_a_matching_checksum() {
    let w = world("0.6.0-rc.1", Settings::default());
    let v = Version::parse("v0.7.0").unwrap();
    let archive = release::archive_name(&v, w.target);
    let junk = b"this is not an archive at all".to_vec();
    w.server
        .route(&format!("/files/v0.7.0/{archive}"), 200, junk.clone());
    w.set_sums("v0.7.0", &format!("{}  {archive}\n", sha(&junk)));
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("damaged") || err.contains("archive"), "{err}");
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
    assert_eq!(w.leftovers(), Vec::<String>::new());
}

#[test]
fn an_archive_without_the_program_is_refused() {
    let w = world("0.6.0-rc.1", Settings::default());
    let v = Version::parse("v0.7.0").unwrap();
    let archive = release::archive_name(&v, w.target);
    let work = w.sb.mkdir("serverfiles");
    let bytes = if archive.ends_with(".zip") {
        zip_bytes(&work, &archive, &[("somewhere/else", b"x")])
    } else {
        tar_gz(&work, &archive, &[("somewhere/else", b"x")])
    };
    w.server
        .route(&format!("/files/v0.7.0/{archive}"), 200, bytes.clone());
    w.set_sums("v0.7.0", &format!("{}  {archive}\n", sha(&bytes)));
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("does not contain"), "{err}");
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
}

#[test]
fn a_download_cut_short_installs_nothing() {
    let w = world("0.6.0-rc.1", Settings::default());
    let (archive, _) = w.publish("v0.7.0", false, &program(NEW));
    let full = w.server.routes.lock().unwrap()[&format!("/files/v0.7.0/{archive}")]
        .body
        .clone();
    w.server.route_truncated(
        &format!("/files/v0.7.0/{archive}"),
        full[..full.len() / 2].to_vec(),
    );
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(!err.is_empty());
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
    assert_eq!(w.leftovers(), Vec::<String>::new());
}

#[test]
fn a_release_without_checksums_or_without_our_file_is_not_installed() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &program(NEW));
    w.list(&[("v0.7.0", false)]);
    let mut r = w.newest();
    r.assets.retain(|a| a.name != "SHA256SUMS");
    let err = update::apply(&w.ctx, &r, &mut quiet).unwrap_err();
    assert!(err.contains("SHA256SUMS"), "{err}");

    let mut r = w.newest();
    r.assets.retain(|a| a.name == "SHA256SUMS");
    let err = update::apply(&w.ctx, &r, &mut quiet).unwrap_err();
    assert!(err.contains("no file for this system"), "{err}");

    let mut w2 = world("0.6.0-rc.1", Settings::default());
    w2.ctx.target = None;
    w2.publish("v0.7.0", false, &program(NEW));
    w2.list(&[("v0.7.0", false)]);
    let err = update::apply(&w2.ctx, &w2.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("no ready-made rada"), "{err}");
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
}

#[cfg(unix)]
#[test]
fn a_program_that_does_not_say_the_right_version_is_not_installed() {
    let w = world("0.6.0-rc.1", Settings::default());
    // The release is 0.7.0 but the program inside says 0.6.9.
    w.publish("v0.7.0", false, &program("0.6.9"));
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("not rada 0.7.0"), "{err}");
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
    assert_eq!(w.leftovers(), Vec::<String>::new());

    // A program that does not run at all.
    let w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, b"not a program");
    w.list(&[("v0.7.0", false)]);
    assert!(update::apply(&w.ctx, &w.newest(), &mut quiet).is_err());
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
    assert_eq!(w.leftovers(), Vec::<String>::new());
}

#[test]
fn a_folder_that_cannot_be_written_is_explained() {
    let mut w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &program(NEW));
    w.list(&[("v0.7.0", false)]);
    // The program lives in a folder that does not exist: nothing can be staged beside it.
    w.ctx.exe = w.sb.path("gone/bin").join(exe_name());
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn going_back_restores_the_program_that_was_replaced() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &program(NEW));
    w.list(&[("v0.7.0", false)]);
    let mut ctx = w.ctx.clone();
    update::apply(&ctx, &w.newest(), &mut quiet).unwrap();
    assert_ne!(fs::read(&w.exe).unwrap(), b"the old program");

    ctx.current = Version::parse(NEW).unwrap();
    // The old program is only bytes, so only a system that does not run the check can take it
    // back; on the others the check refuses a program that does not start, which is the point.
    ctx.run_check = false;
    assert_eq!(update::rollback(&ctx).unwrap(), "0.6.0-rc.1");
    assert_eq!(fs::read(&w.exe).unwrap(), b"the old program");
    let (_, info) = update::install::read_backup(&ctx.update_dir().join("previous")).unwrap();
    assert_eq!(info.version, NEW);
    assert_eq!(w.leftovers(), Vec::<String>::new());
}

#[test]
fn there_is_nothing_to_go_back_to_before_the_first_update() {
    let w = world("0.6.0-rc.1", Settings::default());
    let err = update::rollback(&w.ctx).unwrap_err();
    assert!(err.contains("no earlier version"), "{err}");
}

// -------------------------------------------------------------------------- signatures

impl World {
    /// Nothing was installed, nothing was left behind, and the new program never ran.
    fn assert_untouched(&self) {
        assert_eq!(fs::read(&self.exe).unwrap(), b"the old program");
        assert_eq!(self.leftovers(), Vec::<String>::new());
        assert!(!self.exe.parent().unwrap().join("was-run").exists());
    }

    fn archive_requests(&self, tag: &str) -> usize {
        let v = Version::parse(tag).unwrap();
        self.server.count(&format!(
            "/files/{tag}/{}",
            release::archive_name(&v, self.target)
        ))
    }
}

#[test]
fn the_signature_is_checked_before_the_checksum_and_before_the_program_ever_runs() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &marking_program(NEW));
    w.list(&[("v0.7.0", false)]);
    let mut steps = Vec::new();
    update::apply(&w.ctx, &w.newest(), &mut |s| steps.push(s)).unwrap();
    let at = |what: &str| {
        steps
            .iter()
            .position(|s| s.starts_with(what))
            .unwrap_or_else(|| panic!("no step '{what}' in {steps:?}"))
    };
    assert!(
        at("checking the signature") < at("downloading"),
        "{steps:?}"
    );
    assert!(at("downloading") < at("checking the download"), "{steps:?}");
    assert!(at("checking the download") < at("unpacking"), "{steps:?}");
    // The log says which key signed.
    let log = fs::read_to_string(w.ctx.log_path()).unwrap();
    assert!(
        log.contains("signed by key 1 of 1: \"rada v0.7.0\""),
        "{log}"
    );
    #[cfg(unix)]
    assert!(w.exe.parent().unwrap().join("was-run").exists());
}

#[test]
fn a_release_signed_with_another_key_is_refused_before_anything_is_downloaded_or_run() {
    let w = world("0.6.0-rc.1", Settings::default());
    let (archive, sums) = w.publish("v0.7.0", false, &marking_program(NEW));
    // Same files, signed by somebody else: the checksum is right, the signature is not ours.
    w.set_sums_signed_by("v0.7.0", &sums, &Signer::new(), "rada v0.7.0");
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(
        err.contains("signature of SHA256SUMS is not valid"),
        "{err}"
    );
    assert!(err.contains("nothing was installed"), "{err}");
    assert_eq!(w.archive_requests("v0.7.0"), 0, "{archive} was downloaded");
    w.assert_untouched();
}

#[test]
fn a_genuine_signature_of_another_release_does_not_do_for_this_one() {
    let w = world("0.6.0-rc.1", Settings::default());
    let (_, sums) = w.publish("v0.7.0", false, &marking_program(NEW));
    // The author's own signature, but made for v0.6.9: an old release shown again.
    w.set_sums_signed_by("v0.7.0", &sums, &w.signer, "rada v0.6.9");
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("rada v0.6.9"), "{err}");
    assert!(err.contains("rada v0.7.0"), "{err}");
    assert_eq!(w.archive_requests("v0.7.0"), 0);
    w.assert_untouched();

    // The comment has to be exactly that, not something that starts or ends like it.
    for comment in [
        "rada v0.7.0 ",
        "rada v0.7.0-rc.1",
        "rada v0.7.",
        "v0.7.0",
        "RADA V0.7.0",
        "",
    ] {
        w.set_sums_signed_by("v0.7.0", &sums, &w.signer, comment);
        let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
        assert!(err.contains("not for"), "'{comment}': {err}");
        w.assert_untouched();
    }
}

#[test]
fn checksums_changed_after_they_were_signed_are_refused() {
    let w = world("0.6.0-rc.1", Settings::default());
    let (archive, sums) = w.publish("v0.7.0", false, &marking_program(NEW));
    // An attacker swaps the program and writes the matching line in SHA256SUMS; the signature
    // file is the genuine one for the original text.
    let evil = marking_program("0.7.0 but not the author's");
    let work = w.sb.mkdir("evil");
    let inner = release::inner_path(&Version::parse("v0.7.0").unwrap(), w.target);
    let bytes = if archive.ends_with(".zip") {
        zip_bytes(&work, &archive, &[(&inner, &evil)])
    } else {
        tar_gz(&work, &archive, &[(&inner, &evil)])
    };
    w.server
        .route(&format!("/files/v0.7.0/{archive}"), 200, bytes.clone());
    w.server.route(
        "/files/v0.7.0/SHA256SUMS",
        200,
        format!("{}  {archive}\n", sha(&bytes)),
    );
    assert_ne!(sums, format!("{}  {archive}\n", sha(&bytes)));
    w.list(&[("v0.7.0", false)]);
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(
        err.contains("signature of SHA256SUMS is not valid"),
        "{err}"
    );
    assert_eq!(w.archive_requests("v0.7.0"), 0);
    w.assert_untouched();
}

#[test]
fn a_release_without_a_signature_is_not_installed() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &marking_program(NEW));
    w.list(&[("v0.7.0", false)]);

    // No signature file among the files of the release.
    let mut r = w.newest();
    r.assets.retain(|a| a.name != "SHA256SUMS.minisig");
    let err = update::apply(&w.ctx, &r, &mut quiet).unwrap_err();
    assert!(err.contains("has no SHA256SUMS.minisig"), "{err}");
    assert!(err.contains("not signed"), "{err}");
    assert_eq!(
        w.server.count("/files/v0.7.0/SHA256SUMS"),
        0,
        "nothing was read"
    );
    assert_eq!(w.archive_requests("v0.7.0"), 0);
    w.assert_untouched();

    // Listed, but the file is not there.
    w.server
        .routes
        .lock()
        .unwrap()
        .remove("/files/v0.7.0/SHA256SUMS.minisig");
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("404"), "{err}");
    assert_eq!(w.archive_requests("v0.7.0"), 0);
    w.assert_untouched();

    // A file that is not a signature.
    for junk in [
        "",
        "hello",
        "untrusted comment: x\nnot base64\ntrusted comment: y\nnor this",
    ] {
        w.server
            .route("/files/v0.7.0/SHA256SUMS.minisig", 200, junk.to_string());
        let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
        assert!(
            err.contains("signature of SHA256SUMS is not valid"),
            "'{junk}': {err}"
        );
        w.assert_untouched();
    }
}

#[test]
fn a_build_with_no_real_key_installs_nothing() {
    let mut w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &marking_program(NEW));
    w.list(&[("v0.7.0", false)]);
    // A list that holds no real key (an unofficial build, or one not finished).
    w.ctx.signing_keys = vec![format!("{}-main", update::sign::PLACEHOLDER)];
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("no public key"), "{err}");
    assert_eq!(w.archive_requests("v0.7.0"), 0);
    w.assert_untouched();
}

#[test]
fn the_reserve_key_signs_releases_as_well_as_the_main_one() {
    let mut w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &program(NEW));
    w.list(&[("v0.7.0", false)]);
    let main_key_not_used = Signer::new();
    w.ctx.signing_keys = vec![main_key_not_used.public(), w.signer.public()];
    let done = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap();
    assert_eq!(done.version, NEW);
    let log = fs::read_to_string(w.ctx.log_path()).unwrap();
    assert!(log.contains("signed by key 2 of 2"), "{log}");
}

#[test]
fn a_refused_update_leaves_the_way_back_as_it_was() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &program(NEW));
    w.list(&[("v0.7.0", false)]);
    update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap();
    let kept = w.ctx.update_dir().join("previous");
    let (path, info) = update::install::read_backup(&kept).unwrap();
    let before = (fs::read(&path).unwrap(), info.version.clone());
    let installed = fs::read(&w.exe).unwrap();

    // The next release is not signed by the author.
    let mut ctx = w.ctx.clone();
    ctx.current = Version::parse(NEW).unwrap();
    let (_, sums) = w.publish("v0.8.0", false, &program("0.8.0"));
    w.set_sums_signed_by("v0.8.0", &sums, &Signer::new(), "rada v0.8.0");
    w.list(&[("v0.8.0", false)]);
    let release = update::newest_release(&ctx)
        .unwrap()
        .expect("a newer release");
    assert!(update::apply(&ctx, &release, &mut quiet).is_err());

    assert_eq!(fs::read(&w.exe).unwrap(), installed);
    let (path, info) = update::install::read_backup(&kept).unwrap();
    assert_eq!((fs::read(&path).unwrap(), info.version), before);
    assert_eq!(w.leftovers(), Vec::<String>::new());
}

// -------------------------------------------------------------------------- package mode

#[test]
fn a_program_of_a_package_manager_is_not_touched() {
    let mut w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &program(NEW));
    w.list(&[("v0.7.0", false)]);
    let managed =
        w.sb.mkdir("opt/homebrew/Cellar/rada/0.6.0/bin")
            .join(exe_name());
    fs::write(&managed, b"managed").unwrap();
    w.ctx.exe = managed.clone();
    w.ctx.settings.method = Method::Auto;
    let Mode::Package { hint } = w.ctx.mode().unwrap() else {
        panic!()
    };
    assert!(hint.contains("brew upgrade rada"), "{hint}");
    let err = update::apply(&w.ctx, &w.newest(), &mut quiet).unwrap_err();
    assert!(err.contains("brew upgrade"), "{err}");
    assert_eq!(fs::read(&managed).unwrap(), b"managed");
    // Nothing was even downloaded.
    assert_eq!(w.server.count("/files/v0.7.0/SHA256SUMS"), 0);
    assert!(update::rollback(&w.ctx).is_err());
}

// ----------------------------------------------------------------------------- the worker

#[test]
fn the_worker_reports_what_it_finds_and_what_it_does() {
    let w = world("0.6.0-rc.1", Settings::default());
    w.publish("v0.7.0", false, &program(NEW));
    w.list(&[("v0.7.0", false)]);
    let (tx, rx) = unbounded();
    let up = Updater::spawn(w.ctx.clone(), tx);

    up.check_in_background();
    let mut notice = None;
    for ev in rx.iter().take(1) {
        if let CoreEvent::Update(UpdateEvent::Available { notice: n, how }) = ev {
            assert_eq!(how.unwrap(), Mode::Binary);
            notice = Some(n);
        }
    }
    assert_eq!(notice.unwrap().version, NEW);

    up.apply();
    let mut progressed = false;
    let done = loop {
        match rx.recv_timeout(Duration::from_secs(60)).expect("an event") {
            CoreEvent::Update(UpdateEvent::Progress(_)) => progressed = true,
            CoreEvent::Update(UpdateEvent::Done(a)) => break a,
            CoreEvent::Update(UpdateEvent::Failed { message, .. }) => panic!("{message}"),
            _ => {}
        }
    };
    assert!(progressed);
    assert_eq!(done.version, NEW);
    assert_eq!(fs::read(&w.exe).unwrap(), program(NEW));
}

#[test]
fn the_background_check_is_silent_when_there_is_nothing_or_no_network_but_a_manual_one_answers() {
    let mut w = world("0.6.0-rc.2", Settings::default());
    w.list(&[("v0.6.0-rc.1", true)]);
    let (tx, rx) = unbounded();
    let up = Updater::spawn(w.ctx.clone(), tx.clone());
    up.check_in_background();
    up.check_now();
    match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
        CoreEvent::Update(UpdateEvent::UpToDate) => {}
        _ => panic!("the manual check should answer first, the background one says nothing"),
    }
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());

    // With the setting off, the background check never starts.
    w.ctx.settings.check = false;
    w.list(&[("v0.9.0", false)]);
    let (tx2, rx2) = unbounded();
    let up2 = Updater::spawn(w.ctx.clone(), tx2);
    up2.check_in_background();
    assert!(rx2.recv_timeout(Duration::from_millis(400)).is_err());
    let _ = tx;
}

// ----------------------------------------------------------------------------- source mode

#[cfg(unix)]
mod source {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    struct Repo {
        w: World,
        /// The checkout rada updates from.
        work: PathBuf,
        /// Another clone, where "the author" makes commits.
        dev: PathBuf,
        cargo_log: PathBuf,
        env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    }

    fn git(env: &[(std::ffi::OsString, std::ffi::OsString)], dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit_version(env: &[(std::ffi::OsString, std::ffi::OsString)], dir: &Path, version: &str) {
        fs::write(
            dir.join("Cargo.toml"),
            format!("[workspace.package]\nversion = \"{version}\"\n"),
        )
        .unwrap();
        git(env, dir, &["add", "-A"]);
        git(
            env,
            dir,
            &["commit", "-q", "-m", &format!("version {version}")],
        );
    }

    /// A script standing in for `cargo`: `build` writes a program that says `NEW` into
    /// `target/release`, `clean` removes `target`, and a file named `fail` beside the script
    /// makes `build` fail. Every call is written to a log.
    fn fake_cargo(dir: &Path, log: &Path) -> PathBuf {
        let path = dir.join("cargo");
        fs::write(
            &path,
            format!(
                r#"#!/bin/sh
echo "$@" >> '{log}'
case "$1" in
  build)
    if [ -f '{dir}/fail' ]; then echo "error[E0000]: the fake build broke" >&2; exit 101; fi
    echo "   Compiling serde v1.0.0" >&2
    echo "   Compiling rada v{NEW}" >&2
    d="${{CARGO_TARGET_DIR:-$PWD/target}}/release"
    mkdir -p "$d"
    printf '#!/bin/sh\necho "rada {NEW}"\n' > "$d/rada"
    chmod 755 "$d/rada"
    ;;
  clean)
    rm -rf "${{CARGO_TARGET_DIR:-$PWD/target}}"
    ;;
esac
"#,
                log = log.display(),
                dir = dir.display(),
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn repo() -> Repo {
        let mut w = world("0.6.0-rc.1", Settings::default());
        let env: Vec<(std::ffi::OsString, std::ffi::OsString)> = vec![
            ("HOME".into(), w.sb.path("home").into()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_CONFIG_SYSTEM".into(), "/dev/null".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            // The sandbox is inside rada's own checkout: a folder without a repository of its
            // own must not be taken for part of that one.
            ("GIT_CEILING_DIRECTORIES".into(), w.sb.path("").into()),
            ("GIT_AUTHOR_NAME".into(), "t".into()),
            ("GIT_AUTHOR_EMAIL".into(), "t@example.com".into()),
            ("GIT_COMMITTER_NAME".into(), "t".into()),
            ("GIT_COMMITTER_EMAIL".into(), "t@example.com".into()),
        ];
        let origin = w.sb.mkdir("origin.git");
        git(&env, &origin, &["init", "-q", "--bare", "-b", "main"]);
        let dev = w.sb.mkdir("dev");
        git(&env, &dev, &["init", "-q", "-b", "main"]);
        git(
            &env,
            &dev,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        commit_version(&env, &dev, "0.6.0-rc.1");
        git(&env, &dev, &["push", "-q", "-u", "origin", "main"]);
        let work = w.sb.path("rada-fm");
        git(
            &env,
            &w.sb.path(""),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        let tools = w.sb.mkdir("tools");
        let cargo_log = w.sb.path("cargo.log");
        w.ctx.cargo = fake_cargo(&tools, &cargo_log).into();
        w.ctx.extra_env = env.clone();
        w.ctx.settings.method = Method::Source;
        w.ctx.settings.source_dir = Some(work.clone());
        Repo {
            w,
            work,
            dev,
            cargo_log,
            env,
        }
    }

    impl Repo {
        fn release_new_version(&self) {
            commit_version(&self.env, &self.dev, NEW);
            git(&self.env, &self.dev, &["push", "-q"]);
        }

        fn cargo_calls(&self) -> Vec<String> {
            fs::read_to_string(&self.cargo_log)
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        fn apply(&self) -> Result<update::Applied, String> {
            let r = release::Release {
                version: Version::parse(NEW).unwrap(),
                tag: format!("v{NEW}"),
                prerelease: false,
                notes: String::new(),
                assets: Vec::new(),
            };
            update::apply(&self.w.ctx, &r, &mut quiet)
        }
    }

    #[test]
    fn a_source_update_pulls_builds_installs_and_cleans() {
        let r = repo();
        r.release_new_version();
        assert!(matches!(r.w.ctx.mode().unwrap(), Mode::Source { .. }));
        let mut steps = Vec::new();
        let rel = release::Release {
            version: Version::parse(NEW).unwrap(),
            tag: format!("v{NEW}"),
            prerelease: false,
            notes: String::new(),
            assets: Vec::new(),
        };
        let done = update::apply(&r.w.ctx, &rel, &mut |s| steps.push(s)).unwrap();
        assert_eq!(done.version, NEW);
        assert!(done.restart);
        // The checkout has the new commit, the program is the new one, the build folder is gone.
        assert_eq!(
            fs::read_to_string(r.work.join("Cargo.toml")).unwrap(),
            format!("[workspace.package]\nversion = \"{NEW}\"\n")
        );
        assert_eq!(fs::read(&r.w.exe).unwrap(), program(NEW));
        assert!(!r.work.join("target").exists());
        assert_eq!(r.w.leftovers(), Vec::<String>::new());
        let calls = r.cargo_calls();
        assert_eq!(calls, ["build --release --locked", "clean"], "{calls:?}");
        for expected in ["checking the checkout", "pulling the latest changes"] {
            assert!(steps.iter().any(|s| s == expected), "{steps:?}");
        }
        assert!(
            steps.iter().any(|s| s.starts_with("building: ")),
            "{steps:?}"
        );
        // The old program was kept for a rollback, and the log has the build output.
        let (_, info) =
            update::install::read_backup(&r.w.ctx.update_dir().join("previous")).unwrap();
        assert_eq!(info.version, "0.6.0-rc.1");
        let log = fs::read_to_string(r.w.ctx.log_path()).unwrap();
        assert!(log.contains("Compiling serde"), "{log}");
        assert!(log.contains("git"), "{log}");
    }

    #[test]
    fn uncommitted_changes_or_another_branch_stop_it_before_anything_changes() {
        let r = repo();
        r.release_new_version();
        fs::write(r.work.join("Cargo.toml"), "edited by hand").unwrap();
        let err = r.apply().unwrap_err();
        assert!(err.contains("not committed"), "{err}");
        assert!(err.contains("Nothing was changed"), "{err}");
        assert_eq!(fs::read(&r.w.exe).unwrap(), b"the old program");
        assert!(r.cargo_calls().is_empty());
        // The edit is still there, untouched.
        assert_eq!(
            fs::read_to_string(r.work.join("Cargo.toml")).unwrap(),
            "edited by hand"
        );
        git(&r.env, &r.work, &["checkout", "-q", "--", "."]);

        git(&r.env, &r.work, &["switch", "-q", "-c", "feature"]);
        let err = r.apply().unwrap_err();
        assert!(
            err.contains("`feature`") && err.contains("not `main`"),
            "{err}"
        );
        assert_eq!(fs::read(&r.w.exe).unwrap(), b"the old program");
        assert!(r.cargo_calls().is_empty());
        assert_eq!(
            git(&r.env, &r.work, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "feature"
        );
    }

    #[test]
    fn untracked_files_do_not_count_as_changes() {
        let r = repo();
        r.release_new_version();
        fs::write(r.work.join("notes.txt"), "mine").unwrap();
        r.apply().unwrap();
        assert_eq!(
            fs::read_to_string(r.work.join("notes.txt")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn a_failed_build_leaves_the_old_program_and_a_log() {
        let r = repo();
        r.release_new_version();
        fs::write(
            r.w.ctx
                .cargo
                .clone()
                .into_string()
                .map(PathBuf::from)
                .unwrap()
                .parent()
                .unwrap()
                .join("fail"),
            "",
        )
        .unwrap();
        let err = r.apply().unwrap_err();
        assert!(err.contains("the build failed"), "{err}");
        assert!(err.contains("untouched"), "{err}");
        assert_eq!(fs::read(&r.w.exe).unwrap(), b"the old program");
        assert_eq!(r.w.leftovers(), Vec::<String>::new());
        let log = fs::read_to_string(r.w.ctx.log_path()).unwrap();
        assert!(log.contains("the fake build broke"), "{log}");
        assert!(log.contains("failed:"), "{log}");
        // No clean after a failure: the partial build helps the next try.
        assert_eq!(r.cargo_calls(), ["build --release --locked"]);
    }

    #[test]
    fn a_pull_that_cannot_fast_forward_stops() {
        let r = repo();
        // The checkout has a commit of its own and so does the upstream.
        commit_version(&r.env, &r.work, "0.6.0-local");
        r.release_new_version();
        let err = r.apply().unwrap_err();
        assert!(err.contains("git pull --ff-only"), "{err}");
        assert_eq!(fs::read(&r.w.exe).unwrap(), b"the old program");
        assert!(r.cargo_calls().is_empty());
    }

    #[test]
    fn nothing_new_means_nothing_to_build() {
        let mut r = repo();
        // The program was built from this very checkout.
        r.w.ctx.current = Version::parse("0.6.0-rc.1").unwrap();
        let done = r.apply().unwrap();
        assert!(!done.restart);
        assert!(done.note.contains("nothing to build"), "{}", done.note);
        assert!(r.cargo_calls().is_empty());
    }

    #[test]
    fn a_program_running_from_the_build_folder_keeps_it() {
        let mut r = repo();
        let target = r.work.join("target/release");
        fs::create_dir_all(&target).unwrap();
        let exe = target.join("rada");
        fs::write(&exe, b"the old program").unwrap();
        r.w.ctx.exe = exe.clone();
        r.release_new_version();
        let done = r.apply().unwrap();
        assert!(
            done.note.contains("kept because rada runs from it"),
            "{}",
            done.note
        );
        // The build wrote the new program where the old one was.
        assert_eq!(fs::read(&exe).unwrap(), program(NEW));
        assert_eq!(r.cargo_calls(), ["build --release --locked"]);
    }

    #[test]
    fn no_checkout_is_explained() {
        let mut r = repo();
        r.w.ctx.settings.source_dir = Some(r.w.sb.path("not-there"));
        let err = r.apply().unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
        r.w.ctx.settings.source_dir = Some(r.w.sb.mkdir("plain-folder"));
        let err = r.apply().unwrap_err();
        assert!(err.contains("not a git checkout"), "{err}");
    }
}

// ------------------------------------------------------------- the tool that signs releases

/// Files made by `rsign` (the tool `scripts/sign-release.sh` signs with) for a key pair made
/// to be thrown away: the format the program reads is the format that tool writes.
#[test]
fn a_signature_made_by_the_rsign_tool_is_read() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minisign");
    let sums = fs::read(dir.join("SHA256SUMS")).unwrap();
    let sig = fs::read_to_string(dir.join("SHA256SUMS.minisig")).unwrap();
    let public = fs::read_to_string(dir.join("test.pub")).unwrap();
    // The second line of the .pub file is the key the program's list holds.
    let key = public.lines().nth(1).unwrap().trim().to_string();
    let keys = vec![key];

    let signed = update::sign::verify_sums(&sums, &sig, "v0.0.1-fixture", &keys).unwrap();
    assert_eq!(signed.comment, "rada v0.0.1-fixture");
    assert_eq!(signed.key, 0);

    let wrong_tag = update::sign::verify_sums(&sums, &sig, "v0.0.2", &keys).unwrap_err();
    assert!(wrong_tag.contains("rada v0.0.1-fixture"), "{wrong_tag}");
    let mut changed = sums.clone();
    changed[0] = b'3';
    assert!(update::sign::verify_sums(&changed, &sig, "v0.0.1-fixture", &keys).is_err());
}
