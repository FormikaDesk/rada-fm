//! Putting text on the system clipboard from a terminal program. Two ways:
//!
//! - the OSC 52 escape sequence, which many terminals (kitty, iTerm2, WezTerm, Windows
//!   Terminal, Ghostty, foot, tmux with `set-clipboard on`) accept and hand to the
//!   clipboard; it also works over ssh;
//! - the system's own clipboard program, for the terminals that ignore the sequence
//!   (Terminal.app, GNOME Terminal, Konsole): `pbcopy`, `wl-copy` / `xclip` / `xsel`,
//!   `clip.exe`.

use std::io::{self, Write};
use std::process::{Command, Stdio};

const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = match chunk.len() {
            3 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32,
            2 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8,
            _ => (chunk[0] as u32) << 16,
        };
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The escape sequence that asks the terminal to put `text` on the clipboard.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// A clipboard program: its name, its arguments, and the bytes it is to be given.
type NativeCommand = (&'static str, &'static [&'static str], Vec<u8>);

/// The programs that put their standard input on the clipboard here, best first, and the
/// bytes each wants.
fn native_commands(text: &str) -> Vec<NativeCommand> {
    const NONE: &[&str] = &[];
    let utf8 = || text.as_bytes().to_vec();
    let mut v: Vec<NativeCommand> = Vec::new();
    if cfg!(windows) {
        // clip.exe reads the console's code page unless the text is UTF-16 with its mark.
        let mut utf16 = vec![0xff, 0xfe];
        utf16.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        v.push(("clip.exe", NONE, utf16));
    } else if cfg!(target_os = "macos") {
        v.push(("pbcopy", NONE, utf8()));
    } else {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            v.push(("wl-copy", NONE, utf8()));
        }
        if std::env::var_os("DISPLAY").is_some() {
            v.push(("xclip", &["-selection", "clipboard"], utf8()));
            v.push(("xsel", &["--clipboard", "--input"], utf8()));
        }
    }
    v
}

/// Hand `text` to the system's clipboard program. An error when there is none, or it fails.
pub fn copy_native(text: &str) -> io::Result<()> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no clipboard program found");
    for (program, args, bytes) in native_commands(text) {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                last = e;
                continue;
            }
        };
        // Closing the pipe is what tells the program the text is complete.
        let written = match child.stdin.take() {
            Some(mut stdin) => stdin.write_all(&bytes),
            None => Ok(()),
        };
        match (written, child.wait()) {
            (Ok(()), Ok(status)) if status.success() => return Ok(()),
            (Err(e), _) | (_, Err(e)) => last = e,
            (Ok(()), Ok(status)) => {
                last = io::Error::other(format!("{program} ended with {status}"));
            }
        }
    }
    Err(last)
}

/// Put `text` on the clipboard: through the terminal when it is known to take the
/// sequence, otherwise with the system's program, and with the sequence after all when
/// that program is missing or fails (it costs nothing to try, except on a plain terminal,
/// where an unknown sequence shows up as garbage). `terminal` is where the sequence is
/// written.
pub fn copy(text: &str, osc52_works: bool, basic: bool, terminal: &mut dyn Write) {
    if !osc52_works {
        match copy_native(text) {
            Ok(()) => return,
            Err(e) => tracing::warn!("the system clipboard could not be reached: {e}"),
        }
        if basic {
            return;
        }
    }
    let _ = terminal.write_all(osc52(text).as_bytes());
    let _ = terminal.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_examples() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_sequence_wraps_the_encoded_text() {
        assert_eq!(osc52("/tmp/a b"), "\x1b]52;c;L3RtcC9hIGI=\x07");
    }

    #[test]
    fn every_native_command_gets_the_text_in_its_own_encoding() {
        for (program, _, bytes) in native_commands("è") {
            if program == "clip.exe" {
                assert_eq!(bytes, [0xff, 0xfe, 0xe8, 0x00]);
            } else {
                assert_eq!(bytes, "è".as_bytes());
            }
        }
    }

    #[test]
    fn a_terminal_that_takes_the_sequence_gets_it_and_nothing_else_happens() {
        let mut out: Vec<u8> = Vec::new();
        copy("/tmp/a b", true, false, &mut out);
        assert_eq!(out, osc52("/tmp/a b").as_bytes());
    }
}
