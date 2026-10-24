//! The disk ring beside its figures, and the bar where pictures are not drawn.

use crate::common::*;
use crate::harness::*;
use rada_tui::Theme;
use rada_tui::theme::ColorDepth;

/// The scene on a terminal that draws pictures (Kitty), with a cell of 10 × 20 pixels.
fn ring_scene(theme: &str) -> H {
    let mut h = scene(160, 46, theme, false);
    h.app.preview.image_ui = Some(rada_tui::ImageUi::sized(
        ratatui_image::picker::ProtocolType::Kitty,
        (10, 20),
    ));
    h.app.th = Theme::named(theme, ColorDepth::True).unwrap();
    h
}

#[test]
fn disk_is_a_ring_where_pictures_draw() {
    let mut h = ring_scene("neon");
    let s = h.screen();
    let ring = h.app.frame.ring.as_ref().expect("the ring was made");
    assert_eq!(ring.pieces.len(), 4, "around the hole for the text");
    // In the hole, the percentage and the word; beside it the name, used of total, free. The
    // cells that carry a picture are left out of the rows.
    let buf = h.redrawn_buffer();
    let plain: Vec<String> = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .filter(|c| c.len() <= 4)
                .collect()
        })
        .collect();
    let at = plain
        .iter()
        .position(|l| l.contains("usato") || l.contains("used"))
        .expect("the word under the percentage");
    assert!(
        plain[at - 1].contains("62%"),
        "the percentage above the word"
    );
    assert!(
        plain.iter().any(|l| l.contains("293 / 477 GB")),
        "used of total"
    );
    assert!(plain.iter().any(|l| l.contains("183 GB free")), "free");
    // The main disk has no bar across the box (the others have twelve cells).
    assert!(
        !s.lines().any(|l| l.contains("■■■■■■■■■■■■■■")),
        "a bar for the main disk"
    );
    // The other volumes keep their bars of twelve cells.
    assert!(
        s.contains("■■■■") || s.contains("□□□□"),
        "bars of the others"
    );
}

#[test]
fn the_ring_is_made_again_only_when_the_percentage_changes() {
    let mut h = ring_scene("neon");
    let _ = h.screen();
    let _ = h.screen();
    h.app.redraw.dirty = true;
    let _ = h.screen();
    assert_eq!(
        h.app.frame.ring.as_ref().unwrap().made,
        1,
        "three frames, one picture"
    );
    // Another percentage: another picture.
    let total = 512_000_000_000u64;
    // The first volume of the demo is the main disk (its mount point is the root of the
    // system, which is not `/` everywhere).
    h.app.devices.volumes[0].available = Some(total / 10);
    let _ = h.screen();
    assert_eq!(h.app.frame.ring.as_ref().unwrap().made, 2);
}

#[test]
fn without_pictures_or_true_colours_the_main_disk_stays_a_bar() {
    // Half blocks: the terminal draws no pictures.
    let mut h = scene(160, 46, "neon", false);
    let s = h.screen();
    assert!(h.app.frame.ring.is_none());
    assert!(s.contains("■"), "a bar");
    // Pictures, but a theme in 256 colours.
    let mut h = ring_scene("neon");
    h.app.th = Theme::named("neon", ColorDepth::Ansi256).unwrap();
    let s = h.screen();
    assert!(h.app.frame.ring.is_none());
    assert!(s.contains("■"), "a bar");
}

/// The cells of the ring on the screen, as the pieces cover them.
fn ring_cells(h: &H) -> Vec<ratatui::layout::Rect> {
    h.app
        .frame
        .ring
        .as_ref()
        .unwrap()
        .pieces
        .iter()
        .map(|p| p.0)
        .collect()
}

#[test]
fn the_ring_follows_the_cell_when_the_font_is_zoomed() {
    let mut h = ring_scene("neon");
    let _ = h.screen();
    let cells = ring_cells(&h);
    let before = h.app.frame.ring.as_ref().unwrap().made;
    // The same cell: nothing is made again.
    h.app.use_cell_px((10, 20));
    let _ = h.screen();
    assert_eq!(h.app.frame.ring.as_ref().unwrap().made, before);
    // Same columns and rows, other pixels (the font zoomed in a window of the same size).
    h.app.use_cell_px((14, 28));
    assert!(h.app.take_full_redraw(), "the old picture is cleared");
    let _ = h.screen();
    assert_eq!(h.app.preview.image_ui.as_ref().unwrap().cell_px(), (14, 28));
    assert_eq!(
        h.app.frame.ring.as_ref().unwrap().made,
        1,
        "made for the new cell"
    );
    assert_eq!(ring_cells(&h), cells, "in the same cells");
    // Columns and rows change as well (the window kept its size).
    h.app.use_cell_px((7, 14));
    let _ = h.screen();
    assert_eq!(h.app.preview.image_ui.as_ref().unwrap().cell_px(), (7, 14));
    assert_eq!(h.app.frame.ring.as_ref().unwrap().made, 1);
    assert_eq!(ring_cells(&h), cells);
}

#[test]
fn a_cell_the_system_does_not_report_leaves_the_ring_as_it_is() {
    let mut h = ring_scene("neon");
    let _ = h.screen();
    h.app.use_cell_px((0, 0));
    assert!(!h.app.take_full_redraw());
    assert_eq!(h.app.preview.image_ui.as_ref().unwrap().cell_px(), (10, 20));
}
