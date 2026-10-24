//! The panels drawn as text and as a map of colour tokens, at the sizes that matter.

use crate::common::*;
use crate::harness::*;
use rada_tui::compat::{CompatBackend, Rendering};
use rada_tui::panels::TWO_SPACE_FROM_ROWS;
use rada_tui::{IconSet, LayoutPref, Theme, ui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

fn shot(h: &mut H) -> String {
    h.screen()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Every cell reduced to the theme token that colours it: the background first (the box, what
/// is around it, the selection, a pill), then the text.
fn token_map(h: &mut H, th: &Theme) -> String {
    let buf = h.redrawn_buffer();
    let hues = [
        (th.hues.cyan, 'n'),
        (th.hues.pink, 'p'),
        (th.hues.yellow, 'y'),
        (th.hues.green, 'g'),
        (th.hues.orange, 'o'),
        (th.hues.blue, 'b'),
    ];
    let letter = |c: Color, background: bool| -> char {
        if c == Color::Reset {
            return '.';
        }
        let mut tokens = vec![
            (th.canvas, 'c'),
            (th.inner, 'i'),
            (th.cursor, 'C'),
            (th.sel, 'S'),
            (th.accent, 'A'),
            (th.selection, 'E'),
            (th.button, 'U'),
            (th.field, 'F'),
            (th.key_bg, 'K'),
            (th.mark, 'M'),
        ];
        if !background {
            tokens = vec![
                (th.text, 't'),
                (th.text_dim, 'd'),
                (th.muted, 'm'),
                (th.accent, 'A'),
                (th.on_accent, 'O'),
                (th.cursor_text, 'W'),
                (th.success, 's'),
                (th.warn, 'w'),
                (th.error, 'e'),
                (th.disk, 'D'),
                (th.check, 'k'),
            ];
        }
        if let Some((_, l)) = tokens.iter().find(|(t, _)| *t == c) {
            return *l;
        }
        if !background && let Some((_, l)) = hues.iter().find(|(t, _)| *t == c) {
            return *l;
        }
        '+'
    };
    let mut out = String::from(
        "background: c canvas, i inner, C cursor, S sel, A accent, E selection, U button,\n\
         F field, K key_bg, M mark, + a tint, . none\n\
         text: t text, d dim, m muted, A accent, O on_accent, W cursor_text, s success, w warn,\n\
         e error, D disk, k check, n cyan, p pink, y yellow, g green, o orange, b blue, + other\n\n\
         background\n",
    );
    for y in 0..buf.area.height {
        out.extend((0..buf.area.width).map(|x| letter(buf[(x, y)].bg, true)));
        out.push('\n');
    }
    out.push_str("\ntext\n");
    for y in 0..buf.area.height {
        out.extend((0..buf.area.width).map(|x| letter(buf[(x, y)].fg, false)));
        out.push('\n');
    }
    out
}

fn assert_fits(h: &mut H, w: u16, hh: u16) {
    for l in h.screen().lines() {
        assert!(
            l.trim_end().width() <= w as usize,
            "{w}x{hh}: a line is wider than the screen: {l:?}"
        );
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn the_panels_at_160_by_46_in_each_theme() {
    for name in THEMES {
        let mut h = scene(160, 46, name, false);
        assert!(
            h.app.frame.panels_shown && !h.app.frame.panels_two,
            "{name}: three columns at 160x46"
        );
        let th = h.app.settings.th.clone();
        let tag = name.replace('-', "_");
        insta::assert_snapshot!(format!("panels_160x46_{tag}"), shot(&mut h));
        insta::assert_snapshot!(
            format!("panels_160x46_{tag}_tokens"),
            token_map(&mut h, &th)
        );
        assert_fits(&mut h, 160, 46);
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn the_panels_at_the_smallest_window_that_gets_them() {
    let mut h = scene(140, 36, "rada", false);
    assert!(h.app.frame.panels_shown);
    insta::assert_snapshot!("panels_140x36", shot(&mut h));
    assert_fits(&mut h, 140, 36);
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the sidebar of a system with no tags has no TAGS section, which the Linux snapshot has"
)]
fn small_window_gets_classic_layout() {
    let mut h = scene(69, 30, "rada", false);
    assert!(!h.app.frame.panels_shown, "69x30 is classic");
    let s = shot(&mut h);
    assert!(!s.contains("Enlarge"), "auto asks nothing:\n{s}");
    insta::assert_snapshot!("panels_classic_69x30", s);
    // With `layout = "panels"` the window is told what is wanted.
    h.app.settings.layout_pref = LayoutPref::Panels;
    let s = shot(&mut h);
    assert!(
        s.contains("Enlarge the window for the full layout (now 69×30)"),
        "{s}"
    );
    for (w, hh) in [(69, 46), (100, 23), (160, 23)] {
        h.term.backend_mut().resize(w, hh);
        let _ = h.screen();
        assert!(!h.app.frame.panels_shown, "{w}x{hh} is classic");
    }
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn the_two_columns_at_78_by_45_and_at_100_by_30() {
    for (w, hh) in [(78, 45), (100, 30)] {
        let mut h = scene(w, hh, "rada", false);
        assert!(
            h.app.frame.panels_shown && h.app.frame.panels_two,
            "{w}x{hh}"
        );
        let s = shot(&mut h);
        assert!(
            !s.contains("Enlarge"),
            "{w}x{hh}: no advice to enlarge\n{s}"
        );
        assert!(
            s.contains("[ NAV ]") && s.contains("[ ~/projects/demo ]"),
            "{s}"
        );
        assert!(
            !s.contains("[ PREVIEW ]"),
            "{w}x{hh}: the preview is not there\n{s}"
        );
        // SPAZIO only where there are rows for it.
        assert_eq!(
            s.contains("[ SPACE ]"),
            hh >= TWO_SPACE_FROM_ROWS,
            "{w}x{hh}\n{s}"
        );
        insta::assert_snapshot!(format!("panels_{w}x{hh}"), s);
        assert_fits(&mut h, w, hh);
    }
}

/// What the terminal gets with the `unicode-std` icon set: after the stand-ins of `compat.rs`.
fn sent_with_unicode_std(h: &mut H, w: u16, hh: u16) -> String {
    let how = Rendering::PLAIN.with_std_glyphs();
    let mut term = Terminal::new(CompatBackend::new(TestBackend::new(w, hh), how)).unwrap();
    term.draw(|f| ui::draw(f, &mut h.app)).unwrap();
    let buf = term.backend().inner().buffer();
    let rows: Vec<String> = (0..hh)
        .map(|y| {
            let row: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
            row.trim_end().to_string()
        })
        .collect();
    rows.join("\n") + "\n"
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn unicode_std_at_110x30() {
    let mut h = scene(110, 30, "rada", false);
    h.app.settings.icons = IconSet::UnicodeStd;
    h.settle();
    let s = sent_with_unicode_std(&mut h, 110, 30);
    assert!(s.contains("[ NAV ]") && !s.contains("[ PREVIEW ]"), "{s}");
    insta::assert_snapshot!("panels_110x30_unicode_std", s);
}
