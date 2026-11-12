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

// ---------------------------------------------------------------------------------------------
// The whole search: other ways of asking, the result as it arrives, what it says at the end
// ---------------------------------------------------------------------------------------------

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use rada_core::testutil::{FaultFs, Op, set_mtime};
#[cfg(unix)]
use rada_core::testutil::{chmod, is_root};
use rada_core::uistate::SavedTab;
#[cfg(unix)]
use rada_tui::hits::{NoticeHit, Target};

fn search_view(a: &rada_tui::app::App) -> bool {
    matches!(a.tab.virtual_view(), Some(ViewKind::Search { .. })) && !a.tab.is_loading()
}

#[test]
fn ctrl_enter_searches_below_even_when_something_here_matches() {
    let mut h = scene();
    h.press("ctrl+f");
    h.keys("alpha");
    h.press("ctrl+enter");
    h.wait("the result", search_view);
    assert_eq!(h.names(), ["alpha.txt"]);
    assert!(h.app.windows.address.is_none());
}

#[test]
fn a_pattern_and_a_regular_expression_are_other_ways_of_asking() {
    let mut h = scene();
    h.press("ctrl+f");
    h.keys("*.md");
    h.press("ctrl+enter");
    h.wait("the result", search_view);
    let mut names = h.names();
    names.sort();
    assert_eq!(names, ["Needle2.md", "hay.md"]);

    h.press("alt+up");
    h.wait("the folder", |a| {
        a.tab.virtual_view().is_none() && !a.tab.is_loading()
    });
    h.press("ctrl+f");
    h.keys("/^need.*md$/");
    h.press("ctrl+enter");
    h.wait("the result", search_view);
    assert_eq!(h.names(), ["Needle2.md"]);
}

#[test]
fn a_regular_expression_that_cannot_be_read_is_said_and_nothing_is_listed() {
    let mut h = scene();
    let folder = h.app.tab.cwd.clone();
    h.press("ctrl+f");
    h.keys("/(unclosed/");
    h.press("ctrl+enter");
    h.wait("an answer", |a| !a.tab.is_loading());
    assert_eq!(h.app.tab.cwd, folder);
}

#[test]
fn the_result_says_where_each_item_is() {
    let mut h = scene();
    search_for(&mut h, "needle");
    let s = h.screen();
    assert!(s.contains("Where"), "{s}");
    let deep = Path::new("sub").join("deep").display().to_string();
    assert!(s.contains(&deep) && s.contains("other"), "{s}");
}

#[test]
fn f5_searches_again_and_what_is_new_is_found() {
    let mut h = scene();
    search_for(&mut h, "needle");
    let root = h.app.tab.cwd.clone();
    let Some(ViewKind::Search { root: folder, .. }) = ViewKind::of(&root) else {
        panic!("not a search")
    };
    std::fs::write(folder.join("sub").join("needle-new.txt"), "").unwrap();
    h.press("f5");
    h.wait("the new file", |a| {
        search_view(a)
            && a.tab
                .listing
                .all()
                .iter()
                .any(|e| e.display == "needle-new.txt")
    });
    assert_eq!(h.names().len(), 3);
}

#[test]
fn showing_hidden_files_searches_again_inside_hidden_folders() {
    let sb = Sandbox::new();
    sb.write("w/seen/needle.txt", "");
    sb.write("w/.hid/needle-hidden.txt", "");
    sb.hide("w/.hid");
    let dir = sb.path("w");
    let mut h = H::new(sb, dir, 140, 36);
    search_for(&mut h, "needle");
    assert_eq!(h.names(), ["needle.txt"]);

    h.app.dispatch(Action::ToggleHidden);

    h.wait("the hidden one", |a| {
        search_view(a) && a.tab.listing.all().len() == 2
    });
}

#[test]
fn an_item_that_an_operation_took_away_leaves_the_result() {
    let mut h = scene();
    search_for(&mut h, "needle");
    h.cursor_to("needle.txt");
    h.app.dispatch(Action::Trash);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    h.wait("the list without it", |a| {
        search_view(a)
            && a.tab
                .listing
                .all()
                .iter()
                .all(|e| e.display != "needle.txt")
    });
    assert_eq!(h.names(), ["Needle2.md"]);
}

#[test]
fn a_search_is_a_tab_that_comes_back() {
    let mut h = scene();
    search_for(&mut h, "needle");
    let saved = h.app.saved_tabs();
    assert_eq!(saved[0].path, h.app.tab.cwd, "the search, not a folder");

    // The next start, with that tab remembered.
    let sb = Sandbox::new();
    sb.write("w/sub/needle.txt", "");
    let dir = sb.path("w");
    let path = rada_core::search::view_path(&dir, "needle").unwrap();
    let tab = SavedTab {
        path,
        ..saved[0].clone()
    };
    let mut again = H::with_config(sb, dir, 140, 36, move |cfg| {
        cfg.remember_tabs = true;
        cfg.saved_ui.tabs = vec![tab];
    });
    again.wait("the search made again", search_view);
    assert_eq!(again.names(), ["needle.txt"]);
}

#[cfg(unix)]
#[test]
fn folders_that_cannot_be_read_are_counted_and_show_lists_them() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.write("w/open/needle.txt", "");
    sb.write("w/shut/needle.txt", "");
    let shut = sb.path("w/shut");
    chmod(&shut, 0o000);
    let dir = sb.path("w");
    let mut h = H::new(sb, dir, 140, 36);

    search_for(&mut h, "needle");
    h.wait("the notice", |a| {
        a.windows
            .notices
            .shown(Instant::now())
            .iter()
            .any(|(n, _)| !n.lines.is_empty())
    });
    chmod(&shut, 0o755);

    assert_eq!(h.names(), ["needle.txt"]);
    let s = h.screen();
    assert!(s.contains("1 folder could not be read"), "{s}");
    let shown = h.app.windows.notices.shown(Instant::now());
    let id = shown
        .iter()
        .find(|(n, _)| !n.lines.is_empty())
        .unwrap()
        .0
        .id;
    h.click(&Target::Notice(id, NoticeHit::Show));
    let s = h.screen();
    assert!(
        matches!(h.app.windows.modal, Some(rada_tui::app::Modal::Result(_))),
        "{s}"
    );
    assert!(s.contains("shut"), "{s}");
}

/// A folder whose search waits in `zz-slow` until the gate opens: 250 matches in the folder
/// itself, which arrive first, and 300 more in the slow one.
fn slow_scene() -> (H, Arc<AtomicBool>) {
    let sb = Sandbox::new();
    for i in 0..250 {
        sb.write(format!("w/hit-{i:03}.txt"), "");
    }
    for i in 0..300 {
        sb.write(format!("w/zz-slow/hit-late-{i:03}.txt"), "");
    }
    let gate = Arc::new(AtomicBool::new(false));
    let ffs = FaultFs::new();
    // Whoever reads the slow folder waits: the search, and any pane that looks into it.
    for nth in 1..=30 {
        let open = gate.clone();
        ffs.hook(Op::ReadDir, "zz-slow", nth, move || {
            while !open.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
    }
    let dir = sb.path("w");
    let system: System = (ffs, sb.platform());
    let mut h = H::with_system(sb, dir, 140, 36, system, |_| {});
    h.press("ctrl+f");
    h.keys("hit");
    h.press("ctrl+enter");
    h.wait("the first matches", |a| {
        a.tab.view_run.is_some_and(|r| r.running) && a.tab.listing.all().len() >= 200
    });
    (h, gate)
}

#[test]
fn matches_are_listed_as_they_arrive_and_put_in_order_once_at_the_end() {
    let (mut h, gate) = slow_scene();
    assert!(h.app.tab.is_loading(), "more may come");
    let s = h.screen();
    assert!(s.contains("Searching…") && s.contains("Esc to stop"), "{s}");
    for _ in 0..5 {
        h.key(KeyCode::Down);
    }
    let under_cursor = h.cursor_name();

    gate.store(true, Ordering::SeqCst);
    h.wait("the whole result", search_view);

    assert_eq!(
        h.cursor_name(),
        under_cursor,
        "the cursor stayed on its item"
    );
    let names = h.names();
    assert_eq!(names.len(), 550, "250 here and 300 below");
    let mut sorted = names.clone();
    sorted.sort_by_key(|n| n.to_lowercase());
    assert_eq!(names, sorted, "in order, by name");
    assert!(h.screen().contains("Search: hit"));
}

#[test]
fn esc_stops_a_search_and_keeps_what_it_found() {
    let (mut h, gate) = slow_scene();

    h.key(KeyCode::Esc);
    gate.store(true, Ordering::SeqCst);
    h.wait("the search to stop", search_view);

    let n = h.app.tab.listing.all().len();
    assert!((200..550).contains(&n), "{n} kept");
    assert!(h.app.tab.view_run.is_some_and(|r| !r.running));
    let s = h.screen();
    let said = h.app.windows.notices.shown(Instant::now());
    assert!(
        said.iter().any(|(n, _)| n.text.contains("search stopped")),
        "{s}"
    );
}

// ---------------------------------------------------------------------------------------------
// Pictures
// ---------------------------------------------------------------------------------------------

/// The clock of the pictures: the dates on the screen do not change with the day.
fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// The result of a search under the home of the sandbox, at `w`x`h`, with fixed dates.
fn picture(w: u16, h: u16) -> String {
    let sb = Sandbox::new();
    let dir = sb.dirs.home.join("demo/work");
    let put = |rel: &str, content: &str| {
        let p: PathBuf = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        set_mtime(&p, fixed_now() - Duration::from_secs(3 * 3600));
    };
    put("report.pdf", "top");
    put("2025/q4/report-final.pdf", "a longer one");
    put("2026/drafts/old/Report draft.txt", "draft");
    put("notes.txt", "n");
    let mut h = H::with_config(sb, dir, w, h, |cfg| cfg.details_pane = Some(false));
    h.app.hooks.clock = Some(fixed_now());
    // The times are written for one zone, whatever the machine's is.
    h.app.settings.tz = jiff::tz::TimeZone::UTC;
    // A file here has the word too: the search below is asked for with Ctrl+Enter.
    h.press("ctrl+f");
    h.keys("report");
    h.press("ctrl+enter");
    h.wait("the result", search_view);
    // The places of the navigation pane come from a worker: the picture waits for them.
    h.wait("the places", |a| !a.places.paths.places.is_empty());
    let screen = h.screen();
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    lines.join("\n") + "\n"
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAGS section, which the Linux snapshot has"
)]
fn the_result_of_a_search_at_100x30() {
    insta::assert_snapshot!("search_where_100x30", picture(100, 30));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "paths are written with the other separator there, which the Linux snapshot does not have"
)]
fn the_result_of_a_search_at_60x20() {
    insta::assert_snapshot!("search_where_60x20", picture(60, 20));
}
