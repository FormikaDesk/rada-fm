//! The basic profile: the same interface, drawn with what every terminal has.
//!
//! The interface is never drawn differently for a plain terminal; its glyphs are replaced
//! on the way out (`compat.rs`). So two things have to hold: every glyph the interface
//! draws has an ASCII stand-in, and a whole screen that went through the basic profile
//! holds nothing but ASCII.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use crossterm::event::KeyCode;
use rada_core::testutil::*;
use rada_tui::compat::{CompatBackend, Rendering, STD_GLYPHS, ascii_for, std_for};
use rada_tui::theme::ColorDepth;
use rada_tui::{IconSet, Theme, ui};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Nerd Font icons live in the private use areas; the basic profile shows no icons.
fn is_private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0x10FFFF)
}

/// Every character beyond ASCII that the interface draws, with where it is written. This
/// reads the sources (but not the tables of stand-ins themselves).
fn interface_glyphs() -> Vec<(char, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut found = Vec::new();
    for f in files {
        if f.file_name().is_some_and(|n| n == "compat.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            // Unit tests come last in a file and draw nothing.
            if t.starts_with("#[cfg(test)]") {
                break;
            }
            if t.starts_with("//") {
                continue;
            }
            for c in line.chars() {
                if !c.is_ascii() && !is_private_use(c) {
                    found.push((c, format!("{}:{}", f.display(), i + 1)));
                }
            }
        }
    }
    found
}

/// A glyph written into the interface without an ASCII stand-in in `compat::ascii_for`
/// fails here, with the place where it is.
#[test]
fn every_glyph_of_the_interface_has_an_ascii_stand_in() {
    let missing: Vec<String> = interface_glyphs()
        .into_iter()
        .filter(|(c, _)| ascii_for(*c).is_none())
        .map(|(c, at)| format!("{at}: {c:?}"))
        .collect();
    assert!(
        missing.is_empty(),
        "glyphs without an ASCII stand-in (add them to compat::ascii_for):\n{}",
        missing.join("\n")
    );
}

/// And for the `unicode-std` icon set: every glyph is one that every monospace font has,
/// or has a stand-in that is.
#[test]
fn every_glyph_of_the_interface_is_standard_or_has_a_standard_stand_in() {
    let missing: Vec<String> = interface_glyphs()
        .into_iter()
        .filter(|(c, _)| {
            let drawn = std_for(*c).unwrap_or(*c);
            !(drawn.is_ascii() || STD_GLYPHS.contains(drawn))
        })
        .map(|(c, at)| format!("{at}: {c:?}"))
        .collect();
    assert!(
        missing.is_empty(),
        "glyphs that only a symbol font has (add them to compat::std_for):\n{}",
        missing.join("\n")
    );
}

/// A folder with plain names: whatever is not ASCII on the screen is then the interface's.
fn plain_scene(w: u16, h: u16) -> H {
    let sb = Sandbox::new();
    let dir = sb.dirs.home.join("work");
    for (name, content) in [
        ("README.md", "# Work\n\nnotes\n"),
        ("main.rs", "fn main() {}\n"),
        ("notes.txt", "remember the milk\n"),
        ("src/lib.rs", "pub fn f() {}\n"),
    ] {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let mut h = H::with_config(sb, dir, w, h, |cfg| {
        // What `main` chooses for the basic profile.
        cfg.icons = IconSet::None;
        cfg.theme = Theme::named("rada", ColorDepth::Ansi16).unwrap();
        cfg.mouse = false;
    });
    h.wait("the places", |a| !a.paths.places.is_empty());
    h
}

/// The screen as the basic profile sends it to the terminal; what is not ASCII on it.
fn not_ascii(h: &mut H, w: u16, hh: u16) -> Vec<String> {
    let backend = CompatBackend::new(TestBackend::new(w, hh), Rendering::BASIC);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, &mut h.app)).unwrap();
    let buf = term.backend().inner().buffer();
    let mut found = Vec::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let sym = buf[(x, y)].symbol();
            if !sym.is_ascii() {
                found.push(format!("({x},{y}) {sym:?}"));
            }
        }
    }
    found
}

#[test]
fn the_basic_profile_sends_only_ascii_to_the_terminal() {
    for (w, hh) in [(120, 30), (80, 24), (40, 16)] {
        let mut h = plain_scene(w, hh);
        let found = not_ascii(&mut h, w, hh);
        assert!(found.is_empty(), "main screen at {w}x{hh}: {found:?}");

        // The windows too: the help, the palette, the context menu.
        h.key(KeyCode::Char('?'));
        let found = not_ascii(&mut h, w, hh);
        assert!(found.is_empty(), "help at {w}x{hh}: {found:?}");
        h.key(KeyCode::Esc);

        h.press("ctrl+p");
        let found = not_ascii(&mut h, w, hh);
        assert!(found.is_empty(), "palette at {w}x{hh}: {found:?}");
        h.key(KeyCode::Esc);

        h.press("shift+f10");
        let found = not_ascii(&mut h, w, hh);
        assert!(found.is_empty(), "context menu at {w}x{hh}: {found:?}");
        h.key(KeyCode::Esc);
    }
}

#[test]
fn the_basic_profile_keeps_the_layout() {
    // Same cells, same text where the text was ASCII already: only the glyphs differ.
    let mut h = plain_scene(120, 30);
    let modern = h.screen();
    let backend = CompatBackend::new(TestBackend::new(120, 30), Rendering::BASIC);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, &mut h.app)).unwrap();
    let buf = term.backend().inner().buffer();
    for (y, line) in modern.lines().enumerate() {
        for (x, c) in line.chars().enumerate() {
            let basic = buf[(x as u16, y as u16)].symbol();
            if c.is_ascii() {
                assert_eq!(basic, c.to_string(), "cell ({x},{y}) moved");
            } else {
                assert_eq!(
                    Some(basic),
                    ascii_for(c).map(String::from).as_deref(),
                    "cell ({x},{y})"
                );
            }
        }
    }
}
