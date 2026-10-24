//! The row of tags in NAV: the seven colours and, after them, the tags of the user's own.

use crate::common::*;
use crate::harness::*;
use rada_core::platform::{Tag, TagColor};
use rada_core::tagsearch::view_path;
use rada_tui::hits::Target;
use rada_tui::panels::NavKind;

const DOT: &str = "●";

/// A scene with `names` known as tags of the user's own (the first is blue, the rest have
/// no colour), or `None` on a system that keeps no tags.
fn with_tags(w: u16, hh: u16, names: &[&str]) -> Option<H> {
    let mut h = scene(w, hh, "rada", false);
    if !h.app.tags_supported() {
        return None;
    }
    h.app.tags.known = names
        .iter()
        .enumerate()
        .map(|(i, n)| Tag {
            name: (*n).into(),
            color: (i == 0).then_some(TagColor::Blue),
        })
        .collect();
    h.settle();
    Some(h)
}

/// The lines of tag dots of NAV, as rows.
fn lines(h: &H) -> Vec<(usize, usize, usize)> {
    h.app
        .nav_rows()
        .iter()
        .filter_map(|r| match r.kind {
            NavKind::Tags { first, len, more } => Some((first, len, more)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_users_own_tags_are_dots_after_the_seven_colours_and_a_click_opens_their_list() {
    let Some(mut h) = with_tags(160, 46, &["Clienti", "Lavoro"]) else {
        return;
    };
    // One line of nine dots: the seven colours, then the two of the user's own.
    assert_eq!(lines(&h), [(0, 9, 0)]);
    let th = h.app.settings.th.clone();
    let _ = h.screen();
    let dot_of = |h: &H, name: &str| {
        let r = h.app.frame.hits.find(&Target::Place(view_path(name)));
        let r = r.unwrap_or_else(|| panic!("{name} has no dot"));
        (r.x, r.y)
    };
    let red = TagColor::Red.label(&h.app.tags.lang);
    let (cx, cy) = dot_of(&h, "Clienti");
    let (lx, ly) = dot_of(&h, "Lavoro");
    let (rx, ry) = dot_of(&h, red);
    assert!(
        cy == ly && cy == ry && rx < cx && cx < lx,
        "one line, in order"
    );
    // Each is a dot in its colour; the one with none is gray.
    let buf = h.redrawn_buffer();
    assert_eq!(buf[(cx, cy)].symbol(), DOT);
    assert_eq!(buf[(cx, cy)].fg, th.tag_color(Some(TagColor::Blue)));
    assert_eq!(buf[(lx, ly)].symbol(), DOT);
    assert_eq!(buf[(lx, ly)].fg, th.tag_color(None));
    // A click opens the same list as in the classic sidebar.
    h.click(&Target::Place(view_path("Clienti")));
    h.wait("the tag list", |a| a.tab.cwd == view_path("Clienti"));
    assert!(h.screen().contains("Clienti"));
}

#[test]
fn the_keyboard_walks_the_dots_and_enter_opens_the_one_it_is_on() {
    let Some(mut h) = with_tags(160, 46, &["Clienti", "Lavoro"]) else {
        return;
    };
    h.press("tab");
    h.app.side_nav.nav_cursor = h
        .app
        .nav_rows()
        .iter()
        .position(|r| matches!(r.kind, NavKind::Tags { .. }))
        .unwrap();
    h.app.side_nav.nav_tag = 0;
    for _ in 0..7 {
        h.press("right");
    }
    assert_eq!(h.app.side_nav.nav_tag, 7, "the first of the user's own");
    h.press("right");
    h.press("right");
    assert_eq!(h.app.side_nav.nav_tag, 8, "and no further than the last");
    h.press("left");
    h.press("enter");
    h.wait("the tag list", |a| a.tab.cwd == view_path("Clienti"));
}

#[test]
fn many_tags_go_to_more_lines_and_the_arrows_follow_them() {
    let names: Vec<String> = (0..14).map(|i| format!("Tag{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    // 56 wide NAV would be needed for 21 dots on a line: the box is narrower.
    let Some(mut h) = with_tags(160, 46, &refs) else {
        return;
    };
    let l = lines(&h);
    assert!(l.len() >= 2, "a second line of dots: {l:?}");
    assert!(l.len() <= 3, "at most three lines: {l:?}");
    let shown: usize = l.iter().map(|(_, n, _)| n).sum();
    assert_eq!(shown + l.last().unwrap().2, 7 + 14);
    // Down from a dot goes to the dot below it, and the keyboard walks on to the next line.
    h.press("tab");
    let first = h
        .app
        .nav_rows()
        .iter()
        .position(|r| matches!(r.kind, NavKind::Tags { .. }))
        .unwrap();
    h.app.side_nav.nav_cursor = first;
    h.app.side_nav.nav_tag = 2;
    h.press("down");
    assert_eq!(h.app.side_nav.nav_cursor, first + 1);
    assert_eq!(h.app.side_nav.nav_tag, l[1].0 + 2, "the same column");
    h.press("up");
    assert_eq!(
        (h.app.side_nav.nav_cursor, h.app.side_nav.nav_tag),
        (first, 2)
    );
    h.app.side_nav.nav_tag = l[0].1 - 1;
    h.press("right");
    assert_eq!(h.app.side_nav.nav_cursor, first + 1, "on to the next line");
    assert_eq!(h.app.side_nav.nav_tag, l[1].0);
}

#[test]
fn no_dot_is_cut_in_a_narrow_box_and_what_does_not_fit_is_counted() {
    let names: Vec<String> = (0..40).map(|i| format!("Tag{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    for (w, hh) in [(160, 46), (110, 30), (78, 45), (140, 36)] {
        let Some(mut h) = with_tags(w, hh, &refs) else {
            return;
        };
        let buf = h.redrawn_buffer();
        assert!(h.app.frame.panels_shown, "{w}x{hh} is drawn as panels");
        let nav = h.app.frame.nav_area;
        let l = lines(&h);
        let shown: usize = l.iter().map(|(_, n, _)| n).sum();
        assert!(shown >= 7, "{w}x{hh}: the colours stay: {l:?}");
        assert!(l.last().unwrap().2 > 0, "{w}x{hh}: forty do not all fit");
        let mut seen = 0;
        for name in
            std::iter::once(TagColor::Red.label(&h.app.tags.lang).to_string()).chain(names.clone())
        {
            let Some(r) = h.app.frame.hits.find(&Target::Place(view_path(&name))) else {
                continue;
            };
            seen += 1;
            let inside = r.x >= nav.x && r.x + 1 < nav.x + nav.width;
            assert!(inside, "{w}x{hh}: {name} at {r:?} is in the box {nav:?}");
            assert_eq!(buf[(r.x, r.y)].symbol(), DOT, "{w}x{hh}: {name}");
        }
        assert!(seen >= 7, "{w}x{hh}: {seen} dots");
        let screen = h.screen();
        let more = l.last().unwrap().2;
        // Where "+N" has room it says how many are left out.
        assert!(
            screen.contains(&format!("+{more}")) || w < 100,
            "{w}x{hh}: +{more} in\n{screen}"
        );
    }
}

#[test]
fn a_short_box_gives_up_the_extra_lines_first_and_keeps_the_colours() {
    let names: Vec<String> = (0..14).map(|i| format!("Tag{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    // The least a panels window has: the box has room for the places, the disks and one line
    // of dots, and the others are counted ("+14").
    let Some(mut h) = with_tags(110, 24, &refs) else {
        return;
    };
    let _ = h.screen();
    assert!(h.app.frame.panels_shown);
    assert_eq!(lines(&h), [(0, 7, 14)]);
    assert!(h.screen().contains("+14"), "{}", h.screen());
    // One row more and the next line comes back, with what is left over counted.
    let Some(mut h) = with_tags(110, 25, &refs) else {
        return;
    };
    let _ = h.screen();
    assert_eq!(lines(&h), [(0, 9, 0), (9, 7, 5)]);
    assert!(h.screen().contains("+5"), "{}", h.screen());
}

#[test]
fn with_no_tags_of_the_users_own_the_row_is_the_seven_colours() {
    let Some(h) = with_tags(160, 46, &[]) else {
        return;
    };
    assert_eq!(lines(&h), [(0, 7, 0)]);
}

#[test]
fn the_status_bar_names_the_tag_under_the_pointer_or_the_keyboard() {
    let Some(mut h) = with_tags(160, 46, &["Clienti", "Lavoro"]) else {
        return;
    };
    assert!(!h.screen().contains("Lavoro"), "the dots carry no names");
    let _ = h.screen();
    let r = h
        .app
        .frame
        .hits
        .find(&Target::Place(view_path("Lavoro")))
        .unwrap();
    h.mouse(
        crossterm::event::MouseEventKind::Moved,
        r.x,
        r.y,
        crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(h.app.nav_tag_name().as_deref(), Some("Lavoro"));
    assert!(h.screen().contains("Lavoro  │"), "{}", h.screen());
    // The pointer goes away; the keyboard comes to the dot.
    h.mouse(
        crossterm::event::MouseEventKind::Moved,
        0,
        0,
        crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(h.app.nav_tag_name(), None);
    h.press("tab");
    h.app.side_nav.nav_cursor = h
        .app
        .nav_rows()
        .iter()
        .position(|r| matches!(r.kind, NavKind::Tags { .. }))
        .unwrap();
    h.app.side_nav.nav_tag = 8;
    assert_eq!(h.app.nav_tag_name().as_deref(), Some("Lavoro"));
}

#[test]
fn where_tags_are_not_kept_there_is_no_row_and_no_height_is_counted_for_one() {
    use rada_core::fs::LocalFs;
    use rada_core::testplatform::{MacLike, MacLikeSpec};
    use rada_tui::panels::NavMode;
    use std::sync::Arc;
    // The stand-in keeps no tags, as Windows does.
    for nav in [NavMode::Path, NavMode::Tree] {
        for hh in 24..=46 {
            let sb = rada_core::testutil::Sandbox::new();
            let home = sb.dirs.home.clone();
            for d in ["Documents", "Downloads", "projects/demo"] {
                std::fs::create_dir_all(home.join(d)).unwrap();
            }
            let p = Arc::new(MacLike::new(sb.platform(), MacLikeSpec::default()));
            let start = home.join("projects/demo");
            let mut h = H::with_system(sb, start, 110, hh, (Arc::new(LocalFs), p), |_| {});
            h.app.settings.layout_pref = rada_tui::LayoutPref::Panels;
            h.app.settings.nav_mode = nav;
            let s = h.screen();
            assert!(!h.app.tags_supported());
            assert!(h.app.frame.panels_shown, "{hh}");
            assert!(!s.contains("TAGS"), "{hh}:\n{s}");
            assert!(lines(&h).is_empty(), "{hh}");
            let rows = h.app.nav_rows();
            let doubled = rows
                .windows(2)
                .any(|w| w[0].kind == NavKind::Gap && w[1].kind == NavKind::Gap);
            assert!(
                !doubled,
                "{hh}: no second blank row where the tags would be"
            );
            assert_eq!(h.app.nav_tag_name(), None);
        }
    }
}
