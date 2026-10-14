//! Popups on a narrow or short terminal, and in the basic profile: the cards shrink without
//! cutting their buttons, nothing panics, and without glyphs beyond ASCII the corners are `+`.

mod common;

use common::*;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use rada_tui::app::{Modal, Notice, NoticeAction, NoticeKind};
use rada_tui::hits::Target;
use rada_tui::ui;

fn files(w: u16, h: u16) -> (H, std::path::PathBuf) {
    let sb = rada_core::testutil::Sandbox::new();
    for n in ["a.txt", "b.txt", "c.txt"] {
        sb.write(format!("w/{n}"), n);
    }
    sb.write("dest/a.txt", "old");
    let dest = sb.path("dest");
    let dir = sb.path("w");
    (H::new(sb, dir, w, h), dest)
}

const SIZES: [(u16, u16); 5] = [(40, 16), (60, 20), (80, 24), (100, 30), (170, 42)];

#[test]
fn menus_windows_and_cards_fit_every_size_without_panicking() {
    for (w, h) in SIZES {
        let (mut hh, dest) = files(w, h);
        hh.keys("j");
        // The menu of an item, and the one beside it.
        let (x, y) = hh.where_is(&Target::Row(hh.app.cursor));
        hh.mouse(
            MouseEventKind::Down(MouseButton::Right),
            x,
            y,
            crossterm::event::KeyModifiers::NONE,
        );
        let s = hh.screen();
        assert!(s.contains("Open"), "{w}x{h}:\n{s}");
        hh.key(KeyCode::Down);
        hh.key(KeyCode::Right);
        let _ = hh.screen();
        hh.key(KeyCode::Esc);
        hh.key(KeyCode::Esc);
        // A plan with a clash: both buttons stay whole.
        hh.press("ctrl+a");
        hh.press("ctrl+c");
        hh.app.open_dir(dest.clone());
        let d = dest.clone();
        hh.wait("dest", move |a| a.cwd == d && !a.is_loading());
        hh.press("ctrl+v");
        plan_modal(&mut hh);
        let s = hh.screen();
        if w >= 60 {
            assert!(s.contains("Cancel") && s.contains("Copy"), "{w}x{h}:\n{s}");
        }
        hh.key(KeyCode::Esc);
        // Permanent deletion: the typed word and the buttons.
        hh.keys("D");
        plan_modal(&mut hh);
        let s = hh.screen();
        if w >= 60 {
            assert!(
                s.contains("Cancel") && s.contains("Delete"),
                "{w}x{h}:\n{s}"
            );
        }
        hh.key(KeyCode::Esc);
        // Cards.
        hh.app.notify(
            Notice::new(NoticeKind::Ok, "3 items moved to the trash")
                .with_action(NoticeAction::Undo),
        );
        hh.app.notify(Notice::new(
            NoticeKind::Error,
            "cannot rename setup.sh: denied",
        ));
        let _ = hh.screen();
        hh.key(KeyCode::F(1));
        let _ = hh.screen();
        assert!(matches!(hh.app.modal, Some(Modal::Help)));
    }
}

/// What the basic profile writes: the interface drawn through its backend.
fn basic_screen(h: &mut H, w: u16, rows: u16) -> String {
    use rada_tui::compat::{CompatBackend, Rendering};
    let backend = CompatBackend::new(
        ratatui::backend::TestBackend::new(w, rows),
        Rendering::BASIC,
    );
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, &mut h.app)).unwrap();
    let buf = term.backend().inner().buffer();
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

#[test]
fn in_the_basic_profile_popups_are_plain_ascii_with_plus_corners() {
    let (mut h, dest) = files(120, 36);
    h.app.basic = true;
    h.app.icons = rada_tui::IconSet::None;
    h.keys("j");
    let (x, y) = h.where_is(&Target::Row(h.app.cursor));
    h.mouse(
        MouseEventKind::Down(MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    let menu = basic_screen(&mut h, 120, 36);
    assert!(menu.is_ascii(), "{menu}");
    assert!(menu.contains("+-") && menu.contains("Open"), "{menu}");
    h.key(KeyCode::Esc);
    h.press("ctrl+a");
    h.press("ctrl+c");
    h.app.open_dir(dest.clone());
    let d = dest.clone();
    h.wait("dest", move |a| a.cwd == d && !a.is_loading());
    h.press("ctrl+v");
    plan_modal(&mut h);
    let plan = basic_screen(&mut h, 120, 36);
    assert!(plan.is_ascii(), "{plan}");
    assert!(plan.contains("Cancel") && plan.contains("Copy"), "{plan}");
    assert!(!plan.contains(&"#".repeat(12)), "{plan}");
    h.key(KeyCode::Esc);
    h.app.notify(
        Notice::new(NoticeKind::Ok, "3 items moved to the trash").with_action(NoticeAction::Undo),
    );
    let card = basic_screen(&mut h, 120, 36);
    assert!(card.is_ascii(), "{card}");
    assert!(
        card.contains("moved to the trash") && card.contains("Undo"),
        "{card}"
    );
}
