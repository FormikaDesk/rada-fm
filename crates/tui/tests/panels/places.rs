//! The places at the top of NAV: favourites, the trash, the current place.

use crate::common::*;
use crate::harness::*;
use rada_tui::app::App;

#[test]
fn the_favourites_are_only_the_folders_the_person_added() {
    let mut h = tree_scene(160, 46, "rada", false);
    h.wait("the tree", |a| a.tree.kids(a.home()).is_some());
    let is_favorite = |a: &App| {
        a.nav_rows()
            .iter()
            .any(|r| matches!(r.kind, rada_tui::panels::NavKind::Favorite(_)))
    };
    assert!(!is_favorite(&h.app), "the standard folders are in the tree");
    assert!(
        !h.screen().contains("PINNED"),
        "no title without favourites"
    );
    let added = h.app.home().join("projects");
    // B in NAV, on the folder.
    h.press("tab");
    h.app.nav_cursor = h
        .app
        .nav_rows()
        .iter()
        .position(|r| r.path == added)
        .unwrap();
    h.press("B");
    h.wait("the bookmark", |a| a.paths.bookmarks.contains(&added));
    let rows = h.app.nav_rows();
    let favs: Vec<_> = rows
        .iter()
        .filter(|r| matches!(r.kind, rada_tui::panels::NavKind::Favorite(_)))
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(favs, vec![added]);
    assert!(h.screen().contains("PINNED"));
}

#[test]
fn nav_lists_the_places_on_top_as_the_classic_sidebar_does() {
    let (mut h, _) = path_scene();
    let rows = h.app.nav_rows();
    let at = |name: &str| {
        rows.iter()
            .position(|r| r.name == name)
            .unwrap_or(usize::MAX)
    };
    let title = rows
        .iter()
        .position(|r| r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")
        .expect("the title of the way");
    let places = [
        "Home",
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Music",
        "Videos",
        "DEVICES",
    ];
    let mut last = 0;
    for p in places {
        assert!(at(p) < title, "{p} is above the way: {rows:?}");
        assert!(at(p) >= last, "{p} is in the order of the sidebar");
        last = at(p);
    }
    assert!(at("USB drive") < title, "{rows:?}");
    // The tags are not on every system (Windows has none): then there is no row, and no
    // second blank row in its place.
    if h.app.tags_supported() {
        assert!(
            at("USB drive") < at("TAGS") && at("TAGS") < title,
            "{rows:?}"
        );
    } else {
        assert_eq!(at("TAGS"), usize::MAX);
        assert!(
            !rows
                .windows(2)
                .any(|w| { w[0].kind == rada_tui::panels::NavKind::Gap && w[1].kind == w[0].kind })
        );
    }
    let text = nav_text(&mut h);
    assert!(
        text.contains("PATH") && text.contains("Documents"),
        "{text}"
    );
}

/// A trash in the places of the demo, which has none.
fn with_trash(h: &mut H) {
    let t = h.app.home().join(".trash");
    let kind = rada_core::platform::PlaceKind::Trash;
    h.app.paths.places.retain(|p| p.kind != kind);
    h.app
        .paths
        .places
        .push(rada_core::places::Place { kind, path: t });
    h.app.redraw.dirty = true;
}

#[test]
fn in_a_place_there_is_no_way_and_the_place_is_the_highlighted_row() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    for place in [
        home.clone(),
        home.join("Documents"),
        h.app.volumes[0].mount_point.clone(),
    ] {
        go_and_wait(&mut h, &place);
        h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
        h.settle();
        let rows = h.app.nav_rows();
        assert!(
            rows.iter()
                .all(|r| !(r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")),
            "no title of the way in {place:?}"
        );
        let at = App::nav_current(&rows, &place).expect("the place is highlighted");
        assert!(matches!(
            rows[at].kind,
            rada_tui::panels::NavKind::Favorite(_)
        ));
        assert!(!nav_text(&mut h).contains("PATH"));
    }
}

#[test]
fn the_trash_is_the_last_row_of_nav_in_every_view() {
    let mut h = scene(160, 46, "rada", false);
    with_trash(&mut h);
    let home = h.app.home().to_path_buf();
    let deep =
        home.join("t1/t2/t3/t4/t5/t6/t7/t8/t9/t10/t11/t12/t13/t14/t15/t16/t17/t18/t19/t20/t21");
    std::fs::create_dir_all(&deep).unwrap();
    for (w, hh, dir) in [
        (160, 46, home.clone()),
        (160, 46, deep.clone()),
        (160, 36, deep.clone()),
        (160, 36, home.join("Documents")),
    ] {
        h.term.backend_mut().resize(w, hh);
        go_and_wait(&mut h, &dir);
        h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
        h.settle();
        // The places are read again now and then: the demo's trash with them.
        with_trash(&mut h);
        let rows = h.app.nav_rows();
        assert_eq!(
            rows.last().map(|r| &r.kind),
            Some(&rada_tui::panels::NavKind::Trash),
            "{dir:?}"
        );
        let text = nav_text(&mut h);
        let last = text.lines().last().unwrap();
        assert!(last.contains("Trash"), "{dir:?}: {text}");
    }
    // A tag's list is a view too, with no way of folders (a system without tags, Windows, has
    // no such list to open).
    if h.app.tags_supported() {
        let view = rada_core::tagsearch::view_path("Red");
        h.app.open_dir(view.clone());
        h.wait("the view", |a| a.cwd == view);
        h.settle();
        with_trash(&mut h);
        let text = nav_text(&mut h);
        assert!(text.lines().last().unwrap().contains("Trash"), "{text}");
    }
}

#[test]
fn in_the_trash_folder_only_the_trash_is_the_current_place() {
    // The classic sidebar and the NAV box of the panels view: the trash is highlighted,
    // and no other place (Downloads, say) is.
    let mut h = demo_scene(120, 44);
    with_trash(&mut h);
    let trash = h.app.paths.places.last().unwrap().path.clone();
    std::fs::create_dir_all(&trash).unwrap();
    go_and_wait(&mut h, &trash);
    // (The places are read again now and then: the demo's trash with them.)
    with_trash(&mut h);
    let items = h.app.side_items();
    let at = rada_tui::sidebar::current_index(&items, &h.app.cwd).expect("a current place");
    assert_eq!(items[at].name, "Trash", "{:?}", items[at]);
    let _ = h.screen();
    let (t, d) = (row_bg(&mut h, "Trash", 4), row_bg(&mut h, "Downloads", 4));
    assert_ne!(t, d, "the trash row is highlighted, Downloads is not");
    let quiet = row_bg(&mut h, "Pictures", 4);
    assert_eq!(
        d, quiet,
        "Downloads is as quiet as the places that are not here"
    );

    let mut h = scene(160, 46, "rada", false);
    with_trash(&mut h);
    let trash = h.app.paths.places.last().unwrap().path.clone();
    std::fs::create_dir_all(&trash).unwrap();
    go_and_wait(&mut h, &trash);
    with_trash(&mut h);
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &trash).expect("a current row");
    assert_eq!(rows[at].kind, rada_tui::panels::NavKind::Trash);
}
