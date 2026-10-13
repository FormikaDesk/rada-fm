//! Tags in the interface: dots beside names, the list in the details pane, the Tags menu, the
//! filter, and the plan that every change goes through (with undo). They use the real tags of
//! the platform (an extended attribute on Linux), so a filesystem without them skips these.
#![cfg(target_os = "linux")]

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_core::platform::{Tag, TagColor};
use rada_core::testutil::*;
use rada_tui::app::Modal;
use rada_tui::hits::Target;

/// A folder with `a.txt`, `b.txt` and a folder `sub`, and the tags asked for.
fn scene(tags: &[(&str, &[&str])]) -> Option<(Sandbox, std::path::PathBuf)> {
    let (sb, dir) = sandbox_with_files();
    let probe = dir.join("a.txt");
    if rada_core::platform::tags::xattr::set(&probe, "user.rada-probe", b"1").is_err() {
        eprintln!("this filesystem keeps no user attributes: test skipped");
        return None;
    }
    let store = sb.platform();
    let store = store.tags().expect("tags on this platform");
    for (name, tags) in tags {
        let tags: Vec<Tag> = tags.iter().map(|t| Tag::named(t)).collect();
        store.write(&dir.join(name), &tags).unwrap();
    }
    Some((sb, dir))
}

fn tags_of(h: &H, name: &str) -> Vec<String> {
    h.app
        .listing
        .all()
        .iter()
        .find(|e| e.display == name)
        .map(|e| e.tags.iter().map(|t| t.name.clone()).collect())
        .unwrap_or_default()
}

/// Walk the context menu to Tags and choose `label` in it.
fn choose_tag(h: &mut H, label: &str) {
    let (x, y) = h.where_is(&Target::Row(h.app.cursor));
    h.mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    let s = h.screen();
    assert!(s.contains("Tags"), "the menu has Tags: {s}");
    // Down to the entry called Tags and open it.
    let mut guard = 0;
    while !menu_selected(h).starts_with("Tags") && guard < 30 {
        h.key(KeyCode::Down);
        guard += 1;
    }
    h.key(KeyCode::Enter);
    guard = 0;
    while !menu_selected(h).contains(label) && guard < 30 {
        h.key(KeyCode::Down);
        guard += 1;
    }
    assert!(menu_selected(h).contains(label), "{label}: {}", h.screen());
    h.key(KeyCode::Enter);
}

/// The text of the menu entry the keyboard is on.
fn menu_selected(h: &mut H) -> String {
    let Some(Modal::Menu(m)) = &h.app.modal else {
        return String::new();
    };
    m.items
        .get(m.selected)
        .map(|i| i.label.clone())
        .unwrap_or_default()
}

fn confirm(h: &mut H) {
    plan_modal(h);
    h.key(KeyCode::Enter);
    h.wait("the operation", |a| {
        a.running.is_none() && !matches!(a.modal, Some(Modal::Scanning { .. }))
    });
    // The folder is read again: an attribute change does not always reach the watcher.
    h.app.refresh();
    h.wait("reloaded", |a| !a.is_loading());
}

#[test]
fn a_tagged_file_shows_dots_in_its_colours_in_the_tag_column() {
    let Some((sb, dir)) = scene(&[
        ("a.txt", &["Red"]),
        ("b.txt", &["Blue", "Work", "Green", "Gray"]),
    ]) else {
        return;
    };
    let mut h = H::new(sb, dir, 170, 30);
    let s = h.screen();
    // The column has its title, and its dots are on the right of the name.
    assert!(s.contains("Tags"), "the column has a title:\n{s}");
    let after = |l: &str, what: &str| l[l.find(what).unwrap() + what.len()..].to_string();
    let a_row = s.lines().find(|l| l.contains("a.txt")).unwrap();
    assert_eq!(after(a_row, "a.txt").matches('●').count(), 1, "{a_row}");
    // More than three: three dots and +N.
    let b_row = s.lines().find(|l| l.contains("b.txt")).unwrap();
    let rest = after(b_row, "b.txt");
    assert!(rest.contains("● ● ● +1"), "{b_row}");
    // An untagged one has none.
    let sub_row = s.lines().find(|l| l.contains(" sub")).unwrap();
    assert!(!after(sub_row, "sub").contains('●'), "{sub_row}");
    // The dot is drawn in the tag's colour.
    let red = h.app.th.tag_color(Some(TagColor::Red));
    let buf = h.term.backend().buffer().clone();
    let mut found = false;
    for y in 0..buf.area.height {
        let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        if let Some(at) = line.find("a.txt") {
            let from = line[..at].chars().count();
            for x in from as u16..buf.area.width {
                if buf[(x, y)].symbol() == "●" {
                    assert_eq!(buf[(x, y)].fg, red);
                    found = true;
                    break;
                }
            }
        }
    }
    assert!(found, "the dot of a.txt was not found");
}

#[test]
fn the_details_pane_lists_the_tags() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Red", "Project X"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, 150, 36);
    h.keys("j"); // a.txt
    h.wait("details", |a| a.preview.name == "a.txt");
    let s = h.screen();
    let row = s
        .lines()
        .find(|l| l.contains("Tags") && l.contains("● Red"))
        .expect("a Tags row");
    assert!(
        row.contains("● Red") && row.contains("● Project X"),
        "{row}"
    );
}

#[test]
fn the_icons_view_puts_the_dots_under_the_name() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Green"])]) else {
        return;
    };
    let mut h = H::with_config(sb, dir, 140, 36, |c| c.view = rada_tui::ViewMode::Icons);
    assert!(h.screen().contains('●'));
}

#[test]
fn choosing_a_colour_in_the_menu_plans_runs_and_can_be_undone() {
    let Some((sb, dir)) = scene(&[]) else { return };
    let mut h = H::new(sb, dir, 170, 36);
    h.keys("j"); // a.txt
    choose_tag(&mut h, "Red");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(
        s.contains("Tag Red on a.txt") && s.contains("retag"),
        "the plan says what changes: {s}"
    );
    // The steps say how its tags change.
    h.key(KeyCode::Tab);
    assert!(h.screen().contains("none → Red"), "{}", h.screen());
    h.key(KeyCode::Tab);
    confirm(&mut h);
    assert_eq!(tags_of(&h, "a.txt"), ["Red"]);
    assert!(
        h.screen()
            .lines()
            .any(|l| l.contains("a.txt")
                && l.rsplit("a.txt").next().is_some_and(|r| r.contains('●'))),
        "the dot is in the Tag column"
    );

    // Undo, like any operation.
    h.keys("u");
    plan_modal(&mut h);
    h.key(KeyCode::Enter);
    h.wait("undone", |a| {
        a.running.is_none() && !matches!(a.modal, Some(Modal::Scanning { .. }))
    });
    h.app.refresh();
    h.wait("reloaded", |a| !a.is_loading());
    assert!(tags_of(&h, "a.txt").is_empty());
}

#[test]
fn the_same_colour_again_takes_it_off() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, 140, 36);
    h.keys("j");
    choose_tag(&mut h, "Red");
    confirm(&mut h);
    assert!(tags_of(&h, "a.txt").is_empty());
}

#[test]
fn a_selection_is_tagged_as_one_operation() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, 140, 36);
    h.keys("j  "); // a.txt, then Space twice: marks it and b.txt (each Space moves on)
    assert_eq!(h.marked_names().len(), 2, "{:?}", h.marked_names());
    choose_tag(&mut h, "Red");
    plan_modal(&mut h);
    // One had Red already: only the other changes, and the plan counts one step.
    let Some(Modal::Plan(pv)) = &h.app.modal else {
        panic!()
    };
    assert_eq!(pv.plan.steps.len(), 1);
    confirm(&mut h);
    assert_eq!(tags_of(&h, "a.txt"), ["Red"]);
    assert_eq!(tags_of(&h, "b.txt"), ["Red"]);
}

#[test]
fn a_new_tag_asks_for_its_name_and_refuses_a_bad_one() {
    let Some((sb, dir)) = scene(&[]) else { return };
    let mut h = H::new(sb, dir, 140, 36);
    h.keys("j");
    choose_tag(&mut h, "New tag");
    assert!(matches!(h.app.modal, Some(Modal::Input(_))));
    h.keys("a,b");
    h.key(KeyCode::Enter);
    assert!(h.screen().contains("comma"), "{}", h.screen());
    // Fix it.
    h.key(KeyCode::Backspace);
    h.key(KeyCode::Backspace);
    h.keys("Work");
    // (the text is now "aWork"; any good name works)
    h.key(KeyCode::Enter);
    confirm(&mut h);
    assert_eq!(tags_of(&h, "a.txt"), ["aWork"]);
}

#[test]
fn the_menu_ticks_what_the_item_has_and_offers_to_take_it_all_off() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Blue", "Client"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, 140, 36);
    h.keys("j");
    let (x, y) = h.where_is(&Target::Row(h.app.cursor));
    h.mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    while !menu_selected(&mut h).starts_with("Tags") {
        h.key(KeyCode::Down);
    }
    h.key(KeyCode::Enter);
    let Some(Modal::Menu(m)) = &h.app.modal else {
        panic!("{}", h.screen())
    };
    let labels: Vec<(String, Option<bool>, bool)> = m
        .items
        .iter()
        .map(|i| (i.label.clone(), i.checked, i.enabled))
        .collect();
    assert_eq!(labels[0].0, "● Red");
    assert_eq!(labels[0].1, Some(false));
    let blue = labels.iter().find(|l| l.0 == "● Blue").unwrap();
    assert_eq!(blue.1, Some(true), "{labels:?}");
    // A tag of the person's own is listed so that it can be taken off.
    let client = labels.iter().find(|l| l.0 == "● Client").unwrap();
    assert_eq!(client.1, Some(true));
    assert!(labels.iter().any(|l| l.0 == "New tag…" && l.2));
    assert!(labels.iter().any(|l| l.0 == "Remove all tags" && l.2));
    // …and with nothing to remove, that entry is not available.
    h.key(KeyCode::Esc);
}

#[test]
fn remove_all_tags_clears_them() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Red", "Work"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, 140, 36);
    h.keys("j");
    choose_tag(&mut h, "Remove all tags");
    confirm(&mut h);
    assert!(tags_of(&h, "a.txt").is_empty());
}

#[test]
fn the_filter_finds_by_tag_name_or_colour() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"]), ("b.txt", &["Project X"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, "140".parse().unwrap(), 30);
    h.keys("/");
    h.keys("tag:red");
    assert_eq!(h.names(), ["a.txt"]);
    h.key(KeyCode::Esc);
    h.keys("/");
    h.keys("tag:project");
    assert_eq!(h.names(), ["b.txt"]);
    h.key(KeyCode::Esc);
    // `tag:` alone is anything tagged; words beside it still match names.
    h.keys("/");
    h.keys("tag: b.");
    assert_eq!(h.names(), ["b.txt"]);
    h.key(KeyCode::Esc);
    h.keys("/");
    h.keys("tag:");
    assert_eq!(h.names(), ["a.txt", "b.txt"]);
}

#[test]
fn a_system_without_tags_has_no_tags_menu() {
    use rada_core::testplatform::{MacLike, MacLikeSpec};
    use std::sync::Arc;
    let (sb, dir) = sandbox_with_files();
    let p = Arc::new(MacLike::new(sb.platform(), MacLikeSpec::default()));
    let mut h = H::with_system(
        sb,
        dir,
        140,
        36,
        (Arc::new(rada_core::fs::LocalFs), p),
        |_| {},
    );
    h.keys("j");
    let (x, y) = h.where_is(&Target::Row(h.app.cursor));
    h.mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    assert!(!h.screen().contains("Tags"));
}

#[test]
fn the_colours_are_called_what_the_system_calls_them() {
    // On an Italian Mac the Finder knows its blue tag as "Blu"; a tag called "Blue" gets no
    // colour from it, which showed as a grey dot. The menu, the tag written and the dot all
    // follow the language.
    let Some((sb, dir)) = scene(&[("a.txt", &["Rosso"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, 140, 36);
    h.app.tag_lang = "it".into();
    h.keys("j"); // a.txt
    // A file tagged "Rosso" is red: the dot is drawn in red, and the menu ticks Rosso.
    assert_eq!(
        h.app.current().unwrap().tags[0].color,
        Some(rada_core::platform::TagColor::Red)
    );
    let (x, y) = h.where_is(&Target::Row(h.app.cursor));
    h.mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    while !menu_selected(&mut h).starts_with("Tags") {
        h.key(KeyCode::Down);
    }
    h.key(KeyCode::Enter);
    let Some(Modal::Menu(m)) = &h.app.modal else {
        panic!("{}", h.screen())
    };
    let labels: Vec<(String, Option<bool>)> = m
        .items
        .iter()
        .map(|i| (i.label.clone(), i.checked))
        .collect();
    let names: Vec<&str> = labels.iter().take(7).map(|l| l.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "● Rosso",
            "● Arancione",
            "● Giallo",
            "● Verde",
            "● Blu",
            "● Viola",
            "● Grigio"
        ]
    );
    assert_eq!(labels[0].1, Some(true), "Rosso is ticked: {labels:?}");
    h.key(KeyCode::Esc);
    // Choosing Blu puts a tag called Blu, blue, on the file.
    choose_tag(&mut h, "Blu");
    plan_modal(&mut h);
    confirm(&mut h);
    let blu = h
        .app
        .current()
        .unwrap()
        .tags
        .iter()
        .find(|t| t.name == "Blu")
        .cloned()
        .expect("a tag called Blu");
    assert_eq!(blu.color, Some(rada_core::platform::TagColor::Blue));
    // The filter finds the colour by either name.
    for want in ["tag:blu", "tag:blue"] {
        h.keys("/");
        h.keys(want);
        h.press("enter");
        assert_eq!(h.names(), ["a.txt"], "{want}");
        h.press("esc");
    }
}
