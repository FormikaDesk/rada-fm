//! Draws the logo (`assets/linux/rada.svg`) at every size and writes the icon files of macOS
//! and Windows into `assets/`. Run it when the logo changes:
//! `cargo run -p rada --example make-icons`, then commit the two files it writes.

use std::io::Cursor;
use std::path::Path;

#[path = "../src/icons.rs"]
mod icons;

fn png_of(svg: &[u8], size: u32) -> Option<Vec<u8>> {
    let img = rada_core::preview::render_svg(svg, size).ok()?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let svg = std::fs::read(here.join("assets/linux/rada.svg"))?;
    let draw = |size: u32| png_of(&svg, size);
    std::fs::write(here.join("assets/macos/rada.icns"), icons::build_icns(&draw)?)?;
    std::fs::write(here.join("assets/windows/rada.ico"), icons::build_ico(&draw)?)?;
    println!("wrote assets/macos/rada.icns and assets/windows/rada.ico");
    Ok(())
}
