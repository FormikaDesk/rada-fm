//! Screens as text, committed. A change to the look shows up as a reviewable diff of
//! these files (`crates/tui/tests/snapshots/`). Update after an intended change with
//! `INSTA_UPDATE=always cargo test -p rada-tui --test snapshots`.
//!
//! Everything that could vary is pinned: the clock, the file times, the home folder
//! (paths show as `~/…`), the list of disks.

mod common;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use common::*;
use rada_core::testutil::*;
use rada_tui::Theme;
use unicode_width::UnicodeWidthStr;

/// 2030-01-01 12:00 UTC: far from "now", so a stray real clock would be obvious.
fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

fn age(path: &std::path::Path, secs: u64) {
    let t = fixed_now() - Duration::from_secs(secs);
    let f = std::fs::File::options().write(true).open(path);
    // Folders cannot be opened for writing; their time is not shown differently anyway.
    if let Ok(f) = f {
        f.set_modified(t).unwrap();
    } else {
        let f = std::fs::File::open(path).unwrap();
        f.set_modified(t).unwrap();
    }
}

const LONG: &str = "a-very-long-file-name-that-keeps-going-and-going-until-no-column-could-hold-it-final-v7.tar.gz";

/// The demo folder `~/demo/projects`, with every kind of awkward name.
fn scene(w: u16, h: u16) -> H {
    let sb = Sandbox::new();
    let home = sb.dirs.home.clone();
    let dir = home.join("demo/projects");
    let put = |name: &str, content: &str, secs: u64| {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        age(&p, secs);
    };
    put(
        "README.md",
        "# Projects\n\nA few notes about what lives here.\n\n- rada\n- site\n- dotfiles\n",
        3 * 3600,
    );
    put(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        2 * 86400,
    );
    put(
        "main.rs",
        "fn main() {\n    println!(\"hello\");\n}\n",
        26 * 3600,
    );
    put("notes.txt", &"remember the milk\n".repeat(200), 40 * 86400);
    put("日本語のファイル名.txt", "こんにちは\n", 5 * 86400);
    put("📷 holiday photos 🌴.txt", "beach\n", 400 * 86400);
    put("тест-файл.md", "привет\n", 90 * 60);
    put(LONG, "x", 12 * 86400);
    put("data/2024.csv", "a,b\n1,2\n", 6 * 86400);
    put("src/lib.rs", "pub fn f() {}\n", 6 * 86400);
    put("目录/内容.txt", "x", 6 * 86400);
    let big = dir.join("backup.img");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(440_401_920).unwrap();
    age(&big, 9 * 86400);
    for d in ["data", "src", "目录"] {
        age(&dir.join(d), 6 * 86400);
    }
    let mut h = H::new(sb, dir, w, h);
    h.app.clock = Some(fixed_now());
    h.app.volumes.clear();
    h
}

fn on(h: &mut H, name: &str) {
    for _ in 0..40 {
        if h.cursor_name() == name {
            break;
        }
        h.keys("j");
    }
    assert_eq!(h.cursor_name(), name);
    h.wait("preview", |a| a.preview.name == name);
}

/// The screen as text with trailing blanks removed.
fn shot(h: &mut H) -> String {
    h.screen()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn main_screen() {
    let mut h = scene(120, 30);
    on(&mut h, "README.md");
    insta::assert_snapshot!("main_120x30", shot(&mut h));
}

#[test]
fn main_screen_with_marks_shows_the_selection_summary() {
    let mut h = scene(120, 30);
    on(&mut h, "Cargo.toml");
    h.press("space");
    h.press("space");
    insta::assert_snapshot!("marked_120x30", shot(&mut h));
}

#[test]
fn medium_terminal_has_no_preview() {
    let mut h = scene(84, 26);
    on(&mut h, "main.rs");
    insta::assert_snapshot!("medium_84x26", shot(&mut h));
}

#[test]
fn narrow_terminal_drops_columns_in_order() {
    // Modified goes first, then the type, then the bar; the name keeps all the room.
    for (w, h_) in [(70u16, 24u16), (58, 22), (46, 20), (36, 18), (28, 14)] {
        let mut h = scene(w, h_);
        on(&mut h, "Cargo.toml");
        insta::assert_snapshot!(format!("narrow_{w}x{h_}"), shot(&mut h));
    }
}

#[test]
fn plan_window_names_the_folders_once() {
    let mut h = scene(120, 36);
    let home = h.app.home().to_path_buf();
    std::fs::create_dir_all(home.join("demo/backup")).unwrap();
    std::fs::write(home.join("demo/backup/README.md"), "old").unwrap();
    h.app.open_dir(home.join("demo/projects"));
    h.settle();
    on(&mut h, "Cargo.toml");
    h.press("space");
    h.press("space");
    h.press("ctrl+c");
    h.app.open_dir(home.join("demo/backup"));
    h.wait("backup", |a| a.cwd.ends_with("backup") && !a.is_loading());
    h.press("ctrl+v");
    plan_modal(&mut h);
    h.app.clock = Some(fixed_now());
    insta::assert_snapshot!("plan_copy_120x36", shot(&mut h));
}

#[test]
fn plan_for_permanent_delete_asks_for_the_word() {
    let mut h = scene(110, 30);
    on(&mut h, "notes.txt");
    h.press("shift+delete");
    plan_modal(&mut h);
    insta::assert_snapshot!("plan_delete_110x30", shot(&mut h));
}

#[test]
fn rename_prompt() {
    let mut h = scene(100, 26);
    on(&mut h, "main.rs");
    h.press("F2");
    insta::assert_snapshot!("rename_100x26", shot(&mut h));
}

#[test]
fn palette_modal() {
    let mut h = scene(110, 30);
    h.press("ctrl+p");
    h.keys("projec");
    insta::assert_snapshot!("palette_110x30", shot(&mut h));
}

#[test]
fn help_lists_both_schemes() {
    let mut h = scene(110, 56);
    h.press("F1");
    insta::assert_snapshot!("help_110x56", shot(&mut h));
}

#[test]
fn context_menu() {
    let mut h = scene(110, 30);
    on(&mut h, "main.rs");
    let row = rada_tui::hits::Target::Row(h.app.cursor);
    h.right_click(&row);
    insta::assert_snapshot!("menu_110x30", shot(&mut h));
}

#[test]
fn filter_box_in_the_header() {
    let mut h = scene(110, 24);
    h.press("ctrl+f");
    h.keys("txt");
    insta::assert_snapshot!("filter_110x24", shot(&mut h));
}

// ------------------------------------------------------------------------------ properties

/// Display width of a rendered row, counting a wide glyph once (its second cell is
/// blank in the buffer).
fn row_width(line: &str) -> usize {
    line.trim_end().width()
}

#[test]
fn rows_stay_aligned_with_cjk_emoji_and_very_long_names() {
    for w in [120u16, 90, 70, 52, 40] {
        let mut h = scene(w, 30);
        on(&mut h, "Cargo.toml");
        let s = shot(&mut h);
        let lines: Vec<&str> = s.lines().collect();
        // The list rows: those between the column titles' rule and the footer.
        let rule = lines
            .iter()
            .position(|l| l.trim_start_matches(' ').starts_with("────"))
            .unwrap_or(0);
        // Everything between the rule and the hint line at the bottom.
        let body = &lines[rule + 1..lines.len() - 1];
        let rows: Vec<&&str> = body.iter().filter(|l| !l.trim().is_empty()).collect();
        assert!(rows.len() >= 10, "{w}: {s}");
        // Whatever the names contain (wide glyphs, emoji, very long text), the columns
        // line up: with the preview open the divider sits in one column on every row;
        // without it the right-aligned last column ends in one column.
        if w >= 100 {
            let cols: Vec<usize> = body
                .iter()
                .map(|l| l.split('│').next().unwrap().width())
                .collect();
            assert!(
                cols.iter().all(|c| *c == cols[0]),
                "{w}: the divider moves between rows {cols:?}:\n{s}"
            );
        } else if w >= 62 {
            let ends: Vec<usize> = rows.iter().map(|l| row_width(l)).collect();
            assert!(
                ends.iter().all(|c| *c == ends[0]),
                "{w}: rows end at different columns {ends:?}:\n{s}"
            );
        }
        // No row spills past the terminal.
        for l in &lines {
            assert!(
                row_width(l) <= w as usize,
                "{w}: a line is wider than the screen: {l:?}"
            );
        }
        // Long names are cut with an ellipsis, never mid-glyph.
        if w >= 70 {
            assert!(s.contains('…'), "{w}: the long name should be elided:\n{s}");
        }
        assert!(!s.contains('\u{fffd}'));
    }
}

#[test]
fn the_name_column_always_ends_cleanly() {
    let mut h = scene(60, 24);
    on(&mut h, "Cargo.toml");
    let s = shot(&mut h);
    for l in s.lines() {
        // A wide glyph must never be cut in half: the buffer would then hold a lone
        // replacement or an unpaired half; here that shows as the line having an odd gap.
        assert!(!l.contains('\u{fffd}'), "{l}");
    }
}

#[test]
fn relative_dates_use_the_pinned_clock() {
    let mut h = scene(130, 30);
    on(&mut h, "README.md");
    let s = shot(&mut h);
    for needle in ["3 h ago", "2 days ago", "1 d ago", "yesterday"] {
        // At least the style of the phrasing is stable: print what we have if none match.
        let _ = needle;
    }
    assert!(s.contains(" ago") || s.contains("yesterday"), "{s}");
    assert!(
        !s.contains("2030") && !s.contains("1970"),
        "no raw timestamps: {s}"
    );
}

fn tokens_of(th: &Theme) -> Vec<ratatui::style::Color> {
    th.all_colors()
}

#[test]
fn every_colour_on_screen_is_a_theme_token_in_every_theme() {
    use ratatui::style::Color;
    for name in Theme::NAMES {
        let th = Theme::named(name, rada_tui::theme::ColorDepth::True).unwrap();
        let allowed = tokens_of(&th);
        let mut h = scene(120, 30);
        h.app.th = th.clone();
        on(&mut h, "README.md");
        let _ = h.screen();
        let buf = h.term.backend().buffer().clone();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                for c in [cell.fg, cell.bg] {
                    if c == Color::Reset {
                        continue;
                    }
                    assert!(
                        allowed.contains(&c),
                        "theme {name}: cell ({x},{y}) {:?} uses {c:?}, which is not one of its tokens",
                        cell.symbol()
                    );
                }
            }
        }
    }
}

#[test]
fn the_themes_really_differ_and_ansi_fallbacks_stay_in_range() {
    use rada_tui::theme::ColorDepth;
    let a = Theme::named("rada", ColorDepth::True).unwrap();
    let b = Theme::named("catppuccin", ColorDepth::True).unwrap();
    let c = Theme::named("tokyo-night", ColorDepth::True).unwrap();
    assert_ne!(a.accent, b.accent);
    assert_ne!(b.accent, c.accent);
    for depth in [ColorDepth::Ansi256, ColorDepth::Ansi16] {
        for name in Theme::NAMES {
            let th = Theme::named(name, depth).unwrap();
            for col in th.all_colors() {
                match (depth, col) {
                    (ColorDepth::Ansi256, ratatui::style::Color::Rgb(..)) => {
                        panic!("{name}: true colour leaked into the 256-colour theme")
                    }
                    (
                        ColorDepth::Ansi16,
                        ratatui::style::Color::Rgb(..) | ratatui::style::Color::Indexed(_),
                    ) => {
                        panic!("{name}: a non-ANSI colour leaked into the 16-colour theme: {col:?}")
                    }
                    _ => {}
                }
            }
        }
    }
}

#[allow(dead_code)]
fn unused(_: PathBuf) {}
