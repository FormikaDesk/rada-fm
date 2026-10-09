//! Read-only previews, safe for anything: huge files, one-line megabyte JSON, binary
//! data, UTF-16, Latin-1, FIFOs, devices. Only a bounded prefix is ever read, and
//! only from regular files, so a preview can neither hang nor exhaust memory.

pub mod archive;
pub mod card;
pub mod image;
pub mod pdf;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub use archive::ArchivePreview;
pub use card::{ExecInfo, FileCard, mode_string};
pub use image::{ImageInfo, ImageLimits, ImagePreview, ImageState, ImageWorker};

use crate::display;
use crate::fs::{FileKind, FsEngine, FsMeta, SpecialKind};
use crate::ops::LinkState;

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_lines: usize,
    pub max_line_chars: usize,
    pub hex_bytes: usize,
    pub dir_entries: usize,
    /// How many things at the top of an archive its preview lists.
    pub archive_entries: usize,
    /// How long counting an archive's members may take before the preview gives a lower bound.
    pub archive_seconds: f32,
    pub image: ImageLimits,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: 256 * 1024,
            max_lines: 1000,
            max_line_chars: 2000,
            hex_bytes: 512,
            dir_entries: 300,
            archive_entries: 200,
            archive_seconds: 1.5,
            image: ImageLimits::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Preview {
    Empty,
    Text(TextPreview),
    Binary(BinaryPreview),
    Image(ImagePreview),
    Dir(DirPreview),
    /// An archive that is not opened: what it holds.
    Archive(ArchivePreview),
    Symlink {
        target: PathBuf,
        state: LinkState,
        inner: Option<Box<Preview>>,
    },
    Special(String),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct TextPreview {
    pub lines: Vec<String>,
    pub encoding: &'static str,
    /// Only the beginning of the file is shown.
    pub truncated: bool,
    /// How many lines were cut at the width limit.
    pub long_lines: usize,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct BinaryPreview {
    pub size: u64,
    pub kind: &'static str,
    /// The clean summary shown by default.
    pub card: FileCard,
    /// First bytes as a hex dump; shown on request.
    pub hex: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DirPreview {
    /// (escaped name, is_dir)
    pub entries: Vec<(String, bool)>,
    pub truncated: bool,
}

pub fn generate(fs: &dyn FsEngine, path: &Path, limits: &Limits) -> Preview {
    generate_with(fs, path, limits, &|| false)
}

/// [`generate`], giving up (with an empty preview) as soon as `cancel` says the answer is no
/// longer wanted: reading inside an archive can take a while.
pub fn generate_with(
    fs: &dyn FsEngine,
    path: &Path,
    limits: &Limits,
    cancel: &dyn Fn() -> bool,
) -> Preview {
    if let Some(loc) = crate::archive::locate(fs, path)
        && !loc.inner.as_os_str().is_empty()
    {
        return archive::member(&loc, limits, cancel);
    }
    let meta = match fs.lstat(path) {
        Ok(m) => m,
        Err(e) => return Preview::Error(crate::Error::io("read", path, e).to_string()),
    };
    match meta.kind {
        FileKind::Dir => dir_preview(path, limits),
        FileKind::Symlink => {
            let target = fs.read_link(path).unwrap_or_default();
            let (state, inner) = match fs.stat(path) {
                Ok(m) if m.is_dir() => {
                    (LinkState::ToDir, Some(Box::new(dir_preview(path, limits))))
                }
                Ok(m) if m.is_file() => (
                    LinkState::ToFile,
                    Some(Box::new(file_preview(path, &m, limits, cancel))),
                ),
                Ok(_) => (LinkState::ToFile, None),
                #[cfg(unix)]
                Err(e) if e.raw_os_error() == Some(libc::ELOOP) => (LinkState::Circular, None),
                Err(_) => (LinkState::Broken, None),
            };
            Preview::Symlink {
                target,
                state,
                inner,
            }
        }
        FileKind::File => file_preview(path, &meta, limits, cancel),
        FileKind::Other => Preview::Special(
            match meta.special {
                Some(SpecialKind::Fifo) => "named pipe (not read)",
                Some(SpecialKind::Socket) => "socket",
                Some(SpecialKind::BlockDevice) => "block device (not read)",
                Some(SpecialKind::CharDevice) => "character device (not read)",
                _ => "special file (not read)",
            }
            .to_string(),
        ),
    }
}

fn dir_preview(path: &Path, limits: &Limits) -> Preview {
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => return Preview::Error(crate::Error::io("read folder", path, e).to_string()),
    };
    let mut entries = Vec::new();
    let mut truncated = false;
    for e in rd.flatten() {
        if entries.len() >= limits.dir_entries {
            truncated = true;
            break;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        entries.push((display::name(&e.file_name()), is_dir));
    }
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    Preview::Dir(DirPreview { entries, truncated })
}

fn open_nonblocking(path: &Path) -> std::io::Result<File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A FIFO swapped in after the lstat must not block the worker forever.
        o.custom_flags(libc::O_NONBLOCK);
    }
    o.open(path)
}

fn file_preview(path: &Path, meta: &FsMeta, limits: &Limits, cancel: &dyn Fn() -> bool) -> Preview {
    let size = meta.size;
    if size == 0 {
        return Preview::Empty;
    }
    let mut f = match open_nonblocking(path) {
        Ok(f) => f,
        Err(e) => return Preview::Error(crate::Error::io("open", path, e).to_string()),
    };
    let mut buf = Vec::with_capacity(limits.max_bytes.min(size as usize));
    if let Err(e) = (&mut f).take(limits.max_bytes as u64).read_to_end(&mut buf) {
        return Preview::Error(crate::Error::io("read", path, e).to_string());
    }
    // Images are recognised by their content, never by the extension.
    if let Some(img) = image::detect(path, &buf, size, meta.mtime, &limits.image) {
        return Preview::Image(img);
    }
    // So are archives: what they hold, without opening them.
    if let Some(fmt) = crate::archive::format::sniff_head(&buf) {
        let _ = fmt;
        return archive::summarize(path, size, limits, cancel);
    }
    let (modified, accessed, created, mode) = (meta.mtime, meta.atime, meta.btime, meta.mode);
    bytes_preview(&buf, size, limits, move |kind, exec| FileCard {
        kind,
        size,
        modified,
        accessed,
        created,
        mode,
        exec,
    })
}

/// Text, or a card for binary data, from the first bytes of something of `size` bytes.
fn bytes_preview(
    buf: &[u8],
    size: u64,
    limits: &Limits,
    card: impl FnOnce(&'static str, Option<ExecInfo>) -> FileCard,
) -> Preview {
    let truncated = (buf.len() as u64) < size || size > limits.max_bytes as u64;
    match decode(buf, truncated) {
        Some((text, encoding)) => Preview::Text(to_lines(&text, encoding, truncated, size, limits)),
        None => Preview::Binary(BinaryPreview {
            size,
            kind: sniff(buf),
            card: card(sniff(buf), card::parse_exec(buf)),
            hex: hexdump(&buf[..buf.len().min(limits.hex_bytes)]),
        }),
    }
}

fn to_lines(
    text: &str,
    encoding: &'static str,
    truncated: bool,
    size: u64,
    limits: &Limits,
) -> TextPreview {
    let mut lines = Vec::new();
    let mut long_lines = 0;
    let mut cut = truncated;
    for raw in text.split('\n') {
        if lines.len() >= limits.max_lines {
            cut = true;
            break;
        }
        let mut chars = raw.chars();
        let head: String = chars.by_ref().take(limits.max_line_chars).collect();
        let shown = if chars.next().is_some() {
            long_lines += 1;
            let extra = raw.chars().count() - limits.max_line_chars;
            format!("{}… [+{extra} more characters]", display::line(&head, 4))
        } else {
            display::line(&head, 4)
        };
        lines.push(shown);
    }
    // A trailing newline produces one empty last element.
    if lines.last().is_some_and(String::is_empty) && !cut {
        lines.pop();
    }
    TextPreview {
        lines,
        encoding,
        truncated: cut,
        long_lines,
        size,
    }
}

/// Text or not, and in which encoding. `None` means "binary".
fn decode(bytes: &[u8], truncated: bool) -> Option<(String, &'static str)> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some((String::from_utf8_lossy(rest).into_owned(), "UTF-8"));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Some((
            encoding_rs::UTF_16LE
                .decode_without_bom_handling(rest)
                .0
                .into_owned(),
            "UTF-16LE",
        ));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Some((
            encoding_rs::UTF_16BE
                .decode_without_bom_handling(rest)
                .0
                .into_owned(),
            "UTF-16BE",
        ));
    }
    if let Some(enc) = looks_utf16(bytes) {
        let (s, _) = enc.decode_without_bom_handling(bytes);
        return Some((s.into_owned(), enc.name()));
    }
    if bytes.contains(&0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => return text_if_sane(s.to_string(), "UTF-8"),
        Err(e) => {
            // A multi-byte character cut by the read limit is not an encoding problem.
            if truncated && e.error_len().is_none() {
                let s = String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned();
                return text_if_sane(s, "UTF-8");
            }
        }
    }
    let mut det = chardetng::EncodingDetector::new();
    det.feed(bytes, !truncated);
    let enc = det.guess(None, true);
    let (s, _, had_errors) = enc.decode(bytes);
    if had_errors && s.matches('\u{fffd}').count() * 50 > s.chars().count() {
        return None;
    }
    text_if_sane(s.into_owned(), enc.name())
}

/// UTF-16 without a BOM: ASCII-heavy text has a zero in every other byte.
fn looks_utf16(bytes: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    if bytes.len() < 4 {
        return None;
    }
    let n = bytes.len().min(4096) / 2 * 2;
    let (mut even0, mut odd0) = (0usize, 0usize);
    for (i, b) in bytes[..n].iter().enumerate() {
        if *b == 0 {
            if i % 2 == 0 { even0 += 1 } else { odd0 += 1 }
        }
    }
    let half = n / 2;
    if odd0 * 10 >= half * 7 && even0 * 20 <= half {
        Some(encoding_rs::UTF_16LE)
    } else if even0 * 10 >= half * 7 && odd0 * 20 <= half {
        Some(encoding_rs::UTF_16BE)
    } else {
        None
    }
}

/// Reject "text" that is mostly control characters.
fn text_if_sane(s: String, enc: &'static str) -> Option<(String, &'static str)> {
    let total = s.chars().count().max(1);
    let bad = s
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\x0c' | '\x1b'))
        .count();
    if bad * 10 > total {
        None
    } else {
        Some((s, enc))
    }
}

fn sniff(b: &[u8]) -> &'static str {
    let starts = |m: &[u8]| b.starts_with(m);
    if starts(b"\x89PNG\r\n\x1a\n") {
        "PNG image"
    } else if starts(&[0xFF, 0xD8, 0xFF]) {
        "JPEG image"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "GIF image"
    } else if starts(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        "WebP image"
    } else if starts(b"%PDF") {
        "PDF document"
    } else if starts(b"PK\x03\x04") {
        "ZIP archive"
    } else if starts(&[0x1F, 0x8B]) {
        "gzip data"
    } else if starts(b"BZh") {
        "bzip2 data"
    } else if starts(&[0xFD, b'7', b'z', b'X', b'Z', 0]) {
        "xz data"
    } else if starts(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
        "7-Zip archive"
    } else if starts(b"\x7fELF") {
        "ELF executable"
    } else if starts(b"SQLite format 3") {
        "SQLite database"
    } else {
        "binary data"
    }
}

fn hexdump(b: &[u8]) -> Vec<String> {
    b.chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let hex: String = (0..16)
                .map(|j| match chunk.get(j) {
                    Some(x) => format!("{x:02x} "),
                    None => "   ".to_string(),
                })
                .collect();
            let asc: String = chunk
                .iter()
                .map(|&c| {
                    if (0x20..0x7f).contains(&c) {
                        c as char
                    } else {
                        '.'
                    }
                })
                .collect();
            format!("{:08x}  {hex} {asc}", i * 16)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::LocalFs;
    use crate::testutil::*;

    fn pv(sb: &Sandbox, rel: &str) -> Preview {
        generate(&LocalFs, &sb.path(rel), &Limits::default())
    }

    fn text(p: Preview) -> TextPreview {
        match p {
            Preview::Text(t) => t,
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn plain_utf8_text() {
        let sb = Sandbox::new();
        sb.write("a.txt", "héllo\n\tworld 🎉\nlast");
        let t = text(pv(&sb, "a.txt"));
        assert_eq!(t.encoding, "UTF-8");
        assert_eq!(t.lines, ["héllo", "    world 🎉", "last"]);
        assert!(!t.truncated);
    }

    #[test]
    fn a_300kb_single_line_is_cut_not_an_error() {
        // Regression: a very long line made another file manager give up ("token too long").
        let sb = Sandbox::new();
        sb.write("min.json", format!("{{\"k\":\"{}\"}}", "x".repeat(300_000)));
        let t = text(pv(&sb, "min.json"));
        assert_eq!(t.lines.len(), 1);
        assert!(t.lines[0].contains("more characters"));
        assert!(t.lines[0].chars().count() < 2100);
        assert_eq!(t.long_lines, 1);
        assert!(t.truncated, "only the first 256 KiB were read");
    }

    #[test]
    fn two_hundred_thousand_lines_show_the_first_thousand() {
        let sb = Sandbox::new();
        let body: String = (0..200_000).map(|i| format!("line {i}\n")).collect();
        sb.write("big.txt", body);
        let t = text(pv(&sb, "big.txt"));
        assert_eq!(t.lines.len(), 1000);
        assert!(t.truncated);
    }

    #[test]
    fn binary_with_a_code_extension_is_not_printed_as_text() {
        // Regression: a binary file with a source-code extension printed random bytes.
        let sb = Sandbox::new();
        let mut data = vec![0x7fu8, b'E', b'L', b'F'];
        data.extend((0..4000u32).map(|i| (i * 31 % 251) as u8));
        sb.write("binary_named.go", data);
        match pv(&sb, "binary_named.go") {
            Preview::Binary(b) => {
                assert_eq!(b.kind, "ELF executable");
                assert!(b.hex[0].starts_with("00000000  7f 45 4c 46"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn utf16_with_and_without_bom_and_latin1_are_decoded() {
        // Regression: UTF-16 and Latin-1 text files gave an empty panel.
        let sb = Sandbox::new();
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("héllo wörld\n".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        sb.write("le.txt", le);
        let t = text(pv(&sb, "le.txt"));
        assert_eq!(t.encoding, "UTF-16LE");
        assert_eq!(t.lines, ["héllo wörld"]);

        let nobom: Vec<u8> = "plain ascii text in utf16\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        sb.write("nobom.txt", nobom);
        assert_eq!(
            text(pv(&sb, "nobom.txt")).lines,
            ["plain ascii text in utf16"]
        );

        // "café résumé naïve" in Latin-1.
        sb.write(
            "latin1.txt",
            b"caf\xe9 r\xe9sum\xe9 na\xefve, une \xe9t\xe9 tr\xe8s chaude\n",
        );
        let t = text(pv(&sb, "latin1.txt"));
        assert!(
            t.lines[0].contains("café") && t.lines[0].contains("résumé"),
            "{:?} ({})",
            t.lines,
            t.encoding
        );
    }

    #[test]
    fn control_sequences_in_content_are_neutralised() {
        let sb = Sandbox::new();
        sb.write(
            "evil.txt",
            "\x1b]0;pwned\x07 title\n\x1b[2J clear\nbidi \u{202e}txt\n",
        );
        let t = text(pv(&sb, "evil.txt"));
        let all = t.lines.join("\n");
        assert!(
            !all.contains('\x1b') && !all.contains('\u{202e}') && !all.contains('\x07'),
            "{all:?}"
        );
        assert!(all.contains("\\e"));
    }

    #[test]
    fn folders_empty_files_and_errors() {
        let sb = Sandbox::new();
        sb.write("d/b.txt", "1");
        sb.mkdir("d/zdir");
        sb.write("empty", "");
        match pv(&sb, "d") {
            Preview::Dir(d) => assert_eq!(
                d.entries,
                [("zdir".to_string(), true), ("b.txt".to_string(), false)]
            ),
            other => panic!("{other:?}"),
        }
        assert!(matches!(pv(&sb, "empty"), Preview::Empty));
        assert!(matches!(pv(&sb, "missing"), Preview::Error(_)));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_fifos_are_described_and_never_block() {
        let sb = Sandbox::new();
        sb.write("real.txt", "target text");
        sb.symlink("real.txt", "ln");
        sb.symlink("nowhere", "broken");
        sb.symlink("loop2", "loop1");
        sb.symlink("loop1", "loop2");
        match pv(&sb, "ln") {
            Preview::Symlink {
                state: LinkState::ToFile,
                inner: Some(i),
                ..
            } => assert!(matches!(*i, Preview::Text(_))),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            pv(&sb, "broken"),
            Preview::Symlink {
                state: LinkState::Broken,
                ..
            }
        ));
        assert!(matches!(
            pv(&sb, "loop1"),
            Preview::Symlink {
                state: LinkState::Circular,
                ..
            }
        ));
        // A FIFO: lstat says "special"; must return immediately.
        let fifo = sb.path("pipe");
        let c = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(fifo.as_os_str()))
            .unwrap();
        // SAFETY: valid C string; creating a FIFO in the sandbox.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        assert!(matches!(pv(&sb, "pipe"), Preview::Special(_)));
        assert!(started.elapsed().as_secs() < 1);
    }
}
