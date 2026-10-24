//! The file chooser's preview: beside the list from 70 columns (the navigation pane narrows to
//! its icons), in the place of the list below that when asked for with Alt+P, and never the
//! advice to enlarge the window, whatever layout `config.toml` asks for.

mod common;

use std::time::{Duration, SystemTime};

use common::*;
use rada_core::testutil::Sandbox;
use rada_tui::chooser::Chooser;
use rada_tui::sidebar::Mode;
use rada_tui::ui::DetailsMode;
use rada_tui::{LayoutPref, Theme};

/// 2030-01-01 12:00 UTC: far from "now", so a stray real clock would be obvious.
fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// A chooser of files in `~/demo/projects`, with the layout asked for in `config.toml`, the
/// cursor on README.md.
fn chooser(w: u16, h: u16, pref: LayoutPref) -> H {
    let sb = Sandbox::new();
    let dir = sb.dirs.home.join("demo/projects");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("README.md"),
        "# Projects\n\nA few notes about what lives here.\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    let mut h = H::with_config(sb, dir, w, h, move |c| {
        c.chooser = Some(Chooser::file(true));
        c.layout_pref = pref;
        c.theme = Theme::rada();
    });
    h.wait("the places", |a| !a.paths.places.is_empty());
    h.wait("the volumes", |a| a.devices.volumes_seen);
    h.app.hooks.clock = Some(fixed_now());
    h.app.tz = jiff::tz::TimeZone::UTC;
    h.app.devices.volumes.clear();
    while h.cursor_name() != "README.md" {
        h.keys("j");
    }
    h.wait("the preview", |a| a.preview.name == "README.md");
    h
}

fn shot(h: &mut H) -> String {
    h.screen()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAG row, which the Linux snapshot has"
)]
fn preview_beside_the_list_at_78x45() {
    let mut h = chooser(78, 45, LayoutPref::Auto);
    let screen = shot(&mut h);
    assert!(screen.contains("A few notes"), "{screen}");
    insta::assert_snapshot!("chooser_78x45", screen);
}

#[test]
fn panels_in_config_never_ask_for_a_bigger_window() {
    for (w, rows) in [(78, 45), (60, 30), (90, 25), (40, 20)] {
        let mut h = chooser(w, rows, LayoutPref::Panels);
        let screen = h.screen();
        assert!(!screen.contains("Enlarge"), "{w}x{rows}: {screen}");
        assert!(!h.app.size_notice_visible && !h.app.panels_shown);
    }
}

#[test]
fn the_preview_comes_at_70_columns_and_the_sidebar_narrows() {
    let mut h = chooser(70, 30, LayoutPref::Auto);
    assert_eq!(h.app.details_shown, DetailsMode::Column);
    assert_eq!(h.app.side_mode, Mode::Rail);
    assert!(h.screen().contains("A few notes"));
    let mut h = chooser(69, 30, LayoutPref::Auto);
    assert_eq!(h.app.details_shown, DetailsMode::Hidden);
    assert!(!h.screen().contains("A few notes"));
}

#[test]
fn alt_p_below_70_columns_swaps_the_list_for_the_preview_and_back() {
    let mut h = chooser(60, 30, LayoutPref::Auto);
    let list = h.screen();
    assert!(list.contains("Name") && list.contains("main.rs"), "{list}");
    h.press("alt+p");
    let preview = h.screen();
    assert!(preview.contains("A few notes"), "{preview}");
    assert!(
        !preview.contains("main.rs"),
        "the list gives way: {preview}"
    );
    h.press("alt+p");
    assert_eq!(h.screen(), list);
}

#[test]
fn alt_p_from_70_columns_leaves_the_preview_where_it_is() {
    let mut h = chooser(90, 30, LayoutPref::Auto);
    let before = h.screen();
    h.press("alt+p");
    assert_eq!(h.screen(), before);
    assert!(before.contains("A few notes"));
}
