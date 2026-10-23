//! Display-only string handling.
//!
//! Paths are never manipulated as text anywhere in rada; this module is the *only*
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

/// Prepare one line of *file content* for display: tabs become spaces and anything
/// that could be mistaken for a terminal command is escaped. Unlike [`name`], a
/// literal backslash is left alone because this is text, not an identifier.
pub fn line(s: &str, tab: usize) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.trim_end_matches(['\n', '\r']).chars() {
        match c {
            '\t' => out.extend(std::iter::repeat_n(' ', tab)),
            '\x1b' => out.push_str("\\e"),
            c if is_dangerous(c) => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Terminal cell width of an (already escaped) string.
pub fn width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// The longest start of `s` that fits in `max` cells.
pub fn head_cells(s: &str, max: usize) -> &str {
    let mut w = 0;
    for (at, c) in s.char_indices() {
        w += c.width().unwrap_or(0);
        if w > max {
            return &s[..at];
        }
    }
    s
}

/// The longest end of `s` that fits in `max` cells.
pub fn tail_cells(s: &str, max: usize) -> &str {
    let mut w = 0;
    for (at, c) in s.char_indices().rev() {
        w += c.width().unwrap_or(0);
        if w > max {
            return &s[at + c.len_utf8()..];
        }
    }
    s
}

/// Truncate to at most `max` cells, adding an ellipsis when something was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    format!("{}…", head_cells(s, max - 1))
}

/// The longest extension (dot included) that is kept whole when a name is shortened.
const KEPT_EXTENSION: usize = 8;

/// Shorten a file or folder name to at most `max` cells by cutting out the middle, so that the
/// start and the end are both seen: `CV_Alessan…Accenture.pdf`. Cut at the end, five names
/// that differ only there would all read the same. The end is worth a little more than the
/// start, and an extension is never cut into while there is room for it.
pub fn truncate_name(name: &str, max: usize) -> String {
    if width(name) <= max {
        return name.to_string();
    }
    if max < 3 {
        return truncate(name, max);
    }
    let room = max - 1;
    let extension = name
        .rfind('.')
        .filter(|at| *at > 0)
        .map_or(0, |at| width(&name[at..]));
    let kept = if extension <= KEPT_EXTENSION {
        extension
    } else {
        0
    };
    let tail_w = (room - room * 9 / 20).max(kept).min(room - 1);
    let head = head_cells(name, room - tail_w);
    format!("{head}…{}", tail_cells(name, tail_w))
}

/// Human readable size (`1.5 MB`, `1,5 MB` in Italian), in powers of 1024 labelled the way
/// Windows does.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let number = if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    };
    format!("{} {}", crate::i18n::decimal(&number), UNITS[u])
}

/// The same as [`bytes`] for a narrow place: no decimals from 10 up, one below (`53 GB`,
/// `9,5 GB`), so that it is at most six cells wide.
pub fn bytes_short(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let number = if v >= 10.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    };
    format!("{} {}", crate::i18n::decimal(&number), UNITS[u])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_size_has_no_decimals_from_ten_up() {
        assert_eq!(
            bytes_short(53 * 1024 * 1024 * 1024 + 1024 * 1024 * 100),
            "53 GB"
        );
        assert_eq!(bytes_short(156 * 1024 * 1024 * 1024), "156 GB");
        assert_eq!(
            bytes_short(9 * 1024 * 1024 * 1024 + 512 * 1024 * 1024),
            "9.5 GB"
        );
        assert_eq!(bytes_short(300), "300 B");
    }

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
    fn long_names_are_cut_in_the_middle() {
        let name = "CV_Alessandro_Formica_Accenture.pdf";
        assert_eq!(truncate_name(name, 24), "CV_Alessan…Accenture.pdf");
        assert_eq!(truncate_name("Accenture.pdf", 13), "Accenture.pdf");
        assert_eq!(truncate_name("Accenture.pdf", 10), "Acce…e.pdf");
        assert_eq!(truncate_name("Accenture.pdf", 6), "A….pdf");
        assert_eq!(truncate_name("Accenture.pdf", 2), "A…");
    }

    #[test]
    fn folders_keep_their_last_part_too() {
        assert_eq!(truncate_name("Progetti_vecchi_2019", 12), "Prog…hi_2019");
        assert_eq!(truncate_name(".config_backup_old", 10), ".con…p_old");
    }

    #[test]
    fn cut_names_fit_the_cell_width() {
        for name in [
            "Perché_è_già_così_lungo.txt",
            "日本語のファイル名です.md",
            "漢字漢字漢字漢字",
        ] {
            for max in 3..width(name) {
                let cut = truncate_name(name, max);
                assert!(width(&cut) <= max, "{cut} in {max}");
                assert!(cut.contains('…'), "{cut}");
            }
        }
        assert_eq!(
            truncate_name("日本語のファイル名です.md", 12),
            "日本…です.md"
        );
    }

    #[test]
    fn short_names_stay_whole() {
        assert_eq!(truncate_name("notes.txt", 20), "notes.txt");
        assert_eq!(truncate_name("notes.txt", 9), "notes.txt");
        assert_eq!(truncate_name("日本語", 6), "日本語");
    }

    #[test]
    fn byte_formatting() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1536), "1.50 KB");
        assert_eq!(bytes(10 * 1024 * 1024), "10.0 MB");
    }
}
