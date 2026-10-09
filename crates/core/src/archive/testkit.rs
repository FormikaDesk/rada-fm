//! Archives built by hand for tests.
//!
//! The members are written byte by byte, not with the library that reads them back and not
//! with rada's own writer, so that a test can make archives no sane tool would: a name with
//! `..`, an absolute path, a name in code page 437, a member flagged as password-protected, a
//! file cut short. Only the compression of a whole tar stream uses a real encoder.

use std::io::Write;
use std::path::Path;

use super::format::Compression;

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

#[derive(Clone, Debug)]
pub struct Member {
    /// The name exactly as stored.
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    /// Unix permission bits (and the type bits, for a link).
    pub mode: u32,
    pub kind: Kind,
    pub mtime: u32,
    /// For a symlink or hard link.
    pub link: Vec<u8>,
    /// ZIP only: the "this name is UTF-8" flag.
    pub utf8_flag: bool,
    /// ZIP only: marks the member as encrypted (its data is not really).
    pub encrypted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Symlink,
    Hardlink,
}

impl Member {
    pub fn file(name: &str, data: impl AsRef<[u8]>) -> Member {
        Member {
            name: name.as_bytes().to_vec(),
            data: data.as_ref().to_vec(),
            mode: 0o644,
            kind: Kind::File,
            mtime: 1_700_000_000,
            link: Vec::new(),
            utf8_flag: true,
            encrypted: false,
        }
    }

    pub fn dir(name: &str) -> Member {
        Member {
            kind: Kind::Dir,
            mode: 0o755,
            ..Member::file(name, b"")
        }
    }

    pub fn symlink(name: &str, target: &str) -> Member {
        Member {
            kind: Kind::Symlink,
            mode: 0o777,
            link: target.as_bytes().to_vec(),
            ..Member::file(name, b"")
        }
    }

    pub fn hardlink(name: &str, target: &str) -> Member {
        Member {
            kind: Kind::Hardlink,
            link: target.as_bytes().to_vec(),
            ..Member::file(name, b"")
        }
    }

    pub fn raw_name(mut self, name: &[u8]) -> Member {
        self.name = name.to_vec();
        self
    }

    pub fn mode(mut self, mode: u32) -> Member {
        self.mode = mode;
        self
    }

    pub fn mtime(mut self, t: u32) -> Member {
        self.mtime = t;
        self
    }

    pub fn encrypted(mut self) -> Member {
        self.encrypted = true;
        self
    }

    pub fn no_utf8_flag(mut self) -> Member {
        self.utf8_flag = false;
        self
    }
}

// ------------------------------------------------------------------------------- zip

/// A ZIP with every member stored (not compressed), written by hand. Returns the bytes.
pub fn zip_bytes(members: &[Member]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for m in members {
        let offset = out.len() as u32;
        let (data, ext_mode): (Vec<u8>, u32) = match m.kind {
            Kind::Symlink => (m.link.clone(), 0o120000 | (m.mode & 0o7777)),
            Kind::Dir => (Vec::new(), 0o040000 | (m.mode & 0o7777)),
            _ => (m.data.clone(), 0o100000 | (m.mode & 0o7777)),
        };
        let mut name = m.name.clone();
        if m.kind == Kind::Dir && !name.ends_with(b"/") {
            name.push(b'/');
        }
        let flags: u16 = (if m.utf8_flag { 1 << 11 } else { 0 }) | u16::from(m.encrypted);
        let crc = crc32(&data);
        // local header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0x5821u16.to_le_bytes()); // date: 2024-01-01
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        // extra field: extended timestamp
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x5455u16.to_le_bytes());
        extra.extend_from_slice(&5u16.to_le_bytes());
        extra.push(1);
        extra.extend_from_slice(&m.mtime.to_le_bytes());
        out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&data);
        // central header
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made on Unix
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&flags.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x5821u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&(extra.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&(ext_mode << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&name);
        central.extend_from_slice(&extra);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(members.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

pub fn write_zip(path: &Path, members: &[Member]) {
    std::fs::write(path, zip_bytes(members)).expect("write zip");
}

// ------------------------------------------------------------------------------- tar

fn octal(field: &mut [u8], v: u64) {
    let room = field.len() - 1;
    let s = format!("{v:0room$o}");
    let n = s.len().min(room);
    field[..n].copy_from_slice(&s.as_bytes()[..n]);
}

/// The 512-byte blocks of a tar stream (names up to 100 bytes), without compression.
pub fn tar_bytes(members: &[Member]) -> Vec<u8> {
    let mut out = Vec::new();
    for m in members {
        let mut h = [0u8; 512];
        let n = m.name.len().min(100);
        h[..n].copy_from_slice(&m.name[..n]);
        octal(&mut h[100..108], u64::from(m.mode & 0o7777));
        octal(&mut h[108..116], 0);
        octal(&mut h[116..124], 0);
        let size = if m.kind == Kind::File {
            m.data.len() as u64
        } else {
            0
        };
        octal(&mut h[124..136], size);
        octal(&mut h[136..148], u64::from(m.mtime));
        h[156] = match m.kind {
            Kind::File => b'0',
            Kind::Dir => b'5',
            Kind::Symlink => b'2',
            Kind::Hardlink => b'1',
        };
        let l = m.link.len().min(100);
        h[157..157 + l].copy_from_slice(&m.link[..l]);
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| *b as u32).sum();
        let s = format!("{sum:06o}\0 ");
        h[148..156].copy_from_slice(s.as_bytes());
        out.extend_from_slice(&h);
        if m.kind == Kind::File {
            out.extend_from_slice(&m.data);
            let pad = (512 - m.data.len() % 512) % 512;
            out.extend(std::iter::repeat_n(0u8, pad));
        }
    }
    out.extend(std::iter::repeat_n(0u8, 1024));
    out
}

/// Compress a byte stream with a real encoder.
pub fn compress(data: &[u8], c: Compression) -> Vec<u8> {
    match c {
        Compression::None => data.to_vec(),
        Compression::Gzip => {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        }
        Compression::Bzip2 => {
            let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        }
        Compression::Xz => {
            let mut e =
                lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1))
                    .unwrap();
            e.write_all(data).unwrap();
            e.finish().unwrap()
        }
        Compression::Zstd => {
            let mut e = structured_zstd::encoding::StreamingEncoder::new(
                Vec::new(),
                structured_zstd::encoding::CompressionLevel::Fastest,
            );
            e.write_all(data).unwrap();
            e.finish().unwrap()
        }
    }
}

pub fn write_tar(path: &Path, members: &[Member], c: Compression) {
    std::fs::write(path, compress(&tar_bytes(members), c)).expect("write tar");
}

/// Whether a program is installed (tests that need it are skipped without it).
pub fn have_tool(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| {
            let f = d.join(name);
            f.is_file()
        })
    })
}

/// The index of a file read through the external-program path, whatever its format (a `.7z`
/// stands in for a RAR, which cannot be made here).
pub fn index_via_tool(
    path: &Path,
    tool: &super::external::Tool,
) -> Result<super::Index, super::ArchiveError> {
    let size = std::fs::metadata(path)?.len();
    let mut ix = super::Index::new(path, super::Format::Rar, size);
    super::external::list_with(tool, &mut ix, &mut super::ListControl::new(&|| false), true)?;
    Ok(ix.finish())
}
