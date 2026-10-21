//! The discreet line that asks for a bigger window when the explorer does not fit.

mod common;

use common::*;
use crossterm::event::KeyCode;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn resize(h: &mut H, w: u16, rows: u16) {
    h.term = Terminal::new(TestBackend::new(w, rows)).unwrap();
}

/// A window of `w` × `rows` where the panels were asked for (`layout = "panels"`): the only
/// setting with an advice to enlarge the window.
fn asked_for_panels(w: u16, rows: u16) -> H {
    let (sb, dir) = sandbox_with_files();
    H::with_config(sb, dir, w, rows, |c| {
        c.layout_pref = rada_tui::LayoutPref::Panels
    })
}

#[test]
fn a_small_window_is_told_how_big_the_full_layout_is_wanted() {
    let mut h = asked_for_panels(69, 24);
    let s = h.screen();
    assert!(
        s.contains("Enlarge the window for the full layout (now 69×24)"),
        "{s}"
    );
    assert!(h.app.size_notice_visible);
    // A window short rather than narrow is told too.
    resize(&mut h, 120, 20);
    assert!(h.screen().contains("(now 120×20)"));
}

#[test]
fn a_window_that_is_big_enough_is_told_nothing() {
    let mut h = asked_for_panels(100, 24);
    assert!(!h.screen().contains("Enlarge the window"));
    assert!(!h.app.size_notice_visible);
    resize(&mut h, 160, 40);
    assert!(!h.screen().contains("Enlarge the window"));
}

#[test]
fn esc_closes_it_for_the_rest_of_the_session() {
    let mut h = asked_for_panels(69, 24);
    assert!(h.screen().contains("Enlarge the window"));
    h.key(KeyCode::Esc);
    assert!(!h.screen().contains("Enlarge the window"));
    // Not again, however the window changes.
    resize(&mut h, 60, 20);
    assert!(!h.screen().contains("Enlarge the window"));
    resize(&mut h, 140, 40);
    resize(&mut h, 70, 22);
    assert!(!h.screen().contains("Enlarge the window"));
}

#[test]
fn esc_does_what_it_always_did_first() {
    let mut h = asked_for_panels(69, 24);
    h.keys("j ");
    assert!(!h.marked_names().is_empty());
    h.key(KeyCode::Esc);
    assert!(h.marked_names().is_empty(), "the selection goes first");
    assert!(h.screen().contains("Enlarge the window"));
    h.key(KeyCode::Esc);
    assert!(!h.screen().contains("Enlarge the window"));
}

#[test]
fn the_compact_layout_is_a_small_window_on_purpose() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::with_config(sb, dir, 80, 24, |c| {
        c.layout = rada_tui::LayoutKind::Compact
    });
    assert!(!h.screen().contains("Enlarge the window"));
}

#[test]
fn a_window_too_short_to_spare_a_row_is_left_alone() {
    let mut h = asked_for_panels(80, 11);
    assert!(!h.screen().contains("Enlarge the window"));
}

#[test]
fn the_line_never_runs_past_a_narrow_window() {
    let mut h = asked_for_panels(30, 16);
    let s = h.screen();
    let line = s
        .lines()
        .find(|l| l.contains("Enlarge"))
        .expect("the advice");
    assert!(
        unicode_width::UnicodeWidthStr::width(line) <= 30,
        "{line:?}"
    );
    assert!(line.contains('…'));
}

#[test]
fn with_auto_the_view_is_rada_s_to_choose_and_nothing_is_asked() {
    // Small windows, the sizes of three windows side by side among them.
    for (w, rows) in [(78, 22), (69, 24), (60, 20), (100, 23)] {
        let (sb, dir) = sandbox_with_files();
        let mut h = H::with_config(sb, dir, w, rows, |c| {
            c.layout_pref = rada_tui::LayoutPref::Auto
        });
        assert!(!h.screen().contains("Enlarge the window"), "{w}x{rows}");
        assert!(!h.app.size_notice_visible, "{w}x{rows}");
    }
}

#[test]
fn with_classic_it_is_never_asked() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::with_config(sb, dir, 69, 24, |c| {
        c.layout_pref = rada_tui::LayoutPref::Classic
    });
    assert!(!h.screen().contains("Enlarge the window"));
}
