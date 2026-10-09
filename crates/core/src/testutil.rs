//! Test support: an isolated sandbox and a tripwire on the user's real directories.
//!
//! Every test that touches the filesystem builds a [`Sandbox`]. On first use the
//! process is re-pointed at a scratch area (`HOME` and all `XDG_*_HOME` variables),
//! so even a bug that resolves a "default" location cannot reach the real one. On
//! top of that a [`RealPathGuard`] fingerprints the *real* trash, config, state, cache
//! and data directories (and the names in the real home directory) before the first
//! test and re-checks them whenever a sandbox is dropped: if anything changed, the
//! test fails.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use crate::fs::{CopyControl, CopyOutcome, CopyRequest, DirItem, FsEngine, FsMeta, LocalFs};
use crate::journal::Journal;
use crate::ops::{
    Cancel, ConflictPolicy, Engine, ErrorChoice, ExecHandler, ExecReport, Failure, Plan, Progress,
    RunOutcome, Scan, ScanControl, SkipErrors, TransferOptions, UndoPlan,
};
use crate::platform::{self, Dirs, Platform};

// ---------------------------------------------------------------------------------
// Real-path tripwire
// ---------------------------------------------------------------------------------

/// A fingerprint of a set of paths: names, kinds, sizes and mtimes of everything below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RealPathGuard {
    paths: Vec<PathBuf>,
    /// Directories whose *entry names* (not contents) are fingerprinted.
    shallow: Vec<PathBuf>,
    before: BTreeMap<String, String>,
}

impl RealPathGuard {
    pub fn new(paths: Vec<PathBuf>, shallow: Vec<PathBuf>) -> Self {
        let mut g = RealPathGuard {
            paths,
            shallow,
            before: BTreeMap::new(),
        };
        g.before = g.capture();
        g
    }

    fn capture(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for p in &self.paths {
            walk(p, &mut out, 0);
        }
        for p in &self.shallow {
            match std::fs::read_dir(p) {
                Ok(rd) => {
                    for e in rd.flatten() {
                        out.insert(format!("{}", e.path().display()), "entry".into());
                    }
                }
                Err(_) => {
                    out.insert(format!("{}", p.display()), "unreadable".into());
                }
            }
        }
        out
    }

    /// `Err` with a readable diff if anything under the guarded paths changed.
    pub fn check(&self) -> Result<(), String> {
        let now = self.capture();
        if now == self.before {
            return Ok(());
        }
        let mut msg = String::from("a REAL user path was touched by a test:\n");
        for (k, v) in &now {
            match self.before.get(k) {
                None => msg.push_str(&format!("  + {k} ({v})\n")),
                Some(old) if old != v => msg.push_str(&format!("  ~ {k} ({old} -> {v})\n")),
                _ => {}
            }
        }
        for k in self.before.keys() {
            if !now.contains_key(k) {
                msg.push_str(&format!("  - {k}\n"));
            }
        }
        Err(msg)
    }
}

fn walk(p: &Path, out: &mut BTreeMap<String, String>, depth: usize) {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return;
    };
    let kind = if m.is_dir() {
        "dir"
    } else if m.file_type().is_symlink() {
        "link"
    } else {
        "file"
    };
    let fp = if m.is_dir() {
        kind.to_string()
    } else {
        format!("{kind} {} {:?}", m.len(), m.modified().ok())
    };
    out.insert(p.display().to_string(), fp);
    if m.is_dir()
        && depth < 8
        && let Ok(rd) = std::fs::read_dir(p)
    {
        for e in rd.flatten() {
            walk(&e.path(), out, depth + 1);
        }
    }
}

struct Process {
    guard: RealPathGuard,
    scratch: PathBuf,
}

static PROCESS: OnceLock<Process> = OnceLock::new();

/// Base directory for test sandboxes: on the project's own disk, never in `/tmp`.
pub fn disk_base() -> PathBuf {
    let base = std::env::var_os("RADA_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/rada-test-tmp")
        });
    let _ = std::fs::create_dir_all(&base);
    base.canonicalize().unwrap_or(base)
}

/// Remove the scratch areas (`proc-<pid>`) that earlier test runs left in `base`
/// because they were interrupted (killed, power loss). An area still owned by a live
/// process, and anything not named like a scratch area, is left alone.
fn reap_stale_scratch(base: &Path, own_pid: u32) {
    let Ok(rd) = std::fs::read_dir(base) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix("proc-"))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid != own_pid && !scratch_owner_alive(pid, &e.path()) {
            force_remove_dir_all(&e.path());
        }
    }
}

/// Whether the process that created a scratch area may still be running.
#[cfg(target_os = "linux")]
fn scratch_owner_alive(pid: u32, _dir: &Path) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Without a process table to ask, an area is stale once it has not been touched for a day.
#[cfg(not(target_os = "linux"))]
fn scratch_owner_alive(_pid: u32, dir: &Path) -> bool {
    const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age < DAY)
}

/// `remove_dir_all` that first gives back the permissions a test may have taken away.
fn force_remove_dir_all(dir: &Path) {
    restore_permissions(dir);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
fn restore_permissions(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(m) = std::fs::symlink_metadata(dir) else {
        return;
    };
    if !m.is_dir() {
        return;
    }
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            restore_permissions(&e.path());
        }
    }
}

#[cfg(not(unix))]
fn restore_permissions(_dir: &Path) {}

fn process() -> &'static Process {
    PROCESS.get_or_init(|| {
        // Capture the REAL locations before re-pointing the environment.
        let real = Dirs::from_env().ok();
        let mut paths = Vec::new();
        let mut shallow = Vec::new();
        if let Some(r) = &real {
            paths.push(r.home_trash());
            paths.push(r.rada_state());
            paths.push(r.rada_config());
            paths.push(r.rada_cache());
            paths.push(r.data.join("rada"));
            shallow.push(r.home.clone());
        }
        let guard = RealPathGuard::new(paths, shallow);

        let base = disk_base();
        reap_stale_scratch(&base, std::process::id());
        let scratch = base.join(format!("proc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&scratch);
        let d = Dirs::under(&scratch);
        // SAFETY: executed once, before any sandbox exists; the engine itself never
        // reads the environment (all locations are passed explicitly).
        unsafe {
            std::env::set_var("HOME", &d.home);
            std::env::set_var("XDG_CONFIG_HOME", &d.config);
            std::env::set_var("XDG_DATA_HOME", &d.data);
            std::env::set_var("XDG_STATE_HOME", &d.state);
            std::env::set_var("XDG_CACHE_HOME", &d.cache);
        }
        Process { guard, scratch }
    })
}

/// Fail (panic) if a real user path changed since the process started its tests.
pub fn assert_real_paths_untouched() {
    if let Err(msg) = process().guard.check() {
        panic!("{msg}");
    }
}

// ---------------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------------

pub struct Sandbox {
    _tmp: tempfile::TempDir,
    pub root: PathBuf,
    /// Scratch area for fixture trees.
    pub work: PathBuf,
    pub dirs: Dirs,
    platform: Arc<dyn Platform>,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        let p = process();
        let tmp = tempfile::Builder::new()
            .prefix("sb-")
            .tempdir_in(&p.scratch)
            .expect("create sandbox");
        let root = tmp.path().canonicalize().expect("canonicalize sandbox");
        let dirs = Dirs::under(&root);
        let work = root.join("work");
        for d in [
            &dirs.home,
            &dirs.config,
            &dirs.data,
            &dirs.state,
            &dirs.cache,
            &work,
        ] {
            std::fs::create_dir_all(d).expect("create sandbox dir");
        }
        let platform = platform::current(dirs.clone());
        Sandbox {
            _tmp: tmp,
            root,
            work,
            dirs,
            platform,
        }
    }

    pub fn platform(&self) -> Arc<dyn Platform> {
        self.platform.clone()
    }

    /// Engine over the real local filesystem with the sandboxed platform.
    pub fn engine(&self) -> Engine {
        Engine::local(self.platform())
    }

    /// A journal in this sandbox's (isolated) state directory.
    pub fn journal(&self) -> Journal {
        Journal::open(self.dirs.journal_path()).expect("open journal")
    }

    /// Engine over a custom filesystem (fault injection).
    pub fn engine_with(&self, fs: Arc<dyn FsEngine>) -> Engine {
        Engine::new(fs, self.platform())
    }

    pub fn path(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.work.join(rel)
    }

    pub fn mkdir(&self, rel: impl AsRef<Path>) -> PathBuf {
        let p = self.path(rel);
        std::fs::create_dir_all(&p).expect("mkdir");
        p
    }

    pub fn write(&self, rel: impl AsRef<Path>, content: impl AsRef<[u8]>) -> PathBuf {
        let p = self.path(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("mkdir parent");
        }
        std::fs::write(&p, content).expect("write");
        p
    }

    pub fn symlink(&self, target: impl AsRef<Path>, rel: impl AsRef<Path>) -> PathBuf {
        let p = self.path(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("mkdir parent");
        }
        symlink(target.as_ref(), &p).expect("symlink");
        p
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_real_paths_untouched();
        }
    }
}

pub fn scan(engine: &Engine, paths: &[PathBuf]) -> Scan {
    let cancel = Cancel::new();
    engine.scan(
        paths,
        ScanControl {
            cancel: cancel.flag(),
            progress: &mut |_| {},
        },
    )
}

/// Execute a plan, skipping (and recording) every failure.
pub fn run(engine: &Engine, plan: &Plan) -> ExecReport {
    engine.execute(plan, &mut SkipErrors, &Cancel::new())
}

/// Records progress and answers failures from a script.
pub struct Scripted {
    pub choices: std::collections::VecDeque<ErrorChoice>,
    pub default: ErrorChoice,
    pub failures: Vec<String>,
    pub progress: Vec<Progress>,
}

impl Scripted {
    pub fn new(default: ErrorChoice) -> Self {
        Scripted {
            choices: Default::default(),
            default,
            failures: Vec::new(),
            progress: Vec::new(),
        }
    }
    pub fn with(mut self, c: ErrorChoice) -> Self {
        self.choices.push_back(c);
        self
    }
}

impl ExecHandler for Scripted {
    fn progress(&mut self, p: &Progress) {
        self.progress.push(p.clone());
    }
    fn on_failure(&mut self, f: &Failure<'_>) -> ErrorChoice {
        self.failures.push(f.error.to_string());
        self.choices.pop_front().unwrap_or(self.default)
    }
}

pub fn do_copy(
    e: &Engine,
    srcs: &[PathBuf],
    dest: &Path,
    policy: ConflictPolicy,
) -> (Plan, ExecReport) {
    let plan = e.plan_transfer(&scan(e, srcs), dest, &TransferOptions::copy(policy));
    let rep = run(e, &plan);
    (plan, rep)
}

pub fn do_move(
    e: &Engine,
    srcs: &[PathBuf],
    dest: &Path,
    policy: ConflictPolicy,
) -> (Plan, ExecReport) {
    let plan = e.plan_transfer(&scan(e, srcs), dest, &TransferOptions::mv(policy));
    let rep = run(e, &plan);
    (plan, rep)
}

/// Run a plan through the journal, skipping failures.
pub fn run_journaled(e: &Engine, j: &Journal, plan: &Plan) -> RunOutcome {
    e.run_operation(j, plan, &mut SkipErrors, &Cancel::new())
        .expect("run operation")
}

/// Plan and run the undo of `id`.
pub fn undo(e: &Engine, j: &Journal, id: &str) -> (UndoPlan, ExecReport) {
    let up = e.plan_undo(j, id).expect("plan undo");
    let rep = e
        .run_undo(j, &up, &mut SkipErrors, &Cancel::new())
        .expect("run undo");
    (up, rep)
}

/// A scratch directory on a *different* filesystem from the sandbox (tmpfs vs disk),
/// or `None` when this machine has no such location (the test then skips itself).
#[cfg(not(unix))]
pub fn other_filesystem_dir(_sb: &Sandbox) -> Option<tempfile::TempDir> {
    None
}

#[cfg(unix)]
pub fn other_filesystem_dir(sb: &Sandbox) -> Option<tempfile::TempDir> {
    use std::os::unix::fs::MetadataExt;
    let mine = std::fs::metadata(&sb.root).ok()?.dev();
    for cand in ["/dev/shm", "/tmp", "/run/user"] {
        let p = Path::new(cand);
        if !p.is_dir() {
            continue;
        }
        if std::fs::metadata(p).map(|m| m.dev()).ok() == Some(mine) {
            continue;
        }
        if let Ok(t) = tempfile::Builder::new().prefix("rada-xfs-").tempdir_in(p) {
            return Some(t);
        }
    }
    None
}

pub fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
}

pub fn chmod(p: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }
    #[cfg(not(unix))]
    {
        let _ = (p, mode);
    }
}

/// True when running as root (permission-denied fixtures do not work then).
pub fn is_root() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: getuid cannot fail.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

// ---------------------------------------------------------------------------------
// Tree snapshots
// ---------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Snap {
    Dir { mode: u32 },
    File { mode: u32, content: Vec<u8> },
    Link { target: PathBuf },
    Other,
}

/// Everything below `root`, relative, without following symlinks.
pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, Snap> {
    let mut out = BTreeMap::new();
    fn go(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Snap>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            let m = std::fs::symlink_metadata(&p).unwrap();
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o7777
            };
            #[cfg(not(unix))]
            let mode = 0;
            if m.file_type().is_symlink() {
                out.insert(
                    rel,
                    Snap::Link {
                        target: std::fs::read_link(&p).unwrap(),
                    },
                );
            } else if m.is_dir() {
                out.insert(rel, Snap::Dir { mode });
                go(root, &p, out);
            } else if m.is_file() {
                out.insert(
                    rel,
                    Snap::File {
                        mode,
                        content: std::fs::read(&p).unwrap_or_default(),
                    },
                );
            } else {
                out.insert(rel, Snap::Other);
            }
        }
    }
    go(root, root, &mut out);
    out
}

// ---------------------------------------------------------------------------------
// Fault injection
// ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Any,
    Lstat,
    ReadDir,
    CopyFile,
    /// Creating a file to write into (extraction, a new archive). The file has a temporary
    /// name, so a rule matches the folder it is created in.
    CreateFile,
    Rename,
    RemoveFile,
    RemoveDir,
    CreateDir,
    CreateSymlink,
    HardLink,
}

/// What happens when a copy has moved `after` bytes of a matching file.
pub enum ByteAction {
    /// The copy fails with this errno, partway through (a full disk, a revoked right).
    Fail(i32),
    /// Something else happens to the world at that moment (the source is changed, the
    /// destination vanishes...) and the copy carries on.
    Run(Arc<dyn Fn() + Send + Sync>),
}

struct ByteRule {
    suffix: PathBuf,
    after: u64,
    action: ByteAction,
    fired: bool,
}

/// Runs `f` on the `nth` (1-based) matching call, before the call itself.
struct Hook {
    op: Op,
    suffix: PathBuf,
    nth: usize,
    seen: usize,
    f: Arc<dyn Fn() + Send + Sync>,
}

struct Rule {
    op: Op,
    /// Matches when the operation's path ends with this.
    suffix: PathBuf,
    errno: i32,
    remaining: Option<usize>,
}

/// A [`LocalFs`] that fails on demand: unreadable files, `EXDEV`, `ENOSPC`...
pub struct FaultFs {
    inner: LocalFs,
    rules: Mutex<Vec<Rule>>,
    byte_rules: Mutex<Vec<ByteRule>>,
    hooks: Mutex<Vec<Hook>>,
    write_rules: Mutex<Vec<WriteRule>>,
    pub hits: Mutex<Vec<(Op, PathBuf)>>,
}

struct WriteRule {
    suffix: PathBuf,
    after: u64,
    errno: i32,
}

/// A file that accepts `left` more bytes and then fails every write.
struct FaultSink {
    inner: Box<dyn crate::fs::FileSink>,
    left: u64,
    errno: i32,
}

impl io::Write for FaultSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.left == 0 {
            return Err(io::Error::from_raw_os_error(self.errno));
        }
        let n = buf.len().min(self.left as usize);
        let n = self.inner.write(&buf[..n])?;
        self.left -= n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl io::Seek for FaultSink {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl FaultFs {
    pub fn new() -> Arc<FaultFs> {
        Arc::new(FaultFs {
            inner: LocalFs,
            rules: Mutex::new(Vec::new()),
            byte_rules: Mutex::new(Vec::new()),
            hooks: Mutex::new(Vec::new()),
            write_rules: Mutex::new(Vec::new()),
            hits: Mutex::new(Vec::new()),
        })
    }

    /// Fail `op` on paths ending with `suffix`, forever.
    pub fn fail(&self, op: Op, suffix: impl Into<PathBuf>, errno: i32) {
        self.rules.lock().unwrap().push(Rule {
            op,
            suffix: suffix.into(),
            errno,
            remaining: None,
        });
    }

    /// Fail only the next `times` matching calls (to test "retry").
    pub fn fail_times(&self, op: Op, suffix: impl Into<PathBuf>, errno: i32, times: usize) {
        self.rules.lock().unwrap().push(Rule {
            op,
            suffix: suffix.into(),
            errno,
            remaining: Some(times),
        });
    }

    /// Run `f` on the `nth` (1-based) `op` whose path ends with `suffix`, just before it.
    pub fn hook(
        &self,
        op: Op,
        suffix: impl Into<PathBuf>,
        nth: usize,
        f: impl Fn() + Send + Sync + 'static,
    ) {
        self.hooks.lock().unwrap().push(Hook {
            op,
            suffix: suffix.into(),
            nth,
            seen: 0,
            f: Arc::new(f),
        });
    }

    /// When copying a file whose path ends with `suffix` has moved `after` bytes, do `action`.
    pub fn on_bytes(&self, suffix: impl Into<PathBuf>, after: u64, action: ByteAction) {
        self.byte_rules.lock().unwrap().push(ByteRule {
            suffix: suffix.into(),
            after,
            action,
            fired: false,
        });
    }

    /// A file created in a folder whose path ends with `suffix` takes `after` bytes, then
    /// every write fails with `errno` (a full disk in the middle of a file).
    pub fn fail_writes_after(&self, suffix: impl Into<PathBuf>, after: u64, errno: i32) {
        self.write_rules.lock().unwrap().push(WriteRule {
            suffix: suffix.into(),
            after,
            errno,
        });
    }

    pub fn clear(&self) {
        self.write_rules.lock().unwrap().clear();
        self.rules.lock().unwrap().clear();
        self.byte_rules.lock().unwrap().clear();
        self.hooks.lock().unwrap().clear();
    }

    fn check(&self, op: Op, p: &Path) -> io::Result<()> {
        // Hooks first, and outside any lock: they may park the thread or touch the disk.
        let due: Vec<Arc<dyn Fn() + Send + Sync>> = {
            let mut hooks = self.hooks.lock().unwrap();
            hooks
                .iter_mut()
                .filter(|h| (h.op == op || h.op == Op::Any) && p.ends_with(&h.suffix))
                .filter_map(|h| {
                    h.seen += 1;
                    (h.seen == h.nth).then(|| h.f.clone())
                })
                .collect()
        };
        for f in due {
            f();
        }
        let mut rules = self.rules.lock().unwrap();
        for r in rules.iter_mut() {
            if (r.op == op || r.op == Op::Any) && p.ends_with(&r.suffix) {
                if let Some(n) = r.remaining.as_mut() {
                    if *n == 0 {
                        continue;
                    }
                    *n -= 1;
                }
                self.hits.lock().unwrap().push((op, p.to_path_buf()));
                return Err(io::Error::from_raw_os_error(r.errno));
            }
        }
        Ok(())
    }
}

impl FsEngine for FaultFs {
    fn lstat(&self, p: &Path) -> io::Result<FsMeta> {
        self.check(Op::Lstat, p)?;
        self.inner.lstat(p)
    }
    fn stat(&self, p: &Path) -> io::Result<FsMeta> {
        self.inner.stat(p)
    }
    fn read_dir(&self, p: &Path) -> io::Result<Vec<DirItem>> {
        self.check(Op::ReadDir, p)?;
        self.inner.read_dir(p)
    }
    fn read_link(&self, p: &Path) -> io::Result<PathBuf> {
        self.inner.read_link(p)
    }
    fn create_dir(&self, p: &Path, mode: Option<u32>) -> io::Result<()> {
        self.check(Op::CreateDir, p)?;
        self.inner.create_dir(p, mode)
    }
    fn create_symlink(&self, t: &Path, l: &Path) -> io::Result<()> {
        self.check(Op::CreateSymlink, l)?;
        self.inner.create_symlink(t, l)
    }
    fn create_file(&self, p: &Path, mode: u32) -> io::Result<Box<dyn crate::fs::FileSink>> {
        let dir = p.parent().unwrap_or(p);
        self.check(Op::CreateFile, dir)?;
        let sink = self.inner.create_file(p, mode)?;
        // A rule that fails the writes partway: the disk filling up in the middle of a file.
        let rule = self
            .write_rules
            .lock()
            .unwrap()
            .iter()
            .find(|r| dir.ends_with(&r.suffix))
            .map(|r| (r.after, r.errno));
        match rule {
            Some((after, errno)) => Ok(Box::new(FaultSink {
                inner: sink,
                left: after,
                errno,
            })),
            None => Ok(sink),
        }
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        self.check(Op::Rename, a)?;
        self.inner.rename(a, b)
    }
    fn rename_noreplace(&self, a: &Path, b: &Path) -> io::Result<()> {
        self.check(Op::Rename, a)?;
        self.inner.rename_noreplace(a, b)
    }
    fn remove_file(&self, p: &Path) -> io::Result<()> {
        self.check(Op::RemoveFile, p)?;
        self.inner.remove_file(p)
    }
    fn remove_dir(&self, p: &Path) -> io::Result<()> {
        self.check(Op::RemoveDir, p)?;
        self.inner.remove_dir(p)
    }
    fn set_mode(&self, p: &Path, m: u32) -> io::Result<()> {
        self.inner.set_mode(p, m)
    }
    fn hard_link(&self, existing: &Path, link: &Path) -> io::Result<()> {
        self.check(Op::HardLink, link)?;
        self.inner.hard_link(existing, link)
    }
    fn set_mtime(&self, p: &Path, m: SystemTime, a: Option<SystemTime>) -> io::Result<()> {
        self.inner.set_mtime(p, m, a)
    }
    fn copy_file(&self, req: &CopyRequest, ctl: &mut dyn CopyControl) -> io::Result<CopyOutcome> {
        self.check(Op::CopyFile, &req.src)?;
        // A rule that fires partway through the data.
        let has_rule = self
            .byte_rules
            .lock()
            .unwrap()
            .iter()
            .any(|r| !r.fired && req.src.ends_with(&r.suffix));
        if !has_rule {
            return self.inner.copy_file(req, ctl);
        }
        struct Tap<'a> {
            inner: &'a mut dyn CopyControl,
            fs: &'a FaultFs,
            src: &'a Path,
            seen: u64,
            failed: Option<i32>,
        }
        impl CopyControl for Tap<'_> {
            fn advance(&mut self, n: u64) -> bool {
                self.seen += n;
                let mut todo: Option<Arc<dyn Fn() + Send + Sync>> = None;
                {
                    let mut rules = self.fs.byte_rules.lock().unwrap();
                    for r in rules.iter_mut() {
                        if !r.fired && self.src.ends_with(&r.suffix) && self.seen >= r.after {
                            r.fired = true;
                            match &r.action {
                                ByteAction::Fail(e) => self.failed = Some(*e),
                                ByteAction::Run(f) => todo = Some(f.clone()),
                            }
                        }
                    }
                }
                if let Some(f) = todo {
                    f();
                }
                if self.failed.is_some() {
                    return false;
                }
                self.inner.advance(n)
            }
        }
        let mut tap = Tap {
            inner: ctl,
            fs: self,
            src: &req.src,
            seen: 0,
            failed: None,
        };
        let r = self.inner.copy_file(req, &mut tap);
        match (tap.failed, r) {
            // The copy was stopped by the rule: the destination is already cleaned up.
            (Some(errno), _) => Err(io::Error::from_raw_os_error(errno)),
            (None, r) => r,
        }
    }
    fn available_space(&self, p: &Path) -> io::Result<u64> {
        self.inner.available_space(p)
    }
    fn can_read(&self, p: &Path) -> bool {
        self.inner.can_read(p)
    }
    fn can_write(&self, p: &Path) -> bool {
        self.inner.can_write(p)
    }
}

/// `OsString` from raw bytes (non-UTF-8 name fixtures).
pub fn os_from_bytes(b: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b.to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(b).into_owned())
    }
}

pub fn os(s: &str) -> &OsStr {
    OsStr::new(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tripwire_really_trips() {
        // Uses a fake "real" directory inside a sandbox, never a real user path.
        let sb = Sandbox::new();
        let fake_real = sb.mkdir("fake-real-home/Trash");
        let guard = RealPathGuard::new(vec![fake_real.clone()], vec![]);
        assert!(guard.check().is_ok());
        std::fs::write(fake_real.join("stray.txt"), b"x").unwrap();
        let err = guard.check().unwrap_err();
        assert!(err.contains("stray.txt"), "{err}");
    }

    #[test]
    fn the_process_is_repointed_at_the_scratch_area() {
        let sb = Sandbox::new();
        let _ = &sb;
        let home = std::env::var_os("HOME").unwrap();
        assert!(Path::new(&home).starts_with(disk_base()));
        let st = std::env::var_os("XDG_STATE_HOME").unwrap();
        assert!(Path::new(&st).starts_with(disk_base()));
    }

    /// A pid no process can have (above the kernel's maximum).
    #[cfg(target_os = "linux")]
    const DEAD_PID: u32 = 4_000_000_000;

    #[cfg(unix)]
    #[test]
    fn leftovers_without_permissions_can_be_removed() {
        use std::os::unix::fs::PermissionsExt;
        let sb = Sandbox::new();
        let locked = sb.mkdir("left/over/locked");
        std::fs::write(locked.join("f"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        force_remove_dir_all(&sb.path("left"));
        assert!(!sb.path("left").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn scratch_areas_of_interrupted_runs_are_reaped_at_startup() {
        use std::os::unix::fs::PermissionsExt;
        let sb = Sandbox::new();
        let base = sb.mkdir("base");
        let own = std::process::id();

        let stale = base.join(format!("proc-{DEAD_PID}"));
        let locked = stale.join("sb-x/work/dest/top/locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("f"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let mine = base.join(format!("proc-{own}"));
        let foreign = base.join("keep-me");
        let odd = base.join("proc-not-a-pid");
        for d in [&mine, &foreign, &odd] {
            std::fs::create_dir_all(d).unwrap();
        }

        reap_stale_scratch(&base, own);

        assert!(!stale.exists(), "stale area survived");
        assert!(mine.exists(), "own area was removed");
        assert!(
            foreign.exists() && odd.exists(),
            "unrelated entries removed"
        );
    }

    #[test]
    fn sandbox_drop_checks_real_paths() {
        let sb = Sandbox::new();
        drop(sb); // would panic if a real path had been modified
    }
}
