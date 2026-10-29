//! The answer a file chooser writes has one path per line, so a path with a line break in it
//! must never be handed over: the choice is refused with a message and the session goes on.
#![cfg(unix)]

mod common;

use common::*;
use crossterm::event::KeyCode;
use rada_core::testutil::*;
use rada_tui::chooser::{Chooser, render};

const BAD: [&str; 2] = ["two\nlines.txt", "back\rto.txt"];

fn refused(h: &mut H) {
    assert!(h.app.session.chosen.is_none(), "{:?}", h.app.session.chosen);
    assert!(!h.app.session.should_quit);
    let said: Vec<String> = h
        .app
        .shown_notices()
        .iter()
        .map(|n| n.text.clone())
        .collect();
    assert!(
        said.iter().any(|t| t.contains("line break")),
        "{said:?}\n{}",
        h.screen()
    );
}

fn scene(ch: Chooser, bad: &str) -> H {
    let sb = Sandbox::new();
    sb.write("work/a.txt", "a");
    sb.write(format!("work/{bad}"), "b");
    let start = sb.path("work");
    H::with_config(sb, start, 120, 30, move |c| c.chooser = Some(ch))
}

#[test]
fn a_file_with_a_line_break_in_its_name_is_refused_when_chosen() {
    for bad in BAD {
        let mut h = scene(Chooser::file(false), bad);
        h.cursor_to(&rada_core::display::name(std::ffi::OsStr::new(bad)));
        h.key(KeyCode::Enter);
        refused(&mut h);
    }
}

#[test]
fn one_such_file_among_several_marked_refuses_the_whole_choice() {
    let mut h = scene(Chooser::file(true), BAD[0]);
    h.key(KeyCode::Char(' '));
    h.key(KeyCode::Char(' '));
    h.key(KeyCode::Enter);
    refused(&mut h);
    // What would have been written: nothing at all.
    assert!(render(&h.app.session.chosen.clone().unwrap_or_default()).is_empty());
}

#[test]
fn a_folder_with_a_line_break_in_its_name_is_refused_as_a_choice_and_as_the_place_to_save() {
    let sb = Sandbox::new();
    let bad = sb.mkdir("work/odd\nfolder");
    sb.write("work/odd\nfolder/x.txt", "x");
    for ch in [Chooser::folder(false), Chooser::save("report.pdf")] {
        let start = bad.clone();
        let ch2 = ch.clone();
        let mut h = H::with_config(Sandbox::new(), start.clone(), 120, 30, move |c| {
            c.chooser = Some(ch2)
        });
        if ch.is_save() {
            h.press("ctrl+s");
            h.key(KeyCode::Enter);
        } else {
            h.key(KeyCode::Enter);
        }
        refused(&mut h);
    }
}
