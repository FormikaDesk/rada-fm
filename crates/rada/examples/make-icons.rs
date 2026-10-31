//! Draws the logo at every size and writes the icon files: the `.icns` of macOS and the `.ico`
//! of Windows into `assets/`, and the PNGs of the Linux icon theme into `assets/linux/` (the
//! copies in `packaging/linux/` and the picture of the README are written too). From 48 pixels
//! up the sizes come from the picture `assets/logo.png`; the small ones (16, 24, 32) come from
//! the flat logo `assets/linux/rada.svg`, which is sharper there. Run it when a logo changes:
//! `cargo run -p rada --example make-icons`, then commit the files it writes.

use std::io::Cursor;
use std::path::Path;

use image::imageops::{FilterType, resize};
use image::{ImageFormat, RgbaImage};

#[path = "../src/icons.rs"]
mod icons;

/// Below this the flat logo is drawn; from here up, the picture.
const FIRST_PICTURE_SIZE: u32 = 48;
/// The sizes of the Linux icon theme that are written as files.
const LINUX_SIZES: &[u32] = &[48, 64, 128, 256, 512];
/// The size of the logo in the README.
const README_SIZE: u32 = 256;

fn png_bytes(img: &RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// The picture at another size. The colours are scaled with their opacity (premultiplied),
/// so that the transparent border does not tint the edge of the logo.
fn scaled(big: &RgbaImage, size: u32) -> RgbaImage {
    if big.width() == size {
        return big.clone();
    }
    let mut pre = big.clone();
    for p in pre.pixels_mut() {
        let a = u32::from(p[3]);
        for c in &mut p.0[..3] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
    let mut out = resize(&pre, size, size, FilterType::Lanczos3);
    for p in out.pixels_mut() {
        let a = u32::from(p[3]);
        for c in &mut p.0[..3] {
            *c = (u32::from(*c) * 255 + a / 2)
                .checked_div(a)
                .map_or(0, |v| v.min(255)) as u8;
        }
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let svg = std::fs::read(here.join("assets/linux/rada.svg"))?;
    let big = image::load_from_memory_with_format(
        &std::fs::read(here.join("assets/logo.png"))?,
        ImageFormat::Png,
    )?
    .to_rgba8();
    let draw = |size: u32| {
        if size < FIRST_PICTURE_SIZE {
            let img = rada_core::preview::render_svg(&svg, size).ok()?;
            png_bytes(&img)
        } else {
            png_bytes(&scaled(&big, size))
        }
    };

    std::fs::write(
        here.join("assets/macos/rada.icns"),
        icons::build_icns(&draw)?,
    )?;
    std::fs::write(
        here.join("assets/windows/rada.ico"),
        icons::build_ico(&draw)?,
    )?;
    for size in LINUX_SIZES {
        let png = draw(*size).ok_or("no picture")?;
        for dir in ["assets/linux", "../../packaging/linux"] {
            std::fs::write(here.join(dir).join(format!("rada-{size}.png")), &png)?;
        }
    }
    let readme = here.join("../../docs/media");
    if readme.is_dir() {
        std::fs::write(
            readme.join(format!("logo-{README_SIZE}.png")),
            draw(README_SIZE).ok_or("no picture")?,
        )?;
    }
    println!(
        "wrote the icon files of macOS, Windows and Linux (assets/, packaging/linux/, docs/media/)"
    );
    Ok(())
}
