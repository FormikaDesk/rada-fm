//! The terminal's own background, set to the theme's for the length of the session.
//!
//! The margin a terminal keeps around its grid (Ghostty's window padding, for one) is
//! coloured with the terminal's background, not with what the interface draws, so a theme
//! that paints its own paper shows a frame of another colour around it. Setting the
//! background (OSC 11) makes the margin the same colour; resetting it (OSC 111) gives the
//! terminal its own back.
//!
//! Nothing is set where it would not hold or would be unwelcome: themes that leave the
//! terminal's background alone, 16-colour and basic terminals, `NO_COLOR`, multiplexers (the
//! sequence would change the colour of every pane, or none) and output that is not a terminal.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::caps::{Profile, TermCaps, TerminalKind};
use crate::theme::{ColorDepth, Theme, downgrade, rgb_of};

/// Whether the terminal's background was set and not yet given back.
static SET: AtomicBool = AtomicBool::new(false);

/// The colour to set, or `None` where the background is left alone.
pub fn wanted(theme: &Theme, caps: &TermCaps, to_terminal: bool) -> Option<[u8; 3]> {
    // Terminal.app and the Windows console host ignore the sequence.
    let ignores_it = matches!(
        caps.terminal,
        TerminalKind::AppleTerminal | TerminalKind::WindowsConsole
    );
    let holds = to_terminal
        && caps.profile == Profile::Modern
        && caps.depth != ColorDepth::Ansi16
        && !caps.no_color
        && !caps.multiplexer
        && !ignores_it;
    if !holds {
        return None;
    }
    // The colour the interface will really paint, after the depth of the terminal.
    rgb_of(downgrade(theme.bg?, caps.depth))
}

/// Set the terminal's background to `[r, g, b]`.
pub fn set(out: &mut impl Write, [r, g, b]: [u8; 3]) -> io::Result<()> {
    write!(out, "\x1b]11;rgb:{r:02x}/{g:02x}/{b:02x}\x1b\\")?;
    out.flush()?;
    SET.store(true, Ordering::Relaxed);
    Ok(())
}

/// Give the terminal its own background back, if it was taken. Safe to call again.
pub fn restore(out: &mut impl Write) {
    if SET.swap(false, Ordering::Relaxed) {
        let _ = out.write_all(b"\x1b]111\x1b\\");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(modify: impl FnOnce(&mut TermCaps)) -> TermCaps {
        let mut caps = TermCaps::modern();
        modify(&mut caps);
        caps
    }

    fn night() -> Theme {
        Theme::named("rada", ColorDepth::True).unwrap()
    }

    #[test]
    fn a_theme_with_paper_sets_the_colour_it_paints() {
        let theme = night();
        let Some(bg) = theme.bg else {
            panic!("the default theme paints")
        };
        let want = rgb_of(bg);
        assert_eq!(wanted(&theme, &TermCaps::modern(), true), want);
        assert!(want.is_some());
    }

    #[test]
    fn at_256_colours_it_is_the_colour_of_the_palette() {
        let theme = night();
        let got = wanted(&theme, &caps(|c| c.depth = ColorDepth::Ansi256), true).unwrap();
        let painted = downgrade(theme.bg.unwrap(), ColorDepth::Ansi256);
        assert_eq!(Some(got), rgb_of(painted));
    }

    #[test]
    fn nothing_is_set_where_the_terminal_background_stays() {
        let modern = TermCaps::modern();
        let none = |theme: &Theme| wanted(theme, &modern, true);
        assert_eq!(none(&night().transparent()), None);
        for name in ["catppuccin", "tokyo-night"] {
            let theme = Theme::named(name, ColorDepth::True).unwrap();
            assert_eq!(none(&theme), None, "{name}");
        }
        assert_eq!(
            none(&Theme::named("rada", ColorDepth::Ansi16).unwrap()),
            None
        );
    }

    #[test]
    fn nothing_is_set_on_terminals_that_would_not_keep_it() {
        let theme = night();
        assert!(wanted(&theme, &TermCaps::modern(), true).is_some());
        let refused: [(&str, TermCaps); 6] = [
            ("16 colours", caps(|c| c.depth = ColorDepth::Ansi16)),
            ("basic profile", caps(|c| c.profile = Profile::Basic)),
            ("NO_COLOR", caps(|c| c.no_color = true)),
            ("tmux or screen", caps(|c| c.multiplexer = true)),
            (
                "Terminal.app",
                caps(|c| c.terminal = TerminalKind::AppleTerminal),
            ),
            (
                "console host",
                caps(|c| c.terminal = TerminalKind::WindowsConsole),
            ),
        ];
        for (why, caps) in refused {
            assert_eq!(wanted(&theme, &caps, true), None, "{why}");
        }
        assert_eq!(
            wanted(&theme, &TermCaps::modern(), false),
            None,
            "not a terminal"
        );
    }

    /// One test for the whole life of the flag, since it is shared by the process.
    #[test]
    fn the_colour_is_set_and_given_back_once() {
        let mut out = Vec::new();
        restore(&mut out);
        assert!(out.is_empty(), "nothing was set, nothing to give back");

        set(&mut out, [0x1b, 0x1d, 0x27]).unwrap();
        assert_eq!(out, b"\x1b]11;rgb:1b/1d/27\x1b\\");

        out.clear();
        restore(&mut out);
        assert_eq!(out, b"\x1b]111\x1b\\");
        out.clear();
        restore(&mut out);
        assert!(out.is_empty(), "given back twice");
    }
}
