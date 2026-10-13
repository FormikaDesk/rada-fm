//! What the terminal can do, decided in one place and before anything is drawn.
//!
//! There are two profiles and nothing in between:
//!
//! - **modern**: any terminal that speaks UTF-8 and the xterm extensions every current
//!   emulator has (alternate screen, mouse reports, 256 colours). Windows Terminal,
//!   Terminal.app, GNOME Terminal and Konsole all get the same interface; what only some
//!   of them offer (24-bit colour, Sixel, the clipboard sequence) is used where it exists
//!   and replaced by something every one of them has where it does not.
//! - **basic**: one fixed, simple fallback for everything else (a serial line, the Linux
//!   console, a non-UTF-8 locale): ASCII glyphs, the 16 ANSI colours, no mouse, no
//!   pictures, and no question asked to the terminal.
//!
//! The decision is made from the environment alone, so it is the same every time and can
//! be tested; `profile = "modern" | "basic"` (`--profile`, `RADA_PROFILE`) overrides it.

use ratatui_image::picker::ProtocolType;

use crate::compat::{Glyphs, Rendering};
use crate::theme::ColorDepth;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Modern,
    Basic,
}

impl Profile {
    /// `Some(None)` is "decide for me".
    pub fn parse(s: &str) -> Option<Option<Profile>> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(None),
            "modern" | "full" => Some(Some(Profile::Modern)),
            "basic" | "plain" | "simple" | "ascii" | "vt100" => Some(Some(Profile::Basic)),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::Modern => "modern",
            Profile::Basic => "basic",
        }
    }
}

/// The terminals that are told apart, because they differ in something rada uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalKind {
    WindowsTerminal,
    /// The classic Windows console host.
    WindowsConsole,
    AppleTerminal,
    /// GNOME Terminal and the other terminals built on VTE.
    Vte,
    Konsole,
    Ghostty,
    Kitty,
    WezTerm,
    Iterm2,
    Foot,
    LinuxConsole,
    /// `TERM` is empty, `dumb` or a hardware terminal's name.
    Plain,
    Other,
}

impl TerminalKind {
    pub fn name(self) -> &'static str {
        match self {
            TerminalKind::WindowsTerminal => "Windows Terminal",
            TerminalKind::WindowsConsole => "the Windows console",
            TerminalKind::AppleTerminal => "Terminal.app",
            TerminalKind::Vte => "GNOME Terminal (VTE)",
            TerminalKind::Konsole => "Konsole",
            TerminalKind::Ghostty => "Ghostty",
            TerminalKind::Kitty => "kitty",
            TerminalKind::WezTerm => "WezTerm",
            TerminalKind::Iterm2 => "iTerm2",
            TerminalKind::Foot => "foot",
            TerminalKind::LinuxConsole => "the Linux console",
            TerminalKind::Plain => "a plain terminal",
            TerminalKind::Other => "an xterm-compatible terminal",
        }
    }
}

/// How pictures will be drawn, as far as the environment can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImagePlan {
    /// Only the information card.
    Off,
    /// Coloured half blocks: every terminal with colour and UTF-8 draws them.
    Halfblocks,
    /// The terminal is known to draw this protocol; nothing needs asking but the cell size.
    Protocol(ProtocolType),
    /// An unknown terminal: it is asked.
    Ask,
}

/// The environment, as a function, so that tests can stand in for it.
pub struct Env<'a> {
    pub var: &'a dyn Fn(&str) -> Option<String>,
    /// This is a Windows console program (where `TERM` is normally not set).
    pub windows: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermCaps {
    pub profile: Profile,
    /// Why this profile, in words (for `rada doctor` and the log).
    pub why: String,
    pub terminal: TerminalKind,
    /// Inside tmux or GNU screen.
    pub multiplexer: bool,
    pub depth: ColorDepth,
    /// `NO_COLOR` is set: no colour is written at all.
    pub no_color: bool,
    /// The glyphs of the interface can be Unicode.
    pub unicode: bool,
    pub mouse: bool,
    /// The terminal takes text for the clipboard (OSC 52).
    pub osc52: bool,
    /// The terminal may be asked things (background colour, cell size).
    pub may_query: bool,
    pub images: ImagePlan,
}

/// The character set a locale names, when it names one: `it_IT.ISO-8859-1` → `ISO-8859-1`.
fn codeset(locale: &str) -> Option<&str> {
    let after = locale.split_once('.')?.1;
    Some(after.split('@').next().unwrap_or(after))
}

/// A locale that explicitly asks for something other than UTF-8. No locale at all (or `C`)
/// says nothing either way: containers and remote shells often have none, on terminals
/// that are UTF-8 all the same.
fn non_utf8_locale(var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find_map(|k| var(k).filter(|v| !v.is_empty()))?;
    let set = codeset(&locale)?.to_ascii_lowercase().replace('-', "");
    (set != "utf8").then_some(locale)
}

fn kind(env: &Env) -> TerminalKind {
    let get = |k: &str| (env.var)(k).unwrap_or_default();
    let has = |k: &str| !get(k).is_empty();
    let (term, prog) = (get("TERM"), get("TERM_PROGRAM"));
    // Terminals that name themselves come first: the variables of the terminal a program
    // was started from (VTE_VERSION, KONSOLE_VERSION, WT_SESSION) are inherited by any
    // other terminal opened from it.
    if term == "xterm-ghostty"
        || prog.eq_ignore_ascii_case("ghostty")
        || has("GHOSTTY_RESOURCES_DIR")
    {
        TerminalKind::Ghostty
    } else if term == "xterm-kitty" || has("KITTY_WINDOW_ID") {
        TerminalKind::Kitty
    } else if prog.eq_ignore_ascii_case("wezterm") || has("WEZTERM_EXECUTABLE") {
        TerminalKind::WezTerm
    } else if prog == "iTerm.app" {
        TerminalKind::Iterm2
    } else if prog == "Apple_Terminal" {
        TerminalKind::AppleTerminal
    } else if term.starts_with("foot") {
        TerminalKind::Foot
    } else if has("WT_SESSION") {
        TerminalKind::WindowsTerminal
    } else if has("KONSOLE_VERSION") {
        TerminalKind::Konsole
    } else if has("VTE_VERSION") {
        TerminalKind::Vte
    } else if term == "linux" {
        TerminalKind::LinuxConsole
    } else if env.windows && term.is_empty() {
        TerminalKind::WindowsConsole
    } else if term.is_empty()
        || term == "dumb"
        || term == "ansi"
        || term == "cons25"
        || (term.starts_with("vt") && term[2..].starts_with(|c: char| c.is_ascii_digit()))
    {
        TerminalKind::Plain
    } else {
        TerminalKind::Other
    }
}

impl TermCaps {
    /// The whole decision. `forced` is the user's own choice of profile.
    pub fn detect(forced: Option<Profile>, env: &Env) -> TermCaps {
        let get = |k: &str| (env.var)(k).unwrap_or_default();
        let has = |k: &str| !get(k).is_empty();
        let terminal = kind(env);
        let term = get("TERM");
        let multiplexer =
            has("TMUX") || has("STY") || term.starts_with("tmux") || term.starts_with("screen");
        let no_color = has("NO_COLOR");

        let (profile, why) = match forced {
            Some(p) => (p, "set by you (profile / RADA_PROFILE)".to_string()),
            None => match terminal {
                TerminalKind::Plain if term.is_empty() => {
                    (Profile::Basic, "TERM is not set".to_string())
                }
                TerminalKind::Plain => (Profile::Basic, format!("TERM={term} is a plain terminal")),
                TerminalKind::LinuxConsole => (
                    Profile::Basic,
                    "the Linux console has a small font and 16 colours".to_string(),
                ),
                _ => match non_utf8_locale(env.var).filter(|_| !env.windows) {
                    Some(locale) => (Profile::Basic, format!("the locale {locale} is not UTF-8")),
                    None => (
                        Profile::Modern,
                        format!("{} is a modern terminal", terminal.name()),
                    ),
                },
            },
        };

        if profile == Profile::Basic {
            return TermCaps {
                profile,
                why,
                terminal,
                multiplexer,
                depth: ColorDepth::Ansi16,
                no_color,
                unicode: false,
                mouse: false,
                osc52: false,
                may_query: false,
                images: ImagePlan::Off,
            };
        }

        // Without colour the selection has to be shown with reverse video, which is what
        // the 16-colour themes do.
        let depth = if no_color {
            ColorDepth::Ansi16
        } else {
            ColorDepth::from_env(env.var, env.windows)
        };
        let images = if no_color {
            ImagePlan::Off
        } else if multiplexer {
            // Graphics need passthrough to be switched on, and nothing says whether it is.
            ImagePlan::Halfblocks
        } else {
            match terminal {
                TerminalKind::Ghostty | TerminalKind::Kitty => {
                    ImagePlan::Protocol(ProtocolType::Kitty)
                }
                TerminalKind::WezTerm | TerminalKind::Iterm2 => {
                    ImagePlan::Protocol(ProtocolType::Iterm2)
                }
                TerminalKind::Foot | TerminalKind::WindowsTerminal | TerminalKind::Konsole => {
                    ImagePlan::Protocol(ProtocolType::Sixel)
                }
                // No graphics protocol: VTE keeps Sixel switched off, Terminal.app and
                // the Windows console have none.
                TerminalKind::Vte
                | TerminalKind::AppleTerminal
                | TerminalKind::WindowsConsole
                | TerminalKind::LinuxConsole
                | TerminalKind::Plain => ImagePlan::Halfblocks,
                TerminalKind::Other => ImagePlan::Ask,
            }
        };
        // Where the sequence is known to be ignored the system's own clipboard is used.
        let osc52 = !matches!(
            terminal,
            TerminalKind::AppleTerminal
                | TerminalKind::Vte
                | TerminalKind::Konsole
                | TerminalKind::WindowsConsole
                | TerminalKind::LinuxConsole
                | TerminalKind::Plain
        );
        TermCaps {
            profile,
            why,
            terminal,
            multiplexer,
            depth,
            no_color,
            unicode: true,
            mouse: true,
            osc52,
            may_query: !has("RADA_NO_TERM_QUERY"),
            images,
        }
    }

    /// With the real environment.
    pub fn detect_here(forced: Option<Profile>) -> TermCaps {
        // A program started from the desktop (a double click, the Run box) gets a console
        // of its own, which Windows hands over to Windows Terminal when that is the
        // default terminal. Nothing in the environment says so then: `WT_SESSION` is only
        // set for what Windows Terminal starts itself.
        let hosted = shown_in_windows_terminal();
        TermCaps::detect(
            forced,
            &Env {
                var: &|k| match std::env::var(k).ok() {
                    None if hosted && k == "WT_SESSION" => Some("handed-over".to_string()),
                    v => v,
                },
                windows: cfg!(windows),
            },
        )
    }

    /// What a modern terminal with everything gets: for tests and as a starting point.
    pub fn modern() -> TermCaps {
        TermCaps {
            profile: Profile::Modern,
            why: "assumed".to_string(),
            terminal: TerminalKind::Other,
            multiplexer: false,
            depth: ColorDepth::True,
            no_color: false,
            unicode: true,
            mouse: true,
            osc52: true,
            may_query: true,
            images: ImagePlan::Ask,
        }
    }

    /// How cells are adapted on their way to the terminal.
    pub fn rendering(&self) -> Rendering {
        Rendering {
            glyphs: if self.unicode {
                Glyphs::Full
            } else {
                Glyphs::Ascii
            },
            depth: self.depth,
            no_color: self.no_color,
        }
    }

    /// One line per capability, for `rada doctor` and the log.
    pub fn describe(&self) -> Vec<String> {
        let yes_no = |b: bool| if b { "yes" } else { "no" };
        vec![
            format!("profile: {} ({})", self.profile.name(), self.why),
            format!(
                "terminal: {}{}",
                self.terminal.name(),
                if self.multiplexer {
                    ", inside tmux or screen"
                } else {
                    ""
                }
            ),
            format!(
                "colours: {}",
                match (self.no_color, self.depth) {
                    (true, _) => "none (NO_COLOR)",
                    (false, ColorDepth::True) => "24-bit",
                    (false, ColorDepth::Ansi256) => "256",
                    (false, ColorDepth::Ansi16) => "16",
                }
            ),
            format!("glyphs: {}", if self.unicode { "Unicode" } else { "ASCII" }),
            format!(
                "pictures: {}",
                match self.images {
                    ImagePlan::Off => "off",
                    ImagePlan::Halfblocks => "half blocks",
                    ImagePlan::Protocol(ProtocolType::Kitty) => "kitty graphics",
                    ImagePlan::Protocol(ProtocolType::Sixel) => "Sixel",
                    ImagePlan::Protocol(ProtocolType::Iterm2) => "iTerm2 images",
                    ImagePlan::Protocol(ProtocolType::Halfblocks) => "half blocks",
                    ImagePlan::Ask => "the terminal is asked",
                }
            ),
            format!("mouse: {}", yes_no(self.mouse)),
            format!(
                "clipboard: {}",
                if self.osc52 {
                    "through the terminal (OSC 52)"
                } else {
                    "the system's own"
                }
            ),
            format!("terminal queries: {}", yes_no(self.may_query)),
        ]
    }
}

/// The class name of a window, or nothing for no window.
#[cfg(windows)]
fn window_class(window: windows_sys::Win32::Foundation::HWND) -> String {
    if window.is_null() {
        return String::new();
    }
    let mut name = [0u16; 64];
    // SAFETY: the buffer is on the stack and its length is passed along; a window that is
    // gone meanwhile makes the call fail, which leaves an empty name.
    let len = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW(
            window,
            name.as_mut_ptr(),
            name.len() as i32,
        )
    };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

/// Is this console shown in a Windows Terminal window? Told from the windows themselves,
/// for when the environment does not say: a console that a terminal shows has an invisible
/// stand-in window, and Windows Terminal makes that window belong to its own.
#[cfg(windows)]
pub fn shown_in_windows_terminal() -> bool {
    use windows_sys::Win32::System::Console::GetConsoleWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GA_ROOTOWNER, GW_OWNER, GetAncestor, GetParent, GetWindow,
    };
    const STAND_IN: &str = "PseudoConsoleWindow";
    const WINDOWS_TERMINAL: &str = "CASCADIA_HOSTING_WINDOW_CLASS";
    // SAFETY: these calls only read which window is related to which; a null or stale
    // handle makes them return null.
    unsafe {
        let console = GetConsoleWindow();
        if window_class(console) != STAND_IN {
            return false;
        }
        [
            GetParent(console),
            GetWindow(console, GW_OWNER),
            GetAncestor(console, GA_ROOTOWNER),
        ]
        .into_iter()
        .any(|w| window_class(w) == WINDOWS_TERMINAL)
    }
}

#[cfg(not(windows))]
pub fn shown_in_windows_terminal() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_on(vars: &[(&str, &str)], windows: bool, forced: Option<Profile>) -> TermCaps {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        TermCaps::detect(
            forced,
            &Env {
                var: &|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
                windows,
            },
        )
    }

    fn caps(vars: &[(&str, &str)]) -> TermCaps {
        caps_on(vars, false, None)
    }

    const UTF8: (&str, &str) = ("LANG", "it_IT.UTF-8");

    #[test]
    fn the_four_default_terminals_get_the_modern_profile() {
        let wt = caps_on(&[("WT_SESSION", "abc")], true, None);
        let apple = caps(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "Apple_Terminal"),
            UTF8,
        ]);
        let gnome = caps(&[("TERM", "xterm-256color"), ("VTE_VERSION", "7600"), UTF8]);
        let konsole = caps(&[
            ("TERM", "xterm-256color"),
            ("KONSOLE_VERSION", "240800"),
            ("COLORTERM", "truecolor"),
            UTF8,
        ]);
        for c in [&wt, &apple, &gnome, &konsole] {
            assert_eq!(c.profile, Profile::Modern, "{c:?}");
            assert!(c.unicode && c.mouse && c.may_query && !c.no_color, "{c:?}");
        }
        assert_eq!(
            (
                wt.terminal,
                apple.terminal,
                gnome.terminal,
                konsole.terminal
            ),
            (
                TerminalKind::WindowsTerminal,
                TerminalKind::AppleTerminal,
                TerminalKind::Vte,
                TerminalKind::Konsole
            )
        );
        // Colours: 24-bit everywhere but on a Terminal.app that does not announce it.
        assert_eq!(wt.depth, ColorDepth::True);
        assert_eq!(gnome.depth, ColorDepth::True);
        assert_eq!(konsole.depth, ColorDepth::True);
        assert_eq!(apple.depth, ColorDepth::Ansi256);
        // Pictures: Sixel where it exists, half blocks where it does not.
        assert_eq!(wt.images, ImagePlan::Protocol(ProtocolType::Sixel));
        assert_eq!(konsole.images, ImagePlan::Protocol(ProtocolType::Sixel));
        assert_eq!(gnome.images, ImagePlan::Halfblocks);
        assert_eq!(apple.images, ImagePlan::Halfblocks);
        // The clipboard sequence: only Windows Terminal is known to take it.
        assert!(wt.osc52);
        assert!(!apple.osc52 && !gnome.osc52 && !konsole.osc52);
    }

    #[test]
    fn a_terminal_app_that_announces_true_colour_gets_it() {
        let c = caps(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("COLORTERM", "truecolor"),
        ]);
        assert_eq!(c.depth, ColorDepth::True);
    }

    #[test]
    fn plain_terminals_get_the_basic_profile() {
        for term in ["", "dumb", "vt100", "vt220", "ansi", "linux"] {
            let c = caps(&[("TERM", term), UTF8]);
            assert_eq!(c.profile, Profile::Basic, "TERM={term}");
            assert_eq!(c.depth, ColorDepth::Ansi16);
            assert!(!c.unicode && !c.mouse && !c.osc52 && !c.may_query);
            assert_eq!(c.images, ImagePlan::Off);
            assert_eq!(c.rendering(), Rendering::BASIC);
        }
        // Not everything that starts with "vt" is a hardware terminal.
        assert_eq!(
            caps(&[("TERM", "vte-256color"), UTF8]).profile,
            Profile::Modern
        );
    }

    #[test]
    fn a_locale_that_is_not_utf8_gets_the_basic_profile() {
        let latin = caps(&[("TERM", "xterm-256color"), ("LANG", "it_IT.ISO-8859-1")]);
        assert_eq!(latin.profile, Profile::Basic);
        assert!(latin.why.contains("ISO-8859-1"), "{}", latin.why);
        // The first of LC_ALL, LC_CTYPE, LANG that is set decides.
        let over = caps(&[
            ("TERM", "xterm-256color"),
            ("LANG", "it_IT.ISO-8859-1"),
            ("LC_ALL", "en_US.utf8"),
        ]);
        assert_eq!(over.profile, Profile::Modern);
        // No locale, or C, says nothing: the terminal is taken at its word.
        assert_eq!(caps(&[("TERM", "xterm-256color")]).profile, Profile::Modern);
        assert_eq!(
            caps(&[("TERM", "xterm-256color"), ("LANG", "C")]).profile,
            Profile::Modern
        );
        assert_eq!(
            caps(&[("TERM", "xterm"), ("LANG", "en_US.UTF-8@euro")]).profile,
            Profile::Modern
        );
    }

    #[test]
    fn the_windows_console_without_term_is_modern() {
        let c = caps_on(&[], true, None);
        assert_eq!(c.terminal, TerminalKind::WindowsConsole);
        assert_eq!(c.profile, Profile::Modern);
        assert_eq!(c.depth, ColorDepth::True);
        assert_eq!(c.images, ImagePlan::Halfblocks);
        assert!(!c.osc52);
    }

    #[test]
    fn the_users_choice_wins() {
        let basic = caps_on(&[("WT_SESSION", "abc")], true, Some(Profile::Basic));
        assert_eq!(basic.profile, Profile::Basic);
        assert_eq!(basic.rendering(), Rendering::BASIC);
        let modern = caps_on(&[("TERM", "vt100")], false, Some(Profile::Modern));
        assert_eq!(modern.profile, Profile::Modern);
        assert!(modern.unicode && modern.mouse);
        assert_eq!(Profile::parse("auto"), Some(None));
        assert_eq!(Profile::parse("Basic"), Some(Some(Profile::Basic)));
        assert_eq!(Profile::parse("modern"), Some(Some(Profile::Modern)));
        assert_eq!(Profile::parse("fancy"), None);
    }

    #[test]
    fn no_color_removes_colour_and_pictures_in_either_profile() {
        let c = caps(&[("TERM", "xterm-256color"), ("NO_COLOR", "1"), UTF8]);
        assert_eq!(c.profile, Profile::Modern);
        assert!(c.no_color && c.unicode);
        assert_eq!(c.depth, ColorDepth::Ansi16);
        assert_eq!(c.images, ImagePlan::Off);
        assert!(c.rendering().no_color && c.rendering().glyphs == Glyphs::Full);
        let b = caps(&[("TERM", "vt100"), ("NO_COLOR", "1")]);
        assert!(b.rendering().no_color && b.rendering().glyphs == Glyphs::Ascii);
    }

    #[test]
    fn a_terminal_that_names_itself_is_believed_over_inherited_variables() {
        // kitty started from a Konsole window still carries KONSOLE_VERSION.
        let c = caps(&[("TERM", "xterm-kitty"), ("KONSOLE_VERSION", "240800"), UTF8]);
        assert_eq!(c.terminal, TerminalKind::Kitty);
        assert_eq!(c.images, ImagePlan::Protocol(ProtocolType::Kitty));
    }

    #[test]
    fn inside_tmux_pictures_are_half_blocks_and_unknown_terminals_are_asked() {
        let tmux = caps(&[
            ("TERM", "tmux-256color"),
            ("TMUX", "/tmp/tmux-1000/default,1,0"),
            ("WT_SESSION", "abc"),
            UTF8,
        ]);
        assert!(tmux.multiplexer);
        assert_eq!(tmux.images, ImagePlan::Halfblocks);
        let other = caps(&[("TERM", "xterm-256color"), UTF8]);
        assert_eq!(other.terminal, TerminalKind::Other);
        assert_eq!(other.images, ImagePlan::Ask);
        assert!(other.osc52);
        assert!(!caps(&[("TERM", "xterm"), ("RADA_NO_TERM_QUERY", "1")]).may_query);
    }

    #[test]
    fn every_capability_is_described() {
        let lines = TermCaps::modern().describe();
        for word in [
            "profile",
            "terminal",
            "colours",
            "glyphs",
            "pictures",
            "mouse",
            "clipboard",
        ] {
            assert!(lines.iter().any(|l| l.starts_with(word)), "{word}");
        }
    }
}
