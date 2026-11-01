//! The pointer over the boxes of the panels: what it lights is lit whole, as one rectangle, or
//! it lights nothing. A box made of a picture, text and strips of other grounds is never lit in
//! pieces.

use crate::common::*;
use crate::harness::*;
use crossterm::event::{KeyModifiers, MouseEventKind};
use rada_tui::hits::Target;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

type Look = Vec<(Color, Color, Modifier)>;

fn look(h: &mut H) -> Look {
    let _ = h.screen();
    h.term
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| (c.bg, c.fg, c.modifier))
        .collect()
}

fn point_at(h: &mut H, x: u16, y: u16) {
    h.mouse(MouseEventKind::Moved, x, y, KeyModifiers::NONE);
}

/// The cells that are not what they were in `before`, as (x, y).
fn changed(h: &mut H, before: &Look) -> Vec<(u16, u16)> {
    let now = look(h);
    let w = h.term.backend().buffer().area.width as usize;
    (0..before.len())
        .filter(|i| before[*i] != now[*i])
        .map(|i| ((i % w) as u16, (i / w) as u16))
        .collect()
}

fn inside(r: Rect, (x, y): (u16, u16)) -> bool {
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

/// With the pointer in the middle of each thing that can be clicked: either nothing changes
/// (the row of the cursor, the tab in front: already in a stronger tone) or every cell of the
/// rectangle that is lit changes, and none outside it. The dot of a tag is left out: it also
/// says in the status line which view it opens.
fn lit_whole(h: &mut H, label: &str) {
    h.app.settings.mouse = true;
    point_at(h, 0, 0);
    let before = look(h);
    let hits: Vec<(Rect, Target)> = h.app.frame.hits.all().cloned().collect();
    for (r, t) in hits {
        if matches!(&t, Target::Place(p) if p.to_string_lossy().starts_with("tag:")) {
            continue;
        }
        let (x, y) = (r.x + r.width / 2, r.y + r.height / 2);
        point_at(h, x, y);
        let lit = h.app.frame.hits.lit_at(x, y);
        let moved = changed(h, &before);
        if let Some((area, _)) = lit
            && !moved.is_empty()
        {
            let all = (area.width * area.height) as usize;
            assert_eq!(
                moved.iter().filter(|p| inside(area, **p)).count(),
                all,
                "{label}: {t:?} at {r:?} is lit in pieces"
            );
        }
        assert!(
            moved
                .iter()
                .all(|p| lit.as_ref().is_some_and(|(a, _)| inside(*a, *p))),
            "{label}: {t:?} at {r:?} changes cells that are not its own"
        );
        point_at(h, 0, 0);
    }
}

#[test]
fn what_the_pointer_lights_is_lit_whole_at_every_size_and_theme() {
    for theme in ["neon", "rada", "rada-light"] {
        for nerd in [false, true] {
            for (w, hh) in [(160, 46), (120, 35), (100, 30)] {
                let mut h = scene(w, hh, theme, nerd);
                lit_whole(&mut h, &format!("{theme}, nerd {nerd}, {w}x{hh}"));
            }
        }
    }
}

#[test]
fn the_ring_and_the_tree_are_lit_whole_too() {
    let mut h = scene(160, 46, "neon", false);
    h.app.preview.image_ui = Some(rada_tui::ImageUi::sized(
        ratatui_image::picker::ProtocolType::Kitty,
        (10, 20),
    ));
    lit_whole(&mut h, "the ring");
    let mut h = tree_scene(160, 46, "neon", false);
    lit_whole(&mut h, "the tree");
}
