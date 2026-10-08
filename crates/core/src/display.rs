//! Display-only string handling.
//!
//! Paths are never manipulated as text anywhere in vela; this module is the *only*
//! place where an `OsStr` becomes a `String`, and the result is meant to be shown,
//! not parsed. Control characters, escape sequences, bidi controls and invalid
//! bytes are rendered as visible escapes so a hostile file name can neither break
//! the layout nor inject terminal sequences.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::Path;

use unicode_width::UnicodeWidthChar;

/// Escape an `OsStr` for display. Printable text is returned unchanged.
pub fn name(s: &OsStr) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        escape_bytes(s.as_bytes())
    }
    #[cfg(not(unix))]
    {
        match s.to_str() {
            Some(t) => escape_str(t),
            None => escape_str(&s.to_string_lossy()),
        }
    }
}

/// Escape a whole path for display.
pub fn path(p: &Path) -> String {
    name(p.as_os_str())
}

#[cfg(unix)]
fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                push_escaped(&mut out, valid);
                return out;
            }
            Err(e) => {
                let (good, bad) = rest.split_at(e.valid_up_to());
                // SAFETY-free: `good` is valid by construction of `valid_up_to`.
                push_escaped(&mut out, std::str::from_utf8(good).unwrap_or(""));
                let bad_len = e.error_len().unwrap_or(bad.len());
                for b in &bad[..bad_len] {
                    let _ = write!(out, "\\x{b:02x}");
                }
                rest = &bad[bad_len..];
                if rest.is_empty() {
                    return out;
                }
            }
        }
    }
}

#[cfg(not(unix))]
fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    push_escaped(&mut out, s);
    out
}

fn push_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x1b' => out.push_str("\\e"),
            '\\' if cfg!(unix) => out.push_str("\\\\"),
            c if is_dangerous(c) => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn is_dangerous(c: char) -> bool {
    c.is_control()
        || matches!(
            c as u32,
            0x061C | 0x200E | 0x200F | 0x2028 | 0x2029 | 0x202A..=0x202E | 0x2066..=0x2069
                | 0xFEFF | 0xFFF9..=0xFFFB
        )
}

/// Terminal cell width of an (already escaped) string.
pub fn width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Truncate to at most `max` cells, adding an ellipsis when something was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// Human readable size (`1.5 MiB`).
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.2} {}", UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(
            name(OsStr::new("héllo wörld 日本語 🎉.txt")),
            "héllo wörld 日本語 🎉.txt"
        );
    }

    #[test]
    fn control_and_escape_sequences_are_made_visible() {
        assert_eq!(name(OsStr::new("a\nb\tc")), "a\\nb\\tc");
        assert_eq!(name(OsStr::new("\x1b]0;pwned\x07")), "\\e]0;pwned\\u{7}");
        assert_eq!(name(OsStr::new("x\u{202e}gpj.exe")), "x\\u{202e}gpj.exe");
    }

    #[cfg(unix)]
    #[test]
    fn invalid_utf8_is_shown_as_hex() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(name(OsStr::from_bytes(b"a\xff\xfeb")), "a\\xff\\xfeb");
        assert_eq!(name(OsStr::from_bytes(b"\xc3")), "\\xc3");
    }

    #[test]
    fn truncation_respects_cell_width() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("日本語です", 5), "日本…");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn byte_formatting() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1536), "1.50 KiB");
        assert_eq!(bytes(10 * 1024 * 1024), "10.0 MiB");
    }
}
