//! The tone under the pointer shows on every ground of every theme, by about as much, and is
//! neither the row of the cursor nor a selected row.

use rada_tui::theme::{ColorDepth, Theme};
use ratatui::style::Color;

fn rgb(c: Color) -> (f32, f32, f32) {
    match c {
        Color::Rgb(r, g, b) => (r as f32, g as f32, b as f32),
        other => panic!("not a true colour: {other:?}"),
    }
}

/// How bright a colour looks, 0 to 255.
fn luma(c: Color) -> f32 {
    let (r, g, b) = rgb(c);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The distance between two colours in RGB space.
fn distance(a: Color, b: Color) -> f32 {
    let (a, b) = (rgb(a), rgb(b));
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

/// The least brightness it differs from its ground by, out of 255 (the old tone differed by 6
/// in the rada themes and in carta).
const VISIBLE: f32 = 15.0;
/// The least distance between the tone and the colour of a row of the cursor or a selected one.
const APART: f32 = 6.0;

fn grounds(th: &Theme) -> Vec<(Color, Color)> {
    th.hoverable_grounds()
        .into_iter()
        // The terminal's own background is the canvas, as far as a mix is concerned.
        .map(|g| (g, if g == Color::Reset { th.canvas } else { g }))
        .collect()
}

#[test]
fn the_tone_shows_on_every_ground_of_every_theme() {
    for name in Theme::NAMES {
        let th = Theme::named(name, ColorDepth::True).unwrap();
        for (ground, shown) in grounds(&th) {
            let tone = th.hover_over(ground);
            let by = (luma(tone) - luma(shown)).abs();
            assert!(
                by >= VISIBLE,
                "{name}: on {ground:?} the tone {tone:?} differs by {by:.1}"
            );
        }
    }
}

#[test]
fn it_is_not_the_cursor_or_a_selection_and_is_lighter_than_the_cursor() {
    for name in Theme::NAMES {
        let th = Theme::named(name, ColorDepth::True).unwrap();
        for (ground, shown) in grounds(&th) {
            let tone = th.hover_over(ground);
            for (what, other) in [
                ("cursor", th.cursor),
                ("selected row", th.sel),
                ("selection", th.selection),
                ("mark", th.mark),
            ] {
                let d = distance(tone, other);
                assert!(
                    d >= APART,
                    "{name}: on {ground:?} the tone {tone:?} is {d:.1} from the {what}"
                );
            }
            // The cursor row lies on a ground, never on a button or a key cap (which are
            // already near its tone in some themes).
            if [th.button, th.key_bg].contains(&ground) {
                continue;
            }
            let (hover, cursor) = (luma(tone) - luma(shown), luma(th.cursor) - luma(shown));
            assert!(
                hover.abs() < cursor.abs(),
                "{name}: on {ground:?} the tone is stronger than the cursor"
            );
        }
    }
}

#[test]
fn it_is_as_visible_on_the_inner_boxes_as_on_the_canvas() {
    for name in Theme::NAMES {
        let th = Theme::named(name, ColorDepth::True).unwrap();
        let by = |g: Color| (luma(th.hover_over(g)) - luma(g)).abs();
        let (canvas, inner) = (by(th.canvas), by(th.inner));
        assert!(
            (canvas - inner).abs() < 8.0,
            "{name}: {canvas:.1} on the canvas, {inner:.1} in a box"
        );
    }
}
