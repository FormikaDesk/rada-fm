//! The EXIF of a photo, and the place its XMP or IPTC names, from the start of the file only.

use std::fs::File;
use std::io::{Cursor, Read};

use exif::{In, Tag, Value};

use super::{Exif, Gps, place, tidy};

/// What the first `prefix` bytes of `file` say. EXIF and XMP sit near the start of every
/// format that has them; a photo that keeps them further on is simply not read.
pub(super) fn read(file: File, prefix: usize) -> Exif {
    let mut head = Vec::new();
    if file.take(prefix as u64).read_to_end(&mut head).is_err() {
        return Exif::default();
    }
    let mut found = match exif::Reader::new().read_from_container(&mut Cursor::new(&head)) {
        Ok(e) => from_exif(&e),
        Err(_) => Exif::default(),
    };
    found.place = place::of(&head);
    found
}

fn from_exif(e: &exif::Exif) -> Exif {
    let field = |tag| e.get_field(tag, In::PRIMARY).map(|f| &f.value);
    let taken = [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime]
        .into_iter()
        .find_map(|t| field(t).and_then(date_time));
    Exif {
        date: taken.map(|t| t.date()),
        time: taken.map(|t| t.time()),
        camera: camera(field(Tag::Make), field(Tag::Model)),
        orientation: field(Tag::Orientation)
            .and_then(|v| v.get_uint(0))
            .and_then(|o| u16::try_from(o).ok())
            .filter(|o| (1..=8).contains(o)),
        gps: gps(e),
        place: None,
    }
}

/// The first string of an ASCII value.
fn ascii(v: &Value) -> Option<&[u8]> {
    match v {
        Value::Ascii(parts) => parts.first().map(Vec::as_slice),
        _ => None,
    }
}

/// A date and time of the camera's clock; nothing when it is blank or not a real date.
fn date_time(v: &Value) -> Option<jiff::civil::DateTime> {
    let t = exif::DateTime::from_ascii(ascii(v)?).ok()?;
    jiff::civil::DateTime::new(
        i16::try_from(t.year).ok()?,
        i8::try_from(t.month).ok()?,
        i8::try_from(t.day).ok()?,
        i8::try_from(t.hour).ok()?,
        i8::try_from(t.minute).ok()?,
        i8::try_from(t.second).ok()?,
        0,
    )
    .ok()
}

/// "Canon EOS R6" from "Canon" and "Canon EOS R6"; "NIKON Z 6" from "NIKON CORPORATION" and
/// "Z 6": the maker's first word comes first unless the model already names it.
fn camera(make: Option<&Value>, model: Option<&Value>) -> Option<String> {
    let text = |v: Option<&Value>| {
        v.and_then(ascii)
            .map(|b| String::from_utf8_lossy(b).trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let (make, model) = (text(make), text(model));
    let joined = match (make, model) {
        (Some(make), Some(model)) => {
            let brand = make.split_whitespace().next().unwrap_or(&make).to_string();
            if model.to_lowercase().starts_with(&brand.to_lowercase()) {
                model
            } else {
                format!("{brand} {model}")
            }
        }
        (make, model) => model.or(make)?,
    };
    tidy(&joined)
}

/// The position, when both coordinates and their hemispheres are there and make sense.
fn gps(e: &exif::Exif) -> Option<Gps> {
    let coord = |value: Tag, side: Tag, negative: u8| {
        let v = e.get_field(value, In::PRIMARY)?;
        let Value::Rational(parts) = &v.value else {
            return None;
        };
        if parts.len() < 3 || parts.iter().take(3).any(|r| r.denom == 0) {
            return None;
        }
        let deg = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
        let s = e
            .get_field(side, In::PRIMARY)
            .and_then(|f| ascii(&f.value))?;
        Some(if s.first() == Some(&negative) {
            -deg
        } else {
            deg
        })
    };
    let lat = coord(Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S')?;
    let lon = coord(Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W')?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some(Gps { lat, lon })
}
