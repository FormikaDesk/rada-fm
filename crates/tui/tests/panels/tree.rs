//! The tree in the NAV box: opening folders, scrolling a long one.

use crate::common::*;
use crate::harness::*;
use rada_tui::app::App;

#[test]
fn a_click_on_a_folder_of_the_tree_shows_it_in_the_middle_box() {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    go_and_wait(&mut h, &home);
    h.wait("the tree", |a| a.side_nav.tree.kids(a.home()).is_some());
    let docs = home.join("Documents");
    h.click(&rada_tui::hits::Target::Place(docs.clone()));
    h.wait("Documents", {
        let d = docs.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    h.settle();
    let s = h.screen();
    assert!(s.contains("[ ~/Documents ]"), "{s}");
    let rows = h.app.nav_rows();
    assert_eq!(
        App::nav_current(&rows, &h.app.cwd).map(|i| &rows[i].path),
        Some(&docs)
    );
    // The way back is the history's: Alt+Left returns to Home.
    h.press("alt+left");
    h.wait("home again", {
        let d = home.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
}

#[test]
fn tree_enter_shows_arrow_only_opens() {
    let mut h = tree_scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    go_and_wait(&mut h, &home);
    h.wait("the tree", |a| a.side_nav.tree.kids(a.home()).is_some());
    h.press("tab");
    let docs = home.join("Documents");
    let at = h
        .app
        .nav_rows()
        .iter()
        .position(|r| r.path == docs)
        .unwrap();
    h.app.side_nav.nav_cursor = at;
    h.press("enter");
    h.wait("Documents", {
        let d = docs.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    // A click on the arrow of a folder opens its branch and goes nowhere.
    let projects = home.join("projects");
    go_and_wait(&mut h, &home);
    let was_open = h.app.side_nav.tree.is_open(&projects);
    h.click(&rada_tui::hits::Target::TreeToggle(projects.clone()));
    assert_ne!(h.app.side_nav.tree.is_open(&projects), was_open);
    assert_eq!(h.app.cwd, home);
}

/// The demo home with sixty more folders in it, so that the tree is taller than its box, and a
/// chain of folders far down the list.
fn long_tree_scene() -> (H, std::path::PathBuf) {
    let mut h = tree_scene(160, 40, "rada", false);
    let home = h.app.home().to_path_buf();
    for n in 0..60 {
        std::fs::create_dir_all(home.join(format!("folder-{n:02}"))).unwrap();
    }
    let deep = home.join("folder-57").join("one").join("two");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::create_dir_all(home.join("folder-02/inner")).unwrap();
    go_and_wait(&mut h, &home);
    h.wait("the new folders in the tree", |a| {
        a.side_nav
            .tree
            .kids(a.home())
            .is_some_and(|k| k.iter().any(|k| k.name == "folder-59"))
    });
    h.settle();
    (h, deep)
}

#[test]
fn long_tree_scrolls_to_a_deep_folder() {
    let (mut h, deep) = long_tree_scene();
    h.wait("the tree", |a| a.side_nav.tree.kids(a.home()).is_some());
    assert!(
        !nav_text(&mut h).contains("folder-57"),
        "off the box at first"
    );
    h.app.open_dir(deep.clone());
    h.wait("the folder", {
        let d = deep.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    // Every folder on the way is listed, whatever the order the answers come in.
    h.wait("the way down", {
        let d = deep.clone();
        move |a| {
            d.ancestors()
                .skip(1)
                .take_while(|p| p.starts_with(a.home()))
                .all(|p| a.side_nav.tree.kids(p).is_some())
        }
    });
    h.settle();
    let rows = h.app.nav_rows();
    let at = App::nav_current(&rows, &deep).expect("the folder is a row of the tree");
    assert!(h.app.side_nav.tree.is_open(deep.parent().unwrap()));
    let a = h.app.frame.nav_area;
    assert!(
        at >= h.app.frame.nav_scroll && at < h.app.frame.nav_scroll + a.height as usize,
        "row {at} is outside {}..{}",
        h.app.frame.nav_scroll,
        h.app.frame.nav_scroll + a.height as usize
    );
    let text = nav_text(&mut h);
    assert!(text.contains("two"), "{text}");
    assert!(
        text.contains('┃'),
        "a thumb shows that there is more:\n{text}"
    );
}

#[test]
fn the_wheel_and_the_arrows_scroll_a_long_tree() {
    let (mut h, _) = long_tree_scene();
    h.wait("the tree", |a| a.side_nav.tree.kids(a.home()).is_some());
    h.settle();
    let a = h.app.frame.nav_area;
    let (x, y) = (a.x + 2, a.y + 2);
    assert_eq!(h.app.frame.nav_scroll, 0);
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::ScrollDown,
        x,
        y,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.settle();
    assert_eq!(h.app.frame.nav_scroll, 3, "three rows to a turn");
    let t = nav_text(&mut h);
    assert!(!t.lines().next().unwrap().contains("Home"), "{t}");
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::ScrollUp,
        x,
        y,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.settle();
    assert_eq!(h.app.frame.nav_scroll, 0);
    // The wheel does not move the folder shown.
    assert_eq!(h.app.cwd, h.app.home());
    // Down from the keyboard in NAV follows the cursor past the bottom of the box.
    h.press("tab");
    let height = a.height as usize;
    for _ in 0..height + 5 {
        h.press("down");
    }
    let _ = h.screen();
    assert!(h.app.side_nav.nav_cursor >= height);
    assert!(
        h.app.side_nav.nav_cursor >= h.app.frame.nav_scroll
            && h.app.side_nav.nav_cursor < h.app.frame.nav_scroll + height,
        "cursor {} scroll {}",
        h.app.side_nav.nav_cursor,
        h.app.frame.nav_scroll
    );
}

#[test]
fn a_click_on_a_folder_far_down_a_long_tree_shows_it() {
    let (mut h, _) = long_tree_scene();
    h.wait("the tree", |a| a.side_nav.tree.kids(a.home()).is_some());
    let a = h.app.frame.nav_area;
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::ScrollDown,
        a.x + 2,
        a.y + 2,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.settle();
    // The first folder-NN that is on the box, by the screen: click on its name.
    let text = nav_text(&mut h);
    let (dy, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("folder-"))
        .unwrap();
    let name: String = line
        .split_whitespace()
        .find(|w| w.starts_with("folder-"))
        .unwrap()
        .into();
    let dx = line.find(&name).unwrap();
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::Down(
            ratatui::crossterm::event::MouseButton::Left,
        ),
        a.x + dx as u16,
        a.y + dy as u16,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    let want = h.app.home().join(&name);
    h.wait("the folder", move |a| a.cwd == want && !a.is_loading());
}

#[test]
fn nav_click_opens_folder() {
    let mut h = tree_scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let dl = home.join("Scaricati");
    std::fs::create_dir_all(&dl).unwrap();
    std::fs::write(dl.join("fattura.pdf"), "x").unwrap();
    go_and_wait(&mut h, &home);
    h.wait("Scaricati in the tree", |a| {
        a.side_nav
            .tree
            .kids(a.home())
            .is_some_and(|k| k.iter().any(|k| k.name == "Scaricati"))
    });
    // A click on the name, found on the screen and not by what the hit map says.
    let text = nav_text(&mut h);
    let (dy, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("Scaricati"))
        .expect("the row is in NAV");
    let a = h.app.frame.nav_area;
    h.mouse(
        ratatui::crossterm::event::MouseEventKind::Down(
            ratatui::crossterm::event::MouseButton::Left,
        ),
        a.x + line.find("Scaricati").unwrap() as u16 + 2,
        a.y + dy as u16,
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    h.wait("Scaricati", {
        let d = dl.clone();
        move |a| a.cwd == d && !a.is_loading()
    });
    assert!(
        h.screen().contains("fattura.pdf"),
        "its files are in the middle box"
    );
    let rows = h.app.nav_rows();
    assert_eq!(
        App::nav_current(&rows, &dl).map(|i| &rows[i].path),
        Some(&dl)
    );
    // The same with Enter, from NAV.
    go_and_wait(&mut h, &home);
    h.press("tab");
    h.app.side_nav.nav_cursor = h.app.nav_rows().iter().position(|r| r.path == dl).unwrap();
    h.press("enter");
    h.wait("Scaricati again", move |a| a.cwd == dl && !a.is_loading());
    assert!(h.screen().contains("fattura.pdf"));
}
