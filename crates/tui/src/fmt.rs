//! Small text formatting helpers (sizes, dates, durations).

use std::time::{Duration, SystemTime};

pub use vela_core::display::bytes as size;

pub fn date(t: Option<SystemTime>) -> String {
    let Some(t) = t else { return "—".into() };
    match jiff::Timestamp::try_from(t) {
        Ok(ts) => ts
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%Y-%m-%d %H:%M")
            .to_string(),
        Err(_) => "—".into(),
    }
}

pub fn clock(unix_secs: i64) -> String {
    match jiff::Timestamp::from_second(unix_secs) {
        Ok(ts) => ts
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%b %d %H:%M")
            .to_string(),
        Err(_) => "—".into(),
    }
}

pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

pub fn rate(bytes_per_sec: f64) -> String {
    format!("{}/s", size(bytes_per_sec.max(0.0) as u64))
}

pub fn count(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{} {many}", thousands(n))
    }
}

pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Word-wrap by display width without ever dropping a character (long paths included).
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let width = width.max(8);
    let mut lines = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut w = 0;
        for word in raw.split_inclusive(' ') {
            let ww: usize = word.chars().map(|c| c.width().unwrap_or(0)).sum();
            if w + ww <= width {
                line.push_str(word);
                w += ww;
                continue;
            }
            if !line.is_empty() {
                lines.push(line.trim_end().to_string());
                line = String::new();
                w = 0;
            }
            // A single token wider than the line (a long path): hard-break it.
            for c in word.chars() {
                let cw = c.width().unwrap_or(0);
                if w + cw > width {
                    lines.push(std::mem::take(&mut line));
                    w = 0;
                }
                line.push(c);
                w += cw;
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn wrapping_never_loses_characters() {
        let path = "/home/user/a-very-long-folder-name/with/many/segments/and-a-file-name-that-keeps-going.txt";
        let lines = wrap(
            &format!("failed to open source file: open {path}: permission denied"),
            30,
        );
        let joined: String = lines.concat().replace(' ', "");
        assert!(joined.contains(&path.replace(' ', "")), "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| unicode_width::UnicodeWidthStr::width(l.as_str()) <= 30)
        );
    }
}
