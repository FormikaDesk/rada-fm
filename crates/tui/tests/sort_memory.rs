//! The order of a tab is remembered between sessions only where the person chose it. A tab
//! that was never sorted follows `config.toml`, whatever it says at the next start; and a date
//! that is not reversed is the newest first, as in the Finder and in Explorer.

mod common;

use common::*;
use rada_core::model::{SortKey, SortSpec};
use rada_core::uistate::{SavedTab, UiState};
use rada_tui::keymap::Action;
use std::time::{Duration, SystemTime};

fn config_sort(key: SortKey, reverse: bool) -> SortSpec {
    SortSpec {
        key,
        reverse,
        ..SortSpec::default()
    }
}

/// A folder whose names, sizes and dates all go different ways.
fn dated() -> (rada_core::testutil::Sandbox, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    let dir = sb.mkdir("home/dl");
    let now = SystemTime::now();
    sb.mkdir("home/dl/zdir");
    for (name, age_days, size) in [
        ("a-old.txt", 30u64, 10usize),
        ("b-mid.txt", 10, 300),
        ("c-new.txt", 1, 200),
    ] {
        let p = sb.write(format!("home/dl/{name}"), vec![b'x'; size]);
        rada_core::testutil::set_mtime(&p, now - Duration::from_secs(age_days * 86_400));
    }
    (sb, dir)
}

fn saved_tab(
    path: &std::path::Path,
    sort: Option<&str>,
    reverse: bool,
    chosen: Option<bool>,
) -> SavedTab {
    SavedTab {
        path: path.to_path_buf(),
        view: None,
        sort: sort.map(String::from),
        reverse,
        sort_chosen: chosen,
    }
}

fn start_with(saved: UiState, config: SortSpec) -> (H, std::path::PathBuf, std::path::PathBuf) {
    let (sb, dir) = dated();
    let state = sb.dirs.rada_state();
    let d = dir.clone();
    let mut saved = saved;
    for t in &mut saved.tabs {
        t.path = d.clone();
    }
    let h = H::with_config(sb, dir.clone(), 120, 30, move |c| {
        c.saved_ui = saved;
        c.sort = config;
    });
    (h, dir, state)
}

fn state_with(tab: SavedTab) -> UiState {
    UiState {
        tabs: vec![tab],
        ..Default::default()
    }
}

#[test]
fn plain_date_sort_is_newest_first() {
    let (sb, dir) = dated();
    let mut h = H::with_config(sb, dir, 170, 35, |c| {
        c.sort = config_sort(SortKey::Modified, false)
    });
    assert_eq!(h.names(), ["zdir", "c-new.txt", "b-mid.txt", "a-old.txt"]);
    // The arrow says so: downwards, from the newest.
    let head = h
        .screen()
        .lines()
        .find(|l| l.contains("Name") || l.contains("Date modified"))
        .unwrap()
        .to_string();
    assert!(head.contains("Date modified ↓"), "{head}");
    // Reversed, it is the oldest first.
    let (sb, dir) = dated();
    let h = H::with_config(sb, dir, 120, 30, |c| {
        c.sort = config_sort(SortKey::Modified, true)
    });
    assert_eq!(h.names(), ["zdir", "a-old.txt", "b-mid.txt", "c-new.txt"]);
}

#[test]
fn a_tab_that_was_saved_without_a_choice_follows_the_configuration() {
    let tab = saved_tab(std::path::Path::new("/x"), None, false, Some(false));
    let (h, _, _) = start_with(state_with(tab), config_sort(SortKey::Modified, false));
    assert_eq!(h.app.tab.sort.key, SortKey::Modified);
    assert!(!h.app.tab.sort.reverse);
    assert!(!h.app.tab.sort_chosen);
    assert_eq!(
        h.names()[1],
        "c-new.txt",
        "the newest first: {:?}",
        h.names()
    );
}

#[test]
fn a_tab_with_a_choice_keeps_it_over_the_configuration() {
    let tab = saved_tab(std::path::Path::new("/x"), Some("size"), true, Some(true));
    let (h, _, _) = start_with(state_with(tab), config_sort(SortKey::Modified, false));
    assert_eq!(
        (h.app.tab.sort.key, h.app.tab.sort.reverse),
        (SortKey::Size, true)
    );
    assert!(h.app.tab.sort_chosen);
    // The biggest first.
    assert_eq!(h.names(), ["zdir", "b-mid.txt", "c-new.txt", "a-old.txt"]);
}

#[test]
fn old_state_file_means_no_sort_choice() {
    // What the older versions wrote for every tab, chosen or not.
    let name = saved_tab(std::path::Path::new("/x"), Some("name"), false, None);
    let (h, _, _) = start_with(state_with(name), config_sort(SortKey::Modified, false));
    assert_eq!(h.app.tab.sort.key, SortKey::Modified, "follows config.toml");
    assert!(!h.app.tab.sort_chosen);
    // Any other is taken for a choice, and kept.
    let size = saved_tab(std::path::Path::new("/x"), Some("size"), false, None);
    let (h, _, _) = start_with(state_with(size), config_sort(SortKey::Modified, false));
    assert_eq!(
        (h.app.tab.sort.key, h.app.tab.sort.reverse),
        (SortKey::Size, false)
    );
    assert!(h.app.tab.sort_chosen);
    let reversed = saved_tab(std::path::Path::new("/x"), Some("name"), true, None);
    let (h, _, _) = start_with(state_with(reversed), config_sort(SortKey::Modified, false));
    assert_eq!(
        (h.app.tab.sort.key, h.app.tab.sort.reverse),
        (SortKey::Name, true)
    );
}

#[test]
fn an_old_date_that_was_reversed_still_shows_the_newest_first() {
    // Before, a date was the oldest first until it was reversed: "date, reversed" was the
    // newest first. It still is what that tab shows.
    let old = saved_tab(std::path::Path::new("/x"), Some("date"), true, None);
    let (h, _, _) = start_with(state_with(old), config_sort(SortKey::Name, false));
    assert_eq!(h.app.tab.sort.key, SortKey::Modified);
    assert!(
        !h.app.tab.sort.reverse,
        "newest first is the plain date now"
    );
    assert_eq!(h.names()[1], "c-new.txt");
    // And "date, not reversed" (the oldest first) is still the oldest first.
    let old = saved_tab(std::path::Path::new("/x"), Some("date"), false, None);
    let (h, _, _) = start_with(state_with(old), config_sort(SortKey::Name, false));
    assert!(h.app.tab.sort.reverse);
    assert_eq!(h.names()[1], "a-old.txt");
}

#[test]
fn what_is_written_has_the_order_only_for_a_tab_that_was_sorted() {
    let (sb, dir) = dated();
    let state = sb.dirs.rada_state();
    let mut h = H::with_config(sb, dir, 120, 30, |c| {
        c.sort = config_sort(SortKey::Name, false)
    });
    // Anything that saves the state, with no sort touched: no order in the file.
    h.press("ctrl+t");
    h.wait("tab 2", |a| a.tabs.count() == 2 && !a.tab.is_loading());
    let saved = wait_for_tabs(&state, 2);
    assert!(
        saved.tabs.iter().all(|t| t.sort.is_none() && !t.reverse),
        "{saved:?}"
    );
    assert!(
        saved.tabs.iter().all(|t| t.sort_chosen == Some(false)),
        "{saved:?}"
    );
    // Sort the tab in front: it is written, with the choice, and the other is not.
    h.app.dispatch(Action::SortReverse);
    let end = std::time::Instant::now() + Duration::from_secs(5);
    let saved = loop {
        let s = rada_core::uistate::load(&state);
        if s.tabs.iter().any(|t| t.sort_chosen == Some(true)) || std::time::Instant::now() > end {
            break s;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let chosen: Vec<&SavedTab> = saved
        .tabs
        .iter()
        .filter(|t| t.sort_chosen == Some(true))
        .collect();
    assert_eq!(chosen.len(), 1, "{saved:?}");
    assert_eq!(chosen[0].sort.as_deref(), Some("name"));
    assert!(chosen[0].reverse);
    let other = saved
        .tabs
        .iter()
        .find(|t| t.sort_chosen == Some(false))
        .unwrap();
    assert!(other.sort.is_none());
}

fn wait_for_tabs(state: &std::path::Path, n: usize) -> UiState {
    let end = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let s = rada_core::uistate::load(state);
        if s.tabs.len() == n || std::time::Instant::now() > end {
            return s;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn new_config_wins_unless_a_choice_was_made() {
    // First session: the configuration says name; one tab is sorted by hand, one is not.
    let (sb, dir) = dated();
    let state = sb.dirs.rada_state();
    let mut h = H::with_config(sb, dir.clone(), 120, 30, |c| {
        c.sort = config_sort(SortKey::Name, false)
    });
    h.press("ctrl+t");
    h.wait("tab 2", |a| a.tabs.count() == 2 && !a.tab.is_loading());
    h.app.dispatch(Action::SortReverse); // tab 2: chosen
    // One write at a time: each is made by a thread of its own, so two close together may land
    // in either order.
    let wait_saved = |what: &str, ok: &dyn Fn(&UiState) -> bool| {
        let end = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let s = rada_core::uistate::load(&state);
            if ok(&s) {
                return s;
            }
            assert!(std::time::Instant::now() < end, "not saved ({what}): {s:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait_saved("the choice", &|s| {
        s.tabs.len() == 2 && s.tabs.iter().any(|t| t.sort_chosen == Some(true))
    });
    h.press("alt+1");
    h.wait("tab 1", |a| a.tabs.active == 0 && !a.tab.is_loading());
    let saved = wait_saved("the front tab", &|s| s.active_tab == 0 && s.tabs.len() == 2);
    drop(h);
    // Second session: the configuration now says date.
    let (sb, _) = dated();
    let mut h = H::with_config(sb, dir, 120, 30, move |c| {
        c.saved_ui = saved;
        c.sort = config_sort(SortKey::Modified, false);
    });
    assert_eq!(h.app.tabs.count(), 2);
    assert_eq!(
        h.app.tab.sort.key,
        SortKey::Modified,
        "the untouched tab follows the new configuration"
    );
    assert!(!h.app.tab.sort_chosen);
    h.press("alt+2");
    h.wait("tab 2", |a| a.tabs.active == 1 && !a.tab.is_loading());
    assert_eq!(
        (h.app.tab.sort.key, h.app.tab.sort.reverse),
        (SortKey::Name, true),
        "the chosen one is kept"
    );
    assert!(h.app.tab.sort_chosen);
}

#[test]
fn a_new_tab_has_the_order_of_the_one_it_came_from_and_the_same_choice() {
    let (sb, dir) = dated();
    let mut h = H::with_config(sb, dir, 120, 30, |c| {
        c.sort = config_sort(SortKey::Name, false)
    });
    h.press("ctrl+t");
    h.wait("tab 2", |a| a.tabs.count() == 2 && !a.tab.is_loading());
    assert!(!h.app.tab.sort_chosen, "nothing was chosen in the first");
    h.app.dispatch(Action::SortReverse);
    h.press("ctrl+t");
    h.wait("tab 3", |a| a.tabs.count() == 3 && !a.tab.is_loading());
    assert!(h.app.tab.sort.reverse && h.app.tab.sort_chosen);
}

#[test]
fn changing_the_key_keeps_the_arrow() {
    let (sb, dir) = dated();
    let mut h = H::with_config(sb, dir, 120, 30, |c| {
        c.sort = config_sort(SortKey::Name, false)
    });
    // Name ↑ → date ↑: the oldest first.
    h.app.dispatch(Action::Sort); // size
    h.app.dispatch(Action::Sort); // date
    assert_eq!(h.app.tab.sort.key, SortKey::Modified);
    assert!(!h.app.tab.sort.descending());
    assert_eq!(h.names()[1], "a-old.txt");
    // And the other way round.
    h.app.dispatch(Action::SortReverse);
    assert!(h.app.tab.sort.descending());
    assert_eq!(h.names()[1], "c-new.txt");
}
