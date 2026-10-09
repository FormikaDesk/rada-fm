//! RAR through a program that is already installed.
//!
//! RAR's licence does not allow reimplementing the decompressor, so rada never links one:
//! it asks `7z` / `7zz` (7-Zip) or `unrar`, and says so plainly when none is there. The
//! listing is parsed from the program's text output; the data comes from its standard output,
//! all files one after the other in archive order, cut up with the sizes the listing gave.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;

use super::entry::{Entry, EntryKind, sanitize};
use super::index::{Index, ListControl, unix_time};
use super::reader::Emitter;
use super::ArchiveError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    SevenZip(PathBuf),
    Unrar(PathBuf),
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

fn which_in(path_var: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        for candidate in [name.to_string(), format!("{name}.exe")] {
            let p = dir.join(&candidate);
            if is_executable(&p) {
                return Some(p);
            }
        }
    }
    None
}

impl Tool {
    /// The first usable program in `path_var`. `7zz` and `7z` read RAR (`7za` does not).
    pub fn find_in(path_var: &std::ffi::OsStr) -> Option<Tool> {
        for n in ["7zz", "7z"] {
            if let Some(p) = which_in(path_var, n) {
                return Some(Tool::SevenZip(p));
            }
        }
        which_in(path_var, "unrar").map(Tool::Unrar)
    }

    pub fn find() -> Option<Tool> {
        Tool::find_in(&std::env::var_os("PATH")?)
    }
}

pub const MISSING: &str = "RAR archives can only be opened through another program, and none is installed. Install 7-Zip (7z or 7zz) or unrar; on Arch: pacman -S 7zip";

fn tool_or_error() -> Result<Tool, ArchiveError> {
    Tool::find().ok_or_else(|| ArchiveError::ToolMissing(MISSING.to_string()))
}

fn run_text(mut cmd: Command) -> Result<String, ArchiveError> {
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .map_err(|e| ArchiveError::ToolMissing(format!("cannot run the program: {e}")))?;
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        return Err(classify_failure(&stderr, &stdout));
    }
    Ok(stdout)
}

fn classify_failure(stderr: &str, stdout: &str) -> ArchiveError {
    let all = format!("{stderr}\n{stdout}").to_lowercase();
    if all.contains("password") || all.contains("encrypted") && all.contains("wrong") {
        ArchiveError::Encrypted
    } else {
        let first = stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .find(|l| !l.is_empty() && l.to_lowercase().contains("error"))
            .or_else(|| stderr.lines().map(str::trim).find(|l| !l.is_empty()))
            .unwrap_or("the program reported an error");
        ArchiveError::Damaged(first.to_string())
    }
}

/// List a RAR archive.
pub fn list_rar(ix: &mut Index, ctl: &mut ListControl<'_>) -> Result<(), ArchiveError> {
    let tool = tool_or_error()?;
    list_with(&tool, ix, ctl, true)
}

/// List with a given program. `backslash` is whether `\` separates names in its output.
pub fn list_with(
    tool: &Tool,
    ix: &mut Index,
    ctl: &mut ListControl<'_>,
    backslash: bool,
) -> Result<(), ArchiveError> {
    let text = match tool {
        Tool::SevenZip(p) => {
            let mut c = Command::new(p);
            c.args(["l", "-slt", "-ba", "-sccUTF-8", "-p-", "--"]).arg(&ix.path);
            run_text(c)?
        }
        Tool::Unrar(p) => {
            let mut c = Command::new(p);
            c.args(["lt", "-scu", "-p-", "-idq", "--"]).arg(&ix.path);
            run_text(c)?
        }
    };
    let blocks = match tool {
        Tool::SevenZip(_) => parse_7z_listing(&text),
        Tool::Unrar(_) => parse_unrar_listing(&text),
    };
    for (n, b) in blocks.into_iter().enumerate() {
        if n % 512 == 0 && !ctl.tick(n as u64, 0)? {
            ix.stop_with(format!("stopped after {n} members"));
            break;
        }
        let (path, issue) = sanitize(b.path.as_bytes(), backslash);
        let kind = b.kind;
        ix.push(Entry {
            index: 0,
            path,
            kind,
            size: if kind == EntryKind::File { b.size } else { 0 },
            compressed: b.packed,
            mode: b.mode,
            mtime: b.mtime,
            link: None,
            encrypted: b.encrypted,
            issue,
        });
    }
    Ok(())
}

#[derive(Default, Debug)]
struct Block {
    path: String,
    kind: EntryKind,
    size: u64,
    packed: Option<u64>,
    mode: Option<u32>,
    mtime: Option<SystemTime>,
    encrypted: bool,
}

fn unix_mode_text(s: &str) -> Option<u32> {
    let b: Vec<char> = s.chars().collect();
    if b.len() < 10 {
        return None;
    }
    let mut mode = 0u32;
    for (i, c) in b[1..10].iter().enumerate() {
        let bit = match (i % 3, c) {
            (0, 'r') => 4,
            (1, 'w') => 2,
            (2, 'x' | 's' | 't') => 1,
            _ => 0,
        };
        mode |= bit << (6 - 3 * (i / 3));
    }
    Some(mode)
}

fn parse_time(s: &str) -> Option<SystemTime> {
    // "2026-10-09 17:43:59[.1234567]" in local time.
    let s = s.trim();
    let (date, time) = s.split_once(' ')?;
    let time = time.split(['.', ',']).next()?;
    let d: Vec<i64> = date.split('-').filter_map(|x| x.parse().ok()).collect();
    let t: Vec<i64> = time.split(':').filter_map(|x| x.parse().ok()).collect();
    if d.len() != 3 || t.len() != 3 {
        return None;
    }
    let civil = jiff::civil::DateTime::new(
        d[0] as i16,
        d[1] as i8,
        d[2] as i8,
        t[0] as i8,
        t[1] as i8,
        t[2] as i8,
        0,
    )
    .ok()?;
    let z = civil.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    let ts = z.timestamp();
    unix_time(ts.as_second(), 0)
}

fn parse_7z_listing(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur: Option<Block> = None;
    for line in text.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if let Some(b) = cur.take()
                && !b.path.is_empty()
            {
                out.push(b);
            }
            continue;
        }
        let Some((k, v)) = line.split_once(" = ") else {
            continue;
        };
        let b = cur.get_or_insert_with(Block::default);
        match k.trim() {
            "Path" => b.path = v.to_string(),
            "Size" => b.size = v.trim().parse().unwrap_or(0),
            "Packed Size" => b.packed = v.trim().parse().ok(),
            "Modified" => b.mtime = parse_time(v),
            "Encrypted" => b.encrypted = v.trim() == "+",
            "Folder" if v.trim() == "+" => b.kind = EntryKind::Dir,
            "Attributes" => {
                // "A -rw-r--r--", "D drwxr-xr-x", or a plain "..A...." Windows string.
                let mut words = v.split_whitespace();
                let first = words.next().unwrap_or("");
                let unix = words.next().unwrap_or(first);
                if first.starts_with('D') || unix.starts_with('d') {
                    b.kind = EntryKind::Dir;
                } else if unix.starts_with('l') {
                    b.kind = EntryKind::Symlink;
                }
                b.mode = unix_mode_text(unix);
            }
            _ => {}
        }
    }
    out
}

fn parse_unrar_listing(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur: Option<Block> = None;
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once(": ") else {
            if line.is_empty()
                && let Some(b) = cur.take()
                && !b.path.is_empty()
            {
                out.push(b);
            }
            continue;
        };
        if k == "Name" {
            if let Some(b) = cur.take()
                && !b.path.is_empty()
            {
                out.push(b);
            }
            cur = Some(Block {
                path: v.to_string(),
                ..Block::default()
            });
            continue;
        }
        let Some(b) = cur.as_mut() else { continue };
        match k {
            "Type" => {
                let t = v.to_lowercase();
                b.kind = if t.contains("director") {
                    EntryKind::Dir
                } else if t.contains("link") {
                    EntryKind::Symlink
                } else {
                    EntryKind::File
                };
            }
            "Size" => b.size = v.trim().parse().unwrap_or(0),
            "Packed size" => b.packed = v.trim().parse().ok(),
            "mtime" => b.mtime = parse_time(v),
            "Attributes" => b.mode = unix_mode_text(v.trim()),
            "Flags" => b.encrypted = v.to_lowercase().contains("encrypted"),
            _ => {}
        }
    }
    if let Some(b) = cur.take()
        && !b.path.is_empty()
    {
        out.push(b);
    }
    out
}

/// Stream the data of a RAR archive's members.
pub(super) fn produce_rar(
    ix: &Index,
    wanted: &[bool],
    last: usize,
    em: &Emitter,
) -> Result<(), ArchiveError> {
    let tool = tool_or_error()?;
    produce_with(&tool, ix, wanted, last, em)
}

pub(super) fn produce_with(
    tool: &Tool,
    ix: &Index,
    wanted: &[bool],
    last: usize,
    em: &Emitter,
) -> Result<(), ArchiveError> {
    let mut cmd = match tool {
        Tool::SevenZip(p) => {
            let mut c = Command::new(p);
            c.args(["x", "-so", "-bso0", "-bsp0", "-y", "-p-", "--"]).arg(&ix.path);
            c
        }
        Tool::Unrar(p) => {
            let mut c = Command::new(p);
            c.args(["p", "-inul", "-p-", "--"]).arg(&ix.path);
            c
        }
    };
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ArchiveError::ToolMissing(format!("cannot run the program: {e}")))?;
    let mut out = child.stdout.take().expect("stdout is piped");
    let result = deliver(ix, wanted, last, em, &mut out);
    finish_child(&mut child, result)
}

fn finish_child(child: &mut Child, result: Result<(), ArchiveError>) -> Result<(), ArchiveError> {
    // Everything wanted has been read (or it failed): the program has nothing more to say.
    let _ = child.kill();
    let status_text = {
        let mut s = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.by_ref().take(4096).read_to_string(&mut s);
        }
        s
    };
    let _ = child.wait();
    match result {
        Err(ArchiveError::Damaged(w)) if !status_text.trim().is_empty() => {
            if status_text.to_lowercase().contains("password") {
                Err(ArchiveError::Encrypted)
            } else {
                Err(ArchiveError::Damaged(format!(
                    "{w}; {}",
                    status_text.trim().lines().next().unwrap_or("")
                )))
            }
        }
        r => r,
    }
}

fn deliver(
    ix: &Index,
    wanted: &[bool],
    last: usize,
    em: &Emitter,
    out: &mut dyn Read,
) -> Result<(), ArchiveError> {
    for (i, e) in ix.entries.iter().enumerate() {
        if i > last {
            break;
        }
        if e.kind != EntryKind::File {
            continue;
        }
        let mut part = out.take(e.size);
        let want = wanted.get(i).copied().unwrap_or(false);
        if want {
            em.start(i)?;
        }
        let copied = if want {
            let mut counted = Counting { inner: &mut part, n: 0 };
            em.copy(&mut counted)?;
            counted.n
        } else {
            io::copy(&mut part, &mut io::sink())?
        };
        if copied < e.size {
            return Err(ArchiveError::Damaged(
                "the program stopped before the end of the data".into(),
            ));
        }
        if want {
            em.end()?;
        }
    }
    Ok(())
}

struct Counting<'a> {
    inner: &'a mut dyn Read,
    n: u64,
}

impl Read for Counting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.n += n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_7z_listing_is_parsed() {
        let text = "Path = d\nSize = 0\nPacked Size = 0\nModified = 2026-10-09 17:43:59.7153087\nAttributes = D drwxr-xr-x\nEncrypted = -\n\nPath = d/b.txt\nSize = 3000\nPacked Size = 12\nModified = 2026-10-09 17:43:59.7153087\nAttributes = A -rwxr-x---\nEncrypted = +\n";
        let b = parse_7z_listing(text);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].kind, EntryKind::Dir);
        assert_eq!(b[1].size, 3000);
        assert_eq!(b[1].packed, Some(12));
        assert_eq!(b[1].mode, Some(0o750));
        assert!(b[1].encrypted);
        assert!(b[1].mtime.is_some());
    }

    #[test]
    fn the_unrar_listing_is_parsed() {
        let text = "UNRAR 7.00 freeware\n\nArchive: x.rar\nDetails: RAR 5\n\n        Name: a.txt\n        Type: File\n        Size: 5\n Packed size: 7\n       mtime: 2023-01-01 12:00:00,000000000\n  Attributes: -rw-r--r--\n\n        Name: dir\n        Type: Directory\n        Size: 0\n";
        let b = parse_unrar_listing(text);
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].path.as_str(), b[0].size, b[0].mode), ("a.txt", 5, Some(0o644)));
        assert_eq!(b[1].kind, EntryKind::Dir);
    }

    #[test]
    fn a_missing_tool_is_found_or_not_in_the_given_path() {
        assert_eq!(Tool::find_in(std::ffi::OsStr::new("/nonexistent-dir-for-rada")), None);
    }

    #[test]
    fn password_failures_are_recognised() {
        assert!(matches!(
            classify_failure("ERROR: Wrong password : x.rar", ""),
            ArchiveError::Encrypted
        ));
        assert!(matches!(
            classify_failure("ERROR: Unexpected end of archive", ""),
            ArchiveError::Damaged(_)
        ));
    }
}
