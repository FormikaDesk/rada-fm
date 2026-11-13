//! The place a photo names, as written in its XMP packet or its IPTC record by a camera, a
//! phone or a cataloguing program. Nothing is worked out from coordinates.
//!
//! Both are looked for in the bytes already read from the start of the file: XMP is plain text
//! in every format that carries it, and IPTC sits in the JPEG segment of Photoshop's resources.

/// Where a photo was taken: the city (or the spot, or the region) and the country, e.g.
/// "Roma, Italia". `None` when the file names neither.
pub(super) fn of(head: &[u8]) -> Option<String> {
    let parts = xmp(head).filter(Parts::any).or_else(|| iptc(head))?;
    parts.join()
}

#[derive(Default)]
struct Parts {
    sublocation: Option<String>,
    city: Option<String>,
    state: Option<String>,
    country: Option<String>,
}

impl Parts {
    fn any(&self) -> bool {
        self.sublocation.is_some()
            || self.city.is_some()
            || self.state.is_some()
            || self.country.is_some()
    }

    fn join(self) -> Option<String> {
        let first = self.city.or(self.sublocation).or(self.state);
        let joined = match (first, self.country) {
            (Some(a), Some(c)) if !a.eq_ignore_ascii_case(&c) => format!("{a}, {c}"),
            (Some(a), _) => a,
            (None, Some(c)) => c,
            (None, None) => return None,
        };
        super::tidy(&joined)
    }
}

// ------------------------------------------------------------------------------------- XMP

/// The fields of the first XMP packet in `head`.
fn xmp(head: &[u8]) -> Option<Parts> {
    let start = find(head, b"<x:xmpmeta")?;
    let len = find(&head[start..], b"</x:xmpmeta>")?;
    let text = String::from_utf8_lossy(&head[start..start + len]);
    Some(Parts {
        sublocation: property(&text, "Iptc4xmpCore:Location"),
        city: property(&text, "photoshop:City"),
        state: property(&text, "photoshop:State"),
        country: property(&text, "photoshop:Country"),
    })
}

/// A simple property, written either as an attribute (`name="value"`) or as an element
/// (`<name>value</name>`).
fn property(xml: &str, name: &str) -> Option<String> {
    let attr = format!("{name}=\"");
    if let Some(at) = xml.find(&attr) {
        let rest = &xml[at + attr.len()..];
        return clean(&rest[..rest.find('"')?]);
    }
    let open = format!("<{name}>");
    let at = xml.find(&open)?;
    let rest = &xml[at + open.len()..];
    clean(&rest[..rest.find('<')?])
}

/// The text of an XML value, its entities decoded.
fn clean(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';').filter(|s| *s <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        out.push_str(&entity(&rest[1..semi]).unwrap_or_else(|| rest[..=semi].to_string()));
        rest = &rest[semi + 1..];
    }
    out.push_str(rest);
    let out = out.trim();
    (!out.is_empty()).then(|| out.to_string())
}

fn entity(name: &str) -> Option<String> {
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => {
            let code = match name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => name.strip_prefix('#')?.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some(c.to_string())
}

// ------------------------------------------------------------------------------------ IPTC

/// The fields of the IPTC record in Photoshop's resources (a JPEG's APP13 segment).
fn iptc(head: &[u8]) -> Option<Parts> {
    let start = find(head, b"Photoshop 3.0\0")? + 14;
    let data = iptc_block(&head[start..])?;
    let mut parts = Parts::default();
    let mut at = 0;
    // Each dataset: 0x1C, record, number, a two-byte length, the value.
    while at + 5 <= data.len() && data[at] == 0x1C {
        let (record, number) = (data[at + 1], data[at + 2]);
        let len = u16::from_be_bytes([data[at + 3], data[at + 4]]) as usize;
        let value = data.get(at + 5..at + 5 + len)?;
        let slot = match (record, number) {
            (2, 90) => Some(&mut parts.city),
            (2, 92) => Some(&mut parts.sublocation),
            (2, 95) => Some(&mut parts.state),
            (2, 101) => Some(&mut parts.country),
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = text(value);
        }
        at += 5 + len;
    }
    parts.any().then_some(parts)
}

/// The IPTC resource (id 0x0404) among Photoshop's resources.
fn iptc_block(mut res: &[u8]) -> Option<&[u8]> {
    // Each resource: "8BIM", an id, a name padded to an even length, a four-byte size, the
    // data padded to an even length.
    while res.len() >= 12 && res.starts_with(b"8BIM") {
        let id = u16::from_be_bytes([res[4], res[5]]);
        let name_len = res[6] as usize;
        let name_total = (1 + name_len).next_multiple_of(2);
        let size_at = 6 + name_total;
        let size = res.get(size_at..size_at + 4)?;
        let size = u32::from_be_bytes([size[0], size[1], size[2], size[3]]) as usize;
        let data_at = size_at + 4;
        let data = res.get(data_at..data_at.checked_add(size)?)?;
        if id == 0x0404 {
            return Some(data);
        }
        res = res.get(data_at + size.next_multiple_of(2)..)?;
    }
    None
}

/// An IPTC value: UTF-8 when it is valid UTF-8, else Latin-1, the older default.
fn text(raw: &[u8]) -> Option<String> {
    let s = match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    };
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xmp_attributes_and_elements_are_both_read() {
        let attrs = br#"junk<x:xmpmeta><rdf:Description photoshop:City="Roma" photoshop:Country="Italia"/></x:xmpmeta>"#;
        assert_eq!(of(attrs).as_deref(), Some("Roma, Italia"));
        let elems = b"<x:xmpmeta><photoshop:City>Saint-Malo &amp; co</photoshop:City></x:xmpmeta>";
        assert_eq!(of(elems).as_deref(), Some("Saint-Malo & co"));
    }

    #[test]
    fn a_country_alone_or_a_spot_is_a_place_too() {
        let country = br#"<x:xmpmeta photoshop:Country="Norge"></x:xmpmeta>"#;
        assert_eq!(of(country).as_deref(), Some("Norge"));
        let spot = br#"<x:xmpmeta Iptc4xmpCore:Location="Colosseo"></x:xmpmeta>"#;
        assert_eq!(of(spot).as_deref(), Some("Colosseo"));
    }

    #[test]
    fn an_unclosed_packet_or_none_gives_no_place() {
        assert_eq!(of(br#"<x:xmpmeta photoshop:City="Roma""#), None);
        assert_eq!(of(b"just the bytes of a picture"), None);
    }

    #[test]
    fn iptc_city_and_country_in_latin_1() {
        let mut iim = Vec::new();
        for (n, v) in [(90u8, &b"M\xfcnchen"[..]), (101, b"Deutschland")] {
            iim.extend([0x1C, 2, n]);
            iim.extend((v.len() as u16).to_be_bytes());
            iim.extend(v);
        }
        let mut head = b"\xff\xed\x00\x00Photoshop 3.0\0".to_vec();
        head.extend(b"8BIM\x03\xed\0\0\0\0\0\x02xx"); // another resource first
        head.extend(b"8BIM\x04\x04\0\0");
        head.extend((iim.len() as u32).to_be_bytes());
        head.extend(&iim);
        assert_eq!(of(&head).as_deref(), Some("München, Deutschland"));
    }

    #[test]
    fn a_cut_iptc_record_gives_no_place_and_no_panic() {
        let mut head = b"Photoshop 3.0\08BIM\x04\x04\0\0\0\0\0\x40".to_vec();
        head.extend([0x1C, 2, 90, 0, 50, b'R']);
        assert_eq!(of(&head), None);
        assert_eq!(of(b"Photoshop 3.0\08BIM\x04\x04\0\xff\xff\xff"), None);
    }
}
