//! The fields of a bulk rename pattern that come from what a file says about itself
//! (`{exif.date:%Y-%m-%d}`, `{audio.track:02}`, `{pdf.title}`…) or from when it was last
//! changed (`{mtime}`), with the fields to try instead after `|`:
//! `{exif.date|mtime:%Y-%m-%d}` is the day the photo was taken, or the day the file was last
//! changed when the photo does not say.

use std::sync::Arc;
use std::time::SystemTime;

use jiff::civil::{Date, DateTime, Time};

use crate::i18n::{Key, tf};
use crate::meta::Meta;

/// What is known of one item when its new name is made.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// What the file says about itself; `None` when it was not read.
    pub meta: Option<Arc<Meta>>,
    /// When it was last changed.
    pub mtime: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    ExifDate,
    ExifTime,
    ExifCamera,
    ExifPlace,
    AudioArtist,
    AudioTitle,
    AudioAlbum,
    AudioTrack,
    AudioYear,
    PdfTitle,
    PdfAuthor,
    VideoDuration,
    Mtime,
}

/// What a field holds, which says what format it can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// A day and a time, written with `%Y %m %d %H %M %S`…
    Moment,
    Text,
    /// A number, padded with zeros to a width (`:02`).
    Number,
    Duration,
}

const FIELDS: &[(&str, Field)] = &[
    ("exif.date", Field::ExifDate),
    ("exif.time", Field::ExifTime),
    ("exif.camera", Field::ExifCamera),
    ("exif.place", Field::ExifPlace),
    ("audio.artist", Field::AudioArtist),
    ("audio.title", Field::AudioTitle),
    ("audio.album", Field::AudioAlbum),
    ("audio.track", Field::AudioTrack),
    ("audio.year", Field::AudioYear),
    ("pdf.title", Field::PdfTitle),
    ("pdf.author", Field::PdfAuthor),
    ("video.duration", Field::VideoDuration),
    ("mtime", Field::Mtime),
];

/// The conversions a date may use: all give digits, which every system takes in a name.
const CLOCK: &[u8] = b"YymdHMSj%";

impl Field {
    fn named(word: &str) -> Option<Field> {
        FIELDS.iter().find(|(n, _)| *n == word).map(|(_, f)| *f)
    }

    fn name(self) -> &'static str {
        FIELDS
            .iter()
            .find(|(_, f)| *f == self)
            .map_or("", |(n, _)| n)
    }

    fn shape(self) -> Shape {
        use Field::*;
        match self {
            ExifDate | ExifTime | Mtime => Shape::Moment,
            AudioTrack | AudioYear => Shape::Number,
            VideoDuration => Shape::Duration,
            _ => Shape::Text,
        }
    }

    /// The format a moment is written with when the pattern gives none.
    fn clock(self) -> &'static str {
        if self == Field::ExifTime {
            "%H%M%S"
        } else {
            "%Y-%m-%d"
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Format {
    Default,
    Clock(String),
    Width(usize),
}

/// One `{…}` made of fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FieldPart {
    alts: Vec<Field>,
    format: Format,
}

/// Whether `head` (the part of a token before `:`) names fields rather than `{name}` or `{n}`.
pub(super) fn is_field(head: &str) -> bool {
    head.contains('|') || head.split('|').any(|w| Field::named(w).is_some())
}

/// `head` is `field|field…`, `arg` what follows the `:`.
pub(super) fn parse(head: &str, arg: Option<&str>) -> Result<FieldPart, String> {
    let mut alts = Vec::new();
    for word in head.split('|') {
        let f = Field::named(word).ok_or_else(|| tf(Key::RenUnknownField, &[&word]))?;
        if let Some(first) = alts
            .first()
            .copied()
            .filter(|g: &Field| g.shape() != f.shape())
        {
            return Err(tf(Key::RenMixedFields, &[&first.name(), &f.name()]));
        }
        alts.push(f);
    }
    let format = match arg {
        None => Format::Default,
        Some(a) => format_for(alts[0], a)?,
    };
    Ok(FieldPart { alts, format })
}

fn format_for(f: Field, arg: &str) -> Result<Format, String> {
    match f.shape() {
        Shape::Moment => check_clock(arg).map(|_| Format::Clock(arg.to_string())),
        Shape::Number => super::parse_width(arg).map(Format::Width),
        Shape::Text | Shape::Duration => Err(tf(Key::RenNoFormat, &[&f.name()])),
    }
}

/// A date format may only use the conversions that give digits.
fn check_clock(arg: &str) -> Result<(), String> {
    if arg.is_empty() {
        return Err(tf(Key::RenBadClock, &[&arg]));
    }
    let mut bytes = arg.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' && !bytes.next().is_some_and(|c| CLOCK.contains(&c)) {
            return Err(tf(Key::RenBadClock, &[&arg]));
        }
    }
    Ok(())
}

impl FieldPart {
    /// Whether the file has to be read for it (only `{mtime}` does not).
    pub(super) fn needs_meta(&self) -> bool {
        self.alts.iter().any(|f| *f != Field::Mtime)
    }

    pub(super) fn needs_mtime(&self) -> bool {
        self.alts.contains(&Field::Mtime)
    }

    /// The text of the first field the item has, or why there is none.
    pub(super) fn render(&self, facts: &Facts) -> Result<String, String> {
        let empty = Meta::default();
        let meta = facts.meta.as_deref().unwrap_or(&empty);
        self.alts
            .iter()
            .find_map(|f| self.value(*f, meta, facts.mtime))
            .ok_or_else(|| tf(Key::RenNoValue, &[&self.written()]))
    }

    /// The token as it was written, for saying what is missing: `{exif.date|mtime}`.
    fn written(&self) -> String {
        let names: Vec<&str> = self.alts.iter().map(|f| f.name()).collect();
        format!("{{{}}}", names.join("|"))
    }

    fn value(&self, f: Field, meta: &Meta, mtime: Option<SystemTime>) -> Option<String> {
        match f.shape() {
            Shape::Moment => {
                let clock = match &self.format {
                    Format::Clock(c) => c.as_str(),
                    _ => f.clock(),
                };
                let at = moment(f, meta, mtime)?;
                jiff::fmt::strtime::format(clock, at).ok()
            }
            Shape::Number => {
                let n = number(f, meta)?;
                let width = match self.format {
                    Format::Width(w) => w,
                    _ => 1,
                };
                Some(format!("{n:0width$}"))
            }
            Shape::Duration => meta.video.duration.map(|d| duration(d.as_secs())),
            Shape::Text => text(f, meta).map(name_safe),
        }
    }
}

fn moment(f: Field, meta: &Meta, mtime: Option<SystemTime>) -> Option<DateTime> {
    let e = &meta.exif;
    match f {
        Field::ExifDate => e
            .date
            .map(|d| d.to_datetime(e.time.unwrap_or(Time::midnight()))),
        // A camera writes both; a time alone is put on the first day of the epoch.
        Field::ExifTime => e
            .time
            .map(|t| e.date.unwrap_or(Date::constant(1970, 1, 1)).to_datetime(t)),
        Field::Mtime => {
            let at = jiff::Timestamp::try_from(mtime?).ok()?;
            Some(at.to_zoned(jiff::tz::TimeZone::system()).datetime())
        }
        _ => None,
    }
}

fn number(f: Field, meta: &Meta) -> Option<i64> {
    match f {
        Field::AudioTrack => meta.audio.track.map(i64::from),
        Field::AudioYear => meta.audio.year.map(i64::from),
        _ => None,
    }
}

fn text(f: Field, meta: &Meta) -> Option<&str> {
    match f {
        Field::ExifCamera => meta.exif.camera.as_deref(),
        Field::ExifPlace => meta.exif.place.as_deref(),
        Field::AudioArtist => meta.audio.artist.as_deref(),
        Field::AudioTitle => meta.audio.title.as_deref(),
        Field::AudioAlbum => meta.audio.album.as_deref(),
        Field::PdfTitle => meta.pdf.title.as_deref(),
        Field::PdfAuthor => meta.pdf.author.as_deref(),
        _ => None,
    }
}

/// A length as a name can hold it (no `:`): `45s`, `4m05s`, `1h02m03s`.
fn duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    match (h, m) {
        (0, 0) => format!("{s}s"),
        (0, _) => format!("{m}m{s:02}s"),
        _ => format!("{h}h{m:02}m{s:02}s"),
    }
}

/// A text from the file, with what no system takes in a name put as `_`: an artist called
/// `AC/DC` must not make a folder, nor a title make the name unusable on another system.
fn name_safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::{Audio, Exif, Video};
    use std::time::Duration;

    fn photo() -> Meta {
        Meta {
            exif: Exif {
                date: Some(Date::constant(2024, 7, 3)),
                time: Some(Time::constant(18, 4, 5, 0)),
                camera: Some("Pixel 8".into()),
                ..Exif::default()
            },
            ..Meta::default()
        }
    }

    fn facts(meta: Meta) -> Facts {
        Facts {
            meta: Some(Arc::new(meta)),
            mtime: None,
        }
    }

    fn one(token: &str, facts: &Facts) -> Result<String, String> {
        let (head, arg) = match token.split_once(':') {
            Some((h, a)) => (h, Some(a)),
            None => (token, None),
        };
        parse(head, arg)?.render(facts)
    }

    #[test]
    fn a_photo_gives_its_day_its_time_and_its_camera() {
        let f = facts(photo());
        assert_eq!(one("exif.date", &f).unwrap(), "2024-07-03");
        assert_eq!(
            one("exif.date:%Y%m%d_%H%M%S", &f).unwrap(),
            "20240703_180405"
        );
        assert_eq!(one("exif.time", &f).unwrap(), "180405");
        assert_eq!(one("exif.time:%H-%M", &f).unwrap(), "18-04");
        assert_eq!(one("exif.camera", &f).unwrap(), "Pixel 8");
        assert_eq!(one("exif.date:%y %j %%", &f).unwrap(), "24 185 %");
    }

    #[test]
    fn a_missing_field_says_which_and_a_fallback_fills_it() {
        let f = Facts {
            meta: Some(Arc::new(Meta::default())),
            mtime: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 400)),
        };
        let e = one("exif.date", &f).unwrap_err();
        assert!(e.contains("{exif.date}"), "{e}");
        assert!(one("exif.place", &f).unwrap_err().contains("{exif.place}"));
        let day = one("exif.date|mtime:%Y", &f).unwrap();
        assert!(day == "1971" || day == "1970", "{day}");
        let e = one("pdf.title|audio.title", &f).unwrap_err();
        assert!(e.contains("{pdf.title|audio.title}"), "{e}");
    }

    #[test]
    fn songs_give_numbers_padded_and_texts_made_safe_for_a_name() {
        let f = facts(Meta {
            audio: Audio {
                artist: Some("AC/DC: \"Live\"".into()),
                track: Some(7),
                year: Some(1992),
                ..Audio::default()
            },
            ..Meta::default()
        });
        assert_eq!(one("audio.track:02", &f).unwrap(), "07");
        assert_eq!(one("audio.track", &f).unwrap(), "7");
        assert_eq!(one("audio.year", &f).unwrap(), "1992");
        assert_eq!(one("audio.artist", &f).unwrap(), "AC_DC_ _Live_");
    }

    #[test]
    fn a_video_gives_its_length_without_colons() {
        for (secs, shown) in [(45, "45s"), (245, "4m05s"), (3723, "1h02m03s")] {
            let f = facts(Meta {
                video: Video {
                    duration: Some(Duration::from_secs(secs)),
                    ..Video::default()
                },
                ..Meta::default()
            });
            assert_eq!(one("video.duration", &f).unwrap(), shown);
        }
    }

    #[test]
    fn wrong_tokens_are_refused_when_the_pattern_is_read() {
        let f = Facts::default();
        assert!(one("exif.datum", &f).unwrap_err().contains("exif.datum"));
        assert!(one("exif.date|", &f).is_err());
        assert!(
            one("exif.date|audio.title", &f)
                .unwrap_err()
                .contains("audio.title")
        );
        assert!(one("exif.date:%B", &f).unwrap_err().contains("%B"));
        assert!(one("exif.date:%", &f).is_err());
        assert!(one("exif.date:", &f).is_err());
        assert!(one("audio.track:x", &f).is_err());
        assert!(one("pdf.title:%Y", &f).unwrap_err().contains("pdf.title"));
        assert!(one("video.duration:2", &f).is_err());
    }
}
