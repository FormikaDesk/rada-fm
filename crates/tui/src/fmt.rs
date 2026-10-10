//! Small text formatting helpers (sizes, dates, durations).

use std::time::{Duration, SystemTime};

pub use rada_core::display::bytes as size;

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

/// `~/projects/rada` instead of `/home/me/projects/rada`.
pub fn short_path(p: &std::path::Path, home: &std::path::Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        // Always with slashes after the `~`: `~/projects/demo`, not `~/projects\demo`.
        Ok(rest) => {
            let parts: Vec<String> = rest
                .components()
                .map(|c| rada_core::display::name(c.as_os_str()))
                .collect();
            format!("~/{}", parts.join("/"))
        }
        Err(_) => rada_core::display::path(p),
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

// ------------------------------------------------------------------ Explorer-style dates

/// How a calendar date is written: day, month and year in the order and with the separator
/// the user's locale uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateFormat {
    order: DateOrder,
    sep: char,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateOrder {
    DayMonthYear,
    MonthDayYear,
    YearMonthDay,
}

impl DateFormat {
    /// `dd/mm/yyyy`: what is used when the locale says nothing usable.
    pub const DEFAULT: DateFormat = DateFormat {
        order: DateOrder::DayMonthYear,
        sep: '/',
    };

    /// From a locale name such as `it_IT.UTF-8`, `en_US` or `de-DE`. Anything not recognised
    /// gives `dd/mm/yyyy`.
    pub fn from_locale(locale: &str) -> DateFormat {
        let name = locale
            .split(['.', '@'])
            .next()
            .unwrap_or("")
            .replace('-', "_");
        let lang = name.split('_').next().unwrap_or("").to_ascii_lowercase();
        let region = name.split('_').nth(1).unwrap_or("").to_ascii_uppercase();
        let (order, sep) = match (lang.as_str(), region.as_str()) {
            ("en", "US") | ("en", "PH") => (DateOrder::MonthDayYear, '/'),
            ("ja" | "zh" | "ko" | "hu" | "lt" | "sv" | "mn", _) => (DateOrder::YearMonthDay, '-'),
            ("en", "CA") => (DateOrder::YearMonthDay, '-'),
            (
                "de" | "ru" | "pl" | "cs" | "sk" | "fi" | "nb" | "nn" | "no" | "tr" | "uk" | "da"
                | "ro" | "bg" | "hr" | "sl" | "sr" | "et" | "lv" | "is",
                _,
            ) => (DateOrder::DayMonthYear, '.'),
            ("nl", _) => (DateOrder::DayMonthYear, '-'),
            _ => return DateFormat::DEFAULT,
        };
        DateFormat { order, sep }
    }

    /// From the environment (`LC_ALL`, `LC_TIME`, `LANG`), the way programs find the locale.
    pub fn from_env() -> DateFormat {
        ["LC_ALL", "LC_TIME", "LANG"]
            .iter()
            .filter_map(|v| std::env::var(v).ok())
            .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
            .map(|v| DateFormat::from_locale(&v))
            .unwrap_or(DateFormat::DEFAULT)
    }

    pub fn write(&self, year: i16, month: i8, day: i8) -> String {
        let s = self.sep;
        match self.order {
            DateOrder::DayMonthYear => format!("{day:02}{s}{month:02}{s}{year:04}"),
            DateOrder::MonthDayYear => format!("{month:02}{s}{day:02}{s}{year:04}"),
            DateOrder::YearMonthDay => format!("{year:04}{s}{month:02}{s}{day:02}"),
        }
    }
}

/// How the Modified column writes times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateStyle {
    /// "Today 14:03", "Yesterday", "3 days ago", "2 weeks ago", then the full date.
    Relative,
    /// Always the full date and time.
    Absolute,
}

impl DateStyle {
    pub fn parse(s: &str) -> Option<DateStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "relative" => Some(DateStyle::Relative),
            "absolute" => Some(DateStyle::Absolute),
            _ => None,
        }
    }
}

/// The date for the Modified column, in the system's time zone.
pub fn explorer_date(
    t: Option<SystemTime>,
    now: SystemTime,
    style: DateStyle,
    format: DateFormat,
) -> String {
    explorer_date_in(t, now, style, format, &jiff::tz::TimeZone::system())
}

/// [`explorer_date`] in a given time zone (so that tests do not depend on the machine's).
pub fn explorer_date_in(
    t: Option<SystemTime>,
    now: SystemTime,
    style: DateStyle,
    format: DateFormat,
    tz: &jiff::tz::TimeZone,
) -> String {
    let Some(t) = t else { return "—".into() };
    let (Ok(ts), Ok(now_ts)) = (jiff::Timestamp::try_from(t), jiff::Timestamp::try_from(now))
    else {
        return "—".into();
    };
    let (z, zn) = (ts.to_zoned(tz.clone()), now_ts.to_zoned(tz.clone()));
    let full = format!(
        "{} {:02}:{:02}",
        format.write(z.year(), z.month(), z.day()),
        z.hour(),
        z.minute()
    );
    if style == DateStyle::Absolute {
        return full;
    }
    let days = (zn.date() - z.date()).get_days();
    match days {
        d if d < 0 => full,
        0 => format!("Today {:02}:{:02}", z.hour(), z.minute()),
        1 => "Yesterday".into(),
        2..=13 => format!("{days} days ago"),
        14..=55 => format!("{} weeks ago", days / 7),
        _ => format.write(z.year(), z.month(), z.day()),
    }
}

/// "Today, 15:21", "Yesterday, 09:12" or the full date and time, for the details pane.
pub fn date_with_day(
    t: Option<SystemTime>,
    now: SystemTime,
    format: DateFormat,
    tz: &jiff::tz::TimeZone,
) -> String {
    let Some(t) = t else { return "—".into() };
    let (Ok(ts), Ok(now_ts)) = (jiff::Timestamp::try_from(t), jiff::Timestamp::try_from(now))
    else {
        return "—".into();
    };
    let (z, zn) = (ts.to_zoned(tz.clone()), now_ts.to_zoned(tz.clone()));
    let clock = format!("{:02}:{:02}", z.hour(), z.minute());
    match (zn.date() - z.date()).get_days() {
        0 => format!("Today, {clock}"),
        1 => format!("Yesterday, {clock}"),
        _ => format!("{} {clock}", format.write(z.year(), z.month(), z.day())),
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
        if i > 0 && (s.len() - i).is_multiple_of(3) {
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

    fn day(y: i16, m: i8, d: i8, h: i8, min: i8) -> SystemTime {
        let z = jiff::civil::date(y, m, d)
            .at(h, min, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap();
        SystemTime::from(z.timestamp())
    }

    #[test]
    fn explorer_dates_read_like_explorers() {
        let utc = jiff::tz::TimeZone::UTC;
        let now = day(2026, 10, 10, 16, 0);
        let rel =
            |t| explorer_date_in(Some(t), now, DateStyle::Relative, DateFormat::DEFAULT, &utc);
        assert_eq!(rel(day(2026, 10, 10, 14, 3)), "Today 14:03");
        assert_eq!(rel(day(2026, 10, 10, 0, 1)), "Today 00:01");
        assert_eq!(rel(day(2026, 10, 9, 23, 59)), "Yesterday");
        assert_eq!(rel(day(2026, 10, 7, 12, 0)), "3 days ago");
        assert_eq!(rel(day(2026, 9, 28, 12, 0)), "12 days ago");
        assert_eq!(rel(day(2026, 9, 26, 12, 0)), "2 weeks ago");
        assert_eq!(rel(day(2026, 8, 20, 12, 0)), "7 weeks ago");
        assert_eq!(rel(day(2026, 7, 5, 9, 0)), "05/07/2026");
        // A time in the future is never "ago".
        assert_eq!(rel(day(2026, 10, 11, 9, 0)), "11/10/2026 09:00");
        assert_eq!(
            explorer_date_in(None, now, DateStyle::Relative, DateFormat::DEFAULT, &utc),
            "—"
        );
    }

    #[test]
    fn absolute_dates_always_carry_the_time() {
        let utc = jiff::tz::TimeZone::UTC;
        let now = day(2026, 10, 10, 16, 0);
        let s = explorer_date_in(
            Some(day(2026, 10, 10, 14, 3)),
            now,
            DateStyle::Absolute,
            DateFormat::DEFAULT,
            &utc,
        );
        assert_eq!(s, "10/10/2026 14:03");
    }

    #[test]
    fn the_calendar_day_decides_not_the_number_of_hours() {
        let utc = jiff::tz::TimeZone::UTC;
        // 25 minutes ago but across midnight: yesterday.
        let now = day(2026, 10, 10, 0, 10);
        let t = day(2026, 10, 9, 23, 45);
        assert_eq!(
            explorer_date_in(Some(t), now, DateStyle::Relative, DateFormat::DEFAULT, &utc),
            "Yesterday"
        );
    }

    #[test]
    fn the_locale_chooses_the_order_of_a_date() {
        let write = |loc: &str| DateFormat::from_locale(loc).write(2026, 7, 5);
        assert_eq!(write("it_IT.UTF-8"), "05/07/2026");
        assert_eq!(write("en_GB.UTF-8"), "05/07/2026");
        assert_eq!(write("fr_FR"), "05/07/2026");
        assert_eq!(write("en_US.UTF-8"), "07/05/2026");
        assert_eq!(write("de_DE.UTF-8"), "05.07.2026");
        assert_eq!(write("ja_JP.UTF-8"), "2026-07-05");
        assert_eq!(write("sv_SE"), "2026-07-05");
        assert_eq!(write("C"), "05/07/2026");
        assert_eq!(write(""), "05/07/2026");
        assert_eq!(write("xx_YY"), "05/07/2026");
    }

    #[test]
    fn the_details_pane_says_today_and_yesterday() {
        let utc = jiff::tz::TimeZone::UTC;
        let now = day(2026, 10, 10, 16, 0);
        let f = |t| date_with_day(Some(t), now, DateFormat::DEFAULT, &utc);
        assert_eq!(f(day(2026, 10, 10, 15, 21)), "Today, 15:21");
        assert_eq!(f(day(2026, 10, 9, 9, 12)), "Yesterday, 09:12");
        assert_eq!(f(day(2026, 10, 8, 15, 21)), "08/10/2026 15:21");
    }

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
