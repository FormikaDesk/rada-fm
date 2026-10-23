//! Which layout a window gets, and the View menu that chooses it.

use crate::common::*;
use crate::harness::*;
use rada_tui::LayoutPref;
use unicode_width::UnicodeWidthStr;

#[test]
fn a_window_of_78_by_22_is_classic_with_no_advice_no_date_and_the_longest_name_whole() {
    // Three windows side by side on a laptop: too short for two columns, and rada's to choose.
    let mut h = scene(78, 22, "rada", false);
    assert!(!h.app.panels_shown);
    let s = h.screen();
    assert!(!s.contains("Enlarge"), "{s}");
    assert!(
        !s.contains("Date modified"),
        "no date column below 90 columns:\n{s}"
    );
    let longest = h
        .names()
        .into_iter()
        .max_by_key(|n| n.width())
        .expect("files");
    // (The list is short at 22 rows: the cursor goes to the name, which scrolls it into view.)
    put_cursor(&mut h, &longest);
    let s = h.screen();
    assert!(s.contains(&longest), "{longest} is whole:\n{s}");
}

#[test]
fn the_date_column_goes_below_90_columns_in_the_classic_list_too() {
    let mut h = demo_scene(89, 30);
    assert!(!h.screen().contains("Date modified"));
    h.term.backend_mut().resize(90, 30);
    assert!(h.screen().contains("Date modified"));
}

#[test]
fn the_preview_command_puts_the_preview_in_the_place_of_nav_and_back() {
    let mut h = scene(78, 45, "rada", false);
    let on_screen = |h: &mut H| {
        let s = h.screen();
        (s.contains("[ NAV ]"), s.contains("[ PREVIEW ]"))
    };
    assert_eq!(on_screen(&mut h), (true, false));
    h.press("alt+p");
    assert_eq!(
        on_screen(&mut h),
        (false, true),
        "the preview is where NAV was"
    );
    assert!(
        h.screen().contains("photo.jpg"),
        "it shows the file under the cursor"
    );
    h.press("alt+p");
    assert_eq!(
        on_screen(&mut h),
        (true, false),
        "the same command goes back"
    );
    // So does the item of the View menu, "Details pane".
    choose_in_view_menu(&mut h, "Details pane");
    assert_eq!(on_screen(&mut h), (false, true));
    // Tab takes the keyboard to the box on the left, whichever it is, and back.
    h.press("tab");
    assert!(h.app.preview_focus && !h.app.side_focus);
    h.press("alt+p");
    assert!(
        h.app.side_focus && !h.app.preview_focus,
        "the keyboard follows the box"
    );
    h.press("tab");
    assert!(!h.app.side_focus && !h.app.preview_focus);
    // Where there are three columns the command is left as it was: both boxes are there.
    h.term.backend_mut().resize(160, 46);
    assert!(h.screen().contains("[ NAV ]") && h.screen().contains("[ PREVIEW ]"));
}

/// Open the View menu and walk to the entry whose label contains `label`, then choose it.
fn choose_in_view_menu(h: &mut H, label: &str) {
    use rada_tui::app::{MenuKind, Modal};
    h.app.open_menu(MenuKind::View, (60, 4));
    let selected = |h: &H| match &h.app.modal {
        Some(Modal::Menu(m)) => m
            .items
            .get(m.selected)
            .map(|i| i.label.clone())
            .unwrap_or_default(),
        _ => String::new(),
    };
    for _ in 0..30 {
        if selected(h).contains(label) {
            break;
        }
        h.press("down");
    }
    assert!(selected(h).contains(label), "{label}: {}", h.screen());
    h.press("enter");
}

#[test]
fn the_view_menu_chooses_the_layout_and_the_window_follows() {
    use rada_tui::app::{MenuKind, Modal};
    let mut h = scene(120, 40, "rada", false);
    assert!(
        h.app.panels_shown && h.app.panels_two,
        "auto at 120x40 is two columns"
    );
    // The three layouts are in the menu, the one in use ticked.
    h.app.open_menu(MenuKind::View, (60, 4));
    let Some(Modal::Menu(m)) = &h.app.modal else {
        panic!("a menu");
    };
    let ticked: Vec<&str> = m
        .items
        .iter()
        .filter(|i| i.label.starts_with("Layout") && i.checked == Some(true))
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(ticked, ["Layout: automatic"]);
    let s = h.screen();
    for l in ["Layout: automatic", "Layout: panels", "Layout: classic"] {
        assert!(s.contains(l), "{l}:\n{s}");
    }
    h.press("esc");

    // Panels asked for: drawn in a window that auto would give the classic layout.
    choose_in_view_menu(&mut h, "panels");
    assert_eq!(h.app.layout_pref, LayoutPref::Panels);
    let _ = h.screen();
    assert!(
        h.app.panels_shown && !h.app.panels_two,
        "three columns at 120x40 when asked for"
    );
    // Two in a window with room for two only, and none where there is no room.
    h.term.backend_mut().resize(90, 40);
    let _ = h.screen();
    assert!(h.app.panels_shown && h.app.panels_two);
    h.term.backend_mut().resize(65, 40);
    let _ = h.screen();
    assert!(!h.app.panels_shown);
    h.term.backend_mut().resize(160, 46);
    // Classic asked for: never panels.
    choose_in_view_menu(&mut h, "classic");
    let _ = h.screen();
    assert!(!h.app.panels_shown, "classic at 160x46 when asked for");
    // Automatic again: the size decides, by itself, as the window is made larger and smaller.
    choose_in_view_menu(&mut h, "automatic");
    for (w, hh, panels, two) in [
        (160, 46, true, false),
        (139, 46, true, true),
        (140, 36, true, false),
        (140, 35, true, true),
        (70, 24, true, true),
        (69, 24, false, false),
        (200, 60, true, false),
    ] {
        h.term.backend_mut().resize(w, hh);
        let _ = h.screen();
        assert_eq!(h.app.panels_shown, panels, "{w}x{hh}");
        assert_eq!(h.app.panels_two, two, "{w}x{hh}");
    }
}

#[test]
fn a_file_chooser_always_draws_the_classic_layout() {
    for pref in [LayoutPref::Auto, LayoutPref::Panels] {
        let sb = rada_core::testutil::Sandbox::new();
        sb.write("w/a.txt", "a");
        let dir = sb.path("w");
        let mut h = H::with_config(sb, dir, 200, 60, move |c| {
            c.chooser = Some(rada_tui::chooser::Chooser::file(false));
            c.layout_pref = pref;
        });
        let s = h.screen();
        assert!(!h.app.panels_shown, "{pref:?}: a chooser is classic");
        assert!(!s.contains("[ NAV ]") && s.contains("a.txt"), "{s}");
    }
}

#[test]
fn the_compact_arrangement_never_has_panels() {
    let sb = rada_core::testutil::Sandbox::new();
    let dir = sb.mkdir("w");
    let mut h = H::with_config(sb, dir, 200, 60, |c| {
        c.layout = rada_tui::LayoutKind::Compact;
        c.layout_pref = LayoutPref::Panels;
    });
    let _ = h.screen();
    assert!(!h.app.panels_shown);
}
