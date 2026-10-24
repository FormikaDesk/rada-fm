//! The preview of a source file is coloured from the colours of the theme; a file whose
//! language is not known stays as plain as before, and a big file does not hold up the
//! interface.

mod common;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use common::*;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

fn folder() -> H {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/a.rs", "fn main() {\n    let n = 42; // hi\n}\n");
    sb.write("w/b.toml", "[package]\na = \"rada\" # hi\nsize = 12\n");
    sb.write("w/c.zzz", "fn main() {\n    let n = 42; // hi\n}\n");
    let mut big = String::new();
    for i in 0..20_000 {
        big.push_str(&format!("fn f{i}() {{ let v = {i}; // go\n}}\n"));
    }
    sb.write("w/d.rs", big);
    let start = sb.path("w");
    H::new(sb, start, 140, 36)
}

fn put_cursor(h: &mut H, name: &str) {
    h.keys("g");
    for _ in 0..10 {
        if h.cursor_name() == name {
            break;
        }
        h.keys("j");
    }
    assert_eq!(h.cursor_name(), name);
    h.wait("preview", |a| {
        a.preview.name == name && a.preview.content.is_some()
    });
}

/// The foreground colours of the cells that draw `word` on the screen (the first row that has
/// it, from the right half: the preview).
fn colours_of(buf: &Buffer, word: &str) -> HashSet<Color> {
    let (w, h) = (buf.area.width, buf.area.height);
    let want: Vec<String> = word.chars().map(String::from).collect();
    for y in 0..h {
        for x in (w / 2)..w.saturating_sub(want.len() as u16) {
            if (0..want.len()).all(|i| buf[(x + i as u16, y)].symbol() == want[i]) {
                return (0..want.len()).map(|i| buf[(x + i as u16, y)].fg).collect();
            }
        }
    }
    panic!("{word:?} is not drawn in the preview");
}

#[test]
fn a_rust_file_and_a_toml_file_are_drawn_in_several_colours() {
    let mut h = folder();
    put_cursor(&mut h, "a.rs");
    let buf = h.redrawn_buffer();
    let normal = h.app.settings.th.text;
    let comment = colours_of(&buf, "// hi");
    assert!(!comment.contains(&normal), "{comment:?}");
    let line = colours_of(&buf, "let n = 42;");
    assert!(line.len() >= 3, "keyword, plain, number: {line:?}");
    assert!(line.iter().any(|c| *c != normal));
    // Every colour is one the theme has.
    let th = &h.app.settings.th;
    let own: Vec<Color> = vec![
        th.text,
        th.text_dim,
        th.success,
        th.warn,
        th.accent,
        th.kinds.markup,
        th.kinds.data,
        th.kinds.doc,
        th.muted,
    ];
    for c in line.iter().chain(comment.iter()) {
        assert!(
            own.contains(c) || *c == th.text,
            "{c:?} is not a colour of the theme"
        );
    }

    put_cursor(&mut h, "b.toml");
    let buf = h.redrawn_buffer();
    assert!(colours_of(&buf, "a = \"rada\" # hi").len() >= 3);
}

#[test]
fn a_file_of_an_unknown_language_stays_plain() {
    let mut h = folder();
    put_cursor(&mut h, "c.zzz");
    let buf = h.redrawn_buffer();
    let line = colours_of(&buf, "let n = 42; // hi");
    assert_eq!(line.len(), 1, "one colour for the whole line: {line:?}");
}

#[test]
fn switched_off_nothing_is_coloured() {
    let sb = rada_core::testutil::Sandbox::new();
    sb.write("w/a.rs", "fn main() {\n    let n = 42; // hi\n}\n");
    let start = sb.path("w");
    let mut h = H::with_config(sb, start, 140, 36, |cfg| {
        cfg.limits.syntax_highlight = false;
    });
    put_cursor(&mut h, "a.rs");
    let buf = h.redrawn_buffer();
    assert_eq!(colours_of(&buf, "let n = 42; // hi").len(), 1);
}

#[test]
fn a_big_file_does_not_hold_up_the_interface() {
    let mut h = folder();
    put_cursor(&mut h, "d.rs");
    // The text was read in the worker; drawing it is as quick as any other frame.
    let t = Instant::now();
    let buf = h.redrawn_buffer();
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert!(colours_of(&buf, "fn f0()").len() >= 2);
    let Some(rada_core::preview::Preview::Text(t)) = &h.app.preview.content else {
        panic!("a text")
    };
    let coloured = t
        .syntax
        .as_ref()
        .map_or(0, |s| s.iter().filter(|l| !l.is_empty()).count());
    assert!(coloured > 100 && coloured <= t.lines.len(), "{coloured}");
    assert!(t.lines.len() <= 1000);
    // Past the first 64 KB nothing is coloured (a line here is about 30 bytes).
    let last = t.syntax.as_ref().unwrap().last().unwrap();
    assert!(last.is_empty() || t.lines.len() * 30 < 64 * 1024 + 4096);
}
