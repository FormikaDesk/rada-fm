//! What a tab keeps while another one is in front: the folder, the cursor, the selection, the
//! filter, the order and the view; a tab that was still reading its folder; a new tab that is
//! closed again; which tab a closed one hands over to; and the tabs that come back at the
//! next start.

mod common;

use common::*;
use rada_core::model::SortKey;
use rada_core::uistate::{SavedTab, UiState};
use rada_tui::ViewMode;
use rada_tui::keymap::Action;
use std::path::PathBuf;

/// `home/work` with a few files and two folders, one of them with files of its own.
fn world() -> (rada_core::testutil::Sandbox, PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    let work = sb.mkdir("home/work");
    sb.mkdir("home/work/sub");
    sb.mkdir("home/work/other");
    for name in ["alpha.txt", "beta.txt", "gamma.md", "delta.txt", "omega.md"] {
        sb.write(format!("home/work/{name}"), name);
    }
    sb.write("home/work/big.bin", vec![b'x'; 3000]);
    for name in ["one.txt", "two.txt", "three.md"] {
        sb.write(format!("home/work/sub/{name}"), name);
    }
    sb.write("home/work/other/only.txt", "only");
    (sb, work)
}

fn start() -> H {
    let (sb, work) = world();
    H::new(sb, work, 120, 30)
}

/// Everything that is a tab's own, as it is seen from the front.
#[derive(Debug, PartialEq)]
struct Seen {
    cwd: PathBuf,
    cursor: String,
    marked: Vec<String>,
    filter: Option<String>,
    sort: (SortKey, bool),
    chosen: bool,
    view: ViewMode,
}

fn seen(h: &H) -> Seen {
    Seen {
        cwd: h.app.cwd.clone(),
        cursor: h.cursor_name(),
        marked: h.marked_names(),
        filter: h.app.filter.as_ref().map(|f| f.text.clone()),
        sort: (h.app.sort.key, h.app.sort.reverse),
        chosen: h.app.sort_chosen,
        view: h.app.view,
    }
}

fn new_tab(h: &mut H, count: usize) {
    h.press("ctrl+t");
    h.wait("the new tab", |a| {
        a.tabs.count() == count && !a.is_loading()
    });
}

fn go_to(h: &mut H, tab: usize) {
    h.app.switch_tab(tab);
    h.wait("the tab", |a| a.tabs.active == tab && !a.is_loading());
}

/// The cursor on `name`, counting from the top: Down alone never goes back up.
fn cursor_on(h: &mut H, name: &str) {
    h.keys("g");
    h.cursor_to(name);
}

fn enter(h: &mut H, folder: &str) {
    cursor_on(h, folder);
    h.press("enter");
    h.wait(folder, |a| a.cwd.ends_with(folder) && !a.is_loading());
}

fn mark(h: &mut H, name: &str) {
    cursor_on(h, name);
    h.press("space");
}

fn filter(h: &mut H, text: &str) {
    h.press("ctrl+f");
    h.keys(text);
    h.key(crossterm::event::KeyCode::Enter);
}

#[test]
fn each_tab_keeps_its_folder_cursor_selection_filter_order_and_view_when_you_leave_and_return() {
    let mut h = start();
    // Tab 1: the icons view, by size, a filter, one file marked and the cursor on another.
    h.app.dispatch(Action::Sort);
    filter(&mut h, "a");
    mark(&mut h, "beta.txt");
    cursor_on(&mut h, "omega.md");
    h.app.dispatch(Action::ViewIcons);
    let first = seen(&h);
    assert_eq!(first.marked, ["beta.txt"]);
    assert_eq!(first.view, ViewMode::Icons);
    assert!(first.chosen && first.filter.as_deref() == Some("a"));

    // Tab 2: another folder, the details view, the order reversed, another filter and mark.
    new_tab(&mut h, 2);
    h.app.dispatch(Action::ViewDetails);
    enter(&mut h, "sub");
    h.app.dispatch(Action::SortReverse);
    filter(&mut h, "t");
    mark(&mut h, "two.txt");
    cursor_on(&mut h, "one.txt");
    let second = seen(&h);
    assert_ne!(first, second);

    go_to(&mut h, 0);
    assert_eq!(seen(&h), first, "the first tab as it was left");
    go_to(&mut h, 1);
    assert_eq!(seen(&h), second, "the second tab as it was left");
    go_to(&mut h, 0);
    assert_eq!(seen(&h), first, "and again");
}

#[test]
fn a_folder_that_was_still_loading_when_the_tab_was_left_is_not_loading_when_it_is_back() {
    let mut h = start();
    let work = h.app.cwd.clone();
    cursor_on(&mut h, "sub");
    h.press("enter");
    assert!(h.app.is_loading(), "the folder is being read");
    new_tab(&mut h, 2);
    go_to(&mut h, 0);
    h.pump(200);
    assert!(
        !h.app.is_loading(),
        "the read of the first tab does not wait for an answer that is gone"
    );
    assert_eq!(
        h.app.cwd, work,
        "the way into the folder was given up with the tab"
    );
}

#[test]
fn a_step_back_that_was_on_its_way_is_given_up_with_the_tab() {
    let mut h = start();
    let work = h.app.cwd.clone();
    enter(&mut h, "sub");
    h.press("alt+left");
    assert!(h.app.is_loading(), "the step back has been asked for");
    new_tab(&mut h, 2);
    go_to(&mut h, 0);
    assert!(h.app.cwd.ends_with("sub") && h.app.nav.can_back());
    // Going to the folder it was asked for, by another way, is a visit and not that step.
    h.app.dispatch(Action::Parent);
    h.wait("work", |a| a.cwd == work && !a.is_loading());
    assert!(
        !h.app.nav.can_forward(),
        "a new visit, not the step back that was given up"
    );
}

#[test]
fn a_new_tab_that_is_closed_again_leaves_the_tab_it_came_from_as_it_was() {
    let mut h = start();
    h.app.dispatch(Action::SortReverse);
    filter(&mut h, "a");
    mark(&mut h, "gamma.md");
    cursor_on(&mut h, "delta.txt");
    h.app.dispatch(Action::ViewIcons);
    let before = seen(&h);

    new_tab(&mut h, 2);
    assert_eq!(h.app.tabs.active, 1);
    h.app.dispatch(Action::ViewDetails);
    enter(&mut h, "other");
    h.app.close_tab(1);
    h.wait("the first tab", |a| a.tabs.count() == 1 && !a.is_loading());
    assert_eq!(h.app.tabs.active, 0);
    assert_eq!(seen(&h), before);
}

#[test]
fn closing_the_front_tab_goes_to_the_neighbour_it_leaves_in_its_place() {
    let mut h = start();
    // Three tabs, each in a folder of its own with a mark of its own.
    mark(&mut h, "alpha.txt");
    new_tab(&mut h, 2);
    enter(&mut h, "sub");
    mark(&mut h, "one.txt");
    go_to(&mut h, 0);
    new_tab(&mut h, 3);
    enter(&mut h, "other");
    mark(&mut h, "only.txt");
    let folders = |h: &H| -> Vec<String> {
        (0..h.app.tabs.count())
            .map(|i| {
                h.app
                    .tab_cwd(i)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    };
    assert_eq!(folders(&h), ["work", "other", "sub"]);

    // The one in the middle: the tab that was after it takes its place.
    h.app.close_tab(1);
    h.wait("sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    assert_eq!((h.app.tabs.active, h.app.tabs.count()), (1, 2));
    assert_eq!(h.marked_names(), ["one.txt"]);
    assert_eq!(folders(&h), ["work", "sub"]);

    // The last one: the one before it.
    h.app.close_tab(1);
    h.wait("work", |a| a.cwd.ends_with("work") && !a.is_loading());
    assert_eq!((h.app.tabs.active, h.app.tabs.count()), (0, 1));
    assert_eq!(h.marked_names(), ["alpha.txt"]);
}

#[test]
fn closing_the_first_tab_of_several_brings_in_the_next_one() {
    let mut h = start();
    new_tab(&mut h, 2);
    enter(&mut h, "sub");
    mark(&mut h, "two.txt");
    go_to(&mut h, 0);
    h.app.close_tab(0);
    h.wait("sub", |a| a.cwd.ends_with("sub") && !a.is_loading());
    assert_eq!((h.app.tabs.active, h.app.tabs.count()), (0, 1));
    assert_eq!(h.marked_names(), ["two.txt"]);
}

#[test]
fn closing_a_tab_behind_the_front_one_keeps_the_front_one_and_its_place() {
    let mut h = start();
    new_tab(&mut h, 2);
    new_tab(&mut h, 3);
    enter(&mut h, "sub");
    mark(&mut h, "three.md");
    let before = seen(&h);
    h.app.close_tab(0);
    assert_eq!((h.app.tabs.active, h.app.tabs.count()), (1, 2));
    assert_eq!(seen(&h), before);
}

#[test]
fn the_tabs_that_were_saved_come_back_the_same_and_stay_so_whichever_is_in_front() {
    let sb = rada_core::testutil::Sandbox::new();
    let a = sb.mkdir("home/a");
    let b = sb.mkdir("home/b");
    let c = sb.mkdir("home/c");
    let tab = |path: &PathBuf, view: &str, sort: Option<&str>, reverse: bool| SavedTab {
        path: path.clone(),
        view: Some(view.into()),
        sort: sort.map(String::from),
        reverse,
        sort_chosen: Some(sort.is_some()),
    };
    let tabs = vec![
        tab(&a, "icons", Some("size"), true),
        tab(&b, "details", None, false),
        tab(&c, "columns", Some("name"), false),
    ];
    let saved = UiState {
        tabs: tabs.clone(),
        active_tab: 1,
        ..Default::default()
    };
    let mut h = H::with_config(sb, a, 120, 30, move |cfg| cfg.saved_ui = saved);
    assert_eq!(h.app.tabs.active, 1);
    assert_eq!(h.app.saved_tabs(), tabs, "as they were read");
    for to in [0, 2, 1, 0] {
        go_to(&mut h, to);
        assert_eq!(h.app.saved_tabs(), tabs, "with tab {to} in front");
    }
    // What is written is the same list, with the tab in front.
    let dir = h.app.home().parent().unwrap().join("xdg/state/rada");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let written = rada_core::uistate::load(&dir);
        if written.active_tab == 0 && written.tabs == tabs {
            break;
        }
        assert!(std::time::Instant::now() < end, "tabs were not saved");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
