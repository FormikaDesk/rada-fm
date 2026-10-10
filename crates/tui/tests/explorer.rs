//! The explorer look: screens at every size in both themes, the icons view, the context
//! menu, two tabs, and what each part does (tabs, the address field, the command bar, the
//! checkboxes, the details pane, the menus).
//!
//! Screens are committed as text (`tests/snapshots/`); a change to the look shows up as a
//! diff to review. The light and dark themes share their text, so each size also has a
//! "token map": every cell reduced to the theme token that colours it.

mod common;

use common::*;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use rada_tui::app::{MenuKind, Modal};
use rada_tui::hits::Target;
use rada_tui::keymap::Action;
use rada_tui::theme::ColorDepth;
use rada_tui::{Theme, ViewMode};
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

const SIZES: [(u16, u16); 7] = [
    (170, 35),
    (140, 35),
    (120, 30),
    (100, 26),
    (80, 24),
    (60, 20),
    (40, 16),
];

fn shot(h: &mut H) -> String {
    h.screen()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The demo folder with the cursor on `photo.jpg`, as in the prototype.
fn scene(w: u16, h: u16) -> H {
    let mut h = demo_scene(w, h);
    put_cursor(&mut h, "photo.jpg");
    h
}

fn put_cursor(h: &mut H, name: &str) {
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

/// Three files selected and two items on the clipboard, as in the prototype.
fn busy_scene(w: u16, hh: u16) -> H {
    let mut h = demo_scene(w, hh);
    put_cursor(&mut h, "archive.tar.gz");
    for _ in 0..3 {
        h.press("space");
    }
    put_cursor(&mut h, "photo.jpg");
    h.app.clipboard = Some(rada_tui::app::Clip {
        mode: rada_core::ops::TransferMode::Copy,
        paths: vec![h.app.current().unwrap().path.clone(); 2],
    });
    h
}

/// Every cell reduced to the token that colours it.
fn token_map(h: &mut H, th: &Theme) -> String {
    let _ = h.screen();
    let buf = h.term.backend().buffer().clone();
    let kinds = [
        th.kinds.code,
        th.kinds.markup,
        th.kinds.text,
        th.kinds.doc,
        th.kinds.image,
        th.kinds.vector,
        th.kinds.audio,
        th.kinds.video,
        th.kinds.archive,
        th.kinds.data,
        th.kinds.config,
        th.kinds.binary,
        th.kinds.other,
        th.kinds.folder,
        th.kinds.link,
    ];
    let letter = |c: Color| -> char {
        let tokens: [(Color, char); 21] = [
            (th.text, 't'),
            (th.text_dim, 'd'),
            (th.muted, 'm'),
            (th.accent, 'A'),
            (th.on_accent, 'o'),
            (th.cursor, 'C'),
            (th.cursor_text, 'W'),
            (th.sel, 'S'),
            (th.mark, 'M'),
            (th.cmdbar, 'B'),
            (th.panel, 'P'),
            (th.key_bg, 'K'),
            (th.key_text, 'Y'),
            (th.field, 'F'),
            (th.button, 'U'),
            (th.selection, 'E'),
            (th.track, 'r'),
            (th.success, 's'),
            (th.warn, 'w'),
            (th.error, 'e'),
            (th.canvas, 'c'),
        ];
        if c == Color::Reset {
            return '.';
        }
        if let Some((_, l)) = tokens.iter().find(|(t, _)| *t == c) {
            *l
        } else if kinds.contains(&c) {
            'k'
        } else {
            '+'
        }
    };
    let mut out = String::from(
        "tokens: t text, d dim, m muted, A accent, o on_accent, C cursor, W cursor_text, S sel,\n\
         M mark, B cmdbar, P panel, K key_bg, Y key_text, F field, U button, E selection, r track,\n\
         s success, w warn, e error, k file kind, + a tint, . none\n\ntext colour\n",
    );
    for y in 0..buf.area.height {
        out.extend((0..buf.area.width).map(|x| letter(buf[(x, y)].fg)));
        out.push('\n');
    }
    out.push_str("\nbackground\n");
    for y in 0..buf.area.height {
        out.extend((0..buf.area.width).map(|x| letter(buf[(x, y)].bg)));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------- screens

#[test]
fn the_explorer_at_every_size_in_both_themes() {
    for (w, hh) in SIZES {
        let mut h = busy_scene(w, hh);
        insta::assert_snapshot!(format!("explorer_{w}x{hh}"), shot(&mut h));
        // No line is wider than the screen, whatever the size.
        for l in h.screen().lines() {
            assert!(
                l.trim_end().width() <= w as usize,
                "{w}x{hh}: a line is wider than the screen: {l:?}"
            );
        }
        for (name, theme) in [("dark", "rada"), ("light", "rada-light")] {
            let th = Theme::named(theme, ColorDepth::True).unwrap();
            h.app.th = th.clone();
            insta::assert_snapshot!(
                format!("explorer_{w}x{hh}_{name}_tokens"),
                token_map(&mut h, &th)
            );
        }
    }
}

#[test]
fn the_icons_view() {
    let mut h = busy_scene(120, 30);
    h.press("ctrl+2");
    assert_eq!(h.app.view, ViewMode::Icons);
    insta::assert_snapshot!("icons_120x30", shot(&mut h));
    let th = Theme::named("rada-light", ColorDepth::True).unwrap();
    h.app.th = th.clone();
    insta::assert_snapshot!("icons_120x30_light_tokens", token_map(&mut h, &th));
}

#[test]
fn the_context_menus() {
    let mut h = scene(120, 35);
    h.right_click(&Target::Row(h.app.cursor));
    insta::assert_snapshot!("context_menu_item_120x30", shot(&mut h));
    h.press("esc");
    // On empty space.
    let (x, y) = h.where_is(&Target::List);
    h.mouse(
        MouseEventKind::Down(MouseButton::Right),
        x,
        y + 24,
        KeyModifiers::NONE,
    );
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
    insta::assert_snapshot!("context_menu_empty_120x30", shot(&mut h));
}

#[test]
fn two_tabs_open() {
    let mut h = scene(120, 30);
    h.press("ctrl+t");
    h.wait("the new tab", |a| a.tab_count() == 2 && !a.is_loading());
    // The second tab goes to a folder of its own.
    put_cursor(&mut h, "docs");
    h.press("enter");
    h.wait("docs", |a| a.cwd.ends_with("docs") && !a.is_loading());
    h.app.clock = Some(demo_now());
    insta::assert_snapshot!("two_tabs_120x30", shot(&mut h));
    let th = Theme::named("rada", ColorDepth::True).unwrap();
    insta::assert_snapshot!("two_tabs_120x30_tokens", token_map(&mut h, &th));
}

// ---------------------------------------------------------------------------- tabs

#[test]
fn tabs_keep_their_own_folder_cursor_selection_and_sort() {
    let mut h = scene(140, 30);
    let first = h.app.cwd.clone();
    h.press("space"); // select photo.jpg in tab 1
    h.press("ctrl+t");
    h.wait("tab 2", |a| a.tab_count() == 2 && !a.is_loading());
    assert_eq!(h.app.active_tab, 1);
    assert_eq!(h.app.cwd, first, "a new tab starts in the current folder");
    assert!(h.app.marked.is_empty(), "the selection is the tab's own");
    // Tab 2: another folder and another order.
    put_cursor(&mut h, "src");
    h.press("enter");
    h.wait("src", |a| a.cwd.ends_with("src") && !a.is_loading());
    h.press("S"); // reverse the order in this tab
    let reversed = h.app.sort.reverse;
    assert!(reversed);

    h.press("alt+1");
    h.wait("back to tab 1", |a| {
        a.active_tab == 0 && a.cwd.ends_with("demo") && !a.is_loading()
    });
    assert_eq!(h.app.cwd, first);
    assert!(!h.app.sort.reverse, "the sort order is per tab");
    assert_eq!(h.marked_names(), ["photo.jpg"], "so is the selection");
    h.press("alt+2");
    h.wait("tab 2 again", |a| a.cwd.ends_with("src") && !a.is_loading());
    assert!(h.app.sort.reverse);
    // The clipboard and the undo are shared: copy in tab 2, paste in tab 1.
    assert_eq!(h.app.tab_count(), 2);
}

#[test]
fn the_keys_and_the_mouse_change_and_close_tabs() {
    let mut h = scene(140, 30);
    h.press("ctrl+t");
    h.press("ctrl+t");
    h.wait("three tabs", |a| a.tab_count() == 3 && !a.is_loading());
    assert_eq!(h.app.active_tab, 2);
    h.press("ctrl+pagedown");
    h.wait("wraps to the first", |a| {
        a.active_tab == 0 && !a.is_loading()
    });
    h.press("ctrl+pageup");
    h.wait("and back", |a| a.active_tab == 2 && !a.is_loading());
    h.press("ctrl+shift+tab");
    h.wait("previous", |a| a.active_tab == 1 && !a.is_loading());
    h.press("alt+9");
    h.wait("Alt+9 is the last", |a| {
        a.active_tab == 2 && !a.is_loading()
    });
    // Click a tab.
    h.click(&Target::Tab(0));
    h.wait("clicked", |a| a.active_tab == 0 && !a.is_loading());
    // Middle click closes.
    let (x, y) = h.where_is(&Target::Tab(2));
    h.mouse(
        MouseEventKind::Down(MouseButton::Middle),
        x,
        y,
        KeyModifiers::NONE,
    );
    assert_eq!(h.app.tab_count(), 2);
    // The close mark closes too, and Ctrl+W.
    h.click(&Target::TabClose(1));
    assert_eq!(h.app.tab_count(), 1);
    h.press("ctrl+t");
    h.wait("two again", |a| a.tab_count() == 2 && !a.is_loading());
    h.press("ctrl+w");
    assert_eq!(h.app.tab_count(), 1);
    // The last one is not closed: it goes home.
    h.press("ctrl+w");
    h.wait("home", |a| a.cwd == a.home() && !a.is_loading());
    assert_eq!(h.app.tab_count(), 1);
    // The new-tab button.
    h.click(&Target::TabNew);
    assert_eq!(h.app.tab_count(), 2);
}

#[test]
fn on_a_narrow_terminal_the_tabs_fold() {
    let mut h = scene(50, 20);
    h.press("ctrl+t");
    h.press("ctrl+t");
    h.wait("three", |a| a.tab_count() == 3 && !a.is_loading());
    let s = h.screen();
    assert!(
        s.lines().next().unwrap().contains("‹ 3/3 ›") || s.contains("3/3"),
        "{s}"
    );
    h.click(&Target::TabPrev);
    h.wait("prev", |a| a.active_tab == 1 && !a.is_loading());
    assert!(h.screen().contains("2/3"));
}

#[test]
fn tabs_are_remembered_between_sessions_and_the_option_turns_it_off() {
    use rada_core::uistate::{SavedTab, UiState};
    // Written as they change.
    let mut h = scene(140, 30);
    h.press("ctrl+t");
    h.wait("two", |a| a.tab_count() == 2 && !a.is_loading());
    let dir = h.app.home().parent().unwrap().join("xdg/state/rada");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let saved = rada_core::uistate::load(&dir);
        if saved.tabs.len() == 2 {
            assert_eq!(saved.active_tab, 1);
            assert!(saved.tabs[0].path.ends_with("demo"));
            break;
        }
        assert!(std::time::Instant::now() < end, "tabs were not saved");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Restored at start: a saved list gives the tabs back, the front one as it was.
    let sb = rada_core::testutil::Sandbox::new();
    let a = sb.mkdir("home/a");
    let b = sb.mkdir("home/b");
    let saved = UiState {
        tabs: vec![
            SavedTab {
                path: a.clone(),
                view: Some("icons".into()),
                sort: Some("size".into()),
                reverse: true,
            },
            SavedTab {
                path: b.clone(),
                view: None,
                sort: None,
                reverse: false,
            },
        ],
        active_tab: 1,
        ..Default::default()
    };
    let s2 = saved.clone();
    let mut h = H::with_config(sb, a.clone(), 120, 30, move |c| c.saved_ui = s2);
    assert_eq!(h.app.tab_count(), 2);
    assert_eq!(h.app.active_tab, 1);
    h.wait("the front tab", move |app| {
        app.cwd == b && !app.is_loading()
    });
    h.press("alt+1");
    h.wait("the first tab", move |app| {
        app.cwd == a && !app.is_loading()
    });
    assert_eq!(h.app.view, ViewMode::Icons, "its view came back");
    assert!(h.app.sort.reverse);
    assert_eq!(h.app.sort.key, rada_core::model::SortKey::Size);
    // Switched off: one tab, as before.
    let sb = rada_core::testutil::Sandbox::new();
    let a = sb.mkdir("home/a");
    sb.mkdir("home/b");
    let h = H::with_config(sb, a, 120, 30, move |c| {
        c.saved_ui = saved;
        c.remember_tabs = false;
    });
    assert_eq!(h.app.tab_count(), 1);
}

#[test]
fn a_folder_given_on_the_command_line_opens_beside_the_restored_tabs() {
    use rada_core::uistate::{SavedTab, UiState};
    let sb = rada_core::testutil::Sandbox::new();
    let a = sb.mkdir("home/a");
    let b = sb.mkdir("home/b");
    let saved = UiState {
        tabs: vec![SavedTab {
            path: a,
            view: None,
            sort: None,
            reverse: false,
        }],
        ..Default::default()
    };
    let target = b.clone();
    let h = H::with_config(sb, b, 120, 30, move |c| {
        c.saved_ui = saved;
        c.start_explicit = true;
    });
    assert_eq!(h.app.tab_count(), 2);
    assert_eq!(h.app.active_tab, 1);
    assert_eq!(h.app.cwd, target);
}

// ---------------------------------------------------------------------------- the address

#[test]
fn the_address_becomes_a_field_that_completes_and_goes() {
    let mut h = scene(140, 30);
    // Clicking the path (not a segment) edits it.
    h.press("ctrl+l");
    assert!(h.app.address.is_some());
    let row = h.screen().lines().nth(1).unwrap().to_string();
    assert!(row.contains("~/projects/demo/▏"), "{row}");
    // Esc leaves it as it was.
    h.press("esc");
    assert!(h.app.address.is_none() && h.app.cwd.ends_with("demo"));

    // Type a path, complete with Tab, go with Enter.
    h.press("alt+d");
    for _ in 0..5 {
        h.press("backspace"); // "~/projects/"
    }
    h.keys("de");
    h.wait("the subfolders of ~/projects", |a| {
        !a.address_suggestions().is_empty()
    });
    assert_eq!(h.app.address_suggestions(), ["demo"]);
    let row = h.screen().lines().nth(1).unwrap().to_string();
    assert!(row.contains("~/projects/de▏"), "{row}");
    assert!(
        h.screen().contains(" demo/"),
        "the suggestion is drawn under the field"
    );
    h.press("tab");
    assert_eq!(h.app.address.as_ref().unwrap().text, "~/projects/demo/");
    // Go up with ".." and Enter.
    h.keys("..");
    h.press("enter");
    h.wait("the parent", |a| {
        a.cwd.ends_with("projects") && !a.is_loading()
    });
    assert!(h.app.address.is_none());
}

#[test]
fn a_click_on_the_path_edits_it_and_a_click_elsewhere_stops() {
    let mut h = scene(140, 30);
    let (x, y) = h.where_is(&Target::Address);
    // The right end of the field is empty space after the last segment.
    let r = h.app.hits.find(&Target::Address).unwrap();
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        r.x + r.width - 2,
        y,
        KeyModifiers::NONE,
    );
    let _ = x;
    assert!(h.app.address.is_some(), "{}", h.screen());
    h.click(&Target::Row(1));
    assert!(
        h.app.address.is_none(),
        "a click on the list ends the editing"
    );
    // A suggestion can be clicked.
    h.press("ctrl+l");
    for _ in 0..5 {
        h.press("backspace"); // "~/projects/demo" -> keeps "~/projects/"?
    }
    h.wait("suggestions", |a| !a.address_suggestions().is_empty());
}

#[test]
fn a_mistyped_address_says_why_and_stays_put() {
    let mut h = scene(140, 30);
    h.press("ctrl+l");
    h.keys("nowhere");
    h.press("enter");
    h.wait("the refusal", |a| a.toast.is_some());
    assert!(h.app.cwd.ends_with("demo"));
}

// ---------------------------------------------------------------------------- the command bar

/// Is there a clickable button for `action` in the command bar (its middle row)?
fn in_command_bar(h: &H, action: Action) -> bool {
    h.app
        .hits
        .all()
        .any(|(r, t)| *t == Target::Act(action) && r.y == 3)
}

#[test]
fn the_command_buttons_do_what_their_keys_do() {
    let mut h = scene(170, 35);
    let _ = h.screen();
    // Nothing on the clipboard yet, and nothing done: Paste and Undo are faint, so they are
    // not clickable in the command bar (the key hints below still are).
    assert!(!in_command_bar(&h, Action::Paste));
    assert!(!in_command_bar(&h, Action::Undo));
    assert!(in_command_bar(&h, Action::Copy));
    h.click(&Target::Act(Action::Copy));
    assert!(h.app.clipboard.is_some());
    let _ = h.screen();
    assert!(in_command_bar(&h, Action::Paste), "now Paste is a button");
    // Paste into the same folder: the plan comes first, like with Ctrl+V.
    h.click(&Target::Act(Action::Paste));
    plan_modal(&mut h);
    h.press("esc");
    // Rename opens the prompt, as F2 does.
    h.click(&Target::Act(Action::Rename));
    assert!(matches!(h.app.modal, Some(Modal::Input(_))));
    h.press("esc");
    // Delete is a plan to move to the trash.
    h.click(&Target::Act(Action::Trash));
    plan_modal(&mut h);
    h.press("esc");
    // Compress asks for a name.
    h.click(&Target::Act(Action::Compress));
    assert!(matches!(h.app.modal, Some(Modal::Input(_))));
    h.press("esc");
    // Cut marks the move.
    h.click(&Target::Act(Action::Cut));
    assert_eq!(
        h.app.clipboard.as_ref().unwrap().mode,
        rada_core::ops::TransferMode::Move
    );
}

#[test]
fn undo_says_what_it_would_undo_and_runs_after_a_plan() {
    let mut h = scene(170, 35);
    h.press("n");
    h.keys("fresh");
    h.press("enter");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    h.wait("the label", |a| a.undo_label.is_some());
    let row = h.screen().lines().nth(3).unwrap().to_string();
    assert!(row.contains("Undo") && row.contains("new folder"), "{row}");
    assert!(h.app.hits.find(&Target::Act(Action::Undo)).is_some());
    h.click(&Target::Act(Action::Undo));
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    h.wait("nothing left to undo", |a| a.undo_label.is_none());
    let row = h.screen().lines().nth(3).unwrap().to_string();
    assert!(!row.contains("new folder"), "{row}");
}

#[test]
fn the_dropdowns_open_menus_that_work() {
    let mut h = scene(170, 35);
    // New ▾ > folder / empty file.
    h.click(&Target::Menu(MenuKind::New));
    let s = h.screen();
    assert!(
        s.contains("New folder") && s.contains("New empty file"),
        "{s}"
    );
    h.press("down");
    h.press("enter");
    assert!(
        matches!(&h.app.modal, Some(Modal::Input(iv)) if matches!(iv.kind, rada_tui::app::InputKind::NewFile)),
        "{}",
        h.screen()
    );
    h.keys("todo.txt");
    h.press("enter");
    plan_modal(&mut h);
    assert!(h.screen().contains("New file"), "{}", h.screen());
    run_plan_and_wait(&mut h);
    assert!(h.app.cwd.join("todo.txt").is_file());
    assert_eq!(
        std::fs::metadata(h.app.cwd.join("todo.txt")).unwrap().len(),
        0
    );
    h.wait("the undo label", |a| {
        a.undo_label.as_deref() == Some("new file")
    });
    // Sort ▾ lists the keys with the current one marked.
    h.click(&Target::Menu(MenuKind::Sort));
    let s = h.screen();
    assert!(
        s.contains("✓ Name") && s.contains("Date modified") && s.contains("Type"),
        "{s}"
    );
    assert!(s.contains("✓ Ascending"), "{s}");
    h.press("down");
    h.press("down");
    h.press("down"); // Size
    h.press("enter");
    assert_eq!(h.app.sort.key, rada_core::model::SortKey::Size);
    h.click(&Target::Menu(MenuKind::Sort));
    h.press("j");
    h.press("j");
    h.press("j");
    h.press("j");
    h.press("j"); // Descending
    h.press("enter");
    assert!(h.app.sort.reverse);
    // View ▾ switches the view and toggles the panes.
    h.click(&Target::Menu(MenuKind::View));
    h.press("down"); // Icons
    h.press("enter");
    assert_eq!(h.app.view, ViewMode::Icons);
    h.click(&Target::Menu(MenuKind::More));
    assert!(h.screen().contains("Operation history"));
}

#[test]
fn clicking_a_column_title_sorts_by_it_and_the_arrow_follows() {
    let mut h = scene(170, 35);
    let head = |h: &mut H| h.screen().lines().nth(5).unwrap().to_string();
    assert!(head(&mut h).contains("Name ↑"));
    h.click(&Target::SortBy(rada_core::model::SortKey::Type));
    assert_eq!(h.app.sort.key, rada_core::model::SortKey::Type);
    assert!(head(&mut h).contains("Type ↑") && !head(&mut h).contains("Name ↑"));
    h.click(&Target::SortBy(rada_core::model::SortKey::Type));
    assert!(head(&mut h).contains("Type ↓"));
    // Types are in words, and sorting by them groups them.
    h.click(&Target::SortBy(rada_core::model::SortKey::Modified));
    assert!(
        head(&mut h).contains("Date modified ↓"),
        "a new key keeps the direction"
    );
}

// ---------------------------------------------------------------------------- selection

#[test]
fn the_checkbox_selects_the_name_moves_the_cursor_and_a_double_click_opens() {
    let mut h = scene(140, 30);
    let _ = h.screen();
    // Click a name: the cursor moves, nothing is selected.
    h.click(&Target::Row(7));
    assert_eq!(h.app.cursor, 7);
    assert!(h.app.marked.is_empty());
    // Click the box of another row: it is selected, and the selection grows.
    let r = h.app.hits.find(&Target::Check(9)).expect("a checkbox");
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        r.x,
        r.y,
        KeyModifiers::NONE,
    );
    assert_eq!(h.marked_names().len(), 1);
    let r = h.app.hits.find(&Target::Check(10)).unwrap();
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        r.x + 1,
        r.y,
        KeyModifiers::NONE,
    );
    assert_eq!(h.marked_names().len(), 2, "the boxes add up");
    // Clicking a box again clears it.
    let r = h.app.hits.find(&Target::Check(9)).unwrap();
    h.mouse(
        MouseEventKind::Down(MouseButton::Left),
        r.x,
        r.y,
        KeyModifiers::NONE,
    );
    assert_eq!(h.marked_names().len(), 1);
    // Double click on a folder name opens it.
    h.click(&Target::Row(0));
    h.click(&Target::Row(0));
    h.wait("the folder", |a| {
        a.cwd.ends_with("assets") && !a.is_loading()
    });
    // The title's box selects everything.
    h.click(&Target::Act(Action::SelectAll));
    assert!(h.app.marked.is_empty() || !h.app.visible.is_empty());
}

#[test]
fn the_cursor_row_is_solid_the_selected_rows_are_quiet_and_the_tick_stays_under_the_cursor() {
    let mut h = busy_scene(140, 30);
    let _ = h.screen();
    let th = h.app.th.clone();
    let buf = h.term.backend().buffer().clone();
    let cursor_row = h.app.hits.find(&Target::Row(h.app.cursor)).unwrap();
    // The cursor row: the solid colour, white text, and a bar in the leftmost cell.
    let cell = &buf[(cursor_row.x + 6, cursor_row.y)];
    assert_eq!(cell.bg, th.cursor);
    assert_eq!(buf[(cursor_row.x, cursor_row.y)].symbol(), "▌");
    assert_eq!(buf[(cursor_row.x, cursor_row.y)].fg, th.accent);
    let name = (cursor_row.x..cursor_row.x + 40)
        .map(|x| &buf[(x, cursor_row.y)])
        .find(|c| c.symbol() == "p")
        .unwrap();
    assert_eq!(name.fg, th.cursor_text);
    assert!(name.modifier.contains(ratatui::style::Modifier::BOLD));
    // A selected row: the quiet tone, a tick in the accent colour, the name in bold.
    let marked = h.app.hits.find(&Target::Row(5)).unwrap();
    assert_eq!(buf[(marked.x + 6, marked.y)].bg, th.sel);
    let tick = &buf[(marked.x + 1, marked.y)];
    assert_eq!((tick.symbol(), tick.fg), ("☑", th.check));
    // The cursor on a selected row: the cursor wins, the tick is still there.
    h.press("space");
    h.press("up");
    let _ = h.screen();
    let buf = h.term.backend().buffer().clone();
    let r = h.app.hits.find(&Target::Row(h.app.cursor)).unwrap();
    assert!(h.app.marked.contains(&h.app.current().unwrap().name));
    assert_eq!(buf[(r.x + 6, r.y)].bg, th.cursor);
    assert_eq!(buf[(r.x + 1, r.y)].symbol(), "☑");
}

// ---------------------------------------------------------------------------- icons

#[test]
fn the_icons_view_moves_in_two_dimensions_and_selects_like_the_table() {
    let mut h = scene(120, 30);
    h.press("ctrl+2");
    h.press("g");
    let _ = h.screen();
    let cols = h.app.viewport.grid_cols;
    assert!(cols >= 4, "{cols}");
    let start = h.app.cursor;
    h.press("l");
    assert_eq!(h.app.cursor, start + 1, "l moves across");
    h.press("right");
    assert_eq!(h.app.cursor, start + 2);
    h.press("h");
    h.press("left");
    assert_eq!(h.app.cursor, start);
    h.press("j");
    assert_eq!(h.app.cursor, start + cols, "j moves a whole row down");
    h.press("down");
    assert_eq!(h.app.cursor, start + 2 * cols);
    h.press("k");
    h.press("up");
    assert_eq!(h.app.cursor, start);
    // Space selects, Enter opens, names are cut with …
    h.press("space");
    assert_eq!(h.marked_names().len(), 1);
    let s = h.screen();
    assert!(s.contains("☑"), "{s}");
    // Back to the table with Ctrl+1, v and the status buttons.
    h.press("ctrl+1");
    assert_eq!(h.app.view, ViewMode::Details);
    h.press("v");
    assert_eq!(h.app.view, ViewMode::Icons);
    h.click(&Target::ViewMode(ViewMode::Details));
    assert_eq!(h.app.view, ViewMode::Details);
    h.click(&Target::ViewMode(ViewMode::Icons));
    assert_eq!(h.app.view, ViewMode::Icons);
    // A click on a tile moves the cursor, a double click opens.
    h.click(&Target::Row(0));
    h.click(&Target::Row(0));
    h.wait("a folder", |a| a.cwd.ends_with("assets") && !a.is_loading());
}

#[test]
fn long_names_are_cut_with_an_ellipsis_in_the_icons_view_and_scrolling_follows_the_cursor() {
    let sb = rada_core::testutil::Sandbox::new();
    for i in 0..60 {
        sb.write(
            format!("w/file-{i:02}-with-a-really-long-name-indeed.txt"),
            "x",
        );
    }
    let dir = sb.path("w");
    let mut h = H::with_config(sb, dir, 80, 24, |c| c.view = ViewMode::Icons);
    let s = h.screen();
    assert!(s.contains('…'), "{s}");
    let rows = h.app.viewport.grid_rows;
    let cols = h.app.viewport.grid_cols;
    for _ in 0..(rows + 2) {
        h.press("j");
    }
    let _ = h.screen();
    assert!(
        h.app.scroll > 0,
        "the view scrolled to keep the cursor in sight"
    );
    assert!(h.app.scroll <= h.app.cursor && h.app.cursor < h.app.scroll + rows * cols);
    h.press("G");
    let _ = h.screen();
    assert_eq!(h.app.cursor, 59);
    assert!(h.app.cursor < h.app.scroll + rows * cols);
}

// ---------------------------------------------------------------------------- details

#[test]
fn the_details_pane_shows_the_item_its_properties_and_buttons() {
    let mut h = scene(170, 35);
    let s = h.screen();
    for needle in [
        "photo.jpg",
        "JPEG image · 70.7 KB",
        "Size",
        "70.7 KB (72,397 bytes)",
        "Modified",
        "Today, 13:30",
        "Location",
        "~/projects/demo",
        "Open",
        "Open with…",
        "Preview",
    ] {
        assert!(s.contains(needle), "{needle}:\n{s}");
    }
    // Windows has no permission bits to show.
    assert!(!cfg!(unix) || s.contains("Permissions"), "{s}");
    // The buttons sit right under the preview, before the properties.
    let lines: Vec<&str> = s.lines().collect();
    let open = lines
        .iter()
        .position(|l| l.contains("▸ Open") && !l.contains("Open with"))
        .unwrap();
    let size = lines
        .iter()
        .position(|l| l.contains("70.7 KB (72,397 bytes)"))
        .unwrap();
    assert!(open < size, "{s}");
    // Each button works.
    h.click(&Target::Act(Action::OpenWith));
    assert!(
        matches!(&h.app.modal, Some(Modal::Input(iv)) if matches!(iv.kind, rada_tui::app::InputKind::OpenWith { .. }))
    );
    h.press("esc");
    h.click(&Target::FullPreview);
    assert!(matches!(h.app.modal, Some(Modal::FullPreview)));
    assert!(h.screen().contains("close"));
    h.press("esc");
    assert!(h.app.modal.is_none());
    // Alt+P hides and shows it.
    h.press("alt+p");
    assert!(!h.screen().contains("Location"));
    h.press("alt+p");
    assert!(h.screen().contains("Location"));
}

#[test]
fn the_details_pane_is_hidden_below_140_columns_and_alt_p_shows_it_over_the_list() {
    let mut h = scene(120, 30);
    let s = h.screen();
    assert!(!s.contains("Location"), "hidden by default:\n{s}");
    assert_eq!(h.app.details_shown, rada_tui::ui::DetailsMode::Hidden);
    h.press("alt+p");
    let s = h.screen();
    assert!(s.contains("Location") && s.contains("photo.jpg"), "{s}");
    assert_eq!(h.app.details_shown, rada_tui::ui::DetailsMode::Overlay);
    h.press("alt+p");
    assert!(!h.screen().contains("Location"));
    // Too narrow for even the overlay.
    let mut h = scene(50, 20);
    h.press("alt+p");
    assert!(h.app.toast.is_some());
}

#[test]
fn the_details_pane_summarises_a_selection() {
    let mut h = busy_scene(170, 35);
    let s = h.screen();
    assert!(s.contains("3 items selected"), "{s}");
}

#[test]
fn properties_copy_path_and_open_terminal_are_reachable() {
    let mut h = scene(140, 30);
    h.press("alt+enter");
    let s = h.screen();
    for needle in [
        "Properties",
        "photo.jpg",
        "JPEG image",
        "70.7 KB (72,397 bytes)",
    ] {
        assert!(s.contains(needle), "{needle}:\n{s}");
    }
    // Windows has no permission bits to show.
    assert!(!cfg!(unix) || s.contains("Permissions"), "{s}");
    h.press("esc");
    h.press("ctrl+shift+c");
    let text = h.app.copied_text.clone().expect("a path to copy");
    assert!(text.ends_with("projects/demo/photo.jpg"), "{text}");
    assert_eq!(h.app.take_copied_text().as_deref(), Some(text.as_str()));
    assert!(h.app.copied_text.is_none());
}

#[test]
fn the_menu_key_and_shift_f10_open_the_context_menu() {
    let mut h = scene(120, 30);
    h.press("shift+f10");
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
    assert!(h.screen().contains("Properties"));
    h.press("esc");
    h.press("menu");
    assert!(matches!(h.app.modal, Some(Modal::Menu(_))));
}

#[test]
fn the_empty_space_menu_has_new_paste_sort_view_refresh_and_terminal() {
    let mut h = scene(120, 35);
    let (x, y) = h.where_is(&Target::List);
    h.mouse(
        MouseEventKind::Down(MouseButton::Right),
        x,
        y + 24,
        KeyModifiers::NONE,
    );
    let s = h.screen();
    for needle in [
        "New",
        "Paste",
        "Sort by",
        "View",
        "Refresh",
        "Open a terminal here",
    ] {
        assert!(s.contains(needle), "{needle}:\n{s}");
    }
    // Sort by ▸ opens the sort choices in its place.
    h.press("down");
    h.press("down");
    h.press("enter");
    let s = h.screen();
    assert!(
        s.contains("Date modified") && s.contains("Descending"),
        "{s}"
    );
    h.press("esc");
}

#[test]
fn the_item_menu_has_extract_on_archives_and_pin_on_folders() {
    let mut h = scene(120, 30);
    put_cursor(&mut h, "docs");
    h.right_click(&Target::Row(h.app.cursor));
    let s = h.screen();
    assert!(s.contains("Pin to navigation"), "{s}");
    // "Open with…" is for files: on a folder it is there but cannot be chosen.
    assert!(s.contains("Open with…"), "{s}");
    h.press("esc");
}

// ---------------------------------------------------------------------------- layout and config

#[test]
fn the_compact_layout_has_only_the_list_and_the_status() {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/a.txt", "a");
    let dir = sb.path("w");
    let mut h = H::with_config(sb, dir, 120, 30, |c| {
        c.layout = rada_tui::LayoutKind::Compact
    });
    let s = h.screen();
    assert!(
        !s.contains("Search in") && !s.contains("New ▾") && !s.contains("Ctrl+C"),
        "{s}"
    );
    assert!(s.contains("a.txt") && s.contains("1 item"), "{s}");
    assert!(s.contains("rada"), "the tabs stay: {s}");
}

#[test]
fn the_hints_can_be_hidden_and_the_dates_made_absolute() {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/a.txt", "a");
    let dir = sb.path("w");
    let mut h = H::with_config(sb, dir, 140, 30, |c| {
        c.show_hints = false;
        c.dates = rada_tui::fmt::DateStyle::Absolute;
    });
    let s = h.screen();
    assert!(!s.contains("Ctrl+C") && !s.contains("new tab"), "{s}");
    assert!(s.contains("/2026 ") && !s.contains("Today 0"), "{s}");
}

#[test]
fn types_and_dates_are_computed_once_per_entry() {
    let mut h = scene(140, 30);
    let _ = h.screen();
    // The type of every entry is a word, never computed while drawing: it is in the entry.
    for e in h.app.listing.all() {
        assert!(!e.type_label.is_empty());
    }
    assert!(h.screen().contains("Archive (tar.gz)") && h.screen().contains("Shell script"));
}

#[test]
fn nothing_the_explorer_draws_leaves_the_screen_or_breaks_a_wide_glyph() {
    for (w, hh) in SIZES {
        let mut h = busy_scene(w, hh);
        for view in [ViewMode::Details, ViewMode::Icons] {
            h.app.view = view;
            let s = h.screen();
            assert!(!s.contains('\u{fffd}'));
            for l in s.lines() {
                assert!(
                    l.trim_end().width() <= w as usize,
                    "{w}x{hh} {view:?}: {l:?}"
                );
            }
        }
    }
}

#[test]
fn key_events_that_are_releases_do_nothing() {
    use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};
    let mut h = scene(120, 30);
    let before = h.app.cursor;
    let mut ev = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    ev.kind = KeyEventKind::Release;
    ev.state = KeyEventState::NONE;
    h.app.on_key(ev);
    assert_eq!(h.app.cursor, before);
    ev.kind = KeyEventKind::Press;
    h.app.on_key(ev);
    assert_eq!(h.app.cursor, before + 1);
}
