//! Windows on a terminal that is hardly bigger than they are: no panic, whatever the height.

mod common;

use common::*;
use crossterm::event::KeyCode;

fn plan_on(w: u16, h: u16, typed_word: bool) -> H {
    let sb = rada_core::testutil::Sandbox::new();
    for n in ["a.txt", "b.txt", "c.txt"] {
        sb.write(format!("w/{n}"), n);
    }
    let dir = sb.path("w");
    let mut hh = H::new(sb, dir, w, h);
    hh.press("ctrl+a");
    hh.keys(if typed_word { "D" } else { "c" });
    if !typed_word {
        hh.press("ctrl+c");
        hh.press("ctrl+v");
    }
    plan_modal(&mut hh);
    hh
}

#[test]
fn the_plan_window_opens_at_every_height() {
    for h in 8..=16 {
        for typed in [false, true] {
            let mut hh = plan_on(40, h, typed);
            let s = hh.screen();
            assert!(s.contains("Cancel"), "40x{h} typed={typed}:\n{s}");
            hh.key(KeyCode::Esc);
        }
    }
}
