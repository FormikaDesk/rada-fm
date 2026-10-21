//! The ring of the SPAZIO box: how full the main disk is, as a ring that goes from cyan to
//! violet, drawn with the terminal's graphics protocol where it has one.
//!
//! The picture is made here, pixel by pixel, at the size of the cells it covers, and handed to
//! the protocol in four pieces around a hole: the hole is ordinary cells, where the percentage
//! and *used* are written as text (a picture would lie over them). It is made again only when
//! the percentage, the size of the box, the cell size or the colours change.

use image::{DynamicImage, RgbImage};
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;
use ratatui_image::protocol::Protocol;

use crate::images::ImageUi;
use crate::theme::Theme;

/// The ring is this many cells wide and high; the hole in the middle is `HOLE` of them, in the
/// middle rows, where the text goes.
pub const COLS: u16 = 10;
pub const ROWS: u16 = 6;
pub const HOLE: Rect = Rect {
    x: 2,
    y: 2,
    width: 6,
    height: 2,
};

/// The colours of the picture, as 8-bit channels: what is behind it, the part of the ring that
/// is not used, and the two ends of the used part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub bg: [u8; 3],
    pub track: [u8; 3],
    pub from: [u8; 3],
    pub to: [u8; 3],
}

impl Colors {
    /// `None` unless all four are true colours: a theme drawn in 256 or 16 colours has no ring.
    pub fn of(bg: Color, track: Color, from: Color, to: Color) -> Option<Colors> {
        let rgb = Theme::channels;
        Some(Colors {
            bg: rgb(bg)?,
            track: rgb(track)?,
            from: rgb(from)?,
            to: rgb(to)?,
        })
    }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] as f32 + (b[i] as f32 - a[i] as f32) * t)
}

fn over(base: [f32; 3], with: [f32; 3], cover: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| base[i] + (with[i] - base[i]) * cover.clamp(0.0, 1.0))
}

/// The ring for `percent` (0 to 100) on a picture `w` × `h` pixels: a track all round, and over
/// it, from the top and clockwise, the used part with round ends, shading from `from` to `to`.
pub fn image(percent: u8, w: u32, h: u32, c: &Colors) -> RgbImage {
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let outer = w.min(h) as f32 / 2.0 - 1.0;
    let thick = outer * 0.22;
    let mid = outer - thick / 2.0;
    let sweep = percent.min(100) as f32 / 100.0 * std::f32::consts::TAU;
    let at = |a: f32| (cx + mid * a.sin(), cy - mid * a.cos());
    let (sx, sy) = at(0.0);
    let (ex, ey) = at(sweep);
    let cap = |px: f32, py: f32, (x, y): (f32, f32)| {
        (thick / 2.0 + 0.5 - ((px - x).powi(2) + (py - y).powi(2)).sqrt()).clamp(0.0, 1.0)
    };
    let mut img = RgbImage::new(w, h);
    for (x, y, out) in img.enumerate_pixels_mut() {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let (dx, dy) = (px - cx, py - cy);
        let dist = (dx * dx + dy * dy).sqrt();
        let band = (thick / 2.0 + 0.5 - (dist - mid).abs()).clamp(0.0, 1.0);
        let mut px_rgb = over(c.bg.map(f32::from), c.track.map(f32::from), band);
        if percent > 0 {
            let angle = dx.atan2(-dy).rem_euclid(std::f32::consts::TAU);
            let inside = if angle <= sweep { band } else { 0.0 };
            let cover = inside.max(cap(px, py, (sx, sy))).max(cap(px, py, (ex, ey)));
            if cover > 0.0 {
                // Past the end of the arc is a cap: the start's if it is nearer round the
                // ring that way, the end's if not.
                let along = if angle <= sweep {
                    angle / sweep
                } else if angle > (sweep + std::f32::consts::TAU) / 2.0 {
                    0.0
                } else {
                    1.0
                };
                px_rgb = over(px_rgb, mix(c.from, c.to, along), cover);
            }
        }
        out.0 = px_rgb.map(|v| v.round().clamp(0.0, 255.0) as u8);
    }
    img
}

/// What was last drawn: the picture's four pieces (their places relative to the ring) and what
/// they were made for.
pub struct Cache {
    key: (u8, (u16, u16), Colors),
    /// How many times the picture was made: it is not on every frame.
    pub made: u32,
    pub pieces: Vec<(Rect, Protocol)>,
}

/// The pieces for `percent`: the rows above and below the hole, and what is left and right of
/// it. Made again only when something they depend on changed.
pub fn pieces<'a>(
    cache: &'a mut Option<Cache>,
    ui: &ImageUi,
    percent: u8,
    c: &Colors,
) -> Option<&'a [(Rect, Protocol)]> {
    let font = ui.cell_px();
    let key = (percent, font, *c);
    if cache.as_ref().map(|k| k.key) != Some(key) {
        let (fw, fh) = (font.0 as u32, font.1 as u32);
        if fw == 0 || fh == 0 {
            return None;
        }
        let full = DynamicImage::ImageRgb8(image(percent, COLS as u32 * fw, ROWS as u32 * fh, c));
        let around = [
            Rect::new(0, 0, COLS, HOLE.y),
            Rect::new(0, HOLE.bottom(), COLS, ROWS - HOLE.bottom()),
            Rect::new(0, HOLE.y, HOLE.x, HOLE.height),
            Rect::new(HOLE.right(), HOLE.y, COLS - HOLE.right(), HOLE.height),
        ];
        let mut made = Vec::new();
        for r in around {
            let part = full.crop_imm(
                r.x as u32 * fw,
                r.y as u32 * fh,
                r.width as u32 * fw,
                r.height as u32 * fh,
            );
            made.push((r, ui.protocol(part, Size::new(r.width, r.height))?));
        }
        let made_before = cache.as_ref().map_or(0, |k| k.made);
        *cache = Some(Cache {
            key,
            made: made_before + 1,
            pieces: made,
        });
    }
    cache.as_ref().map(|k| k.pieces.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Colors = Colors {
        bg: [0x13, 0x15, 0x2b],
        track: [0x26, 0x2a, 0x52],
        from: [0x5f, 0xe1, 0xe6],
        to: [0xa7, 0x8b, 0xfa],
    };

    fn at(img: &RgbImage, x: u32, y: u32) -> [u8; 3] {
        img.get_pixel(x, y).0
    }

    #[test]
    fn the_used_part_goes_clockwise_from_the_top_and_the_rest_is_the_track() {
        let (w, h) = (100, 120);
        let img = image(64, w, h, &C);
        // The middle of the ring at the top, at the right, at the bottom and at the left (the
        // ring is 100 wide, so its middle line is 43 from the centre of the picture).
        let (cx, cy, r) = (50u32, 60u32, 43u32);
        let top = at(&img, cx + 2, cy - r);
        let right = at(&img, cx + r, cy);
        let bottom = at(&img, cx, cy + r);
        let left = at(&img, cx - r, cy);
        // 64% ends past the bottom, a little way up the left: top, right and bottom are used.
        let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) < 40);
        assert!(near(top, C.from), "{top:?}");
        // The round start sticks out a little to the left of the top, in the same colour.
        let cap = at(&img, cx - 2, cy - r);
        assert!(near(cap, C.from), "{cap:?}");
        assert!(!near(right, C.track) && !near(bottom, C.track));
        assert!(near(left, C.track), "{left:?}: not used yet");
        // The hole and the corners are what is behind the picture.
        assert_eq!(at(&img, cx, cy), C.bg);
        assert_eq!(at(&img, 0, 0), C.bg);
        // The colour goes from cyan towards violet along the arc.
        assert!(
            right[0] > top[0] && bottom[0] > right[0],
            "{top:?} {right:?} {bottom:?}"
        );
    }

    #[test]
    fn nothing_is_used_at_zero_and_all_of_it_at_a_hundred() {
        let (w, h) = (100, 100);
        let empty = image(0, w, h, &C);
        assert_eq!(
            at(&empty, 50, 7),
            C.track,
            "an empty ring is the track alone"
        );
        let full = image(100, w, h, &C);
        assert_ne!(at(&full, 7, 50), C.track, "every side is used");
        assert_ne!(at(&full, 50, 93), C.track);
    }

    #[test]
    fn only_true_colours_make_a_ring() {
        use crate::theme::ColorDepth;
        let t = Theme::named("neon", ColorDepth::True).unwrap();
        let ok = |t: &Theme| Colors::of(t.inner, t.track, t.hues.cyan, t.accent);
        assert!(ok(&t).is_some());
        let t = Theme::named("neon", ColorDepth::Ansi256).unwrap();
        assert!(ok(&t).is_none(), "a theme in 256 colours has no ring");
    }

    #[test]
    fn the_text_hole_is_in_the_middle_of_the_ring() {
        assert_eq!(HOLE.x * 2 + HOLE.width, COLS);
        assert_eq!(HOLE.y * 2 + HOLE.height, ROWS);
    }
}
