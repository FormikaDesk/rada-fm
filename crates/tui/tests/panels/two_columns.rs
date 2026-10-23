//! The two-column arrangement: the preview in the place of NAV, the bars, the box titles.

use crate::common::*;

/// The rows of the box titled `title` in the left column of a screen, as they are drawn.
fn left_box(screen: &str, title: &str) -> Vec<String> {
    let mut rows = Vec::new();
    let mut inside = false;
    for line in screen.lines() {
        let cells: Vec<char> = line.chars().collect();
        // The left column is 28 cells at most.
        let col: String = cells.iter().take(28).collect();
        if col.contains(&format!("[ {title} ]")) {
            inside = true;
            continue;
        }
        if inside {
            if col.contains('╰') {
                break;
            }
            rows.push(col);
        }
    }
    rows
}

#[test]
fn nothing_in_spazio_ends_with_an_ellipsis_at_78_by_45() {
    for ring in [false, true] {
        let mut h = scene(78, 45, "rada", false);
        if ring {
            h.app.image_ui = Some(rada_tui::ImageUi::sized(
                ratatui_image::picker::ProtocolType::Kitty,
                (10, 20),
            ));
        }
        let s = h.screen();
        let rows = left_box(&s, "SPACE");
        assert!(rows.len() > 4, "{s}");
        for r in rows {
            let text = r.trim_end().trim_end_matches('│').trim_end();
            assert!(!text.ends_with('…'), "ring {ring}: {r:?}\n{s}");
        }
    }
}

#[test]
fn ctrl_b_takes_the_left_column_away_in_two_columns_too() {
    let mut h = scene(78, 45, "rada", false);
    assert!(h.screen().contains("[ NAV ]"));
    h.press("ctrl+b");
    let s = h.screen();
    assert!(!s.contains("[ NAV ]") && !s.contains("[ PREVIEW ]"), "{s}");
    assert!(
        s.contains("[ ~/projects/demo ]"),
        "the folder takes the room\n{s}"
    );
    h.press("ctrl+b");
    assert!(h.screen().contains("[ NAV ]"));
}

#[test]
fn the_pills_are_whole_in_two_columns_down_to_70() {
    for (w, hh) in [(70, 30), (78, 45), (100, 30)] {
        let mut h = scene(w, hh, "rada", true);
        let what = format!("{w}x{hh}");
        let behind = [h.app.th.inner, h.app.th.canvas];
        let pills = assert_pills(&h.redrawn_buffer(), &what, &behind);
        assert!(pills > 5, "{what}: only {pills} pills");
        h.press("alt+p");
        assert_pills(
            &h.redrawn_buffer(),
            &format!("{what}, the preview"),
            &behind,
        );
        h.press("tab");
        assert_pills(
            &h.redrawn_buffer(),
            &format!("{what}, keyboard in it"),
            &behind,
        );
    }
}

#[test]
fn the_boxes_have_their_names_on_their_top_edge() {
    let mut h = scene(160, 46, "rada", false);
    let s = h.screen();
    for title in [
        "[ NAV ]",
        "[ SPACE ]",
        "[ ~/projects/demo ]",
        "[ PREVIEW ]",
        "[ DETAILS ]",
    ] {
        assert!(s.contains(title), "{title}:\n{s}");
    }
    // Rounded corners, and the tree and the disks inside; the favourites only when the person
    // added some.
    assert!(s.contains("╭─[ NAV ]") && s.contains('╯'), "{s}");
    assert!(
        !s.contains("PINNED") && s.contains("System") && s.contains("Data"),
        "{s}"
    );
    // The size column is the number alone: no bars of any kind.
    assert!(!s.contains('▏') && !s.contains('▎'), "{s}");
}

/// The rows of the screen at `hh` rows tall and 110 wide, and where the boxes start and end.
fn bars_of(hh: u16) -> (Vec<String>, usize, usize) {
    let mut h = scene(110, hh, "rada", false);
    let lines: Vec<String> = h.screen().lines().map(str::to_string).collect();
    let first_box = lines.iter().position(|l| l.contains("[ NAV ]")).unwrap();
    let last_box = lines.iter().rposition(|l| l.contains('╯')).unwrap();
    (lines, first_box, last_box)
}

#[test]
fn from_42_rows_all_blank_rows_stay() {
    let (lines, first_box, last_box) = bars_of(42);
    // Tabs, navigation, buttons: each on its own row with a blank one after it.
    assert_eq!(first_box, 6);
    assert!(
        lines[last_box + 1].trim().is_empty(),
        "above the status bar"
    );
    assert!(lines[last_box + 2].contains("items"));
    assert!(
        lines[last_box + 3].trim().is_empty(),
        "under the status bar"
    );
}

#[test]
fn from_30_rows_two_blank_rows_stay() {
    for hh in [41, 34, 30] {
        let (lines, first_box, last_box) = bars_of(hh);
        assert_eq!(
            first_box, 4,
            "{hh} rows: the three bars, a blank row, the boxes"
        );
        assert!(lines[first_box - 1].trim().is_empty(), "{hh} rows");
        assert!(lines[last_box + 1].contains("items"), "{hh} rows");
        assert!(lines[last_box + 2].trim().is_empty(), "{hh} rows");
        assert!(lines[last_box + 3].contains("Ctrl+C") || lines[last_box + 3].contains("Enter"));
    }
}

#[test]
fn below_30_rows_the_blank_rows_go() {
    for hh in [29, 24] {
        let (lines, first_box, last_box) = bars_of(hh);
        assert_eq!(
            first_box, 3,
            "{hh} rows: the three bars, and the boxes right under"
        );
        assert!(lines[last_box + 1].contains("items"), "{hh} rows");
        assert!(lines[last_box + 2].contains("Ctrl+C") || lines[last_box + 2].contains("Enter"));
    }
}

#[test]
fn ctrl_b_takes_the_left_column_away() {
    let mut h = scene(160, 46, "rada", false);
    h.press("ctrl+b");
    let s = h.screen();
    assert!(!s.contains("[ NAV ]") && !s.contains("[ SPACE ]"), "{s}");
    assert!(s.contains("[ ~/projects/demo ]") && s.contains("[ PREVIEW ]"));
    h.press("ctrl+b");
    assert!(h.screen().contains("[ NAV ]"));
}
