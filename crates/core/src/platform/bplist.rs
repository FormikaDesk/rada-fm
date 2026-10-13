//! Just enough of the binary property list format (`bplist00`) to read and write a list of
//! strings: what the Finder keeps its tags in. Other kinds of values are not understood (and
//! are not guessed at). An XML property list with `<string>` items is read too, since some
//! programs write tags that way.

/// A list of strings as a `bplist00` file.
pub fn encode_string_array(items: &[String]) -> Vec<u8> {
    let n = items.len();
    // Object 0 is the array, objects 1.. are the strings.
    let mut body: Vec<Vec<u8>> = Vec::with_capacity(n + 1);
    let ref_size: usize = if n + 1 > 0xff { 2 } else { 1 };
    let mut array = Vec::new();
    write_marker(&mut array, 0xA0, n);
    for i in 0..n {
        write_uint(&mut array, (i + 1) as u64, ref_size);
    }
    body.push(array);
    for s in items {
        let mut obj = Vec::new();
        if s.is_ascii() {
            write_marker(&mut obj, 0x50, s.len());
            obj.extend_from_slice(s.as_bytes());
        } else {
            let units: Vec<u16> = s.encode_utf16().collect();
            write_marker(&mut obj, 0x60, units.len());
            for u in units {
                obj.extend_from_slice(&u.to_be_bytes());
            }
        }
        body.push(obj);
    }
    let mut out = b"bplist00".to_vec();
    let mut offsets = Vec::with_capacity(body.len());
    for obj in &body {
        offsets.push(out.len() as u64);
        out.extend_from_slice(obj);
    }
    let table_at = out.len() as u64;
    let offset_size: usize = if table_at > 0xffff_ffff {
        8
    } else if table_at > 0xffff {
        4
    } else if table_at > 0xff {
        2
    } else {
        1
    };
    for o in &offsets {
        write_uint(&mut out, *o, offset_size);
    }
    out.extend_from_slice(&[0u8; 6]);
    out.push(offset_size as u8);
    out.push(ref_size as u8);
    out.extend_from_slice(&(body.len() as u64).to_be_bytes());
    out.extend_from_slice(&0u64.to_be_bytes()); // the array is the top object
    out.extend_from_slice(&table_at.to_be_bytes());
    out
}

/// A marker byte with a length: the length goes in the low nibble, or after a `0xF` there as
/// an integer object when it is 15 or more.
fn write_marker(out: &mut Vec<u8>, base: u8, len: usize) {
    if len < 15 {
        out.push(base | len as u8);
    } else {
        out.push(base | 0x0F);
        // An integer object: 0x10 | log2(size), then the number.
        if len <= 0xff {
            out.push(0x10);
            out.push(len as u8);
        } else if len <= 0xffff {
            out.push(0x11);
            out.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            out.push(0x12);
            out.extend_from_slice(&(len as u32).to_be_bytes());
        }
    }
}

fn write_uint(out: &mut Vec<u8>, value: u64, size: usize) {
    out.extend_from_slice(&value.to_be_bytes()[8 - size..]);
}

fn read_uint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    Some(bytes.iter().fold(0u64, |acc, b| (acc << 8) | *b as u64))
}

/// The strings of a property list that is an array of strings: binary, or XML.
pub fn decode_string_array(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.starts_with(b"bplist00") {
        decode_binary(bytes)
    } else {
        decode_xml(bytes)
    }
}

fn decode_binary(b: &[u8]) -> Option<Vec<String>> {
    if b.len() < 8 + 32 {
        return None;
    }
    let trailer = &b[b.len() - 32..];
    let offset_size = trailer[6] as usize;
    let ref_size = trailer[7] as usize;
    let count = read_uint(&trailer[8..16])? as usize;
    let top = read_uint(&trailer[16..24])? as usize;
    let table_at = read_uint(&trailer[24..32])? as usize;
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) || top >= count {
        return None;
    }
    let table_end = table_at.checked_add(count.checked_mul(offset_size)?)?;
    if table_end > b.len() - 32 {
        return None;
    }
    let offset_of = |i: usize| -> Option<usize> {
        let at = table_at + i * offset_size;
        read_uint(b.get(at..at + offset_size)?).map(|o| o as usize)
    };
    // The length of an object whose marker is at `at`: the nibble, or an integer object.
    let length = |at: usize| -> Option<(usize, usize)> {
        let nibble = (*b.get(at)? & 0x0F) as usize;
        if nibble < 15 {
            return Some((nibble, at + 1));
        }
        let int = *b.get(at + 1)?;
        if int & 0xF0 != 0x10 {
            return None;
        }
        let size = 1usize << (int & 0x0F);
        let n = read_uint(b.get(at + 2..at + 2 + size)?)? as usize;
        Some((n, at + 2 + size))
    };
    let top_at = offset_of(top)?;
    if b.get(top_at)? & 0xF0 != 0xA0 {
        return None;
    }
    let (n_items, refs_at) = length(top_at)?;
    if n_items > count {
        return None;
    }
    let mut out = Vec::with_capacity(n_items);
    for i in 0..n_items {
        let at = refs_at + i * ref_size;
        let r = read_uint(b.get(at..at + ref_size)?)? as usize;
        if r >= count {
            return None;
        }
        let obj = offset_of(r)?;
        let marker = *b.get(obj)?;
        let (len, data) = length(obj)?;
        match marker & 0xF0 {
            // ASCII (and, loosely, UTF-8) string
            0x50 => out.push(String::from_utf8(b.get(data..data + len)?.to_vec()).ok()?),
            // UTF-16, big endian
            0x60 => {
                let raw = b.get(data..data + len * 2)?;
                let units: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                out.push(String::from_utf16(&units).ok()?);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// `<array><string>a</string><string>b</string></array>`, with the usual entities.
fn decode_xml(b: &[u8]) -> Option<Vec<String>> {
    let text = std::str::from_utf8(b).ok()?;
    let start = text.find("<array")?;
    let end = text.rfind("</array>")?;
    if end < start {
        return None;
    }
    let mut out = Vec::new();
    let mut rest = &text[start..end];
    while let Some(i) = rest.find("<string>") {
        let after = &rest[i + "<string>".len()..];
        let close = after.find("</string>")?;
        out.push(unescape(&after[..close]));
        rest = &after[close + "</string>".len()..];
    }
    // `<string/>` is an empty one; anything but strings makes this not a tag list.
    if rest.contains("<dict") || rest.contains("<integer") || rest.contains("<data") {
        return None;
    }
    Some(out)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(items: &[&str]) {
        let items: Vec<String> = items.iter().map(|s| s.to_string()).collect();
        let bytes = encode_string_array(&items);
        assert_eq!(decode_string_array(&bytes).as_deref(), Some(&items[..]), "{items:?}");
    }

    #[test]
    fn lists_of_strings_round_trip() {
        rt(&[]);
        rt(&["Red\n6"]);
        rt(&["a", "b", "c"]);
        rt(&["Città", "日本語", "emoji 🌴 ok"]);
        // Lengths of 15 and more need the long form.
        rt(&["a name of exactly fifteen"[..15].trim_end(), &"x".repeat(15), &"y".repeat(300)]);
        rt(&[&"é".repeat(40)]);
        // More than 14 items: the count of the array needs the long form too.
        let many: Vec<String> = (0..40).map(|i| format!("tag {i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        rt(&refs);
    }

    #[test]
    fn the_file_has_the_layout_of_a_property_list() {
        let b = encode_string_array(&["Red\n6".into()]);
        assert!(b.starts_with(b"bplist00"));
        // An array of one, then a 5-byte ASCII string.
        assert_eq!(b[8], 0xA1);
        assert_eq!(b[8 + 1 + 1], 0x55);
        // The trailer: offset size 1, reference size 1, two objects, the array on top.
        let t = &b[b.len() - 32..];
        assert_eq!((t[6], t[7]), (1, 1));
        assert_eq!(u64::from_be_bytes(t[8..16].try_into().unwrap()), 2);
        assert_eq!(u64::from_be_bytes(t[16..24].try_into().unwrap()), 0);
    }

    /// A list the way macOS writes it for `Red\n6` and `Work` (offset and reference size 1).
    #[test]
    fn a_list_written_by_the_system_is_read() {
        let mut b = b"bplist00".to_vec();
        b.extend_from_slice(&[0xA2, 0x01, 0x02]); // array of two: objects 1, 2
        b.extend_from_slice(&[0x55]);
        b.extend_from_slice(b"Red\n6");
        b.extend_from_slice(&[0x54]);
        b.extend_from_slice(b"Work");
        let table_at = b.len() as u64;
        b.extend_from_slice(&[8, 11, 17]); // offsets of the three objects
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&[1, 1]);
        b.extend_from_slice(&3u64.to_be_bytes());
        b.extend_from_slice(&0u64.to_be_bytes());
        b.extend_from_slice(&table_at.to_be_bytes());
        assert_eq!(
            decode_string_array(&b).unwrap(),
            ["Red\n6".to_string(), "Work".to_string()]
        );
    }

    #[test]
    fn xml_lists_are_read_with_their_entities() {
        let x = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><array><string>Red&#10;6</string><string>R&amp;D</string><string>a&lt;b</string></array></plist>"#;
        // (A numeric entity is not handled: that item reads as written, which is harmless.)
        let got = decode_string_array(x).unwrap();
        assert_eq!(got[1], "R&D");
        assert_eq!(got[2], "a<b");
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn things_that_are_not_lists_of_strings_are_refused_not_guessed() {
        assert!(decode_string_array(b"").is_none());
        assert!(decode_string_array(b"bplist00").is_none());
        assert!(decode_string_array(b"bplist00 not really").is_none());
        assert!(decode_string_array(b"hello").is_none());
        assert!(decode_string_array(b"<plist><dict><key>a</key><string>b</string></dict></plist>").is_none());
        // A truncated real one.
        let good = encode_string_array(&["Red\n6".into(), "Work".into()]);
        for cut in 1..good.len() {
            let _ = decode_string_array(&good[..cut]); // never panics
        }
        assert!(decode_string_array(&good[..good.len() - 5]).is_none());
        // An array that claims more items than there are objects.
        let mut bad = good.clone();
        bad[8] = 0xAF;
        assert!(decode_string_array(&bad).is_none());
    }
}
