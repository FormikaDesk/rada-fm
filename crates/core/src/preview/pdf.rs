//! PDF previews with poppler's command-line tools: the first page as a picture
//! (`pdftoppm`), its text as a fallback (`pdftotext`), the page count, title and author
//! (`pdfinfo`).
//!
//! Everything runs in the image worker, never on the interface thread, every tool call has a
//! timeout (and is stopped at once if the user has moved on to another file), and every
//! rendered page is kept in the cache folder so that coming back to a PDF is instant. The
//! cache is pruned by age and size.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

use xxhash_rust::xxh3::xxh3_64;

use super::image::{ImageInfo, ImageLimits};

/// Title, author and number of pages, when the PDF says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocFacts {
    pub pages: Option<u32>,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// Why a PDF could not be shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PdfProblem {
    /// Needs a password to be opened.
    Password,
    /// Not a readable PDF (damaged, truncated); the reader's own words.
    Damaged(String),
    /// A tool took longer than allowed.
    TooSlow(Duration),
    /// The user moved on to another file while it was being worked on.
    Abandoned,
    /// The tool is not installed (or cannot be run).
    NoTool(&'static str),
}

impl PdfProblem {
    pub fn message(&self) -> String {
        match self {
            PdfProblem::Password => "this PDF is protected by a password".into(),
            PdfProblem::Damaged(why) => format!("cannot read this PDF: {why}"),
            PdfProblem::TooSlow(t) => format!(
                "the first page took more than {} s to prepare",
                t.as_secs().max(1)
            ),
            PdfProblem::Abandoned => "stopped".into(),
            PdfProblem::NoTool(t) => format!("{t} is not installed"),
        }
    }
}

/// What to say when poppler is missing.
pub const INSTALL_HINT: &str = "to see PDFs as pictures, install poppler: `sudo pacman -S poppler`, `sudo apt install poppler-utils` or `brew install poppler`";

// ------------------------------------------------------------------------------ tools

#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub pdftoppm: Option<PathBuf>,
    pub pdftotext: Option<PathBuf>,
    pub pdfinfo: Option<PathBuf>,
}

impl Tools {
    /// Look in `path` (a `PATH`-style list).
    pub fn find_in(path: &OsStr) -> Tools {
        let find = |name: &str| {
            std::env::split_paths(path)
                .map(|d| d.join(name))
                .find(|p| p.is_file() && is_executable(p))
        };
        Tools {
            pdftoppm: find("pdftoppm"),
            pdftotext: find("pdftotext"),
            pdfinfo: find("pdfinfo"),
        }
    }

    /// The tools on the process's own `PATH`, looked for once.
    pub fn installed() -> &'static Tools {
        static T: OnceLock<Tools> = OnceLock::new();
        T.get_or_init(|| Tools::find_in(&std::env::var_os("PATH").unwrap_or_default()))
    }

    pub fn any(&self) -> bool {
        self.pdftoppm.is_some() || self.pdftotext.is_some()
    }
}

/// The tools the limits point at: the ones in `pdf_tools_path` when set (tests, odd setups),
/// else the installed ones.
pub(super) fn tools_for(limits: &ImageLimits) -> Tools {
    match &limits.pdf_tools_path {
        Some(p) => Tools::find_in(p),
        None => Tools::installed().clone(),
    }
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

// ------------------------------------------------------------------------------ running a tool

struct Run {
    status_ok: bool,
    stdout: Vec<u8>,
    stderr: String,
}

/// Run `cmd` with a time limit, keeping at most `max_out` bytes of its output, and stop it
/// at once when `abandon()` says so.
fn run_limited(
    mut cmd: Command,
    timeout: Duration,
    max_out: usize,
    abandon: &dyn Fn() -> bool,
) -> Result<Run, PdfProblem> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("LC_ALL", "C");
    let mut child = cmd
        .spawn()
        .map_err(|e| PdfProblem::Damaged(format!("cannot run the tool: {e}")))?;
    // Read both pipes on threads so that a chatty tool can never block on a full pipe.
    let drain = |mut r: Box<dyn Read + Send>, cap: usize| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buf = [0u8; 8192];
            while let Ok(n) = r.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if kept.len() < cap {
                    let room = cap - kept.len();
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
            }
            kept
        })
    };
    let out_h = child.stdout.take().map(|s| drain(Box::new(s), max_out));
    let err_h = child.stderr.take().map(|s| drain(Box::new(s), 16 * 1024));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => {}
            Err(e) => break Err(PdfProblem::Damaged(e.to_string())),
        }
        if abandon() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PdfProblem::Abandoned);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PdfProblem::TooSlow(timeout));
        }
        std::thread::sleep(Duration::from_millis(8));
    }?;
    Ok(Run {
        status_ok: status.success(),
        stdout: out_h.and_then(|h| h.join().ok()).unwrap_or_default(),
        stderr: err_h
            .and_then(|h| h.join().ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default(),
    })
}

/// A failed run, in the reader's own words, as a problem.
fn problem_of(stderr: &str) -> PdfProblem {
    let lower = stderr.to_lowercase();
    if lower.contains("incorrect password") || lower.contains("password") {
        return PdfProblem::Password;
    }
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("the file is damaged");
    PdfProblem::Damaged(
        line.trim_start_matches("Syntax Error: ")
            .trim_start_matches("Command Line Error: ")
            .trim_start_matches("Syntax Warning: ")
            .to_string(),
    )
}

// ------------------------------------------------------------------------------ facts and text

/// Page count, title and author, from `pdfinfo`.
pub fn facts(
    tools: &Tools,
    path: &Path,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<DocFacts, PdfProblem> {
    let exe = tools
        .pdfinfo
        .as_ref()
        .ok_or(PdfProblem::NoTool("pdfinfo"))?;
    let mut cmd = Command::new(exe);
    cmd.arg(path);
    let run = run_limited(cmd, limits.pdf_timeout, 64 * 1024, abandon)?;
    if !run.status_ok {
        return Err(problem_of(&run.stderr));
    }
    Ok(parse_info(&String::from_utf8_lossy(&run.stdout)))
}

pub(super) fn parse_info(text: &str) -> DocFacts {
    let mut f = DocFacts::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        match k.trim() {
            "Title" => f.title = Some(v.to_string()),
            "Author" => f.author = Some(v.to_string()),
            "Pages" => f.pages = v.parse().ok(),
            _ => {}
        }
    }
    f
}

/// The text of the first page, from `pdftotext`, at most `max_bytes` of it.
pub fn first_page_text(
    tools: &Tools,
    path: &Path,
    limits: &ImageLimits,
    max_bytes: usize,
    abandon: &dyn Fn() -> bool,
) -> Result<String, PdfProblem> {
    let exe = tools
        .pdftotext
        .as_ref()
        .ok_or(PdfProblem::NoTool("pdftotext"))?;
    let mut cmd = Command::new(exe);
    cmd.args(["-f", "1", "-l", "1", "-layout", "-nopgbrk", "-enc", "UTF-8"])
        .arg(path)
        .arg("-");
    let run = run_limited(cmd, limits.pdf_timeout, max_bytes, abandon)?;
    if !run.status_ok {
        return Err(problem_of(&run.stderr));
    }
    Ok(String::from_utf8_lossy(&run.stdout).into_owned())
}

// ------------------------------------------------------------------------------ the picture

/// Where the picture of `path`'s first page is (or will be) in the cache: it changes when the
/// file, its size or its time change.
pub fn cache_file(dir: &Path, path: &Path, info: &ImageInfo, edge: u32) -> PathBuf {
    let mut key: Vec<u8> = Vec::new();
    key.extend_from_slice(path.as_os_str().as_encoded_bytes());
    key.extend_from_slice(&info.size.to_le_bytes());
    if let Some(t) = info.modified
        && let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH)
    {
        key.extend_from_slice(&d.as_secs().to_le_bytes());
        key.extend_from_slice(&d.subsec_nanos().to_le_bytes());
    }
    key.extend_from_slice(&edge.to_le_bytes());
    key.extend_from_slice(b"pdf-v1");
    dir.join(format!("pdf-{:016x}.png", xxh3_64(&key)))
}

/// The first page as a PNG file: from the cache if it is there, else rendered by `pdftoppm`.
pub fn render_first_page(
    tools: &Tools,
    path: &Path,
    info: &ImageInfo,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> Result<PathBuf, PdfProblem> {
    let exe = tools
        .pdftoppm
        .as_ref()
        .ok_or(PdfProblem::NoTool("pdftoppm"))?;
    let cache = limits
        .pdf_cache
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("rada-previews"));
    let png = cache_file(&cache, path, info, limits.max_edge);
    if png.is_file() {
        // Used just now: keeps it out of the way of the pruning.
        if let Ok(f) = std::fs::File::options().append(true).open(&png) {
            let _ = f.set_modified(SystemTime::now());
        }
        return Ok(png);
    }
    std::fs::create_dir_all(&cache)
        .map_err(|e| PdfProblem::Damaged(format!("cannot use the preview cache: {e}")))?;
    // Rendered under a temporary name and renamed: a half-written picture is never cached.
    let stem = png.with_extension("part");
    let mut cmd = Command::new(exe);
    cmd.args(["-png", "-f", "1", "-l", "1", "-singlefile", "-scale-to"])
        .arg(limits.max_edge.to_string())
        .arg(path)
        .arg(&stem);
    let run = run_limited(cmd, limits.pdf_timeout, 16 * 1024, abandon);
    let made = stem.with_extension("part.png");
    let finish = || -> Result<PathBuf, PdfProblem> {
        let run = run?;
        if !run.status_ok {
            return Err(problem_of(&run.stderr));
        }
        std::fs::rename(&made, &png)
            .map_err(|e| PdfProblem::Damaged(format!("cannot store the preview: {e}")))?;
        Ok(png.clone())
    };
    let out = finish();
    if out.is_err() {
        let _ = std::fs::remove_file(&made);
    }
    if out.is_ok() {
        maybe_prune(&cache, limits);
    }
    out
}

// ------------------------------------------------------------------------------ the cache

static RENDERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Prune on the first render of a run and then every ten renders.
fn maybe_prune(dir: &Path, limits: &ImageLimits) {
    let n = RENDERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n.is_multiple_of(10) {
        prune_cache(dir, limits.pdf_cache_max_bytes, limits.pdf_cache_max_age);
    }
}

/// Delete cached pictures older than `max_age`, then the oldest ones until the folder is
/// under `max_bytes`. Only files that are ours (`pdf-*.png`, `*.part*`) are ever touched.
pub fn prune_cache(dir: &Path, max_bytes: u64, max_age: Duration) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut files: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    let mut removed = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let ours = name.starts_with("pdf-") && name.ends_with(".png");
        let leftover = name.starts_with("pdf-") && name.contains(".part");
        let ours = ours || leftover;
        if !ours {
            continue;
        }
        let Ok(m) = e.metadata() else { continue };
        if !m.is_file() {
            continue;
        }
        let t = m.modified().unwrap_or(now);
        if now.duration_since(t).unwrap_or_default() > max_age
            || leftover && now.duration_since(t).unwrap_or_default() > Duration::from_secs(600)
        {
            if std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
            continue;
        }
        files.push((e.path(), t, m.len()));
    }
    let mut total: u64 = files.iter().map(|f| f.2).sum();
    files.sort_by_key(|f| f.1);
    for (p, _, len) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total = total.saturating_sub(len);
            removed += 1;
        }
    }
    removed
}

#[doc(hidden)]
pub fn parse_info_for_tests(text: &str) -> DocFacts {
    parse_info(text)
}
