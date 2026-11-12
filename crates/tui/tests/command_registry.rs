//! What the commands are and where they can be used, written down as it was before the
//! registry of commands existed: the list the bar offers, what a chooser refuses, the entries
//! of the menus and which are greyed, the buttons that answer. In a folder, in an archive, in
//! a list that is not a folder and while choosing a file. The registry must give the same.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_core::ViewKind;
use rada_core::archive::testkit::{Member, write_zip};
use rada_tui::app::{MenuItem, MenuKind};
use rada_tui::chooser::Chooser;
use rada_tui::hits::Target;
use rada_tui::keymap::Action;

fn listed(title: &str, items: &[MenuItem], out: &mut String) {
    out.push_str(&format!("-- {title}\n"));
    for item in items {
        let mark = if item.enabled { "x" } else { " " };
        let gap = if item.gap_before { " (gap)" } else { "" };
        out.push_str(&format!("[{mark}] {}{gap}\n", item.label));
    }
}

/// Everything the tests of this file compare, as text.
fn report(h: &mut H) -> String {
    let mut out = String::from("-- the bar offers\n");
    for a in h.app.bar_commands() {
        out.push_str(&format!("{} | {}\n", a.id(), a.label()));
    }
    out.push_str("-- a chooser refuses\n");
    let refused: Vec<&str> = Action::ALL
        .iter()
        .filter(|a| {
            h.app
                .session
                .chooser
                .as_ref()
                .is_some_and(|c| !c.allows(**a))
        })
        .map(|a| a.id())
        .collect();
    out.push_str(&format!("{}\n", refused.join(" ")));
    for kind in [MenuKind::New, MenuKind::More, MenuKind::Help] {
        listed(&format!("menu {kind:?}"), &h.app.menu_items(kind), &mut out);
    }
    listed(
        "menu of an item",
        &h.app.context_menu(true, (0, 0)).items,
        &mut out,
    );
    listed(
        "menu of the empty space",
        &h.app.context_menu(false, (0, 0)).items,
        &mut out,
    );
    out.push_str("-- buttons that answer\n");
    let _ = h.screen();
    let buttons = [
        Action::Cut,
        Action::Copy,
        Action::Paste,
        Action::Rename,
        Action::Compress,
        Action::Trash,
    ];
    for a in buttons {
        let on = h.app.frame.hits.find(&Target::Act(a)).is_some();
        out.push_str(&format!("{}: {}\n", a.id(), if on { "on" } else { "off" }));
    }
    let new = h
        .app
        .frame
        .hits
        .find(&Target::Menu(MenuKind::New))
        .is_some();
    out.push_str(&format!("new: {}\n", if new { "on" } else { "off" }));
    out
}

fn scene() -> (rada_core::testutil::Sandbox, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/alpha.txt", "a");
    sb.write("w/sub/needle.txt", "n");
    let members = [
        Member::dir("docs"),
        Member::file("docs/readme.txt", "hello"),
    ];
    write_zip(&sb.path("w/box.zip"), &members);
    let dir = sb.path("w");
    (sb, dir)
}

fn on(h: &mut H, name: &str) {
    h.wait("the folder", |a| {
        !a.tab.is_loading() && !a.tab.visible.is_empty()
    });
    h.cursor_to(name);
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the menus have entries of their own on each system (tags, Quick Look)"
)]
fn in_a_folder() {
    let (sb, dir) = scene();
    let mut h = H::new(sb, dir, 140, 36);
    on(&mut h, "alpha.txt");
    insta::assert_snapshot!("commands_in_a_folder", report(&mut h));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the menus have entries of their own on each system (tags, Quick Look)"
)]
fn in_a_folder_with_something_copied() {
    let (sb, dir) = scene();
    let mut h = H::new(sb, dir, 140, 36);
    on(&mut h, "sub");
    h.press("ctrl+c");
    insta::assert_snapshot!("commands_with_a_clipboard", report(&mut h));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the menus have entries of their own on each system (tags, Quick Look)"
)]
fn on_an_archive_and_inside_it() {
    let (sb, dir) = scene();
    let mut h = H::new(sb, dir, 140, 36);
    on(&mut h, "box.zip");
    insta::assert_snapshot!("commands_on_an_archive", report(&mut h));
    h.key(KeyCode::Enter);
    h.wait("inside the archive", |a| {
        a.tab.archive.is_some() && !a.tab.is_loading()
    });
    insta::assert_snapshot!("commands_in_an_archive", report(&mut h));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the menus have entries of their own on each system (tags, Quick Look)"
)]
fn in_a_list_that_is_not_a_folder() {
    let (sb, dir) = scene();
    let mut h = H::new(sb, dir, 140, 36);
    on(&mut h, "alpha.txt");
    h.press("ctrl+f");
    h.keys("needle");
    h.press("ctrl+enter");
    h.wait("the result", |a| {
        matches!(a.tab.virtual_view(), Some(ViewKind::Search { .. })) && !a.tab.is_loading()
    });
    insta::assert_snapshot!("commands_in_a_search", report(&mut h));
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the menus have entries of their own on each system (tags, Quick Look)"
)]
fn while_choosing_a_file_and_a_folder() {
    let (sb, dir) = scene();
    let file = Chooser::file(false);
    let mut h = H::with_config(sb, dir, 140, 36, move |c| c.chooser = Some(file));
    on(&mut h, "alpha.txt");
    insta::assert_snapshot!("commands_choosing_a_file", report(&mut h));

    let (sb, dir) = scene();
    let folder = Chooser::folder(false);
    let mut h = H::with_config(sb, dir, 140, 36, move |c| c.chooser = Some(folder));
    h.wait("the folder", |a| !a.tab.is_loading());
    insta::assert_snapshot!("commands_choosing_a_folder", report(&mut h));
}
