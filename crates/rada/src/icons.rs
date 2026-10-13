//! The icon files of the systems that want their own container: `.icns` for macOS and `.ico`
//! for Windows. Both hold PNG pictures, so all that is written here is the container.
//! `examples/make-icons.rs` draws the logo at every size and writes the two files into
//! `assets/`, which the program then embeds.

// The builders are for the example and the tests; the program only embeds the files.
#![allow(dead_code)]

/// The sizes an `.icns` file carries, with the type code of each entry. The 2× entries
/// repeat a size under another code (a 32-pixel picture is both `icp5` and the 2× of 16).
pub const ICNS_ENTRIES: &[(&[u8; 4], u32)] = &[
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic13", 256),
    (b"ic14", 512),
];

/// The pixel sizes needed to fill [`ICNS_ENTRIES`], once each.
pub fn icns_sizes() -> Vec<u32> {
    let mut v: Vec<u32> = ICNS_ENTRIES.iter().map(|e| e.1).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The sizes of a Windows icon: the ones the shell, the taskbar and the title bar ask for.
pub const ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];

/// An `.icns` file from PNG pictures by pixel size.
pub fn build_icns(png_for: &dyn Fn(u32) -> Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    for (code, size) in ICNS_ENTRIES {
        let png = png_for(*size).ok_or_else(|| format!("no {size}-pixel picture"))?;
        body.extend_from_slice(*code);
        body.extend_from_slice(&((png.len() as u32 + 8).to_be_bytes()));
        body.extend_from_slice(&png);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&((body.len() as u32 + 8).to_be_bytes()));
    out.extend_from_slice(&body);
    Ok(out)
}

/// An `.ico` file from PNG pictures by pixel size (the format Windows has read since Vista;
/// a side of 256 is written as 0).
pub fn build_ico(png_for: &dyn Fn(u32) -> Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut pics: Vec<(u32, Vec<u8>)> = Vec::new();
    for size in ICO_SIZES {
        pics.push((
            *size,
            png_for(*size).ok_or_else(|| format!("no {size}-pixel picture"))?,
        ));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(pics.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pics.len() as u32;
    for (size, png) in &pics {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &pics {
        out.extend_from_slice(png);
    }
    Ok(out)
}

/// What an `.icns` holds: the type code and the length of each entry's data, in order.
pub fn icns_entries(file: &[u8]) -> Option<Vec<([u8; 4], usize)>> {
    if file.get(..4)? != b"icns" {
        return None;
    }
    let total = u32::from_be_bytes(file.get(4..8)?.try_into().ok()?) as usize;
    if total != file.len() {
        return None;
    }
    let mut out = Vec::new();
    let mut at = 8;
    while at < file.len() {
        let code: [u8; 4] = file.get(at..at + 4)?.try_into().ok()?;
        let len = u32::from_be_bytes(file.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        if len < 8 || at + len > file.len() {
            return None;
        }
        out.push((code, len - 8));
        at += len;
    }
    Some(out)
}

/// What an `.ico` holds: side (0 means 256), bits per pixel and the length of each picture.
pub fn ico_entries(file: &[u8]) -> Option<Vec<(u8, u16, usize)>> {
    if file.get(..4)? != [0, 0, 1, 0] {
        return None;
    }
    let n = u16::from_le_bytes(file.get(4..6)?.try_into().ok()?) as usize;
    let mut out = Vec::new();
    for i in 0..n {
        let e = file.get(6 + 16 * i..6 + 16 * (i + 1))?;
        let len = u32::from_le_bytes(e[8..12].try_into().ok()?) as usize;
        let off = u32::from_le_bytes(e[12..16].try_into().ok()?) as usize;
        file.get(off..off + len)?;
        out.push((e[0], u16::from_le_bytes([e[6], e[7]]), len));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_png(size: u32) -> Option<Vec<u8>> {
        // The container does not look inside: a signature and the size will do.
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(&size.to_be_bytes());
        Some(v)
    }

    #[test]
    fn an_icns_has_every_size_under_its_code_and_a_correct_length() {
        let f = build_icns(&fake_png).unwrap();
        let entries = icns_entries(&f).expect("a well-formed file");
        assert_eq!(entries.len(), ICNS_ENTRIES.len());
        assert_eq!(&entries[0].0, b"icp4");
        assert_eq!(&entries[6].0, b"ic10");
        assert!(entries.iter().all(|(_, len)| *len == 12));
        assert_eq!(icns_sizes(), [16, 32, 64, 128, 256, 512, 1024]);
        // A missing size is an error naming it.
        let e = build_icns(&|s| (s != 128).then(|| vec![1])).unwrap_err();
        assert!(e.contains("128"), "{e}");
    }

    #[test]
    fn an_ico_has_every_size_and_points_at_its_pictures() {
        let f = build_ico(&fake_png).unwrap();
        let entries = ico_entries(&f).expect("a well-formed file");
        let sides: Vec<u8> = entries.iter().map(|e| e.0).collect();
        assert_eq!(sides, [16, 24, 32, 48, 64, 128, 0]);
        assert!(entries.iter().all(|e| e.1 == 32 && e.2 == 12));
        assert!(ico_entries(b"nope").is_none());
    }

    #[test]
    fn the_shipped_icons_are_the_logo_at_every_size() {
        let icns = include_bytes!("../assets/macos/rada.icns");
        let entries = icns_entries(icns).expect("assets/macos/rada.icns is well formed");
        assert_eq!(entries.len(), ICNS_ENTRIES.len());
        let ico = include_bytes!("../assets/windows/rada.ico");
        let entries = ico_entries(ico).expect("assets/windows/rada.ico is well formed");
        assert_eq!(entries.len(), ICO_SIZES.len());
        // Every picture in them is a PNG.
        let mut at = 8;
        while at < icns.len() {
            let len = u32::from_be_bytes(icns[at + 4..at + 8].try_into().unwrap()) as usize;
            assert_eq!(&icns[at + 8..at + 16], b"\x89PNG\r\n\x1a\n");
            at += len;
        }
    }
}
