//! Developer tool: shows what a terminal really delivers for each key.
//!
//! Run it inside the terminal you want to test; every key press is decoded by crossterm
//! (the same code vela uses) and appended to the log file given as the first argument:
//! the raw event, the chord, and the action the default keymap binds to it.
//!
//! `cargo run -p vela-tui --example keyprobe -- /path/to/log`   (Ctrl+Backslash stops it)

use std::io::Write;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use vela_tui::keymap::{Chord, Keymap};

fn main() -> std::io::Result<()> {
    let path = std::env::args().nth(1).expect("usage: keyprobe LOGFILE");
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let km = Keymap::default();
    enable_raw_mode()?;
    writeln!(log, "READY")?;
    println!("keyprobe: press keys (Ctrl+\\ stops)\r");
    loop {
        if let Event::Key(k) = event::read()? {
            if k.code == KeyCode::Char('\\') && k.modifiers.contains(KeyModifiers::CONTROL) {
                break;
            }
            let action = km.action_for(&k).map(|a| a.id()).unwrap_or("-");
            writeln!(
                log,
                "{:?} {:?} | {} | {action}",
                k.code,
                k.modifiers,
                Chord::from_event(&k)
            )?;
            println!("{:?} {:?} -> {action}\r", k.code, k.modifiers);
        }
    }
    disable_raw_mode()
}
