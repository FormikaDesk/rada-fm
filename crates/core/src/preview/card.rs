//! The "card" shown for binary files: what the file is, how big, when, who may do what,
//! and for executables the architecture, read from the header (never executed).

use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct FileCard {
    /// Human description ("ELF executable", "ZIP archive", "binary data").
    pub kind: &'static str,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub mode: Option<u32>,
    pub exec: Option<ExecInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecInfo {
    pub format: &'static str,
    pub arch: String,
    pub bits: Option<u8>,
    pub endian: Option<&'static str>,
    /// "PIE executable", "shared library", "static executable", "relocatable object"...
    pub kind: String,
    pub interpreter: Option<String>,
}

/// `-rwxr-xr-x` style string for a Unix mode (type letter included).
pub fn mode_string(mode: u32) -> String {
    let t = match mode & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o100000 => '-',
        0o010000 => 'p',
        0o140000 => 's',
        0o060000 => 'b',
        0o020000 => 'c',
        _ => '?',
    };
    let bit = |m: u32, c: char| if mode & m != 0 { c } else { '-' };
    let x = |m: u32, special: u32, on: char, off: char| match (mode & m != 0, mode & special != 0) {
        (true, true) => on,
        (false, true) => off,
        (true, false) => 'x',
        (false, false) => '-',
    };
    format!(
        "{t}{}{}{}{}{}{}{}{}{}",
        bit(0o400, 'r'),
        bit(0o200, 'w'),
        x(0o100, 0o4000, 's', 'S'),
        bit(0o040, 'r'),
        bit(0o020, 'w'),
        x(0o010, 0o2000, 's', 'S'),
        bit(0o004, 'r'),
        bit(0o002, 'w'),
        x(0o001, 0o1000, 't', 'T'),
    )
}

/// Identify an executable or library from its first bytes.
pub fn parse_exec(b: &[u8]) -> Option<ExecInfo> {
    if b.starts_with(b"\x7fELF") {
        return elf(b);
    }
    if b.starts_with(b"MZ") {
        return pe(b);
    }
    macho(b)
}

struct Rd<'a> {
    b: &'a [u8],
    le: bool,
}

impl Rd<'_> {
    fn u16(&self, o: usize) -> Option<u16> {
        let v: [u8; 2] = self.b.get(o..o + 2)?.try_into().ok()?;
        Some(if self.le { u16::from_le_bytes(v) } else { u16::from_be_bytes(v) })
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let v: [u8; 4] = self.b.get(o..o + 4)?.try_into().ok()?;
        Some(if self.le { u32::from_le_bytes(v) } else { u32::from_be_bytes(v) })
    }
    fn u64(&self, o: usize) -> Option<u64> {
        let v: [u8; 8] = self.b.get(o..o + 8)?.try_into().ok()?;
        Some(if self.le { u64::from_le_bytes(v) } else { u64::from_be_bytes(v) })
    }
}

fn elf(b: &[u8]) -> Option<ExecInfo> {
    if b.len() < 20 {
        return None;
    }
    let is64 = b[4] == 2;
    let le = b[5] != 2;
    let r = Rd { b, le };
    let e_type = r.u16(16)?;
    let machine = r.u16(18)?;
    let arch = match machine {
        3 => "x86",
        8 => "MIPS",
        20 => "PowerPC",
        21 => "PowerPC 64",
        22 => "IBM S/390",
        40 => "ARM",
        42 => "SuperH",
        43 => "SPARC V9",
        50 => "IA-64",
        62 => "x86-64",
        183 => "AArch64",
        243 => "RISC-V",
        247 => "eBPF",
        258 => "LoongArch",
        _ => "unknown architecture",
    }
    .to_string();

    // PT_INTERP tells a PIE executable from a plain shared library.
    let (phoff, phentsize, phnum) = if is64 {
        (r.u64(32)? as usize, r.u16(54)? as usize, r.u16(56)? as usize)
    } else {
        (r.u32(28)? as usize, r.u16(42)? as usize, r.u16(44)? as usize)
    };
    let mut interpreter = None;
    for i in 0..phnum.min(64) {
        let o = phoff + i * phentsize;
        if r.u32(o) != Some(3) {
            continue;
        }
        let (off, len) = if is64 {
            (r.u64(o + 8)? as usize, r.u64(o + 32)? as usize)
        } else {
            (r.u32(o + 4)? as usize, r.u32(o + 16)? as usize)
        };
        if let Some(raw) = b.get(off..off.saturating_add(len.min(512))) {
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            interpreter = Some(String::from_utf8_lossy(&raw[..end]).into_owned());
        }
        break;
    }
    let kind = match (e_type, interpreter.is_some()) {
        (1, _) => "relocatable object",
        (2, true) => "executable",
        (2, false) => "static executable",
        (3, true) => "PIE executable",
        (3, false) => "shared library",
        (4, _) => "core dump",
        _ => "ELF file",
    }
    .to_string();
    Some(ExecInfo {
        format: "ELF",
        arch,
        bits: Some(if is64 { 64 } else { 32 }),
        endian: Some(if le { "little-endian" } else { "big-endian" }),
        kind,
        interpreter,
    })
}

fn pe(b: &[u8]) -> Option<ExecInfo> {
    let r = Rd { b, le: true };
    let pe_off = r.u32(0x3c)? as usize;
    if b.get(pe_off..pe_off + 4)? != b"PE\0\0" {
        return None;
    }
    let machine = r.u16(pe_off + 4)?;
    let characteristics = r.u16(pe_off + 22)?;
    let magic = r.u16(pe_off + 24)?;
    let arch = match machine {
        0x14c => "x86",
        0x8664 => "x86-64",
        0x1c0 | 0x1c4 => "ARM",
        0xaa64 => "ARM64",
        0x200 => "IA-64",
        0x5064 => "RISC-V",
        _ => "unknown architecture",
    }
    .to_string();
    let kind = if characteristics & 0x2000 != 0 { "DLL" } else { "executable" }.to_string();
    Some(ExecInfo {
        format: "PE (Windows)",
        arch,
        bits: Some(if magic == 0x20b { 64 } else { 32 }),
        endian: Some("little-endian"),
        kind,
        interpreter: None,
    })
}

fn macho_arch(cpu: u32) -> &'static str {
    match cpu {
        7 => "x86",
        0x0100_0007 => "x86-64",
        12 => "ARM",
        0x0100_000c => "ARM64",
        0x0200_000c => "ARM64_32",
        18 => "PowerPC",
        0x0100_0012 => "PowerPC 64",
        _ => "unknown architecture",
    }
}

fn macho(b: &[u8]) -> Option<ExecInfo> {
    let magic: [u8; 4] = b.get(0..4)?.try_into().ok()?;
    // Fat (universal) binaries list several architectures, always big-endian.
    if magic == [0xca, 0xfe, 0xba, 0xbe] {
        let r = Rd { b, le: false };
        let n = r.u32(4)? as usize;
        if !(1..30).contains(&n) {
            return None; // a Java class file has the same magic
        }
        let archs: Vec<&str> = (0..n).filter_map(|i| r.u32(8 + i * 20)).map(macho_arch).collect();
        return Some(ExecInfo {
            format: "Mach-O (macOS)",
            arch: archs.join(" + "),
            bits: None,
            endian: None,
            kind: "universal binary".into(),
            interpreter: None,
        });
    }
    let (is64, le) = match magic {
        [0xfe, 0xed, 0xfa, 0xce] => (false, false),
        [0xce, 0xfa, 0xed, 0xfe] => (false, true),
        [0xfe, 0xed, 0xfa, 0xcf] => (true, false),
        [0xcf, 0xfa, 0xed, 0xfe] => (true, true),
        _ => return None,
    };
    let r = Rd { b, le };
    let cpu = r.u32(4)?;
    let kind = match r.u32(12)? {
        1 => "relocatable object",
        2 => "executable",
        6 => "dynamic library",
        8 => "bundle",
        _ => "Mach-O file",
    };
    Some(ExecInfo {
        format: "Mach-O (macOS)",
        arch: macho_arch(cpu).into(),
        bits: Some(if is64 { 64 } else { 32 }),
        endian: Some(if le { "little-endian" } else { "big-endian" }),
        kind: kind.into(),
        interpreter: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_strings() {
        assert_eq!(mode_string(0o100755), "-rwxr-xr-x");
        assert_eq!(mode_string(0o100640), "-rw-r-----");
        assert_eq!(mode_string(0o040755), "drwxr-xr-x");
        assert_eq!(mode_string(0o104755), "-rwsr-xr-x");
        assert_eq!(mode_string(0o041777), "drwxrwxrwt");
        assert_eq!(mode_string(0o120777), "lrwxrwxrwx");
    }

    #[test]
    fn elf_headers_of_real_system_binaries() {
        // /bin/sh exists on every Linux machine this runs on; the header is read, not run.
        let Ok(bytes) = std::fs::read("/bin/sh") else { return };
        let info = parse_exec(&bytes[..bytes.len().min(256 * 1024)]).expect("ELF");
        assert_eq!(info.format, "ELF");
        assert!(["x86-64", "AArch64", "x86", "ARM", "RISC-V"].contains(&info.arch.as_str()), "{info:?}");
        assert_eq!(info.endian, Some("little-endian"));
        assert!(info.interpreter.as_deref().is_none_or(|i| i.contains("ld")), "{info:?}");
    }

    #[test]
    fn synthetic_headers() {
        // Minimal ELF64 little-endian, ET_DYN, x86-64, no program headers.
        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[16] = 3;
        elf[18] = 62;
        let i = parse_exec(&elf).unwrap();
        assert_eq!((i.arch.as_str(), i.bits, i.kind.as_str()), ("x86-64", Some(64), "shared library"));

        // PE32+ DLL for ARM64.
        let mut pe = vec![0u8; 0x100];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c] = 0x80;
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0xaa64u16.to_le_bytes());
        pe[0x80 + 22..0x80 + 24].copy_from_slice(&0x2000u16.to_le_bytes());
        pe[0x80 + 24..0x80 + 26].copy_from_slice(&0x20bu16.to_le_bytes());
        let i = parse_exec(&pe).unwrap();
        assert_eq!((i.arch.as_str(), i.bits, i.kind.as_str()), ("ARM64", Some(64), "DLL"));

        // Mach-O 64 little-endian x86-64 executable.
        let mut m = vec![0u8; 32];
        m[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        m[4..8].copy_from_slice(&0x0100_0007u32.to_le_bytes());
        m[12..16].copy_from_slice(&2u32.to_le_bytes());
        let i = parse_exec(&m).unwrap();
        assert_eq!((i.arch.as_str(), i.kind.as_str()), ("x86-64", "executable"));

        // A Java class file shares the Mach-O fat magic and must not match.
        assert!(parse_exec(&[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 52, 0, 0]).is_none());
        assert!(parse_exec(b"just text").is_none());
    }
}
