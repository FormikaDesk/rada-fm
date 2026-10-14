//! The size of folders in the Size column and the details pane: "···" while the background
//! worker counts, the figure after, the choices of `folder_sizes`, the order by size, and the
//! counting of a folder being dropped when another one is opened.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_core::foldersize::{SizeLimits, SizeMode, SizeSettings};
use rada_core::testutil::*;
use rada_tui::fmt;

fn settings(mode: SizeMode) -> SizeSettings {
    SizeSettings {
        mode,
        limits: SizeLimits::default(),
    }
}

/// `d` holds `small` (a file of 2000 bytes below) and `big` (300 000 bytes in two files), and a
/// file `f`.
fn scene(w: u16, h: u16) -> H {
    let sb = Sandbox::new();
    sb.write("d/small/a", vec![0u8; 2000]);
    sb.write("d/big/a", vec![0u8; 100_000]);
    sb.write("d/big/deeper/b", vec![0u8; 200_000]);
    sb.write("d/f", "x");
    let dir = sb.path("d");
    let mut h = H::new(sb, dir, w, h);
    h.wait("the listing", |a| a.listing.len() == 3);
    h.app.details = Some(false); // the pane would cover the right of the list
    h
}

fn row_of(h: &mut H, name: &str) -> String {
    h.screen()
        .lines()
        .find(|l| l.contains(name))
        .unwrap_or_else(|| panic!("no row for {name}"))
        .to_string()
}

fn counted(h: &mut H) {
    h.wait("the sizes", |a| {
        a.listing.all().iter().filter(|e| e.is_dir()).all(|e| {
            !matches!(
                a.folder_size_view(e),
                rada_tui::app::FolderSizeView::Counting
            )
        })
    });
}

#[test]
fn folders_show_dots_while_counted_and_the_size_after() {
    let mut h = scene(120, 30);
    h.app.set_folder_size_settings(settings(SizeMode::Auto));
    // Drawn before the answer is read: the count has been asked for, not heard.
    let row = row_of(&mut h, "small");
    assert!(row.contains("···"), "{row}");
    counted(&mut h);
    assert!(
        row_of(&mut h, "small").contains(&fmt::size(2000)),
        "{}",
        h.screen()
    );
    assert!(
        row_of(&mut h, "big").contains(&fmt::size(300_000)),
        "{}",
        h.screen()
    );
    // Files keep their own size, and nothing is invented for them.
    // (The row of the file `f`, not the bar's "Search files…".)
    assert!(
        h.screen()
            .lines()
            .any(|l| l.contains(" f ") && l.contains("1 B")),
        "{}",
        h.screen()
    );
}

#[test]
fn off_leaves_the_column_empty_for_folders() {
    let mut h = scene(120, 30);
    h.app.set_folder_size_settings(settings(SizeMode::Off));
    h.pump(300);
    let s = h.screen();
    assert!(!s.contains("···"), "{s}");
    assert!(!row_of(&mut h, "small").contains("KB"), "{s}");
}

#[test]
fn the_items_mode_shows_how_many_items_each_folder_holds() {
    let mut h = scene(120, 30);
    h.app.set_folder_size_settings(settings(SizeMode::Items));
    counted(&mut h);
    let big = row_of(&mut h, "big");
    assert!(big.contains("2 items"), "{big}");
    let small = row_of(&mut h, "small");
    assert!(small.contains("1 item"), "{small}");
}

#[test]
fn a_count_that_hits_its_limit_says_more_than() {
    let mut h = scene(120, 30);
    h.app.set_folder_size_settings(SizeSettings {
        mode: SizeMode::Auto,
        limits: SizeLimits {
            max_entries: 2,
            ..SizeLimits::default()
        },
    });
    counted(&mut h);
    let big = row_of(&mut h, "big");
    assert!(big.contains("> "), "{big}");
    assert!(
        !big.contains(&fmt::size(300_000)),
        "not the whole figure: {big}"
    );
}

#[test]
fn ordering_by_size_uses_the_counted_size_of_folders() {
    let mut h = scene(120, 30);
    h.keys("s"); // by size
    // Nothing counted yet: folders are alike and stay in name order.
    assert_eq!(h.names(), ["big", "small", "f"]);
    h.app.set_folder_size_settings(settings(SizeMode::Auto));
    counted(&mut h);
    h.pump(50);
    // The smaller folder comes first, then the larger, before the files.
    assert_eq!(h.names(), ["small", "big", "f"], "{}", h.screen());
    h.keys("S");
    assert_eq!(h.names(), ["big", "small", "f"], "{}", h.screen());
}

#[test]
fn the_details_pane_says_how_many_items_and_how_big() {
    let mut h = scene(170, 35);
    h.app.details = Some(true);
    h.app.set_folder_size_settings(settings(SizeMode::Auto));
    counted(&mut h);
    h.keys("g");
    while h.cursor_name() != "big" {
        h.key(KeyCode::Down);
    }
    h.pump(50);
    let s = h.screen();
    assert!(
        s.contains(&format!("2 items · {}", fmt::size(300_000))),
        "{s}"
    );
}

#[test]
fn leaving_a_folder_drops_what_was_being_counted_and_the_next_is_counted() {
    let sb = Sandbox::new();
    sb.write("a/one/x", vec![0u8; 500]);
    sb.write("b/two/y", vec![0u8; 700]);
    let dir = sb.path("a");
    let other = sb.path("b");
    let mut h = H::new(sb, dir, 120, 30);
    h.app.details = Some(false);
    h.app.set_folder_size_settings(settings(SizeMode::Auto));
    h.wait("the listing", |a| a.listing.len() == 1);
    let _ = h.screen();
    // Away before the answer is read.
    h.app.open_dir(other);
    h.wait("the other folder", |a| {
        a.listing.all().iter().any(|e| e.display == "two")
    });
    counted(&mut h);
    let row = row_of(&mut h, "two");
    assert!(row.contains(&fmt::size(700)), "{row}");
}

#[test]
fn a_folder_that_appears_is_counted_and_one_that_changes_is_counted_again() {
    let mut h = scene(120, 30);
    h.app.set_folder_size_settings(settings(SizeMode::Auto));
    counted(&mut h);
    let dir = h.app.cwd.clone();
    // A new folder arrives through the watcher.
    std::fs::create_dir_all(dir.join("fresh")).unwrap();
    std::fs::write(dir.join("fresh/n"), vec![0u8; 4000]).unwrap();
    h.wait("the new folder", |a| {
        a.listing.all().iter().any(|e| e.display == "fresh")
    });
    counted(&mut h);
    let row = row_of(&mut h, "fresh");
    assert!(row.contains(&fmt::size(4000)), "{row}");
    // A folder replaced by one of another size: the old figure is not kept.
    std::fs::remove_dir_all(dir.join("small")).unwrap();
    std::fs::create_dir_all(dir.join("small")).unwrap();
    std::fs::write(dir.join("small/a"), vec![0u8; 9000]).unwrap();
    h.wait("the new figure", |a| {
        a.listing.all().iter().any(|e| {
            e.display == "small"
                && matches!(
                    a.folder_size_view(e),
                    rada_tui::app::FolderSizeView::Counted(c) if c.bytes == Some(9000)
                )
        })
    });
}
