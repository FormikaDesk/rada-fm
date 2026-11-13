//! Small media files made byte by byte for the tests: a photo with EXIF, XMP and nothing
//! else, a song with ID3 or Vorbis tags, a PDF with an information dictionary. Not one of
//! them holds a picture or a sound: none is needed to read what they say about themselves.

use std::io::Cursor;

use exif::experimental::Writer;
use exif::{Field, In, Rational, Tag, Value};

fn ascii(tag: Tag, ifd: In, s: &str) -> Field {
    Field {
        tag,
        ifd_num: ifd,
        value: Value::Ascii(vec![s.as_bytes().to_vec()]),
    }
}

fn rationals(tag: Tag, v: &[(u32, u32)]) -> Field {
    Field {
        tag,
        ifd_num: In::PRIMARY,
        value: Value::Rational(v.iter().map(|&(n, d)| Rational::from((n, d))).collect()),
    }
}

/// The EXIF of a Canon photo taken in Rome on 2024-05-01 at 14:30:05.
pub fn exif_block() -> Vec<u8> {
    let fields = [
        ascii(Tag::Make, In::PRIMARY, "Canon"),
        ascii(Tag::Model, In::PRIMARY, "Canon EOS R6"),
        Field {
            tag: Tag::Orientation,
            ifd_num: In::PRIMARY,
            value: Value::Short(vec![6]),
        },
        ascii(Tag::DateTimeOriginal, In::PRIMARY, "2024:05:01 14:30:05"),
        ascii(Tag::GPSLatitudeRef, In::PRIMARY, "N"),
        rationals(Tag::GPSLatitude, &[(41, 1), (54, 1), (10, 1)]),
        ascii(Tag::GPSLongitudeRef, In::PRIMARY, "E"),
        rationals(Tag::GPSLongitude, &[(12, 1), (29, 1), (2, 1)]),
    ];
    let mut w = Writer::new();
    for f in &fields {
        w.push_field(f);
    }
    let mut out = Cursor::new(Vec::new());
    w.write(&mut out, false).unwrap();
    out.into_inner()
}

/// A JPEG segment: its marker, its length, its bytes.
pub fn segment(marker: u8, body: &[u8]) -> Vec<u8> {
    let mut s = vec![0xFF, marker];
    s.extend(((body.len() + 2) as u16).to_be_bytes());
    s.extend(body);
    s
}

/// A JPEG with these segments after the start (no picture: none is needed to read them).
pub fn jpeg(segments: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    for s in segments {
        out.extend(s);
    }
    out.extend([0xFF, 0xD9]);
    out
}

pub fn exif_segment() -> Vec<u8> {
    let mut body = b"Exif\0\0".to_vec();
    body.extend(exif_block());
    segment(0xE1, &body)
}

pub fn xmp_segment(place: &str) -> Vec<u8> {
    let mut body = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    body.extend(
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description {place}/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
        )
        .bytes(),
    );
    segment(0xE1, &body)
}

/// An ID3v2.4 text frame in UTF-8.
fn id3_frame(id: &str, text: &str) -> Vec<u8> {
    let mut body = vec![3u8];
    body.extend(text.as_bytes());
    let mut f = id.as_bytes().to_vec();
    f.extend(syncsafe(body.len()));
    f.extend([0, 0]);
    f.extend(body);
    f
}

fn syncsafe(n: usize) -> [u8; 4] {
    let n = n as u32;
    [
        (n >> 21) as u8 & 0x7F,
        (n >> 14) as u8 & 0x7F,
        (n >> 7) as u8 & 0x7F,
        n as u8 & 0x7F,
    ]
}

/// A few silent MPEG-1 layer III frames (128 kbit/s, 44.1 kHz).
pub fn mpeg_frames() -> Vec<u8> {
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x64];
    frame.resize(417, 0);
    frame.repeat(4)
}

pub fn mp3(frames: &[(&str, &str)]) -> Vec<u8> {
    let body: Vec<u8> = frames.iter().flat_map(|(id, t)| id3_frame(id, t)).collect();
    let mut out = b"ID3\x04\x00\x00".to_vec();
    out.extend(syncsafe(body.len()));
    out.extend(body);
    out.extend(mpeg_frames());
    out
}

/// A FLAC with its stream information and a block of Vorbis comments.
pub fn flac(comments: &[&str]) -> Vec<u8> {
    let mut out = b"fLaC".to_vec();
    out.extend([0x00, 0, 0, 34]);
    let mut info = vec![0x10, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0, 0];
    // 44100 Hz, 2 channels, 16 bits, 44100 samples; then the MD5 of the sound, unknown.
    info.extend([0x0A, 0xC4, 0x42, 0xF0, 0x00, 0x00, 0xAC, 0x44]);
    info.extend([0u8; 16]);
    out.extend(info);
    let mut block = Vec::new();
    let vendor = b"rada test";
    block.extend((vendor.len() as u32).to_le_bytes());
    block.extend(vendor);
    block.extend((comments.len() as u32).to_le_bytes());
    for c in comments {
        block.extend((c.len() as u32).to_le_bytes());
        block.extend(c.as_bytes());
    }
    out.push(0x84);
    out.extend(&(block.len() as u32).to_be_bytes()[1..]);
    out.extend(block);
    out.extend([0xFF, 0xF8, 0x69, 0x08, 0, 0, 0, 0]);
    out
}

/// A one-page PDF with an information dictionary.
pub fn pdf(info: &str) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>".to_string(),
        info.to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let x = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offs {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    let n = objs.len();
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info {n} 0 R >>\nstartxref\n{x}\n%%EOF\n",
            n + 1
        )
        .bytes(),
    );
    out
}
