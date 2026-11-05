//! Words typed in the bar that match nothing in the folder can be searched for in the folders
//! under it: the result is a list like a tag's, read-only, that one leaves the way one came.

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_core::ViewKind;
use rada_core::testutil::Sandbox;
use rada_tui::app::BarItem;
use rada_tui::keymap::Action;

fn scene() -> H {
    let sb = Sandbox::new();
    sb.write("w/alpha.txt", "a");
    sb.write("w/sub/deep/needle.txt", "n");
    sb.write("w/other/Needle2.md", "n");
    sb.write("w/other/hay.md", "h");
    let dir = sb.path("w");
    H::new(sb, dir, 140, 36)
}

fn search_for(h: &mut H, words: &str) {
    h.press("ctrl+f");
    h.keys(words);
    let a = h.app.windows.address.as_ref().expect("the field is open");
    assert!(
        matches!(a.hits.last().map(|x| &x.item), Some(BarItem::Search(w)) if w == words),
        "{:?}",
        a.hits.iter().map(|x| &x.item).collect::<Vec<_>>()
    );
    h.key(KeyCode::Tab);
    // Tab walks the hits: go on until the search is the one picked.
    while h.app.windows.address.as_ref().and_then(|a| a.hit_selected)
        != Some(h.app.windows.address.as_ref().unwrap().hits.len() - 1)
    {
        h.key(KeyCode::Tab);
    }
    h.key(KeyCode::Enter);
    h.wait("the result", |a| {
        matches!(a.tab.virtual_view(), Some(ViewKind::Search { .. })) && !a.tab.is_loading()
    });
}

#[test]
fn it_is_offered_only_when_nothing_in_the_folder_matches() {
    let mut h = scene();
    h.press("ctrl+f");
    h.keys("alp");
    let a = h.app.windows.address.as_ref().unwrap();
    assert!(
        !a.hits.iter().any(|x| matches!(x.item, BarItem::Search(_))),
        "a file here matches"
    );
    h.keys("xyz");
    let a = h.app.windows.address.as_ref().unwrap();
    assert!(a.hits.iter().any(|x| matches!(x.item, BarItem::Search(_))));
}

#[test]
fn enter_alone_does_not_start_the_search() {
    let mut h = scene();
    let folder = h.app.tab.cwd.clone();
    h.press("ctrl+f");
    h.keys("qzx");
    h.key(KeyCode::Enter);
    h.settle();
    assert_eq!(h.app.tab.cwd, folder);
    assert!(h.app.windows.address.is_none());
}

#[test]
fn the_result_lists_what_is_below_under_its_title() {
    let mut h = scene();
    search_for(&mut h, "needle");

    let mut names = h.names();
    names.sort();
    assert_eq!(names, ["Needle2.md", "needle.txt"]);
    let s = h.screen();
    assert!(s.contains("Search: needle"), "{s}");
    assert!(
        h.app.tab.archive.is_none() && h.app.tab.cwd.to_string_lossy().starts_with("find:"),
        "it is a list, not a folder"
    );
}

#[test]
fn nothing_can_be_made_in_the_result_and_up_leaves_it_the_way_one_came() {
    let mut h = scene();
    let before = h.app.tab.cwd.clone();
    search_for(&mut h, "needle");

    h.app.dispatch(Action::NewFolder);
    assert!(h.app.windows.modal.is_none(), "no window to name a folder");

    h.press("alt+up");
    h.wait("the folder", |a| {
        a.tab.virtual_view().is_none() && !a.tab.is_loading()
    });
    assert_eq!(h.app.tab.cwd, before);
}

#[test]
fn a_result_is_not_remembered_as_a_place_and_the_filter_works_inside_it() {
    let mut h = scene();
    search_for(&mut h, "needle");
    h.press("ctrl+f");
    h.keys("md");
    assert_eq!(h.names(), ["Needle2.md"]);
}

#[test]
fn no_search_is_offered_inside_a_result() {
    let mut h = scene();
    search_for(&mut h, "needle");
    h.press("ctrl+f");
    h.keys("zzz");
    let a = h.app.windows.address.as_ref().unwrap();
    assert!(!a.hits.iter().any(|x| matches!(x.item, BarItem::Search(_))));
}
