//! The way from the nearest place down to the folder shown.

use crate::common::*;
use crate::harness::*;
use rada_tui::app::App;
use rada_tui::panels::NavMode;
use rada_tui::sidebar::Section;

#[test]
fn nav_lists_only_the_ancestors_and_the_subfolders_of_the_folder_shown() {
    let (mut h, c) = path_scene();
    let home = h.app.home().to_path_buf();
    let rows = h.app.nav_rows();
    let title = rows
        .iter()
        .position(|r| r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")
        .unwrap();
    let way: Vec<_> = rows[title + 1..]
        .iter()
        .map(|r| (r.path.clone(), r.depth))
        .collect();
    // (Three levels of indent at most: the subfolders of c, the fourth, stay at the third.)
    let want: Vec<_> = [
        (home.clone(), 0),
        (home.join("a"), 1),
        (home.join("a/b"), 2),
        (c.clone(), 3),
        (c.join("d"), 3),
        (c.join("kid-1"), 3),
        (c.join("kid-2"), 3),
    ]
    .into();
    assert_eq!(
        way, want,
        "no sibling of an ancestor, nothing of Home's own folders"
    );
    // The folder shown carries its number of items and is the highlighted row.
    let here = &rows[title + 4];
    assert_eq!(here.path, c);
    assert_eq!(here.count, Some(h.app.listing.all().len()));
    assert_eq!(App::nav_current(&rows, &c), Some(title + 4));
    let text = nav_text(&mut h);
    assert!(
        !text.contains("sib-") && !text.contains("Desktop ("),
        "{text}"
    );
    // Moving to a folder in the middle box moves the way by itself.
    go_and_wait(&mut h, &home.join("a"));
    h.wait("the subfolders of a", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let text = nav_text(&mut h);
    assert!(text.contains("sib-a") && !text.contains("kid-1"), "{text}");
}

#[test]
fn a_click_on_an_ancestor_of_the_way_shows_that_folder_in_the_middle_box() {
    let (mut h, c) = path_scene();
    let b = c.parent().unwrap().to_path_buf();
    h.click(&rada_tui::hits::Target::Place(b.clone()));
    h.wait("b", {
        let b = b.clone();
        move |a| a.cwd == b && !a.is_loading()
    });
    h.settle();
    assert!(h.screen().contains("[ ~/a/b ]"));
    // Enter does the same from the keyboard, on a subfolder.
    h.wait("the subfolders of b", |a| a.tree.kids(&a.cwd).is_some());
    h.press("tab");
    let at = h
        .app
        .nav_rows()
        .iter()
        .position(|r| r.path == c)
        .expect("c is listed under b");
    h.app.nav_cursor = at;
    h.press("enter");
    h.wait("c", move |a| a.cwd == c && !a.is_loading());
}

#[test]
fn what_does_not_fit_under_the_way_is_counted_in_a_row_of_more() {
    let (mut h, c) = path_scene();
    for n in 0..40 {
        std::fs::create_dir_all(c.join(format!("many-{n:02}"))).unwrap();
    }
    go_and_wait(&mut h, &c);
    h.app.refresh();
    h.wait("the subfolders of c", |a| {
        a.tree.kids(&a.cwd).is_some_and(|k| k.len() == 43)
    });
    h.settle();
    let rows = h.app.nav_rows();
    assert!(
        rows.len() <= h.app.nav_area.height as usize,
        "{}",
        rows.len()
    );
    let more = rows.last().unwrap();
    assert_eq!(more.kind, rada_tui::panels::NavKind::More);
    let shown = rows
        .iter()
        .filter(|r| {
            r.path.parent() == Some(c.as_path()) && r.kind != rada_tui::panels::NavKind::More
        })
        .count();
    assert_eq!(more.name, format!("… {} more", 43 - shown));
    assert!(!more.selectable());
}

#[test]
fn with_nav_set_to_albero_the_box_keeps_the_whole_tree() {
    let h = tree_scene(160, 46, "rada", false);
    let rows = h.app.nav_rows();
    assert!(
        rows.iter().all(|r| r.name != "PATH"),
        "no title of the way in the tree"
    );
    let home = h.app.home().to_path_buf();
    // Home's own folders are all there, with their arrows, as before.
    for d in ["Documents", "projects"] {
        let r = rows.iter().find(|r| r.path == home.join(d)).unwrap();
        assert_eq!(r.depth, 1);
    }
    assert!(rows.iter().any(|r| r.path == home.join("projects/demo")));
    assert!(matches!(
        rows[0].kind,
        rada_tui::panels::NavKind::Folder { open: Some(true) }
    ));
    assert_eq!(NavMode::parse("albero"), Some(NavMode::Tree));
    assert_eq!(NavMode::parse("percorso"), Some(NavMode::Path));
    assert_eq!(NavMode::default(), NavMode::Path);
}

#[test]
fn the_way_stays_in_view_when_the_box_gets_shorter_after_the_first_frames() {
    // The disks arrive after the first frames: SPAZIO appears and NAV loses rows. Here the
    // window gets shorter instead, with the same effect on the box.
    let mut h = scene(160, 56, "rada", false);
    let home = h.app.home().to_path_buf();
    // How far down the rows the first folder under Home is depends on the system (tags, disks),
    // and so does the height of the box: the chain is made as deep as the last row of the tall
    // box needs.
    std::fs::create_dir_all(home.join("d0")).unwrap();
    go_and_wait(&mut h, &home.join("d0"));
    h.wait("the way", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let tall = h.app.nav_area.height as usize;
    let rows = h.app.nav_rows();
    let first = App::nav_current(&rows, &home.join("d0")).unwrap();
    let levels = tall - first;
    let mut deep = home.join("d0");
    for n in 1..levels {
        deep.push(format!("d{n}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    go_and_wait(&mut h, &deep);
    h.wait("the way", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    // (Indented as deep as that the name does not fit, so the row is looked for by its place.)
    let in_view = |h: &H| {
        let rows = h.app.nav_rows();
        let at = App::nav_current(&rows, &h.app.cwd).unwrap();
        let (from, height) = (h.app.nav_scroll, h.app.nav_area.height as usize);
        (from..from + height).contains(&at)
    };
    let tall_box = tall;
    assert!(in_view(&h), "in view at first");
    h.term.backend_mut().resize(160, 36);
    let _ = h.screen();
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &deep).unwrap();
    let shown = h.app.nav_area.height as usize;
    assert!(shown < tall_box, "the box is shorter: {shown} < {tall_box}");
    assert!(
        rows.len() <= shown,
        "the way gave up rows to fit: {} of {shown}",
        rows.len()
    );
    assert!(at < shown && in_view(&h), "the folder shown is in view");
}

/// What is listed under the title of the way, as paths.
fn way_paths(h: &H) -> Vec<std::path::PathBuf> {
    let rows = h.app.nav_rows();
    match rows
        .iter()
        .position(|r| r.kind == rada_tui::panels::NavKind::Heading && r.name == "PATH")
    {
        Some(t) => rows[t + 1..]
            .iter()
            .filter(|r| r.kind != rada_tui::panels::NavKind::Trash)
            .map(|r| r.path.clone())
            .collect(),
        None => Vec::new(),
    }
}

#[test]
fn the_way_starts_at_the_nearest_place_that_holds_the_folder() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let deep = home.join("Documents/x/y");
    std::fs::create_dir_all(&deep).unwrap();
    go_and_wait(&mut h, &deep);
    h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let docs = home.join("Documents");
    assert_eq!(
        way_paths(&h),
        vec![docs.clone(), docs.join("x"), deep.clone()],
        "Documents, x, y"
    );
    // The first row is the place, with its icon.
    let rows = h.app.nav_rows();
    let t = rows.iter().position(|r| r.name == "PATH").unwrap();
    assert!(matches!(
        rows[t + 1].kind,
        rada_tui::panels::NavKind::Favorite(_)
    ));
    // Under Home, and not in a place of its own, it starts at Home.
    let abc = home.join("a/b/c");
    std::fs::create_dir_all(&abc).unwrap();
    go_and_wait(&mut h, &abc);
    h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
    let way = way_paths(&h);
    assert_eq!(way[0], home);
    assert_eq!(&way[1..], [home.join("a"), home.join("a/b"), abc.clone()]);
}

#[test]
fn a_deep_way_keeps_the_name_of_the_folder_shown_whole() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let deep = home.join("uno/due/tre/quattro/cinque/sei/sette/cartella8");
    std::fs::create_dir_all(&deep).unwrap();
    go_and_wait(&mut h, &deep);
    h.wait("the subfolders", |a| a.tree.kids(&a.cwd).is_some());
    h.settle();
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &deep).unwrap();
    assert_eq!(rows[at].name, "cartella8");
    // Every row of the way is indented by three levels at most.
    let t = rows.iter().position(|r| r.name == "PATH").unwrap();
    let most = rows[t + 1..].iter().map(|r| r.depth).max().unwrap();
    assert_eq!(most, rada_tui::panels::NAV_INDENT_MAX);
    let text = nav_text(&mut h);
    assert!(
        text.contains("cartella8"),
        "the name is there whole:\n{text}"
    );
}

/// The demo home with a folder `levels` deep in it, shown, and its subfolders listed.
fn deep_scene(w: u16, hh: u16, levels: usize) -> H {
    let mut h = scene(w, hh, "rada", false);
    let mut deep = h.app.home().to_path_buf();
    for n in 0..levels {
        deep.push(format!("level-{n}"));
    }
    for kid in ["kid-1", "kid-2", "kid-3"] {
        std::fs::create_dir_all(deep.join(kid)).unwrap();
    }
    go_and_wait(&mut h, &deep);
    h.wait("the subfolders", |a| {
        a.tree.kids(&a.cwd).is_some_and(|k| k.len() == 3)
    });
    h.settle();
    h
}

#[test]
fn the_places_stay_in_sight_and_the_way_gives_up_rows() {
    let mut h = deep_scene(170, 42, 12);
    let nav = nav_text(&mut h);
    let top: Vec<&str> = nav.lines().take(3).collect();
    assert!(top[0].contains("Home"), "Home is the first row:\n{nav}");
    assert!(top[1].contains("Desktop"), "{nav}");
    assert!(
        nav.contains("DEVICES") && nav.contains("USB drive"),
        "{nav}"
    );
    assert!(nav.contains("… ") && nav.contains(" more"), "{nav}");
    assert!(nav.contains("level-11"), "the folder shown stays:\n{nav}");
    assert_eq!(h.app.nav_scroll, 0, "nothing scrolls:\n{nav}");
}

/// Fixed disks `Extra 1`.. added to the demo ones, and the box drawn once with them: its height
/// is what the rows are made for.
fn add_disks(h: &mut H, count: usize) {
    h.app
        .volumes
        .extend((1..=count).map(|n| rada_core::platform::Volume {
            mount_point: format!("/mnt/extra{n}").into(),
            label: Some(format!("Extra {n}")),
            fs_type: "ext4".into(),
            device: "demo".into(),
            kind: rada_core::platform::VolumeKind::Fixed,
            drive_letter: None,
            total: Some(1_000_000_000_000),
            available: Some(500_000_000_000),
            read_only: false,
            responsive: true,
        }));
    let _ = h.screen();
    let _ = h.screen();
}

#[test]
fn the_devices_give_up_all_but_the_main_disk_when_the_way_cannot_be_cut_further() {
    let mut h = deep_scene(170, 42, 12);
    add_disks(&mut h, 2);
    let disks = h
        .app
        .side_items()
        .iter()
        .filter(|i| i.section == Section::Devices)
        .count();
    let nav = nav_text(&mut h);
    assert!(nav.lines().next().unwrap().contains("Home"), "{nav}");
    assert!(nav.contains("System"), "the main disk stays:\n{nav}");
    assert!(
        !nav.contains("Extra") && !nav.contains("USB drive"),
        "{nav}"
    );
    let others = format!("… {} more", disks - 1);
    assert!(nav.contains(&others), "the other disks are counted:\n{nav}");
    assert!(nav.contains("level-11"), "{nav}");
}

#[test]
fn space_gives_a_disk_name_the_room_it_needs_and_shortens_the_bar() {
    let mut h = scene(160, 46, "rada", false);
    let screen = h.screen();
    let row = screen
        .lines()
        .find(|l| l.contains("USB drive") && l.contains('%'))
        .unwrap_or_else(|| panic!("the name is whole:\n{screen}"));
    let cells = row.chars().filter(|c| matches!(c, '■' | '□')).count();
    assert!((6..12).contains(&cells), "the bar is cut down: {row}");
}

#[test]
fn the_blank_row_above_the_way_goes_when_the_box_is_one_row_short() {
    let mut h = deep_scene(170, 42, 12);
    add_disks(&mut h, 4);
    let nav = nav_text(&mut h);
    let rows: Vec<&str> = nav.lines().collect();
    let path = rows.iter().position(|r| r.contains("PATH")).unwrap();
    assert!(rows[path - 1].contains("TAGS"), "no blank row:\n{nav}");
    assert!(rows[0].contains("Home") && h.app.nav_scroll == 0, "{nav}");
    assert!(nav.contains("level-11"), "{nav}");
}

#[test]
fn the_blank_row_above_tags_goes_before_the_disks_are_cut() {
    let mut h = deep_scene(170, 42, 12);
    add_disks(&mut h, 4);
    let nav = nav_text(&mut h);
    let rows: Vec<&str> = nav.lines().collect();
    let tags = rows.iter().position(|r| r.contains("TAGS")).unwrap();
    assert!(rows[tags - 1].contains("Extra") || rows[tags - 1].contains("more"));
    assert!(rows[0].contains("Home") && h.app.nav_scroll == 0, "{nav}");
    assert!(nav.contains("level-11"), "{nav}");
}
