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

/// `~/projects/vela` instead of `/home/user/projects/vela`.
pub fn short_path(p: &std::path::Path, home: &std::path::Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", vela_core::display::path(rest)),
        Err(_) => vela_core::display::path(p),
    }
}

/// "2 h ago", "yesterday", "Sep 17": a date read at a glance.
pub fn relative(t: Option<SystemTime>, now: SystemTime) -> String {
    let Some(t) = t else { return "—".into() };
    let Ok(age) = now.duration_since(t) else {
        return "just now".into();
    };
    let s = age.as_secs();
    let (m, h, d) = (s / 60, s / 3600, s / 86400);
    if s < 45 {
        "just now".into()
    } else if m < 2 {
        "1 min ago".into()
    } else if m < 60 {
        format!("{m} min ago")
    } else if h < 24 {
        format!("{h} h ago")
    } else if h < 48 {
        "yesterday".into()
    } else if d < 14 {
        format!("{d} days ago")
    } else if d < 60 {
        format!("{} weeks ago", d / 7)
    } else {
        match jiff::Timestamp::try_from(t) {
            Ok(ts) => {
                let z = ts.to_zoned(jiff::tz::TimeZone::system());
                let this_year = jiff::Timestamp::try_from(now)
                    .map(|n| n.to_zoned(jiff::tz::TimeZone::system()).year())
                    .ok()
                    == Some(z.year());
                z.strftime(if this_year { "%b %d" } else { "%b %Y" })
                    .to_string()
            }
            Err(_) => "—".into(),
        }
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
    use std::time::Duration;

    #[test]
    fn relative_dates() {
        let now = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let ago = |s: u64| super::relative(Some(now - Duration::from_secs(s)), now);
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(60), "1 min ago");
        assert_eq!(ago(600), "10 min ago");
        assert_eq!(ago(2 * 3600), "2 h ago");
        assert_eq!(ago(30 * 3600), "yesterday");
        assert_eq!(ago(3 * 86400), "3 days ago");
        assert_eq!(ago(21 * 86400), "3 weeks ago");
        assert_eq!(super::relative(None, now), "—");
    }

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
