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
    let mut h = H::new(sb, home.join("Scaricati"), w, h);
    h.wait("the places and bookmarks", |a| {
        a.paths.places.len() >= 3 && a.paths.bookmarks.len() == 2
    });
    set_volumes(&mut h);
    h
}

/// Two disks, as if the system had reported them (the real list depends on the machine).
fn set_volumes(h: &mut H) {
    h.app.volumes = vec![
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
fn the_full_sidebar_shows_places_under_their_real_names_and_only_those_that_exist() {
    let mut h = italian(150, 30);
    let s = h.screen();
    for needle in [
        "PLACES",
        "Home",
        "Documenti",
        "Scaricati",
        "BOOKMARKS",
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
        h.app.th.selection,
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
fn tab_puts_the_keyboard_in_the_sidebar_with_visible_focus_and_enter_opens() {
    let mut h = italian(150, 30);
    let _ = h.screen();
    h.key(KeyCode::Tab);
    assert!(h.app.side_focus);
    let _ = h.screen();
    let items = h.app.side_items();
    let cur = h.app.side_cursor;
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
    assert_eq!(h.app.side_items()[h.app.side_cursor].path, docs);
    h.key(KeyCode::Enter);
    assert!(!h.app.side_focus, "the focus goes back to the list");
    h.wait("Documenti", move |a| a.cwd == docs && !a.is_loading());

    // Tab again and Esc: out.
    h.key(KeyCode::Tab);
    assert!(h.app.side_focus);
    h.key(KeyCode::Esc);
    assert!(!h.app.side_focus);
}

#[test]
fn file_operations_are_not_done_behind_the_sidebars_back() {
    let mut h = italian(150, 30);
    h.key(KeyCode::Tab);
    h.press("ctrl+c");
    assert!(h.app.clipboard.is_none(), "nothing was copied");
    assert!(h.app.toast.is_some(), "and it was said why");
    assert!(h.app.side_focus);
}

#[test]
fn narrow_terminals_get_a_column_of_icons_with_the_places_only() {
    let mut h = italian(110, 30);
    let s = h.screen();
    assert!(
        !s.contains("PLACES") && !s.contains("BOOKMARKS") && !s.contains("DEVICES"),
        "{s}"
    );
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
fn in_the_icon_column_the_name_appears_in_the_bottom_bar_on_hover_and_on_focus() {
    let mut h = italian(110, 30);
    let docs = place(&h, "Documenti");
    let (x, y) = h.where_is(&Target::Place(docs));
    h.mouse(
        MouseEventKind::Moved,
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    let s = h.screen();
    let last = s.lines().last().unwrap();
    assert!(last.contains("▸ Documenti"), "{last}");
    // Away from it: gone.
    h.mouse(
        MouseEventKind::Moved,
        90,
        10,
        crossterm::event::KeyModifiers::NONE,
    );
    assert!(!h.screen().lines().last().unwrap().contains("Documenti"));
    // Keyboard focus shows it too.
    h.key(KeyCode::Tab);
    h.key(KeyCode::Up);
    let s = h.screen();
    assert!(s.lines().last().unwrap().contains("▸ "), "{s}");
}

#[test]
fn below_a_hundred_columns_the_sidebar_is_gone_and_the_preview_stays() {
    let mut h = italian(96, 30);
    let _ = h.screen();
    assert_eq!(h.app.side_mode, rada_tui::sidebar::Mode::Hidden);
    assert!(
        h.app
            .hits
            .find(&Target::Place(place(&h, "Documenti")))
            .is_none()
    );
    h.key(KeyCode::Tab);
    assert!(!h.app.side_focus, "no focus on what is not there");
    assert!(h.app.toast.is_some(), "and it says why");
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
    assert!(!s.contains("PLACES"), "{s}");
    let file = ui_state_file(&h);
    let end = Instant::now() + Duration::from_secs(3);
    while !file.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let saved = rada_core::uistate::load(file.parent().unwrap());
    assert_eq!(saved.sidebar, Some(false), "{file:?}");
    h.press("ctrl+b");
    assert!(h.screen().contains("PLACES"));
}

#[test]
fn the_menu_adds_and_removes_bookmarks() {
    let mut h = italian(150, 30);
    // A place that is not a bookmark yet: Open, Add to bookmarks.
    let docs = place(&h, "Documenti");
    h.right_click(&Target::Place(docs.clone()));
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
    let s = h.screen();
    assert!(s.contains("Open") && s.contains("Add to bookmarks"), "{s}");
    assert!(!s.contains("Remove from bookmarks"), "{s}");
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    h.wait("the bookmark arrives from the worker", {
        let docs = docs.clone();
        move |a| a.paths.bookmarks.contains(&docs)
    });

    // A bookmark: Open, Remove.
    h.right_click(&Target::Place(docs.clone()));
    let s = h.screen();
    assert!(s.contains("Remove from bookmarks"), "{s}");
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    h.wait("it is gone again", move |a| {
        !a.paths.bookmarks.contains(&docs)
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
fn the_bookmark_key_works_on_the_item_in_the_sidebar_and_delete_removes_a_bookmark() {
    let mut h = italian(150, 30);
    let _ = h.screen();
    // Put the focus on the "docs" place and bookmark it with the key.
    h.key(KeyCode::Tab);
    h.key(KeyCode::Up);
    let docs = place(&h, "Documenti");
    assert_eq!(h.app.side_items()[h.app.side_cursor].path, docs);
    h.press("B");
    h.wait("bookmarked", {
        let d = docs.clone();
        move |a| a.paths.bookmarks.contains(&d)
    });
    // Delete on that item, now a bookmark... it is listed twice (as a place and as a
    // bookmark): the place cannot be removed, the bookmark can.
    h.key(KeyCode::Delete);
    assert!(
        h.app.paths.bookmarks.contains(&docs),
        "a place is not a bookmark entry"
    );
    let items = h.app.side_items();
    let bm = items
        .iter()
        .position(|i| {
            matches!(i.origin, rada_tui::sidebar::Origin::Bookmark { .. }) && i.path == docs
        })
        .unwrap();
    h.app.side_cursor = bm;
    h.key(KeyCode::Delete);
    h.wait("removed", move |a| !a.paths.bookmarks.contains(&docs));
}
