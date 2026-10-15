//! Updates: finding out that a newer rada exists, and replacing the running program with it.
//!
//! * [`check`] reads GitHub's public list of releases (at most once a day, from a worker that
//!   never delays the start), and says which release this copy should move to. Whoever runs
//!   a stable version sees only stable releases; whoever runs a pre-release also sees later
//!   pre-releases. Nothing is sent but the request and `User-Agent: rada/<version>`.
//! * [`apply`] does it, in the way that fits how this copy got here ([`mode::Mode`]):
//!   download and swap a release program, pull and build a git checkout, or leave a program
//!   that a package manager owns alone and say what to run.
//!
//! The functions here are plain blocking code so that the command line and the interface
//! share them; [`Updater`] runs them on a thread and reports back as events.

pub mod http;
pub mod install;
pub mod mode;
pub mod release;
pub mod source;
pub mod version;

use crate::i18n::{Key, tf};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use mode::Mode;
pub use release::{Release, resolve_channel};
pub use version::Version;

use crate::events::CoreEvent;

/// Where the releases are listed.
pub const DEFAULT_API: &str =
    "https://api.github.com/repos/FormikaDesk/rada-fm/releases?per_page=30";

/// A check is made at most once in this long.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// After a check that failed (no network), the next try waits this long.
pub const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

const MAX_LIST_BYTES: u64 = 8 << 20;
const MAX_SUMS_BYTES: u64 = 1 << 20;
const MAX_ARCHIVE_BYTES: u64 = 256 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Channel {
    #[default]
    Auto,
    Stable,
    Prerelease,
}

impl Channel {
    pub fn parse(s: &str) -> Option<Channel> {
        match s {
            "auto" => Some(Channel::Auto),
            "stable" => Some(Channel::Stable),
            "prerelease" | "pre-release" => Some(Channel::Prerelease),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Method {
    #[default]
    Auto,
    Binary,
    Source,
    Package,
}

impl Method {
    pub fn parse(s: &str) -> Option<Method> {
        match s {
            "auto" => Some(Method::Auto),
            "binary" => Some(Method::Binary),
            "source" => Some(Method::Source),
            "package" => Some(Method::Package),
            _ => None,
        }
    }
}

/// What the `[update]` section of the configuration says.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Look for a newer version in the background (`rada update` always can).
    pub check: bool,
    pub channel: Channel,
    pub method: Method,
    pub source_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            check: true,
            channel: Channel::Auto,
            method: Method::Auto,
            source_dir: None,
        }
    }
}

/// Everything an update needs to know about this copy of rada and where it lives.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub current: Version,
    pub settings: Settings,
    /// The URL of the list of releases.
    pub api_url: String,
    /// The running program, links followed.
    pub exe: PathBuf,
    /// rada's state folder; the update's own files are in `update/` inside it.
    pub state_dir: PathBuf,
    pub home: PathBuf,
    /// The target the release files are named after; `None` where there is no build.
    pub target: Option<String>,
    pub user_agent: String,
    /// `ID=` from `/etc/os-release`, to name the right package tool.
    pub os_id: Option<String>,
    pub git: OsString,
    pub cargo: OsString,
    /// Start the new program with `--version` before installing it.
    pub run_check: bool,
    /// Variables set for the programs an update starts (`git`, `cargo`): tests keep them away
    /// from the real configuration.
    pub extra_env: Vec<(OsString, OsString)>,
}

impl Ctx {
    /// For the running program. `RADA_UPDATE_API` replaces the address of the list of
    /// releases (a mirror, or a test).
    pub fn for_this_program(
        version: &str,
        settings: Settings,
        state_dir: PathBuf,
        home: PathBuf,
    ) -> Result<Ctx, String> {
        let current = Version::parse(version).ok_or_else(|| tf(Key::UpNotVersion, &[&version]))?;
        let exe = std::env::current_exe().map_err(|e| tf(Key::UpNoProgram, &[&e]))?;
        let exe = {
            let s = exe.to_string_lossy();
            // Linux appends this to the path of a program whose file has been replaced.
            let clean = PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s).to_string());
            fs::canonicalize(&clean).unwrap_or(clean)
        };
        let translated = cfg!(target_os = "macos") && under_rosetta();
        Ok(Ctx {
            user_agent: format!("rada/{current}"),
            target: release::target_for(std::env::consts::OS, std::env::consts::ARCH, translated),
            os_id: fs::read_to_string("/etc/os-release")
                .ok()
                .and_then(|t| mode::os_release_id(&t)),
            api_url: std::env::var("RADA_UPDATE_API").unwrap_or_else(|_| DEFAULT_API.into()),
            current,
            settings,
            exe,
            state_dir,
            home,
            git: "git".into(),
            cargo: "cargo".into(),
            run_check: true,
            extra_env: Vec::new(),
        })
    }

    pub fn update_dir(&self) -> PathBuf {
        self.state_dir.join("update")
    }

    fn state_file(&self) -> PathBuf {
        self.state_dir.join("update.json")
    }

    /// How this copy is updated.
    pub fn mode(&self) -> Result<Mode, String> {
        mode::detect(
            self.settings.method,
            &self.exe,
            self.settings.source_dir.as_deref(),
            self.os_id.as_deref(),
        )
    }

    pub fn log_path(&self) -> PathBuf {
        source::log_path(&self.state_dir)
    }

    fn http(&self) -> http::Http {
        http::Http::new(&self.user_agent, self.api_url.starts_with("http://"))
    }
}

/// Running an Intel program on Apple silicon (Rosetta 2).
fn under_rosetta() -> bool {
    std::process::Command::new("sysctl")
        .args(["-n", "sysctl.proc_translated"])
        .output()
        .ok()
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
}

// ------------------------------------------------------------------------------- state

/// A newer version that was found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub version: String,
    pub tag: String,
    pub prerelease: bool,
    pub notes: String,
}

impl Notice {
    fn of(r: &Release) -> Notice {
        Notice {
            version: r.version.to_string(),
            tag: r.tag.clone(),
            prerelease: r.prerelease,
            notes: r.notes.clone(),
        }
    }
}

/// What is remembered between runs: when releases were last read, and what they said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckState {
    /// Seconds since the epoch of the last successful check.
    #[serde(default)]
    pub last_check: u64,
    /// Seconds of the last try, successful or not.
    #[serde(default)]
    pub last_attempt: u64,
    #[serde(default)]
    pub latest: Option<Notice>,
}

impl CheckState {
    pub fn load(ctx: &Ctx) -> CheckState {
        fs::read(ctx.state_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, ctx: &Ctx) {
        let write = || -> std::io::Result<()> {
            crate::statefile::write_atomic(
                &ctx.state_file(),
                &serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
            )
        };
        if let Err(e) = write() {
            tracing::warn!("cannot save the update state: {e}");
        }
    }

    /// Whether the network should be asked now.
    pub fn due(&self, now: u64) -> bool {
        let a_day_since_a_check =
            self.last_check == 0 || now.saturating_sub(self.last_check) >= CHECK_EVERY.as_secs();
        let an_hour_since_a_try = now.saturating_sub(self.last_attempt) >= RETRY_AFTER.as_secs();
        a_day_since_a_check && an_hour_since_a_try
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ------------------------------------------------------------------------------- check

/// The releases GitHub lists.
pub fn fetch_releases(ctx: &Ctx) -> Result<Vec<Release>, String> {
    let text = ctx.http().get_text(&ctx.api_url, MAX_LIST_BYTES)?;
    release::parse_releases(&text)
}

/// The release this copy should move to, if there is one (asks the network).
pub fn newest_release(ctx: &Ctx) -> Result<Option<Release>, String> {
    let releases = fetch_releases(ctx)?;
    Ok(release::newest(&releases, &ctx.current, ctx.settings.channel).cloned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// A newer version exists.
    Newer(Notice),
    UpToDate,
    /// The network could not be read (only worth showing to someone who asked).
    Failed(String),
}

/// Look for a newer version. Unless `force`, the network is asked only when a check is due;
/// in between, what the last check found is answered again (if it is still newer than the
/// running program).
pub fn check(ctx: &Ctx, force: bool, now: u64) -> Check {
    check_full(ctx, force, now).0
}

/// [`check`], and the release itself when the network was asked and found one.
pub fn check_full(ctx: &Ctx, force: bool, now: u64) -> (Check, Option<Release>) {
    let mut state = CheckState::load(ctx);
    if !force && !state.due(now) {
        let cached = state
            .latest
            .filter(|n| Version::parse(&n.version).is_some_and(|v| v > ctx.current));
        return (
            match cached {
                Some(n) => Check::Newer(n),
                None => Check::UpToDate,
            },
            None,
        );
    }
    state.last_attempt = now;
    match newest_release(ctx) {
        Ok(found) => {
            state.last_check = now;
            state.latest = found.as_ref().map(Notice::of);
            state.save(ctx);
            match found {
                Some(r) => (Check::Newer(Notice::of(&r)), Some(r)),
                None => (Check::UpToDate, None),
            }
        }
        Err(e) => {
            state.save(ctx);
            (Check::Failed(e), None)
        }
    }
}

// ------------------------------------------------------------------------------- apply

/// What an update did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub version: String,
    /// A sentence for the person.
    pub note: String,
    /// The running program is not the new one: start rada again.
    pub restart: bool,
}

pub fn sha256_of(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = fs::File::open(path).map_err(|e| tf(Key::UpCannotRead, &[&path.display(), &e]))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| tf(Key::UpCannotRead, &[&path.display(), &e]))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn percent(done: u64, total: Option<u64>) -> String {
    match total {
        Some(t) if t > 0 => format!("{}%", (done * 100 / t).min(100)),
        _ => format!("{} KB", done / 1024),
    }
}

/// Replace the running program with `release`, in the way [`Ctx::mode`] says. `report` is
/// told what is going on.
pub fn apply(
    ctx: &Ctx,
    release: &Release,
    report: &mut dyn FnMut(String),
) -> Result<Applied, String> {
    let mut log = source::Log::create(&ctx.state_dir);
    log.line(&format!(
        "rada {} -> {} ({})",
        ctx.current, release.version, release.tag
    ));
    let result = match ctx.mode()? {
        Mode::Package { hint } => Err(hint),
        Mode::Source { dir } => source::run(ctx, &dir, &mut log, report),
        Mode::Binary => apply_binary(ctx, release, &mut log, report),
    };
    match &result {
        Ok(a) => log.line(&format!("done: {}", a.note)),
        Err(e) => log.line(&format!("failed: {e}")),
    }
    if result.is_ok() {
        // What was found is now installed: nothing to announce next time.
        let mut s = CheckState::load(ctx);
        s.latest = None;
        s.save(ctx);
    }
    result
}

fn apply_binary(
    ctx: &Ctx,
    release: &Release,
    log: &mut source::Log,
    report: &mut dyn FnMut(String),
) -> Result<Applied, String> {
    let target = ctx
        .target
        .as_deref()
        .ok_or("there is no ready-made rada for this system and processor: build it from source")?;
    let archive_name = release::archive_name(&release.version, target);
    let archive = release.asset(&archive_name).ok_or_else(|| {
        format!(
            "release {} has no file for this system ({archive_name})",
            release.tag
        )
    })?;
    let sums = release.asset("SHA256SUMS").ok_or_else(|| {
        format!(
            "release {} has no SHA256SUMS: it will not be installed",
            release.tag
        )
    })?;
    let http = ctx.http();
    // Plain addresses are for a mirror or a test, which the API address said to expect.
    let work = ctx
        .update_dir()
        .join(format!("work-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| tf(Key::UpCannotCreate, &[&work.display(), &e]))?;
    let result = (|| {
        report(Key::UpReadingSums.tr().to_string());
        let sums_text = http.get_text(&sums.url, MAX_SUMS_BYTES)?;
        let want = release::checksum_for(&sums_text, &archive_name)
            .ok_or_else(|| tf(Key::UpSumsMissing, &[&archive_name]))?;
        log.line(&format!("{archive_name} should be {want}"));

        let file = work.join(&archive_name);
        let mut last = u64::MAX;
        http.download(
            &archive.url,
            &file,
            MAX_ARCHIVE_BYTES,
            &mut |done, total| {
                let step = done / (256 * 1024);
                if step != last {
                    last = step;
                    report(tf(
                        Key::UpDownloading,
                        &[&archive_name, &percent(done, total)],
                    ));
                }
                true
            },
        )?;
        report(Key::UpChecking.tr().to_string());
        let got = sha256_of(&file)?;
        log.line(&format!("downloaded file is {got}"));
        if got != want {
            return Err(tf(Key::UpChecksumBad, &[&want, &got]));
        }

        report(Key::UpUnpacking.tr().to_string());
        let stage = install::stage_path(&ctx.exe);
        let inner = release::inner_path(&release.version, target);
        install::extract_binary(&file, &inner, &stage).map_err(|e| {
            if e.contains("Permission denied") || e.contains("denied") {
                tf(
                    Key::UpCannotWriteDir,
                    &[
                        &ctx.exe
                            .parent()
                            .map(|d| d.display().to_string())
                            .unwrap_or_default(),
                        &e,
                    ],
                )
            } else {
                e
            }
        })?;
        if ctx.run_check {
            report(Key::UpCheckingNew.tr().to_string());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&stage, fs::Permissions::from_mode(0o755));
            }
            let out = match install::run_version(&stage, Duration::from_secs(15)) {
                Ok(o) => o,
                Err(e) => {
                    let _ = fs::remove_file(&stage);
                    return Err(e);
                }
            };
            log.line(&format!("new program: {out}"));
            if out != format!("rada {}", release.version) {
                let _ = fs::remove_file(&stage);
                return Err(tf(Key::UpNewSays, &[&out, &release.version]));
            }
        }
        if let Err(e) = install::save_backup(
            &ctx.exe,
            &ctx.current.to_string(),
            &ctx.update_dir().join("previous"),
        ) {
            // Not fatal: the update itself is safe, only going back would not be possible.
            log.line(&format!("no copy of the old program kept: {e}"));
        }
        report(Key::UpInstalling.tr().to_string());
        install::replace_exe(&stage, &ctx.exe)?;
        if install::has_quarantine(&ctx.exe) {
            install::clear_quarantine(&ctx.exe);
            log.line("removed the quarantine flag");
        }
        Ok(Applied {
            version: release.version.to_string(),
            note: tf(Key::UpInstalledAt, &[&release.version, &ctx.exe.display()]),
            restart: true,
        })
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

/// Put the saved previous program back.
pub fn rollback(ctx: &Ctx) -> Result<String, String> {
    match ctx.mode()? {
        Mode::Binary | Mode::Source { .. } => {}
        Mode::Package { hint } => return Err(hint),
    }
    install::rollback(
        &ctx.exe,
        &ctx.current.to_string(),
        &ctx.update_dir().join("previous"),
        ctx.run_check,
    )
}

// ------------------------------------------------------------------------------ worker

/// What the update worker tells the interface.
#[derive(Debug)]
pub enum UpdateEvent {
    /// A newer version exists; `how` is what an update would do.
    Available {
        notice: Notice,
        how: Result<Mode, String>,
    },
    /// Answer to a check somebody asked for: nothing newer.
    UpToDate,
    /// Answer to a check somebody asked for: it could not be made.
    CheckFailed(String),
    /// A step of the update in progress.
    Progress(String),
    Done(Applied),
    Failed {
        message: String,
        log: PathBuf,
    },
}

enum Req {
    Check { force: bool },
    Apply,
    Rollback,
}

/// The worker. Cheap to clone; every request is answered with [`CoreEvent::Update`].
#[derive(Clone)]
pub struct Updater {
    tx: Sender<Req>,
    ctx: std::sync::Arc<Ctx>,
}

impl Updater {
    pub fn spawn(ctx: Ctx, out: Sender<CoreEvent>) -> Updater {
        let ctx = std::sync::Arc::new(ctx);
        let (tx, rx) = unbounded::<Req>();
        let worker_ctx = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("rada-update".into())
            .spawn(move || worker(worker_ctx, rx, out));
        Updater { tx, ctx }
    }

    pub fn ctx(&self) -> &Ctx {
        &self.ctx
    }

    /// The daily check, if the settings and the clock say it is time. Silent about failures.
    pub fn check_in_background(&self) {
        if self.ctx.settings.check {
            let _ = self.tx.send(Req::Check { force: false });
        }
    }

    /// A check somebody asked for: always asks the network and always answers.
    pub fn check_now(&self) {
        let _ = self.tx.send(Req::Check { force: true });
    }

    pub fn apply(&self) {
        let _ = self.tx.send(Req::Apply);
    }

    pub fn rollback(&self) {
        let _ = self.tx.send(Req::Rollback);
    }
}

fn worker(ctx: std::sync::Arc<Ctx>, rx: Receiver<Req>, out: Sender<CoreEvent>) {
    let send = |e: UpdateEvent| out.send(CoreEvent::Update(e)).is_ok();
    // A program replaced on Windows leaves its old file behind until the next start.
    if cfg!(windows) {
        install::clean_leftovers(&ctx.exe);
    }
    while let Ok(req) = rx.recv() {
        match req {
            Req::Check { force } => match check(&ctx, force, now_secs()) {
                Check::Newer(notice) => {
                    send(UpdateEvent::Available {
                        notice,
                        how: ctx.mode(),
                    });
                }
                Check::UpToDate if force => {
                    send(UpdateEvent::UpToDate);
                }
                Check::Failed(e) if force => {
                    send(UpdateEvent::CheckFailed(e));
                }
                Check::UpToDate | Check::Failed(_) => {}
            },
            Req::Apply => {
                let release = match newest_release(&ctx) {
                    Ok(Some(r)) => r,
                    Ok(None) => {
                        send(UpdateEvent::UpToDate);
                        continue;
                    }
                    Err(e) => {
                        send(UpdateEvent::Failed {
                            message: e,
                            log: ctx.log_path(),
                        });
                        continue;
                    }
                };
                let mut tell = |text: String| {
                    send(UpdateEvent::Progress(text));
                };
                match apply(&ctx, &release, &mut tell) {
                    Ok(a) => send(UpdateEvent::Done(a)),
                    Err(message) => send(UpdateEvent::Failed {
                        message,
                        log: ctx.log_path(),
                    }),
                };
            }
            Req::Rollback => {
                match rollback(&ctx) {
                    Ok(v) => send(UpdateEvent::Done(Applied {
                        version: v.clone(),
                        note: tf(Key::UpWentBack, &[&v]),
                        restart: true,
                    })),
                    Err(message) => send(UpdateEvent::Failed {
                        message,
                        log: ctx.log_path(),
                    }),
                };
            }
        }
    }
}
