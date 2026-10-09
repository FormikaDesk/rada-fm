//! Image previews: header first, pixels later, never on the caller's thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

use rada_core::events::{CoreEvent, PreviewEvent};
use rada_core::fs::LocalFs;
use rada_core::preview::{ImageLimits, ImageState, Limits, Preview};
use rada_core::testutil::*;
use rada_core::workers::PreviewWorker;

fn worker() -> (PreviewWorker, Receiver<CoreEvent>) {
    let (tx, rx) = unbounded();
    (PreviewWorker::spawn(Arc::new(LocalFs), tx), rx)
}

fn next_for(rx: &Receiver<CoreEvent>, generation: u64, secs: u64) -> PreviewEvent {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(CoreEvent::Preview(p)) if p.generation == generation => return p,
            Ok(_) => {}
            Err(_) => panic!("no preview event for generation {generation}"),
        }
    }
}

/// Wait for the final state of an image preview (skipping the header-only `Loading`).
fn settled(rx: &Receiver<CoreEvent>, generation: u64) -> rada_core::preview::ImagePreview {
    loop {
        let ev = next_for(rx, generation, 20);
        match ev.preview {
            Preview::Image(i) if matches!(i.state, ImageState::Loading) => continue,
            Preview::Image(i) => return i,
            other => panic!("expected an image, got {other:?}"),
        }
    }
}

fn gradient(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 255 / w.max(1)) as u8,
            (y * 255 / h.max(1)) as u8,
            128,
            255,
        ])
    }))
}

fn save(sb: &Sandbox, name: &str, img: &DynamicImage, fmt: ImageFormat) -> PathBuf {
    let p = sb.path(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let img = if fmt == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(img.to_rgb8())
    } else {
        img.clone()
    };
    img.save_with_format(&p, fmt).unwrap();
    p
}

fn request(w: &PreviewWorker, p: &Path, generation: u64, limits: Limits) {
    w.request(p.to_path_buf(), generation, limits);
}

#[test]
fn every_supported_format_gives_header_info_first_then_pixels() {
    let sb = Sandbox::new();
    let img = gradient(64, 48);
    let cases = [
        ("a.png", ImageFormat::Png, "PNG"),
        ("b.jpg", ImageFormat::Jpeg, "JPEG"),
        ("c.gif", ImageFormat::Gif, "GIF"),
        ("d.webp", ImageFormat::WebP, "WebP"),
        ("e.bmp", ImageFormat::Bmp, "BMP"),
    ];
    let (w, rx) = worker();
    for (i, (name, fmt, label)) in cases.iter().enumerate() {
        let p = save(&sb, name, &img, *fmt);
        let g = i as u64 + 1;
        request(&w, &p, g, Limits::default());
        // First answer: header only, immediately.
        let first = next_for(&rx, g, 5);
        let Preview::Image(h) = first.preview else {
            panic!("{name}: {:?}", first.preview)
        };
        assert_eq!(h.info.format, *label);
        assert_eq!(
            (h.info.width, h.info.height),
            (Some(64), Some(48)),
            "{name}"
        );
        assert_eq!(h.info.size, std::fs::metadata(&p).unwrap().len());
        assert!(h.info.modified.is_some());
        assert!(
            matches!(h.state, ImageState::Loading),
            "{name}: {:?}",
            h.state
        );
        // Then the decoded pixels.
        let done = settled(&rx, g);
        let ImageState::Ready(px) = done.state else {
            panic!("{name}: {:?}", done.state)
        };
        assert_eq!((px.width(), px.height()), (64, 48), "{name}");
    }
}

/// Regression: the pixels were handed to the decoding thread before the header-only answer
/// was sent, so under load the decoded picture could arrive first and the header (still
/// "loading") after it. The header must always come first, whatever the machine is doing.
#[test]
fn the_header_always_comes_before_the_pixels_even_when_decoding_is_instant() {
    let sb = Sandbox::new();
    // A tiny picture decodes in microseconds: the best chance for the pixels to win.
    let p = save(&sb, "tiny.png", &gradient(4, 4), ImageFormat::Png);
    let (w, rx) = worker();
    // Keep every core busy meanwhile, as a parallel test run does.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let burners: Vec<_> = (0..std::thread::available_parallelism().map_or(4, |n| n.get()) * 2)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut x = 1u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                    std::hint::black_box(x);
                }
            })
        })
        .collect();
    for g in 1..=150u64 {
        request(&w, &p, g, Limits::default());
        let first = next_for(&rx, g, 10);
        let Preview::Image(h) = first.preview else {
            panic!("generation {g}: {:?}", first.preview)
        };
        assert!(
            matches!(h.state, ImageState::Loading),
            "generation {g}: the first answer must be the header, not {:?}",
            h.state
        );
        let done = settled(&rx, g);
        assert!(matches!(done.state, ImageState::Ready(_)), "generation {g}");
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for b in burners {
        b.join().unwrap();
    }
}

#[test]
fn a_gif_shows_its_first_frame() {
    let sb = Sandbox::new();
    let p = sb.path("anim.gif");
    {
        use image::codecs::gif::GifEncoder;
        let mut enc = GifEncoder::new(std::fs::File::create(&p).unwrap());
        let red = RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 255, 255]));
        enc.encode_frame(image::Frame::new(red)).unwrap();
        enc.encode_frame(image::Frame::new(blue)).unwrap();
    }
    let (w, rx) = worker();
    request(&w, &p, 1, Limits::default());
    let ImageState::Ready(px) = settled(&rx, 1).state else {
        panic!()
    };
    let c = px.to_rgba8().get_pixel(4, 4).0;
    assert!(
        c[0] > 200 && c[2] < 60,
        "first frame must be the red one: {c:?}"
    );
}

#[test]
fn big_images_are_downscaled_in_the_worker_and_report_their_real_size() {
    let sb = Sandbox::new();
    let p = save(&sb, "big.jpg", &gradient(4000, 3000), ImageFormat::Jpeg);
    let (w, rx) = worker();
    request(&w, &p, 1, Limits::default());
    let done = settled(&rx, 1);
    assert_eq!(
        (done.info.width, done.info.height),
        (Some(4000), Some(3000)),
        "the info is the file's, not the preview's"
    );
    let ImageState::Ready(px) = done.state else {
        panic!()
    };
    assert_eq!(px.width().max(px.height()), 1600);
    assert_eq!((px.width(), px.height()), (1600, 1200), "aspect ratio kept");
}

/// A PNG with a valid signature and IHDR claiming 12000x12000 but no pixel data at all.
fn fake_huge_png(w: u32, h: u32) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut chunk = b"IHDR".to_vec();
    chunk.extend_from_slice(&ihdr);
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&crc32(&chunk).to_be_bytes());
    // A first IDAT chunk header, so that the file looks like a PNG with pixel data.
    let mut idat = b"IDAT".to_vec();
    idat.extend_from_slice(&[0x78, 0x9c, 0x01, 0x00]);
    out.extend_from_slice(&4u32.to_be_bytes());
    out.extend_from_slice(&idat);
    out.extend_from_slice(&crc32(&idat).to_be_bytes());
    out
}

#[test]
fn a_12000x12000_image_is_refused_from_the_header_with_a_clear_message() {
    let sb = Sandbox::new();
    let p = sb.write("huge.png", fake_huge_png(12000, 12000));
    let (w, rx) = worker();
    let started = Instant::now();
    request(&w, &p, 1, Limits::default());
    let ev = next_for(&rx, 1, 5);
    let Preview::Image(i) = ev.preview else {
        panic!("{:?}", ev.preview)
    };
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!((i.info.width, i.info.height), (Some(12000), Some(12000)));
    assert_eq!(i.info.format, "PNG");
    let ImageState::TooLarge(msg) = i.state else {
        panic!("{:?}", i.state)
    };
    assert!(
        msg.contains("12000×12000") && msg.contains("144") && msg.contains("limit"),
        "{msg}"
    );
    assert!(
        msg.contains("image_max_megapixels"),
        "names the setting: {msg}"
    );
    // No decode follows.
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn the_limits_are_configurable() {
    let sb = Sandbox::new();
    let p = save(&sb, "mid.png", &gradient(2000, 1500), ImageFormat::Png); // 3 MP
    let (w, rx) = worker();
    let tight = Limits {
        image: ImageLimits {
            max_megapixels: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    request(&w, &p, 1, tight);
    let ImageState::TooLarge(msg) = settled_or_first(&rx, 1).state else {
        panic!()
    };
    assert!(
        msg.contains("3 megapixels") && msg.contains("2 MP"),
        "{msg}"
    );

    let small_file = Limits {
        image: ImageLimits {
            max_file_bytes: 100,
            ..Default::default()
        },
        ..Default::default()
    };
    request(&w, &p, 2, small_file);
    let ImageState::TooLarge(msg) = settled_or_first(&rx, 2).state else {
        panic!()
    };
    assert!(msg.contains("image_max_file_mb"), "{msg}");

    request(
        &w,
        &p,
        3,
        Limits {
            image: ImageLimits {
                max_megapixels: 10,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert!(matches!(settled(&rx, 3).state, ImageState::Ready(_)));
}

fn settled_or_first(rx: &Receiver<CoreEvent>, g: u64) -> rada_core::preview::ImagePreview {
    match next_for(rx, g, 10).preview {
        Preview::Image(i) => i,
        o => panic!("{o:?}"),
    }
}

#[test]
fn corrupt_and_truncated_images_are_reported_not_crashed_on() {
    let sb = Sandbox::new();
    // Valid signature, nonsense after it.
    let mut junk = b"\x89PNG\r\n\x1a\n".to_vec();
    junk.extend((0..200u8).map(|b| b.wrapping_mul(37)));
    let corrupt = sb.write("corrupt_header.png", junk);
    // A real JPEG cut in half: the header is fine, the pixels are not.
    let full = save(&sb, "full.jpg", &gradient(300, 200), ImageFormat::Jpeg);
    let bytes = std::fs::read(&full).unwrap();
    let trunc = sb.write("trunc.jpg", &bytes[..bytes.len() / 2]);

    let (w, rx) = worker();
    request(&w, &corrupt, 1, Limits::default());
    let i = settled_or_first(&rx, 1);
    assert!(
        matches!(i.state, ImageState::Failed(ref m) if m.contains("header")),
        "{:?}",
        i.state
    );

    request(&w, &trunc, 2, Limits::default());
    let i = settled(&rx, 2);
    assert_eq!((i.info.width, i.info.height), (Some(300), Some(200)));
    // Whatever the decoder makes of it, the user is told the file is damaged.
    let told = i.note.as_deref().is_some_and(|n| n.contains("truncated"))
        || matches!(i.state, ImageState::Failed(_));
    assert!(told, "{:?} / {:?}", i.state, i.note);
}

#[test]
fn content_decides_not_the_extension() {
    let sb = Sandbox::new();
    let real = save(&sb, "picture.dat", &gradient(10, 10), ImageFormat::Png);
    let fake = sb.write("fake.png", "this is just text, not an image\nsecond line\n");
    let (w, rx) = worker();
    request(&w, &real, 1, Limits::default());
    assert_eq!(settled(&rx, 1).info.format, "PNG");
    request(&w, &fake, 2, Limits::default());
    assert!(matches!(next_for(&rx, 2, 5).preview, Preview::Text(_)));
}

#[test]
fn svg_is_rendered_with_its_own_size() {
    let sb = Sandbox::new();
    let svg = r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
      <rect width="200" height="100" fill="#ff0000"/><circle cx="100" cy="50" r="30" fill="#0000ff"/></svg>"##;
    let p = sb.write("logo.svg", svg);
    let (w, rx) = worker();
    request(&w, &p, 1, Limits::default());
    let done = settled(&rx, 1);
    assert_eq!(done.info.format, "SVG");
    assert_eq!((done.info.width, done.info.height), (Some(200), Some(100)));
    let ImageState::Ready(px) = done.state else {
        panic!("{:?}", done.state)
    };
    assert_eq!(px.width() / px.height(), 2, "aspect ratio kept");
    let rgba = px.to_rgba8();
    let corner = rgba.get_pixel(2, 2).0;
    let centre = rgba.get_pixel(rgba.width() / 2, rgba.height() / 2).0;
    assert!(
        corner[0] > 200 && corner[2] < 50,
        "red background: {corner:?}"
    );
    assert!(centre[2] > 200 && centre[0] < 50, "blue circle: {centre:?}");
}

#[test]
fn a_broken_svg_is_a_message_not_a_crash() {
    let sb = Sandbox::new();
    let p = sb.write("bad.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect");
    let (w, rx) = worker();
    request(&w, &p, 1, Limits::default());
    assert!(matches!(settled(&rx, 1).state, ImageState::Failed(_)));
}

/// JPEG with an EXIF orientation tag inserted right after SOI.
fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    let mut exif = b"Exif\0\0".to_vec();
    exif.extend_from_slice(b"II*\0\x08\0\0\0"); // TIFF header, IFD at 8
    exif.extend_from_slice(&1u16.to_le_bytes()); // one entry
    exif.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    exif.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    exif.extend_from_slice(&1u32.to_le_bytes());
    exif.extend_from_slice(&orientation.to_le_bytes());
    exif.extend_from_slice(&[0, 0]);
    exif.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    let mut out = vec![0xff, 0xd8, 0xff, 0xe1];
    out.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&exif);
    out.extend_from_slice(&jpeg[2..]);
    out
}

#[test]
fn photos_are_shown_upright() {
    let sb = Sandbox::new();
    let plain = save(&sb, "plain.jpg", &gradient(40, 20), ImageFormat::Jpeg);
    let rotated = sb.write(
        "rot.jpg",
        with_orientation(&std::fs::read(&plain).unwrap(), 6),
    ); // 90° clockwise
    let (w, rx) = worker();
    request(&w, &rotated, 1, Limits::default());
    let done = settled(&rx, 1);
    let ImageState::Ready(px) = done.state else {
        panic!()
    };
    assert_eq!(
        (px.width(), px.height()),
        (20, 40),
        "EXIF orientation applied"
    );
    assert_eq!((done.info.width, done.info.height), (Some(20), Some(40)));
}

#[test]
fn a_slow_image_never_delays_text_previews() {
    let sb = Sandbox::new();
    let big = save(&sb, "big.png", &gradient(6000, 4000), ImageFormat::Png);
    let txt = sb.write("note.txt", "quick text");
    let (w, rx) = worker();
    request(&w, &big, 1, Limits::default());
    let _ = next_for(&rx, 1, 5); // header-only answer
    let t = Instant::now();
    request(&w, &txt, 2, Limits::default());
    let ev = next_for(&rx, 2, 5);
    assert!(matches!(ev.preview, Preview::Text(_)));
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "text preview took {:?} while an image decodes",
        t.elapsed()
    );
}

#[test]
fn binaries_get_a_card_with_architecture_and_hex_data_kept_aside() {
    let sb = Sandbox::new();
    let mut elf = vec![0u8; 4096];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[16] = 2;
    elf[18] = 183; // AArch64
    elf[100] = 0xff; // keep it clearly binary
    let p = sb.write("tool", elf);
    chmod(&p, 0o755);
    let (w, rx) = worker();
    request(&w, &p, 1, Limits::default());
    let Preview::Binary(b) = next_for(&rx, 1, 5).preview else {
        panic!()
    };
    assert_eq!(b.card.kind, "ELF executable");
    let exec = b.card.exec.expect("architecture");
    assert_eq!(
        (exec.format, exec.arch.as_str(), exec.bits),
        ("ELF", "AArch64", Some(64))
    );
    assert!(b.card.modified.is_some());
    #[cfg(unix)]
    assert_eq!(b.card.mode.unwrap() & 0o777, 0o755);
    assert!(b.hex[0].contains("7f 45 4c 46"));
}

#[test]
fn with_rendering_off_nothing_is_decoded() {
    let sb = Sandbox::new();
    let p = save(&sb, "big.jpg", &gradient(4000, 3000), ImageFormat::Jpeg);
    let (w, rx) = worker();
    let off = Limits {
        image: ImageLimits {
            decode: false,
            ..Default::default()
        },
        ..Default::default()
    };
    request(&w, &p, 1, off);
    let Preview::Image(i) = next_for(&rx, 1, 5).preview else {
        panic!()
    };
    assert!(matches!(i.state, ImageState::Disabled), "{:?}", i.state);
    assert_eq!(
        (i.info.width, i.info.height),
        (Some(4000), Some(3000)),
        "the header is still read"
    );
    assert!(
        rx.recv_timeout(Duration::from_millis(500)).is_err(),
        "no decode may follow"
    );
}

#[test]
fn transparent_pictures_and_svgs_are_drawn_on_a_light_checkerboard() {
    let sb = Sandbox::new();
    // A black disc on a transparent PNG: invisible on a dark terminal without a backdrop.
    let mut img = RgbaImage::from_pixel(96, 96, Rgba([0, 0, 0, 0]));
    for y in 30..66 {
        for x in 30..66 {
            img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
    }
    let png = sb.path("disc.png");
    DynamicImage::ImageRgba8(img).save(&png).unwrap();
    let svg = sb.write("disc.svg", r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><circle cx="48" cy="48" r="30"/></svg>"#);
    let (w, rx) = worker();
    for (g, p) in [(1, &png), (2, &svg)] {
        request(&w, p, g, Limits::default());
        let ImageState::Ready(px) = settled(&rx, g).state else {
            panic!()
        };
        let rgba = px.to_rgba8();
        let (cx, cy) = (rgba.width() / 2, rgba.height() / 2);
        let centre = rgba.get_pixel(cx, cy).0;
        assert!(
            centre[0] < 30 && centre[3] == 255,
            "the shape itself stays black: {centre:?}"
        );
        let corner = rgba.get_pixel(1, 1).0;
        assert!(
            corner[0] >= 200 && corner[0] == corner[1] && corner[3] == 255,
            "corner is a light grey tile: {corner:?}"
        );
        // Two different tile shades somewhere along the top row.
        let shades: std::collections::HashSet<u8> = (0..rgba.width())
            .map(|x| rgba.get_pixel(x, 1).0[0])
            .collect();
        assert!(
            shades.len() >= 2,
            "a checkerboard, not a flat colour: {shades:?}"
        );
    }
}

#[test]
fn opaque_pictures_are_left_untouched() {
    let sb = Sandbox::new();
    let p = save(&sb, "solid.png", &gradient(40, 40), ImageFormat::Png);
    let (w, rx) = worker();
    request(&w, &p, 1, Limits::default());
    let ImageState::Ready(px) = settled(&rx, 1).state else {
        panic!()
    };
    assert_eq!(px.to_rgba8().get_pixel(0, 0).0, [0, 0, 128, 255]);
}
