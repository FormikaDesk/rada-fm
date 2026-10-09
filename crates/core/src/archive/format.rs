//! Which kind of archive a file is. The content decides; the extension is only a hint that
//! is used where reading the file would be too costly (deciding what Enter does).

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a stream is compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Compression {
    None,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

/// The most memory an xz block may ask a decoder for. A hostile file can declare a
/// dictionary of gigabytes; past this it is refused instead of exhausting the machine.
const XZ_MEMORY_KIB: u32 = 1 << 20;

impl Compression {
    /// The compression named by the first bytes of a stream.
    pub fn from_magic(head: &[u8]) -> Option<Compression> {
        if head.starts_with(&[0x1F, 0x8B]) {
            Some(Compression::Gzip)
        } else if head.starts_with(b"BZh") && head.get(3).is_some_and(|b| (b'1'..=b'9').contains(b))
        {
            Some(Compression::Bzip2)
        } else if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0]) {
            Some(Compression::Xz)
        } else if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
            Some(Compression::Zstd)
        } else {
            None
        }
    }

    /// A decoder over `r`.
    pub fn reader<'a, R: Read + Send + 'a>(self, r: R) -> io::Result<Box<dyn Read + Send + 'a>> {
        Ok(match self {
            Compression::None => Box::new(r),
            Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(r)),
            Compression::Bzip2 => Box::new(bzip2::read::MultiBzDecoder::new(r)),
            Compression::Xz => Box::new(lzma_rust2::XzReader::new_mem_limit(r, true, XZ_MEMORY_KIB)),
            Compression::Zstd => Box::new(
                structured_zstd::decoding::StreamingDecoder::new(r)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?,
            ),
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Compression::None => "",
            Compression::Gzip => "gzip",
            Compression::Bzip2 => "bzip2",
            Compression::Xz => "xz",
            Compression::Zstd => "zstd",
        }
    }
}

/// What an archive file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Zip,
    /// A tar stream, plain or compressed.
    Tar(Compression),
    /// One compressed file (`notes.txt.gz`): an archive of a single member.
    Single(Compression),
    SevenZ,
    /// Read through an external program only (licence reasons).
    Rar,
}

impl Format {
    pub fn label(self) -> String {
        match self {
            Format::Zip => "ZIP archive".into(),
            Format::Tar(Compression::None) => "tar archive".into(),
            Format::Tar(c) => format!("tar archive, {}", c.label()),
            Format::Single(c) => format!("{} compressed file", c.label()),
            Format::SevenZ => "7-Zip archive".into(),
            Format::Rar => "RAR archive".into(),
        }
    }
}

/// How many bytes of a file are enough to recognise its format.
const HEAD: usize = 1024;

/// A tar header block: "ustar" magic, or a valid checksum over a block that starts with a
/// plausible name (old v7 archives have no magic).
pub fn looks_like_tar(block: &[u8]) -> bool {
    if block.len() < 512 {
        return false;
    }
    if &block[257..262] == b"ustar" {
        return true;
    }
    // Checksum: the sum of all header bytes with the checksum field read as spaces.
    let field = &block[148..156];
    let text: String = field
        .iter()
        .take_while(|b| **b != 0 && **b != b' ')
        .map(|b| *b as char)
        .collect();
    let Ok(stored) = u32::from_str_radix(text.trim(), 8) else {
        return false;
    };
    if text.trim().is_empty() || block[0] == 0 {
        return false;
    }
    let sum: u32 = block[..148]
        .iter()
        .chain(std::iter::repeat_n(&b' ', 8))
        .chain(block[156..512].iter())
        .map(|b| *b as u32)
        .sum();
    sum == stored
}

/// The format named by the first bytes of a file, looking inside a compressed stream for a
/// tar header. `None` when it is not an archive this build can read.
pub fn sniff_head(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Some(Format::Zip);
    }
    if head.starts_with(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some(Format::SevenZ);
    }
    if head.starts_with(b"Rar!\x1a\x07") {
        return Some(Format::Rar);
    }
    if looks_like_tar(head) {
        return Some(Format::Tar(Compression::None));
    }
    let c = Compression::from_magic(head)?;
    // Look inside: a compressed tar is an archive of many files, anything else is one file.
    let inner = c
        .reader(head)
        .ok()
        .map(|mut r| {
            let mut block = vec![0u8; 512];
            let mut got = 0;
            while got < 512 {
                match r.read(&mut block[got..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
            block.truncate(got);
            block
        })
        .unwrap_or_default();
    if looks_like_tar(&inner) {
        Some(Format::Tar(c))
    } else {
        Some(Format::Single(c))
    }
}

/// The format of the file at `path`, from its content. A compressed stream whose first
/// kilobyte is too short to show a tar header is read again from the file.
pub fn sniff(path: &Path) -> io::Result<Option<Format>> {
    let mut f = File::open(path)?;
    let mut head = Vec::with_capacity(HEAD);
    (&mut f).take(HEAD as u64).read_to_end(&mut head)?;
    if let Some(c) = Compression::from_magic(&head)
        && !matches!(sniff_head(&head), Some(Format::Tar(_)))
    {
        // The first kilobyte of compressed data may decode to less than a tar block.
        let mut f = File::open(path)?;
        let mut r = c.reader(&mut f)?;
        let mut block = Vec::with_capacity(512);
        (&mut r).take(512).read_to_end(&mut block).ok();
        return Ok(Some(if looks_like_tar(&block) {
            Format::Tar(c)
        } else {
            Format::Single(c)
        }));
    }
    Ok(sniff_head(&head))
}

/// Extensions that name an archive, longest first. Used for the name only.
const EXTENSIONS: &[&str] = &[
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.zstd", ".tgz", ".tbz2", ".tbz", ".txz",
    ".tzst", ".tar", ".zip", ".7z", ".rar", ".gz", ".bz2", ".xz", ".zst", ".zstd",
];

/// Container formats that happen to be ZIP files but are documents or packages: Enter opens
/// them with their program, not as a folder.
const ZIP_BASED: &[&str] = &[
    "docx", "xlsx", "pptx", "odt", "ods", "odp", "epub", "jar", "war", "apk", "ipa", "xpi", "crx",
    "whl", "nupkg", "vsix", "kra", "ora", "cbz", "3mf", "appx",
];

fn lower_name(name: &OsStr) -> String {
    name.to_string_lossy().to_lowercase()
}

/// Whether the *name* says "archive": what Enter decides on, since reading every file
/// to know would put I/O on the keypress.
pub fn name_suggests_archive(name: &OsStr) -> bool {
    let n = lower_name(name);
    EXTENSIONS.iter().any(|e| n.len() > e.len() && n.ends_with(e))
}

/// Whether the name is one of the ZIP-based document or package formats.
pub fn name_is_zip_document(name: &OsStr) -> bool {
    let n = lower_name(name);
    n.rsplit_once('.')
        .is_some_and(|(_, ext)| ZIP_BASED.contains(&ext))
}

/// The name of the folder an archive extracts to: its file name without the archive
/// extensions (`photos.tar.gz` → `photos`, `v1.2.zip` → `v1.2`).
///
/// This works on the file *name* and never on a whole path: a dot in a folder above
/// (`v1.2/sub/x.zip`) is not part of it.
pub fn archive_stem(name: &OsStr) -> OsString {
    let lower = lower_name(name);
    let text = name.to_string_lossy();
    for ext in EXTENSIONS {
        if lower.len() > ext.len() && lower.ends_with(ext) {
            // The lowercase form can differ in length from the original only for exotic
            // case mappings; cut by characters counted from the end to stay on boundaries.
            let keep = text.chars().count().saturating_sub(ext.chars().count());
            let cut: String = text.chars().take(keep).collect();
            if !cut.is_empty() {
                return rebuild(name, &cut);
            }
        }
    }
    name.to_os_string()
}

#[cfg(unix)]
fn rebuild(name: &OsStr, cut: &str) -> OsString {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    // Keep invalid bytes of the original name when it has any.
    if name.to_str().is_some() {
        OsString::from(cut)
    } else {
        let bytes = name.as_bytes();
        let ext_len = EXTENSIONS
            .iter()
            .find(|e| {
                let b = bytes.to_ascii_lowercase();
                b.len() > e.len() && b.ends_with(e.as_bytes())
            })
            .map_or(0, |e| e.len());
        OsString::from_vec(bytes[..bytes.len() - ext_len].to_vec())
    }
}

#[cfg(not(unix))]
fn rebuild(_name: &OsStr, cut: &str) -> OsString {
    OsString::from(cut)
}

/// The name of the single file inside a `Format::Single` archive: the archive's name without
/// the compression extension (`notes.txt.gz` → `notes.txt`).
pub fn single_member_name(name: &OsStr, c: Compression) -> OsString {
    let lower = lower_name(name);
    let exts: &[&str] = match c {
        Compression::Gzip => &[".gz"],
        Compression::Bzip2 => &[".bz2"],
        Compression::Xz => &[".xz"],
        Compression::Zstd => &[".zst", ".zstd"],
        Compression::None => &[],
    };
    let text = name.to_string_lossy();
    for ext in exts {
        if lower.len() > ext.len() && lower.ends_with(ext) {
            let keep = text.chars().count() - ext.chars().count();
            return OsString::from(text.chars().take(keep).collect::<String>());
        }
    }
    let mut out = name.to_os_string();
    out.push(".out");
    out
}

/// Which formats a user may ask `rada` to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveKind {
    Zip,
    TarGz,
    TarZst,
    TarXz,
}

impl ArchiveKind {
    pub const ALL: [ArchiveKind; 4] = [
        ArchiveKind::Zip,
        ArchiveKind::TarGz,
        ArchiveKind::TarZst,
        ArchiveKind::TarXz,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            ArchiveKind::Zip => ".zip",
            ArchiveKind::TarGz => ".tar.gz",
            ArchiveKind::TarZst => ".tar.zst",
            ArchiveKind::TarXz => ".tar.xz",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArchiveKind::Zip => "zip",
            ArchiveKind::TarGz => "tar.gz",
            ArchiveKind::TarZst => "tar.zst",
            ArchiveKind::TarXz => "tar.xz",
        }
    }

    pub fn next(self) -> ArchiveKind {
        match self {
            ArchiveKind::Zip => ArchiveKind::TarGz,
            ArchiveKind::TarGz => ArchiveKind::TarZst,
            ArchiveKind::TarZst => ArchiveKind::TarXz,
            ArchiveKind::TarXz => ArchiveKind::Zip,
        }
    }

    pub fn compression(self) -> Compression {
        match self {
            ArchiveKind::Zip => Compression::None,
            ArchiveKind::TarGz => Compression::Gzip,
            ArchiveKind::TarZst => Compression::Zstd,
            ArchiveKind::TarXz => Compression::Xz,
        }
    }

    /// The kind whose extension ends `name`, if any.
    pub fn from_name(name: &OsStr) -> Option<ArchiveKind> {
        let n = lower_name(name);
        ArchiveKind::ALL
            .into_iter()
            .find(|k| n.ends_with(k.extension()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stem(s: &str) -> String {
        archive_stem(OsStr::new(s)).to_string_lossy().into_owned()
    }

    #[test]
    fn the_stem_drops_archive_extensions_only() {
        assert_eq!(stem("foto.tar.gz"), "foto");
        assert_eq!(stem("foto.tgz"), "foto");
        assert_eq!(stem("foto.TAR.XZ"), "foto");
        assert_eq!(stem("v1.2.zip"), "v1.2");
        assert_eq!(stem("a.b.c.tar.zst"), "a.b.c");
        assert_eq!(stem("notes.txt.gz"), "notes.txt");
        assert_eq!(stem("plain"), "plain");
        assert_eq!(stem("report.pdf"), "report.pdf");
        // Nothing left to name a folder: keep the name.
        assert_eq!(stem(".zip"), ".zip");
    }

    #[test]
    fn the_stem_is_computed_on_the_name_never_on_a_path() {
        // Regression from another file manager: a dot in a folder above the archive moved
        // the extraction into the wrong place.
        let p = Path::new("/home/u/v1.2/sub/x.zip");
        assert_eq!(archive_stem(p.file_name().unwrap()), OsString::from("x"));
        let p = Path::new("/home/u/v1.2/sub/noext");
        assert_eq!(
            archive_stem(p.file_name().unwrap()),
            OsString::from("noext")
        );
    }

    #[test]
    fn single_member_names() {
        let n = |s: &str, c| {
            single_member_name(OsStr::new(s), c)
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(n("notes.txt.gz", Compression::Gzip), "notes.txt");
        assert_eq!(n("blob.zst", Compression::Zstd), "blob");
        assert_eq!(n("mystery", Compression::Xz), "mystery.out");
    }

    #[test]
    fn names_that_suggest_an_archive() {
        for ok in ["a.zip", "a.tar.gz", "A.TGZ", "x.7z", "y.rar", "z.tar.zst", "k.gz"] {
            assert!(name_suggests_archive(OsStr::new(ok)), "{ok}");
        }
        for no in ["zip", "a.txt", ".zip", "a.docx", "tar"] {
            assert!(!name_suggests_archive(OsStr::new(no)), "{no}");
        }
        assert!(name_is_zip_document(OsStr::new("Report.DOCX")));
        assert!(!name_is_zip_document(OsStr::new("a.zip")));
    }

    #[test]
    fn magic_numbers() {
        assert_eq!(sniff_head(b"PK\x03\x04rest"), Some(Format::Zip));
        assert_eq!(
            sniff_head(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4]),
            Some(Format::SevenZ)
        );
        assert_eq!(sniff_head(b"Rar!\x1a\x07\x00xx"), Some(Format::Rar));
        assert_eq!(sniff_head(b"plain text, not an archive"), None);
    }
}
