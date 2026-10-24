//! The TAGS section of the sidebar and the list a tag opens ("Tag: Blu"): the seven colours
//! always, in the Finder's order and in the language of the system; the user's own tags seen in
//! the folders opened; a click lists what carries the tag, wherever it is, with the usual
//! operations and nothing that would make something *in* the list. The tags are the real
//! ones of the platform (an extended attribute on Linux): a filesystem without them skips.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use crossterm::event::KeyCode;
use rada_core::platform::{Tag, TagColor};
use rada_core::tagsearch::view_path;
use rada_core::testutil::*;
use rada_tui::hits::Target;

fn home_file(sb: &Sandbox, rel: &str, tags: Vec<Tag>) -> PathBuf {
    let p = sb.dirs.home.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "x").unwrap();
    if !tags.is_empty() {
        sb.platform().tags().unwrap().write(&p, &tags).unwrap();
    }
    p
}

fn supported(sb: &Sandbox) -> bool {
    let probe = sb.dirs.home.join("probe");
    std::fs::write(&probe, "x").unwrap();
    let ok = rada_core::platform::tags::xattr::set(&probe, "user.rada-probe", b"1").is_ok();
    if !ok {
        eprintln!("this filesystem keeps no user attributes: test skipped");
    }
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Home with files tagged in several folders: blue in Italian and in English, red, a tag of
/// the user's own, and one without tags.
fn scene() -> Option<(Sandbox, PathBuf)> {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return None;
    }
    let it = |c| Tag::standard_in(c, "it");
    home_file(&sb, "Documents/a.txt", vec![it(TagColor::Blue)]);
    home_file(&sb, "Work/deep/b.txt", vec![Tag::standard(TagColor::Blue)]);
    home_file(&sb, "Work/same.txt", vec![it(TagColor::Red)]);
    home_file(
        &sb,
        "Work/clients.txt",
        vec![Tag {
            name: "Clienti".into(),
            color: Some(TagColor::Blue),
        }],
    );
    home_file(&sb, "Work/plain.txt", vec![]);
    let home = sb.dirs.home.clone();
    Some((sb, home))
}

/// Open the list of a tag the way the sidebar does: a click on its row when it has one.
fn open_tag(h: &mut H, name: &str) {
    h.app.tags.lang = "it".into();
    if h.app.side_items().iter().any(|i| i.path == view_path(name)) {
        h.click(&Target::Place(view_path(name)));
    } else {
        h.app.open_dir(view_path(name));
    }
    h.wait("the tag list", |a| {
        a.cwd == view_path(name) && !a.is_loading()
    });
}

/// The row of the list for the item called `what`.
fn row_with<'a>(screen: &'a str, what: &str) -> &'a str {
    screen
        .lines()
        .find(|l| l.contains(what) && l.contains("~/"))
        .unwrap_or_else(|| panic!("no row with {what:?}:\n{screen}"))
}

#[test]
fn sidebar_lists_seven_colours_then_tags() {
    let Some((sb, home)) = scene() else { return };
    let mut h = H::new(sb, home.clone(), 150, 40);
    h.app.tags.lang = "it".into();
    // The folder with a tag of the user's own is opened: it joins the list.
    h.app.open_dir(home.join("Work"));
    h.wait("Work", |a| a.cwd.ends_with("Work") && !a.is_loading());
    h.settle();
    let s = h.screen();
    assert!(s.contains("TAGS"), "{s}");
    let at = |word: &str| {
        s.lines()
            .position(|l| l.contains(word) && l.contains('●'))
            .unwrap_or_else(|| panic!("{word} is not in the TAGS list:\n{s}"))
    };
    let order = [
        "Rosso",
        "Arancione",
        "Giallo",
        "Verde",
        "Blu",
        "Viola",
        "Grigio",
        "Clienti",
    ];
    let rows: Vec<usize> = order.iter().map(|w| at(w)).collect();
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "order: {rows:?}\n{s}");
    // Each dot is drawn in its colour.
    let blue = h.app.settings.th.tag_color(Some(TagColor::Blue));
    let buf = h.term.backend().buffer().clone();
    let y = rows[4] as u16;
    let dot = (0..buf.area.width)
        .find(|&x| buf[(x, y)].symbol() == "●")
        .expect("a dot on the Blu row");
    assert_eq!(buf[(dot, y)].fg, blue);
}

#[test]
fn a_colour_lists_what_carries_it_wherever_it_is() {
    let Some((sb, home)) = scene() else { return };
    let mut h = H::new(sb, home.clone(), 150, 40);
    open_tag(&mut h, "Blu");
    let mut names = h.names();
    names.sort();
    assert_eq!(
        names,
        ["a.txt", "b.txt"],
        "blue in two languages, not Clienti"
    );
    let s = h.screen();
    assert!(
        s.contains("Tag: Blu"),
        "the address says what this is:\n{s}"
    );
    // Each row says which folder the item is in.
    assert!(row_with(&s, "a.txt").contains("Documents"), "{s}");
    assert!(row_with(&s, "b.txt").contains("Work/deep"), "{s}");
    // The sidebar marks the tag as the place.
    assert!(h.app.cwd == view_path("Blu"));
    // A tab is named after it.
    assert_eq!(h.app.tab_title(0), "Tag: Blu");
    // A tag of the user's own is found by its name only.
    open_tag(&mut h, "Clienti");
    assert_eq!(h.names(), ["clients.txt"]);
    // A tag nothing carries is an empty list, not an error.
    open_tag(&mut h, "Giallo");
    assert!(h.names().is_empty());
}

#[test]
fn nothing_can_be_made_in_the_list_but_the_items_can_be_worked_on() {
    let Some((sb, home)) = scene() else { return };
    let mut h = H::new(sb, home.clone(), 150, 40);
    open_tag(&mut h, "Blu");
    // No new folder, no new file, no paste.
    h.press("f7");
    assert!(h.app.windows.modal.is_none(), "{}", h.screen());
    assert!(
        h.app
            .current_notice()
            .is_some_and(|t| t.text.contains("not a folder")),
        "{:?}",
        h.app.current_notice().map(|t| &t.text)
    );
    h.keys("y"); // copy the first
    h.keys("p");
    assert!(h.app.windows.modal.is_none(), "{}", h.screen());
    // The path copied is the real one.
    h.press("ctrl+shift+c");
    // Rename works on the item where it is.
    let first = h.app.current().unwrap().path.clone();
    h.press("F2");
    for _ in 0..10 {
        h.key(KeyCode::Backspace);
    }
    h.keys("renamed.txt");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    h.wait("the renamed item in the list", |a| {
        a.tag_view().is_some() && a.listing.all().iter().any(|e| e.display == "renamed.txt")
    });
    let renamed = first.parent().unwrap().join("renamed.txt");
    assert!(renamed.exists(), "{}", h.screen());
    assert!(!first.exists());
    assert!(
        h.names().contains(&"renamed.txt".to_string()),
        "{:?}",
        h.names()
    );
    // Up leaves the list the way one came: the home folder here.
    h.press("alt+up");
    h.wait("the folder", |a| a.tag_view().is_none() && !a.is_loading());
    assert_eq!(h.app.cwd, home);
}

#[test]
fn a_list_of_tagged_items_is_not_remembered_as_a_place() {
    let Some((sb, home)) = scene() else { return };
    let mut h = H::new(sb, home.clone(), 150, 40);
    open_tag(&mut h, "Rosso");
    let saved = h.app.saved_tabs();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        Path::new(&saved[0].path),
        home,
        "the home folder, not tag:Rosso"
    );
    assert!(
        !h.app
            .places
            .paths
            .recents
            .iter()
            .any(|p| p.starts_with("tag:"))
    );
}
