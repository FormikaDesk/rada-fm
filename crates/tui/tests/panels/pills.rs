//! The pills at the ends of the labels, with and without a Nerd Font.

use crate::common::*;
use rada_tui::Theme;
use ratatui::style::Color;

#[test]
fn every_pill_has_both_ends_and_one_colour_from_end_to_end() {
    for name in Theme::NAMES {
        for (w, hh) in [(160, 46), (140, 36), (170, 42)] {
            let mut h = scene(w, hh, name, true);
            let what = format!("{name} {w}x{hh}");
            let buf = h.redrawn_buffer();
            let behind = [h.app.settings.th.inner, h.app.settings.th.canvas];
            let pills = assert_pills(&buf, &what, &behind);
            // The selected row, the tab, the "rada" and the count, the keys, the buttons, the
            // labels: a screen has dozens.
            assert!(pills > 20, "{what}: only {pills} pills");
            // With the keyboard in NAV the row it is on is a pill too.
            h.press("tab");
            assert_pills(
                &h.redrawn_buffer(),
                &format!("{what}, keyboard in NAV"),
                &behind,
            );
            h.press("tab");
            assert_pills(
                &h.redrawn_buffer(),
                &format!("{what}, keyboard in the preview"),
                &behind,
            );
        }
    }
}

#[test]
fn without_a_nerd_font_a_pill_is_a_plain_rectangle_of_the_same_width() {
    // No ends at all, and a space at each side where they would be.
    let mut round = scene(160, 46, "rada", true);
    let mut plain = scene(160, 46, "rada", false);
    let (r, p) = (round.redrawn_buffer(), plain.redrawn_buffer());
    let ends = |b: &ratatui::buffer::Buffer| {
        (0..b.area.height)
            .flat_map(|y| (0..b.area.width).map(move |x| (x, y)))
            .filter(|(x, y)| matches!(b[(*x, *y)].symbol(), CAP_LEFT | CAP_RIGHT))
            .count()
    };
    assert!(ends(&r) > 0);
    assert_eq!(ends(&p), 0, "no round ends without a Nerd Font");
}

/// The check above must be able to fail: a pill whose end is of another colour, a pill with
/// an end missing and a cell inside it of another colour are each caught.
#[test]
fn the_pill_check_catches_a_broken_pill() {
    let mut h = scene(160, 46, "rada", true);
    let th = h.app.settings.th.clone();
    let good = h.redrawn_buffer();
    let find = |sym: &str| {
        (0..good.area.height)
            .flat_map(|y| (0..good.area.width).map(move |x| (x, y)))
            .find(|(x, y)| good[(*x, *y)].symbol() == sym)
            .unwrap()
    };
    let caught = |edit: &dyn Fn(&mut ratatui::buffer::Buffer)| {
        let mut b = good.clone();
        edit(&mut b);
        let behind = [th.inner, th.canvas];
        std::panic::catch_unwind(|| assert_pills(&b, "broken", &behind)).is_err()
    };
    let (rx, ry) = find(CAP_RIGHT);
    assert!(
        caught(&|b| {
            b[(rx, ry)].set_fg(Color::Rgb(1, 2, 3));
        }),
        "an end of another colour"
    );
    assert!(
        caught(&|b| {
            b[(rx, ry)].set_symbol(" ");
        }),
        "an end that is missing"
    );
    assert!(
        caught(&|b| {
            b[(rx - 1, ry)].set_bg(Color::Rgb(1, 2, 3));
        }),
        "a cell of another colour inside"
    );
    let (lx, ly) = find(CAP_LEFT);
    assert!(
        caught(&|b| {
            b[(lx, ly)].set_symbol(" ");
        }),
        "a left end that is missing"
    );
}
