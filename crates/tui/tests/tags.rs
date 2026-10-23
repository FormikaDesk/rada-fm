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
        s.contains("Tag Red on a.txt"),
        "the plan says what changes: {s}"
    );
    // The steps say how its tags change (d switches to them and back).
    h.keys("d");
    assert!(h.screen().contains("none → Red"), "{}", h.screen());
    h.keys("d");
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
    assert_eq!(labels[0].0, "Red");
    assert_eq!(labels[0].1, Some(false));
    let blue = labels.iter().find(|l| l.0 == "Blue").unwrap();
    assert_eq!(blue.1, Some(true), "{labels:?}");
    // A tag of the person's own is listed so that it can be taken off.
    let client = labels.iter().find(|l| l.0 == "Client").unwrap();
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
            "Rosso",
            "Arancione",
            "Giallo",
            "Verde",
            "Blu",
            "Viola",
            "Grigio"
        ]
    );
    assert_eq!(labels[0].1, Some(true), "Rosso is ticked: {labels:?}");
    // Esc gives back the menu the tags were opened from; a second one closes it.
    h.key(KeyCode::Esc);
    h.key(KeyCode::Esc);
    assert!(h.app.modal.is_none());
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

/// The line of the column titles of the list.
fn titles(h: &mut H) -> String {
    h.screen()
        .lines()
        .find(|l| l.contains("Name") && l.contains("Size"))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn tag_column_hidden_from_view_menu() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Red", "Blue"])]) else {
        return;
    };
    let mut h = H::new(sb, dir, 170, 30);
    assert!(titles(&mut h).contains("Tags"), "{}", titles(&mut h));
    // View ▾ has the switch, ticked.
    h.click(&Target::Menu(rada_tui::app::MenuKind::View));
    let s = h.screen();
    assert!(
        s.lines()
            .any(|l| l.contains("Tags column") && l.contains('✓')),
        "{s}"
    );
    let mut guard = 0;
    while !menu_selected(&mut h).contains("Tags column") && guard < 20 {
        h.key(KeyCode::Down);
        guard += 1;
    }
    h.key(KeyCode::Enter);
    assert!(!h.app.show_tags);
    assert!(!titles(&mut h).contains("Tags"), "{}", titles(&mut h));
    // The dots are gone from the list too (the details pane still has them).
    let a_row = h
        .screen()
        .lines()
        .find(|l| l.contains("a.txt"))
        .unwrap()
        .to_string();
    assert!(!a_row.contains("● ●"), "{a_row}");
    // Brought back by its key.
    h.press("f8");
    assert!(h.app.show_tags && titles(&mut h).contains("Tags"));
}

#[test]
fn config_and_choice_decide_tag_column() {
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"])]) else {
        return;
    };
    // `tags_column = false` in the configuration.
    let mut h = H::with_config(sb, dir, 170, 30, |c| c.tags_column = Some(false));
    assert!(!h.app.show_tags);
    assert!(!titles(&mut h).contains("Tags"));

    // What was chosen last time, when the configuration says nothing.
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"])]) else {
        return;
    };
    let h = H::with_config(sb, dir, 170, 30, |c| c.saved_ui.tags_column = Some(false));
    assert!(!h.app.show_tags);
    // And the file wins over what was remembered.
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"])]) else {
        return;
    };
    let h = H::with_config(sb, dir, 170, 30, |c| {
        c.tags_column = Some(true);
        c.saved_ui.tags_column = Some(false);
    });
    assert!(h.app.show_tags);
}

#[test]
fn up_to_three_dots_then_plus_n_and_a_tag_with_no_colour_is_a_dim_dot() {
    let Some((sb, dir)) = scene(&[
        ("a.txt", &["Red", "Blue", "Green", "Yellow", "Gray"]),
        ("b.txt", &["Project X"]),
    ]) else {
        return;
    };
    let mut h = H::new(sb, dir, 170, 30);
    let s = h.screen();
    let a = s.lines().find(|l| l.contains("a.txt")).unwrap();
    assert!(a.contains("● ● ● +2"), "{a}");
    let th = h.app.th.clone();
    let buf = h.term.backend().buffer().clone();
    let y = (0..buf.area.height)
        .find(|&y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .contains("b.txt")
        })
        .unwrap();
    let dot = (0..buf.area.width)
        .rev()
        .find(|&x| buf[(x, y)].symbol() == "●" && buf[(x, y)].fg == th.tag_color(None));
    assert!(
        dot.is_some(),
        "a tag of its own, with no colour, is a dim dot"
    );
}

/// How wide the Name column is on the title line, and whether the line has a Tags title.
fn name_width_and_tags(h: &mut H) -> (usize, bool) {
    let line = titles(h);
    let cells: Vec<char> = line.chars().collect();
    let at = |what: &str| {
        let byte = line
            .find(what)
            .unwrap_or_else(|| panic!("{what:?} in {line:?}"));
        line[..byte].chars().count()
    };
    let _ = cells;
    // The name starts at its title and runs up to the gap before the date.
    (at("Date modified") - at("Name") - 2, line.contains("Tags"))
}

#[test]
fn a_folder_without_tags_gives_the_tag_column_to_the_names() {
    use rada_tui::ui::{ListOpts, list_columns};

    // No file here carries a tag, and the window is as narrow as the date column allows.
    let (sb, dir) = sandbox_with_files();
    let mut h = H::with_config(sb, dir.clone(), 90, 24, |c| c.details_pane = Some(false));
    assert!(h.app.tags_supported() && h.app.show_tags);
    let (name, tags) = name_width_and_tags(&mut h);
    assert!(!tags, "{}", titles(&mut h));
    // What the name had before: the table of the same width with the Tag column let in.
    let area = 90 - rada_tui::sidebar::Mode::Rail.columns() as usize - 2;
    let opts = |show_tags| ListOpts {
        show_type: false,
        show_date: true,
        show_tags,
    };
    let rel = rada_tui::fmt::DateStyle::Relative;
    let icons = h.app.icons.width();
    assert_eq!(name, list_columns(area, opts(false), icons, rel).name);
    let before = list_columns(area, opts(true), icons, rel);
    assert!(
        before.tags > 0 && name > before.name,
        "{name} vs {}",
        before.name
    );

    // Wide enough, and still nothing to show: no column.
    let (sb, dir) = sandbox_with_files();
    let mut h = H::with_config(sb, dir, 140, 30, |c| c.details_pane = Some(false));
    let (bare, has) = name_width_and_tags(&mut h);
    assert!(!has, "{}", titles(&mut h));
    drop(h);

    // The same folder with one tag in it has the column (wide), and the names pay for it.
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"])]) else {
        return;
    };
    let mut h = H::with_config(sb, dir, 140, 30, |c| c.details_pane = Some(false));
    let (with_tags, has) = name_width_and_tags(&mut h);
    assert!(has, "{}", titles(&mut h));
    assert!(with_tags < bare, "{with_tags} vs {bare}");
    // Narrow, the same tagged folder has none.
    let Some((sb, dir)) = scene(&[("a.txt", &["Red"])]) else {
        return;
    };
    let mut h = H::with_config(sb, dir, 78, 24, |c| c.details_pane = Some(false));
    let line = titles(&mut h);
    assert!(!line.contains("Tags"), "{line}");
}
