//! The sidebar: standard places under their real names, bookmarks and disks, the icon-only
//! column on narrow terminals, keyboard focus, the mouse, and the menu.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::*;
use crossterm::event::{KeyCode, MouseEventKind};
use rada_core::platform::{Volume, VolumeKind};
use rada_core::testutil::*;
use rada_tui::app::Modal;
use rada_tui::hits::Target;

const ITALIAN: &str = r#"XDG_DESKTOP_DIR="$HOME/Scrivania"
XDG_DOWNLOAD_DIR="$HOME/Scaricati"
XDG_DOCUMENTS_DIR="$HOME/Documenti"
XDG_MUSIC_DIR="$HOME/Musica"
XDG_PICTURES_DIR="$HOME/Immagini"
XDG_VIDEOS_DIR="$HOME/Video"
"#;

fn volume(mount: &str, label: &str, kind: VolumeKind, total_gb: u64, free_gb: u64) -> Volume {
    let gb = 1_000_000_000;
    Volume {
        mount_point: PathBuf::from(mount),
        label: Some(label.into()),
        fs_type: "ext4".into(),
        device: "test".into(),
        kind,
        drive_letter: None,
        total: Some(total_gb * gb),
        available: Some(free_gb * gb),
        read_only: false,
        responsive: true,
    }
}

/// An Italian home: Scaricati and Documenti exist (Immagini and the rest do not), a folder
/// to work in, two bookmarks and two disks.
fn italian(w: u16, h: u16) -> H {
    italian_with(w, h, |_| {})
}

/// [`italian`], with the configuration adjusted by `tweak`.
fn italian_with(w: u16, h: u16, tweak: impl FnOnce(&mut rada_tui::Config) + 'static) -> H {
    let sb = Sandbox::new();
    std::fs::create_dir_all(&sb.dirs.config).unwrap();
    std::fs::write(sb.dirs.config.join("user-dirs.dirs"), ITALIAN).unwrap();
    let home = sb.dirs.home.clone();
    for d in ["Scaricati", "Documenti", "lavoro/progetto", "lavoro/altro"] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    std::fs::write(home.join("Scaricati/a.txt"), "a").unwrap();
    // Two bookmarks, in the file the places worker reads.
    std::fs::create_dir_all(sb.dirs.rada_state()).unwrap();
    std::fs::write(
        sb.dirs.rada_state().join("bookmarks.json"),
        format!(
            "[{:?}, {:?}]",
            home.join("lavoro/progetto").to_str().unwrap(),
            home.join("lavoro/altro").to_str().unwrap()
        ),
    )
    .unwrap();
    let mut h = H::with_config(sb, home.join("Scaricati"), w, h, tweak);
    h.wait("the places and bookmarks", |a| {
        a.places.paths.places.len() >= 3 && a.places.paths.bookmarks.len() == 2
    });
    set_volumes(&mut h);
    h
}

/// Two disks, as if the system had reported them (the real list depends on the machine).
fn set_volumes(h: &mut H) {
    h.app.devices.volumes = vec![
        volume("/", "System", VolumeKind::Fixed, 500, 200),
        volume(
            "/run/media/user/USB",
            "USB drive",
            VolumeKind::Removable,
            64,
            40,
        ),
    ];
}

fn place(h: &H, name: &str) -> PathBuf {
    h.app.home().join(name)
}

#[test]
fn sidebar_lists_existing_places_by_name() {
    let mut h = italian(150, 30);
    let s = h.screen();
    for needle in [
        "PINNED",
        "Home",
        "Documenti",
        "Scaricati",
        "progetto",
        "altro",
        "DEVICES",
        "System",
        "USB drive",
    ] {
        assert!(s.contains(needle), "{needle}:\n{s}");
    }
    for absent in [
        "Immagini",
        "Musica",
        "Video",
        "Scrivania",
        "Trash",
        "Downloads",
    ] {
        assert!(!s.contains(absent), "{absent} does not exist here:\n{s}");
    }
}

#[test]
fn the_current_place_is_highlighted_and_a_click_opens_an_item() {
    let mut h = italian(150, 30);
    let _ = h.screen();
    let scaricati = place(&h, "Scaricati");
    let r = h
        .app
        .hits
        .find(&Target::Place(scaricati))
        .expect("on screen");
    let buf = h.term.backend().buffer().clone();
    assert_eq!(
        buf[(r.x + 4, r.y)].bg,
        h.app.th.sel,
        "the place we are in has its own background"
    );
    let docs = place(&h, "Documenti");
    h.click(&Target::Place(docs.clone()));
    h.wait("Documenti", move |a| a.cwd == docs && !a.is_loading());
    // And a bookmark.
    let b = place(&h, "lavoro/progetto");
    h.click(&Target::Place(b.clone()));
    h.wait("the bookmark", move |a| a.cwd == b && !a.is_loading());
}

#[test]
fn tab_focuses_the_sidebar_enter_opens() {
    let mut h = italian(150, 30);
    let _ = h.screen();
    h.key(KeyCode::Tab);
    assert!(h.app.side_nav.focus);
    let _ = h.screen();
    let items = h.app.side_items();
    let cur = h.app.side_nav.side_cursor;
    assert_eq!(
        items[cur].path,
        place(&h, "Scaricati"),
        "starts on the current place"
    );
    let r = h
        .app
        .hits
        .find(&Target::Place(items[cur].path.clone()))
        .unwrap();
    let buf = h.term.backend().buffer().clone();
    assert_eq!(buf[(r.x + 4, r.y)].bg, h.app.th.cursor, "focus is visible");

    // Up to the previous item (Documenti), and open it.
    h.key(KeyCode::Up);
    let docs = place(&h, "Documenti");
    assert_eq!(h.app.side_items()[h.app.side_nav.side_cursor].path, docs);
    h.key(KeyCode::Enter);
    assert!(!h.app.side_nav.focus, "the focus goes back to the list");
    h.wait("Documenti", move |a| a.cwd == docs && !a.is_loading());

    // Tab again and Esc: out.
    h.key(KeyCode::Tab);
    assert!(h.app.side_nav.focus);
    h.key(KeyCode::Esc);
    assert!(!h.app.side_nav.focus);
}

#[test]
fn file_operations_are_not_done_behind_the_sidebars_back() {
    let mut h = italian(150, 30);
    h.key(KeyCode::Tab);
    h.press("ctrl+c");
    assert!(h.app.ops.clipboard.is_none(), "nothing was copied");
    assert!(h.app.current_notice().is_some(), "and it was said why");
    assert!(h.app.side_nav.focus);
}

#[test]
fn narrow_terminals_get_a_column_of_icons_with_the_places_only() {
    let mut h = italian(90, 30);
    let s = h.screen();
    assert!(!s.contains("PINNED") && !s.contains("DEVICES"), "{s}");
    assert!(
        !s.contains("USB drive") && !s.contains("progetto"),
        "bookmarks and disks stay out:\n{s}"
    );
    let _ = h.screen();
    assert!(
        h.app
            .hits
            .find(&Target::Place(place(&h, "Documenti")))
            .is_some()
    );
    assert!(
        h.app
            .hits
            .find(&Target::Place(place(&h, "lavoro/progetto")))
            .is_none()
    );
    assert!(
        h.app
            .hits
            .find(&Target::Place(PathBuf::from("/")))
            .is_none(),
        "no disks"
    );
}

#[test]
fn icon_column_names_item_on_hover() {
    let mut h = italian(90, 30);
    let docs = place(&h, "Documenti");
    let (x, y) = h.where_is(&Target::Place(docs));
    h.mouse(
        MouseEventKind::Moved,
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    let s = h.screen();
    assert!(s.contains("▸ Documenti"), "{s}");
    // Away from it: gone.
    h.mouse(
        MouseEventKind::Moved,
        80,
        10,
        crossterm::event::KeyModifiers::NONE,
    );
    assert!(!h.screen().contains("▸ Documenti"));
    // Keyboard focus shows it too.
    h.key(KeyCode::Tab);
    h.key(KeyCode::Up);
    let s = h.screen();
    assert!(s.contains("▸ "), "{s}");
}

#[test]
fn below_sixty_columns_the_navigation_pane_is_gone() {
    let mut h = italian(56, 30);
    let _ = h.screen();
    assert_eq!(h.app.side_mode, rada_tui::sidebar::Mode::Hidden);
    assert!(
        h.app
            .hits
            .find(&Target::Place(place(&h, "Documenti")))
            .is_none()
    );
    h.key(KeyCode::Tab);
    assert!(!h.app.side_nav.focus, "no focus on what is not there");
    assert!(h.app.current_notice().is_some(), "and it says why");
}

fn ui_state_file(h: &H) -> PathBuf {
    h.app
        .home()
        .parent()
        .unwrap()
        .join("xdg/state/rada/ui.json")
}

#[test]
fn ctrl_b_hides_and_shows_it_and_the_choice_is_remembered() {
    let mut h = italian(150, 30);
    let _ = h.screen();
    assert_eq!(h.app.side_mode, rada_tui::sidebar::Mode::Full);
    h.press("ctrl+b");
    let s = h.screen();
    assert!(!s.contains("PINNED"), "{s}");
    let file = ui_state_file(&h);
    let end = Instant::now() + Duration::from_secs(20);
    while !file.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let saved = rada_core::uistate::load(file.parent().unwrap());
    assert_eq!(saved.sidebar, Some(false), "{file:?}");
    h.press("ctrl+b");
    assert!(h.screen().contains("PINNED"));
}

#[test]
fn the_menu_adds_and_removes_bookmarks() {
    let mut h = italian(150, 30);
    // A place that is not a bookmark yet: Open, Add to bookmarks.
    let docs = place(&h, "Documenti");
    h.right_click(&Target::Place(docs.clone()));
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
    let s = h.screen();
    assert!(s.contains("Open") && s.contains("Pin to navigation"), "{s}");
    assert!(!s.contains("Unpin from navigation"), "{s}");
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    h.wait("the bookmark arrives from the worker", {
        let docs = docs.clone();
        move |a| a.places.paths.bookmarks.contains(&docs)
    });

    // A bookmark: Open, Remove.
    h.right_click(&Target::Place(docs.clone()));
    let s = h.screen();
    assert!(s.contains("Unpin from navigation"), "{s}");
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    h.wait("it is gone again", move |a| {
        !a.places.paths.bookmarks.contains(&docs)
    });
}

#[test]
fn the_menu_open_goes_there() {
    let mut h = italian(150, 30);
    let docs = place(&h, "Documenti");
    h.right_click(&Target::Place(docs.clone()));
    h.key(KeyCode::Enter); // "Open" is first
    h.wait("Documenti", move |a| a.cwd == docs && !a.is_loading());
}

#[test]
fn bookmark_key_and_delete_in_sidebar() {
    let mut h = italian(150, 30);
    let _ = h.screen();
    // Put the focus on the "docs" place and bookmark it with the key.
    h.key(KeyCode::Tab);
    h.key(KeyCode::Up);
    let docs = place(&h, "Documenti");
    assert_eq!(h.app.side_items()[h.app.side_nav.side_cursor].path, docs);
    h.press("B");
    h.wait("bookmarked", {
        let d = docs.clone();
        move |a| a.places.paths.bookmarks.contains(&d)
    });
    // The folder is already listed as a place: bookmarking it does not list it a second
    // time, and Delete on the place (which is not a bookmark entry) takes nothing away.
    let listed = |h: &H| h.app.side_items().iter().filter(|i| i.path == docs).count();
    assert_eq!(listed(&h), 1, "once, as the place");
    h.key(KeyCode::Delete);
    assert!(
        h.app.places.paths.bookmarks.contains(&docs),
        "a place is not a bookmark entry"
    );
    // The key again takes the bookmark off.
    h.press("B");
    h.wait("removed", move |a| {
        !a.places.paths.bookmarks.contains(&docs)
    });
}

#[test]
fn a_bookmark_of_a_folder_the_pane_already_lists_is_not_listed_again() {
    let mut h = italian(130, 40);
    let docs = h
        .app
        .side_items()
        .into_iter()
        .find(|i| i.name == "Documenti")
        .expect("Documenti is a standard place")
        .path;
    // Pinned by hand (the same folder, written with a trailing slash) and in the configuration.
    h.app.places.paths.bookmarks.push(docs.join(""));
    h.app
        .places
        .paths
        .bookmarks
        .push(PathBuf::from(format!("{}/", docs.display())));
    let names: Vec<String> = h.app.side_items().into_iter().map(|i| i.name).collect();
    assert_eq!(
        names.iter().filter(|n| *n == "Documenti").count(),
        1,
        "{names:?}"
    );
}

// ----------------------------------------------------------------------- air between entries

/// The screen row of an item of the sidebar (the first of its rows: a disk has two), if it is
/// on screen.
fn try_row_of(h: &mut H, path: PathBuf) -> Option<u16> {
    let _ = h.screen();
    h.app
        .hits
        .all()
        .filter(|(_, t)| *t == Target::Place(path.clone()))
        .map(|(r, _)| r.y)
        .min()
}

fn row_of(h: &mut H, path: PathBuf) -> u16 {
    try_row_of(h, path.clone()).unwrap_or_else(|| panic!("{} is on screen", path.display()))
}

/// Two entries that follow each other in the same group, neither of them a disk: the two
/// pinned folders of the scene (whatever the system, it has its two bookmarks).
fn a_pair(h: &H) -> (PathBuf, PathBuf) {
    let pinned: Vec<PathBuf> = h
        .app
        .side_items()
        .into_iter()
        .filter(|i| i.section == rada_tui::sidebar::Section::Pinned)
        .map(|i| i.path)
        .collect();
    assert!(pinned.len() >= 2, "{pinned:?}");
    (pinned[0].clone(), pinned[1].clone())
}

/// The rows between two consecutive entries of a group, on a terminal `height` rows tall.
fn distance(height: u16, tweak: impl FnOnce(&mut rada_tui::Config) + 'static) -> u16 {
    let mut h = italian_with(150, height, tweak);
    let (a, b) = a_pair(&h);
    let (ya, yb) = (row_of(&mut h, a), row_of(&mut h, b));
    yb.abs_diff(ya)
}

#[test]
fn by_default_the_entries_of_a_group_stay_together_at_any_height() {
    for height in [40u16, 56, 120] {
        assert_eq!(
            distance(height, |_| {}),
            1,
            "{height} rows: no blank line between entries, however much room there is"
        );
    }
}

#[test]
fn the_density_in_the_configuration_decides_how_much_air() {
    use rada_tui::sidebar::Density;
    for d in [Density::Auto, Density::Compact] {
        assert_eq!(distance(120, move |c| c.sidebar_density = d), 1, "{d:?}");
    }
    assert_eq!(
        distance(120, |c| c.sidebar_density = Density::Comfortable),
        2,
        "comfortable: a blank line after every entry where there is room for all of them"
    );
    // Comfortable where the terminal is short: the groups are apart (Home and the first group
    // are further from each other than in the compact pane), though the entries are not.
    let apart = |d: Density| {
        let mut h = italian_with(150, 40, move |c| c.sidebar_density = d);
        let (home, first) = (h.app.side_items()[0].path.clone(), a_pair(&h).0);
        let (a, b) = (row_of(&mut h, home), row_of(&mut h, first));
        b - a
    };
    assert!(apart(Density::Comfortable) > apart(Density::Compact));
}

#[test]
fn every_entry_is_still_there_and_clicks_and_arrows_go_to_the_right_one() {
    use rada_tui::sidebar::Density;
    // The default and the compact pane (the same), and the comfortable one at the heights where
    // its blank lines still leave room for every entry.
    let cases = [
        (Density::Auto, 60u16),
        (Density::Compact, 120),
        (Density::Comfortable, 100),
        (Density::Comfortable, 160),
    ];
    for (density, height) in cases {
        let mut h = italian_with(150, height, move |c| c.sidebar_density = density);
        let items = h.app.side_items();
        // Every item has a row of its own, in the order of the list.
        let ys: Vec<u16> = items
            .iter()
            .map(|i| row_of(&mut h, i.path.clone()))
            .collect();
        assert!(
            ys.windows(2).all(|w| w[0] < w[1]),
            "{density:?} {height}: {ys:?}"
        );
        // A click on an empty line does nothing at all.
        let cwd = h.app.cwd.clone();
        for y in (ys[0]..=*ys.last().unwrap()).filter(|y| !ys.contains(y)) {
            if h.app.hits.at(3, y).is_some() {
                continue; // the bar of a disk: not an empty line
            }
            h.mouse(
                MouseEventKind::Down(crossterm::event::MouseButton::Left),
                3,
                y,
                crossterm::event::KeyModifiers::NONE,
            );
            assert_eq!(
                h.app.cwd, cwd,
                "{density:?} {height}: a click on the empty row {y}"
            );
            assert!(h.app.modal.is_none());
        }
        // A click on the row of an entry opens it.
        let (first, second) = a_pair(&h);
        h.click(&Target::Place(first.clone()));
        let want = first.clone();
        h.wait("the first folder", move |a| {
            a.cwd == want && !a.is_loading()
        });
        // The keyboard goes entry by entry, not row by row, and opens the one it is on.
        h.press("tab");
        let at = h
            .app
            .side_items()
            .iter()
            .position(|i| i.path == second)
            .unwrap();
        let from = h.app.side_nav.side_cursor;
        for _ in from..at {
            h.key(KeyCode::Down);
        }
        assert_eq!(h.app.side_nav.side_cursor, at, "{density:?} {height}");
        h.key(KeyCode::Enter);
        let want = second.clone();
        h.wait("the second folder", move |a| {
            a.cwd == want && !a.is_loading()
        });
    }
}

#[test]
fn the_icon_has_a_space_after_it_in_every_entry() {
    let mut h = italian(150, 60);
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let items = h.app.side_items();
    let mut seen = 0;
    for item in &items {
        let glyph = item.glyph(h.app.icons);
        assert!(!glyph.is_empty(), "the scene draws icons");
        let y = row_of(&mut h, item.path.clone()) as usize;
        let name: String = item.name.chars().take(3).collect();
        assert!(
            lines[y].contains(&format!("{glyph}  {name}")),
            "{:?}: the icon, two cells of air, the name:\n{}",
            item.name,
            lines[y]
        );
        seen += 1;
    }
    assert!(
        seen >= 5,
        "Home, places, bookmarks and disks were looked at: {seen}"
    );
}

#[test]
fn sidebar_entries_indent_and_disk_bar() {
    let mut h = italian(150, 60);
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().collect();
    // The column (in cells, from the left edge of the screen) a text starts at in a line.
    let start = |line: &str, text: &str| {
        let at = line
            .find(text)
            .unwrap_or_else(|| panic!("{text:?} in {line:?}"));
        line[..at].chars().count()
    };
    let title_row = lines
        .iter()
        .find(|l| l.contains("DEVICES"))
        .expect("the title of the disks");
    let title_x = start(title_row, "DEVICES");
    let items = h.app.side_items();
    let system = items.iter().find(|i| i.name == "System").unwrap().clone();
    let y = row_of(&mut h, system.path.clone()) as usize;
    let glyph_x = start(lines[y], system.glyph(h.app.icons));
    assert_eq!(
        glyph_x,
        title_x + 2,
        "the icon stands one column further in than the title does:\n{}\n{}",
        title_row,
        lines[y]
    );
    // The name two cells after the icon; the bar of the disk starts right under the name.
    let name_x = start(lines[y], "System");
    let bar_x = lines[y + 1]
        .chars()
        .enumerate()
        .skip(2)
        .find(|(_, c)| !c.is_whitespace())
        .map(|(x, _)| x)
        .expect("the bar");
    assert_eq!(bar_x, name_x, "{}\n{}", lines[y], lines[y + 1]);
    // The same for the other groups: every entry starts at the same column.
    for item in items
        .iter()
        .filter(|i| i.section != rada_tui::sidebar::Section::Devices)
    {
        let y = row_of(&mut h, item.path.clone()) as usize;
        assert_eq!(
            start(lines[y], item.glyph(h.app.icons)),
            glyph_x,
            "{}: {}",
            item.name,
            lines[y]
        );
    }
}

#[test]
fn on_a_short_terminal_no_entry_is_cut_off_whatever_the_density() {
    use rada_tui::sidebar::Density;
    for density in [Density::Auto, Density::Comfortable, Density::Compact] {
        let mut h = italian_with(150, 20, move |c| c.sidebar_density = density);
        h.press("tab");
        let n = h.app.side_items().len();
        // Walk the whole list: the entry the keyboard is on is always on screen.
        for k in 0..n {
            let item = h.app.side_items()[h.app.side_nav.side_cursor].clone();
            assert!(
                try_row_of(&mut h, item.path.clone()).is_some(),
                "{density:?}: entry {k} ({}) is on screen when the keyboard is on it",
                item.name
            );
            h.key(KeyCode::Down);
        }
    }
}

/// Every theme, with and without the half circles of the Nerd Font, with and without the
/// keyboard on the entry.
fn every_look() -> Vec<(rada_tui::icons::IconSet, &'static str, bool)> {
    use rada_tui::icons::IconSet;
    let mut v = Vec::new();
    for icons in [IconSet::UnicodeIcons, IconSet::Nerd] {
        for name in ["rada", "rada-light", "carta"] {
            for focus in [false, true] {
                v.push((icons, name, focus));
            }
        }
    }
    v
}

/// In the icon-only column, the icon of the place you are in is drawn, and in a colour that
/// can be told from the pill behind it, in every theme and with and without the half circles.
#[test]
fn icon_column_marks_the_current_place() {
    use rada_tui::theme::{ColorDepth, Theme};
    use unicode_width::UnicodeWidthStr;

    for (icons, name, focus) in every_look() {
        {
            let mut h = italian(78, 24);
            h.app.th = Theme::named(name, ColorDepth::True).unwrap();
            h.app.icons = icons;
            let want = place(&h, "Scaricati");
            assert_eq!(h.app.cwd, want);
            let item = h
                .app
                .side_items()
                .into_iter()
                .find(|i| i.path == want)
                .unwrap();
            let glyph = item.glyph(icons).to_string();
            assert!(!glyph.is_empty(), "{icons:?}: the place has an icon");
            if focus {
                // The keyboard is on it too: the stronger tint.
                let at = h
                    .app
                    .side_items()
                    .iter()
                    .position(|i| i.path == want)
                    .unwrap();
                h.app.side_nav.focus = true;
                h.app.side_nav.side_cursor = at;
            }

            let y = row_of(&mut h, want.clone());
            let buf = h.redrawn_buffer();
            assert_eq!(h.app.side_mode, rada_tui::sidebar::Mode::Rail);
            let cells: Vec<_> = (0..rada_tui::sidebar::RAIL_WIDTH)
                .map(|x| buf[(x, y)].clone())
                .collect();
            let shown = cells
                .iter()
                .find(|c| c.symbol() == glyph)
                .unwrap_or_else(|| {
                    let row: Vec<_> = cells.iter().map(|c| c.symbol().to_string()).collect();
                    panic!("{icons:?} {name} focus={focus}: the icon {glyph:?} is not in the row {row:?}")
                });
            assert!(glyph.width() > 0);
            assert_ne!(
                shown.fg, shown.bg,
                "{icons:?} {name} focus={focus}: the icon is the colour of the pill ({:?})",
                shown.fg
            );
            // Not just different: a colour that reads on the pill.
            let lum = |c: ratatui::style::Color| match c {
                ratatui::style::Color::Rgb(r, g, b) => {
                    0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64
                }
                other => panic!("{other:?} is not a true colour"),
            };
            assert!(
                (lum(shown.fg) - lum(shown.bg)).abs() > 40.0,
                "{icons:?} {name} focus={focus}: {:?} on {:?} is too faint",
                shown.fg,
                shown.bg
            );
        }
    }
}
