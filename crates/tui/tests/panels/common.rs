//! What several tests of the panels share: the demo scenes and the checks on pills.

use crate::harness::*;
use rada_tui::panels::NavMode;
use rada_tui::theme::ColorDepth;
use rada_tui::{IconSet, LayoutPref, Theme};
use ratatui::style::Color;

/// The themes the panels are drawn in for the snapshots.
pub const THEMES: [&str; 3] = ["neon", "rada", "rada-light"];

pub fn put_cursor(h: &mut H, name: &str) {
    h.keys("g");
    for _ in 0..40 {
        if h.cursor_name() == name {
            break;
        }
        h.keys("j");
    }
    assert_eq!(h.cursor_name(), name);
    h.wait("preview", |a| a.preview.name == name);
}

/// The demo home with the state of the main screen: one item selected, two waiting on the
/// clipboard, the cursor on the picture and the card of the last copy. `nerd` draws with the
/// glyphs of a Nerd Font (the pills have round ends); the text snapshots do without, so that
/// they can be read.
pub fn scene(w: u16, hh: u16, theme: &str, nerd: bool) -> H {
    let mut h = demo_scene(w, hh);
    h.app.layout_pref = LayoutPref::Auto;
    h.app.th = Theme::named(theme, ColorDepth::True).unwrap();
    if nerd {
        h.app.icons = IconSet::Nerd;
    }
    put_cursor(&mut h, "backup.zip");
    h.press("space");
    put_cursor(&mut h, "photo.jpg");
    let paths: Vec<_> = ["data.json", "main.rs"]
        .iter()
        .filter_map(|n| h.app.listing.all().iter().find(|e| e.display == *n))
        .map(|e| e.path.clone())
        .collect();
    h.app.clipboard = Some(rada_tui::app::Clip {
        mode: rada_core::ops::TransferMode::Copy,
        paths,
    });
    let docs = h.app.home().join("Documents");
    // It stays for an hour, so that the bar along its bottom edge is whole whenever the test runs.
    h.app.notify(
        rada_tui::app::Notice::new(rada_tui::app::NoticeKind::Ok, "3 items copied to Documents")
            .with_action(rada_tui::app::NoticeAction::Undo)
            .lasting(std::time::Duration::from_secs(3600))
            .showing(docs, None),
    );
    h.settle();
    h
}

/// The same with `nav = "albero"`: the NAV box lists the whole tree of Home.
pub fn tree_scene(w: u16, hh: u16, theme: &str, nerd: bool) -> H {
    let mut h = scene(w, hh, theme, nerd);
    h.app.nav_mode = NavMode::Tree;
    h.app.tree = Default::default();
    h.wait("the way down in the tree", |a| {
        a.cwd
            .ancestors()
            .skip(1)
            .take_while(|p| p.starts_with(a.home()))
            .all(|p| a.tree.kids(p).is_some())
    });
    h.settle();
    h
}

pub const CAP_LEFT: &str = "\u{e0b6}";

pub const CAP_RIGHT: &str = "\u{e0b4}";

/// Every pill on the screen (a button, a label, a key, the selected row, a tab) has both its
/// round ends, on the same row as its text, and one background colour from end to end: the
/// ends are in that colour on whatever is behind the pill. Returns how many pills there were.
pub fn assert_pills(buf: &ratatui::buffer::Buffer, what: &str, behind: &[Color]) -> usize {
    let mut pills = 0;
    for y in 0..buf.area.height {
        // The colours the pills around are filled with, innermost last.
        let mut open: Vec<(Color, u16)> = Vec::new();
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            match cell.symbol() {
                CAP_LEFT => {
                    // The end is the pill's colour; behind it is what the pill stands on.
                    match open.last() {
                        Some((outer, _)) => assert_eq!(
                            cell.bg, *outer,
                            "{what}: the left end at {x},{y} stands on the pill around it"
                        ),
                        None => assert!(
                            behind.contains(&cell.bg),
                            "{what}: the left end at {x},{y} stands on {:?}, which is not what is behind it",
                            cell.bg
                        ),
                    }
                    open.push((cell.fg, x));
                }
                CAP_RIGHT => {
                    let (fill, at) = open.pop().unwrap_or_else(|| {
                        panic!("{what}: a right end at {x},{y} with no left end before it")
                    });
                    assert_eq!(
                        cell.fg, fill,
                        "{what}: the pill from {at} to {x} on row {y} has a right end of another colour"
                    );
                    match open.last() {
                        Some((outer, _)) => assert_eq!(
                            cell.bg, *outer,
                            "{what}: the right end at {x},{y} stands on the pill around it"
                        ),
                        None => assert!(
                            behind.contains(&cell.bg),
                            "{what}: the right end at {x},{y} stands on {:?}, which is not what is behind it",
                            cell.bg
                        ),
                    }
                    // Something between the ends: a pill with no text is not one.
                    assert!(x > at + 1, "{what}: an empty pill at {at},{y}");
                    pills += 1;
                }
                _ => {
                    if let Some((fill, at)) = open.last() {
                        // Wide glyphs leave a cell after them that the terminal skips.
                        if cell.symbol().is_empty() {
                            continue;
                        }
                        assert_eq!(
                            cell.bg,
                            *fill,
                            "{what}: {:?} at {x},{y} is inside the pill that starts at {at} and is not its colour",
                            cell.symbol()
                        );
                    }
                }
            }
        }
        assert!(
            open.is_empty(),
            "{what}: a pill on row {y} starting at {} has no right end",
            open[0].1
        );
    }
    pills
}

/// Open `dir` and wait until the folder is shown and the tree has been told about it.
pub fn go_and_wait(h: &mut H, dir: &std::path::Path) {
    h.app.open_dir(dir.to_path_buf());
    let d = dir.to_path_buf();
    h.wait("the folder", move |a| a.cwd == d && !a.is_loading());
    h.settle();
}

pub fn nav_text(h: &mut H) -> String {
    let a = h.app.nav_area;
    let s = h.screen();
    s.lines()
        .skip(a.y as usize)
        .take(a.height as usize)
        .map(|l| {
            l.chars()
                .skip(a.x as usize)
                .take(a.width as usize)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The demo home with a chain of folders `a/b/c` that has a sibling at every step, and two
/// folders inside `c` besides `d`; the folder shown is `c`, and the NAV box has answered.
pub fn path_scene() -> (H, std::path::PathBuf) {
    let mut h = scene(160, 46, "rada", false);
    let home = h.app.home().to_path_buf();
    let c = home.join("a/b/c");
    for d in [
        "a/sib-a",
        "a/b/sib-b",
        "a/b/c/d",
        "a/b/c/kid-1",
        "a/b/c/kid-2",
        "sib-home",
    ] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    go_and_wait(&mut h, &c);
    h.wait("the subfolders of c", |a| {
        a.tree.kids(&a.cwd).is_some_and(|k| k.len() == 3)
    });
    h.settle();
    (h, c)
}

/// The row of a screen that has `label` in it, and the background of the cell at `x` there.
pub fn row_bg(h: &mut H, label: &str, x: u16) -> ratatui::style::Color {
    let buf = h.redrawn_buffer();
    let y = (0..buf.area.height)
        .find(|y| {
            let line: String = (0..buf.area.width).map(|x| buf[(x, *y)].symbol()).collect();
            line.contains(label)
        })
        .unwrap_or_else(|| panic!("no row with {label}"));
    buf[(x, y)].bg
}
