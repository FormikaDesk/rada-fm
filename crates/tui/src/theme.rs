//! Themes. Every colour in the interface comes from a token defined here; nothing else
//! in the crate writes a colour by hand.
//!
//! A theme is true-colour by design and is adapted to what the terminal can show:
//! 24-bit colour, the 256-colour palette, or the 16 ANSI colours.

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorDepth {
    True,
    Ansi256,
    Ansi16,
}

impl ColorDepth {
    pub fn detect() -> ColorDepth {
        let ct = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_lowercase();
        let term = std::env::var("TERM").unwrap_or_default();
        if ct.contains("truecolor")
            || ct.contains("24bit")
            || std::env::var_os("RADA_TRUECOLOR").is_some()
            || term.contains("ghostty")
            || term.contains("kitty")
        {
            ColorDepth::True
        } else if term.contains("256color") {
            ColorDepth::Ansi256
        } else {
            ColorDepth::Ansi16
        }
    }
}

/// How much room the list gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Airy,
    Balanced,
    Dense,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStyle {
    /// Lower-half blocks: `▄▄▄▖`
    Half,
    /// A thin line: `━━━╸`
    Thin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeStyle {
    /// Coloured text on a tinted background.
    Filled,
    /// Coloured dot and text, no background.
    Plain,
}

/// Colour of each kind of file, for icons and type badges.
#[derive(Clone, Debug)]
pub struct KindColors {
    pub code: Color,
    pub markup: Color,
    pub text: Color,
    pub doc: Color,
    pub image: Color,
    pub vector: Color,
    pub audio: Color,
    pub video: Color,
    pub archive: Color,
    pub data: Color,
    pub config: Color,
    pub binary: Color,
    pub other: Color,
    pub folder: Color,
    pub link: Color,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    /// `None` leaves the terminal's own background (and its transparency) alone.
    pub bg: Option<Color>,
    /// Main text.
    pub text: Color,
    /// Secondary text: sizes, dates, hints.
    pub text_dim: Color,
    /// Tertiary: separators, line numbers, empty tracks.
    pub muted: Color,
    /// The one vivid colour: selection, the plan window, progress.
    pub accent: Color,
    /// Text drawn on top of the accent colour.
    pub on_accent: Color,
    /// Row under the cursor.
    pub selection: Color,
    /// Marked rows.
    pub mark: Color,
    /// The empty part of a size bar or progress bar.
    pub track: Color,
    pub success: Color,
    pub warn: Color,
    pub error: Color,
    pub kinds: KindColors,
    pub density: Density,
    pub bar: BarStyle,
    pub badge: BadgeStyle,
    pub depth: ColorDepth,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

impl Theme {
    pub const NAMES: [&'static str; 3] = ["rada", "catppuccin", "tokyo-night"];

    /// A theme by name, adapted to the terminal's colour depth.
    pub fn named(name: &str, depth: ColorDepth) -> Option<Theme> {
        let t = match name.to_ascii_lowercase().as_str() {
            "rada" | "default" => Theme::rada(),
            "catppuccin" | "catppuccin-mocha" | "mocha" => Theme::catppuccin(),
            "tokyo-night" | "tokyonight" | "tokyo" => Theme::tokyo_night(),
            _ => return None,
        };
        Some(t.with_depth(depth))
    }

    pub fn detect() -> Theme {
        Theme::named("rada", ColorDepth::detect()).expect("built-in theme")
    }

    /// Base palette of the rada themes (cool neutrals, soft category colours).
    fn rada_base(name: &'static str, accent: Color, selection: Color, mark: Color) -> Theme {
        Theme {
            name,
            bg: None,
            text: rgb(214, 220, 240),
            text_dim: rgb(138, 147, 175),
            muted: rgb(78, 85, 108),
            accent,
            on_accent: rgb(12, 14, 24),
            selection,
            mark,
            track: rgb(36, 42, 62),
            success: rgb(120, 220, 160),
            warn: rgb(240, 200, 110),
            error: rgb(255, 120, 135),
            kinds: KindColors {
                code: rgb(255, 196, 82),
                markup: rgb(190, 130, 250),
                text: rgb(170, 180, 205),
                doc: rgb(240, 130, 110),
                image: rgb(180, 120, 255),
                vector: rgb(70, 190, 255),
                audio: rgb(250, 140, 200),
                video: rgb(250, 120, 160),
                archive: rgb(235, 185, 100),
                data: rgb(110, 210, 190),
                config: rgb(150, 165, 200),
                binary: rgb(130, 220, 140),
                other: rgb(150, 158, 185),
                folder: rgb(110, 165, 255),
                link: rgb(110, 220, 220),
            },
            density: Density::Airy,
            bar: BarStyle::Thin,
            badge: BadgeStyle::Plain,
            depth: ColorDepth::True,
        }
    }

    /// The default: an azure accent, an airy layout, thin size bars, coloured dots for types.
    pub fn rada() -> Theme {
        Theme::rada_base("rada", rgb(86, 156, 255), rgb(36, 52, 86), rgb(30, 40, 66))
    }

    pub fn catppuccin() -> Theme {
        // Catppuccin Mocha.
        let mut t = Theme::rada_base(
            "catppuccin",
            rgb(137, 180, 250),
            rgb(49, 50, 68),
            rgb(40, 42, 58),
        );
        t.text = rgb(205, 214, 244);
        t.text_dim = rgb(147, 153, 178);
        t.muted = rgb(88, 91, 112);
        t.on_accent = rgb(17, 17, 27);
        t.success = rgb(166, 227, 161);
        t.warn = rgb(249, 226, 175);
        t.error = rgb(243, 139, 168);
        t.kinds = KindColors {
            code: rgb(249, 226, 175),
            markup: rgb(203, 166, 247),
            text: rgb(186, 194, 222),
            doc: rgb(250, 179, 135),
            image: rgb(245, 194, 231),
            vector: rgb(116, 199, 236),
            audio: rgb(242, 205, 205),
            video: rgb(235, 160, 172),
            archive: rgb(249, 226, 175),
            data: rgb(148, 226, 213),
            config: rgb(147, 153, 178),
            binary: rgb(166, 227, 161),
            other: rgb(147, 153, 178),
            folder: rgb(137, 180, 250),
            link: rgb(148, 226, 213),
        };
        t
    }

    pub fn tokyo_night() -> Theme {
        let mut t = Theme::rada_base(
            "tokyo-night",
            rgb(122, 162, 247),
            rgb(41, 46, 66),
            rgb(34, 38, 56),
        );
        t.text = rgb(192, 202, 245);
        t.text_dim = rgb(130, 139, 184);
        t.muted = rgb(68, 75, 106);
        t.on_accent = rgb(22, 22, 30);
        t.success = rgb(158, 206, 106);
        t.warn = rgb(224, 175, 104);
        t.error = rgb(247, 118, 142);
        t.kinds = KindColors {
            code: rgb(224, 175, 104),
            markup: rgb(187, 154, 247),
            text: rgb(169, 177, 214),
            doc: rgb(255, 158, 100),
            image: rgb(187, 154, 247),
            vector: rgb(125, 207, 255),
            audio: rgb(255, 117, 127),
            video: rgb(247, 118, 142),
            archive: rgb(224, 175, 104),
            data: rgb(115, 218, 202),
            config: rgb(130, 139, 184),
            binary: rgb(158, 206, 106),
            other: rgb(130, 139, 184),
            folder: rgb(122, 162, 247),
            link: rgb(125, 207, 255),
        };
        t
    }

    /// Plain 16-colour theme for terminals that offer nothing more.
    fn ansi16(mut self) -> Theme {
        self.text = Color::Reset;
        self.text_dim = Color::Gray;
        self.muted = Color::DarkGray;
        self.accent = Color::Blue;
        self.on_accent = Color::Black;
        self.selection = Color::DarkGray;
        self.mark = Color::DarkGray;
        self.track = Color::DarkGray;
        self.success = Color::Green;
        self.warn = Color::Yellow;
        self.error = Color::Red;
        self.kinds = KindColors {
            code: Color::Yellow,
            markup: Color::Magenta,
            text: Color::Gray,
            doc: Color::Red,
            image: Color::Magenta,
            vector: Color::Cyan,
            audio: Color::Magenta,
            video: Color::Red,
            archive: Color::Yellow,
            data: Color::Cyan,
            config: Color::Gray,
            binary: Color::Green,
            other: Color::Gray,
            folder: Color::Blue,
            link: Color::Cyan,
        };
        self.depth = ColorDepth::Ansi16;
        self
    }

    /// Adapt every colour to what the terminal can display.
    pub fn with_depth(self, depth: ColorDepth) -> Theme {
        match depth {
            ColorDepth::True => self,
            ColorDepth::Ansi16 => self.ansi16(),
            ColorDepth::Ansi256 => {
                let mut t = self;
                let f = to_256;
                t.text = f(t.text);
                t.text_dim = f(t.text_dim);
                t.muted = f(t.muted);
                t.accent = f(t.accent);
                t.on_accent = f(t.on_accent);
                t.selection = f(t.selection);
                t.mark = f(t.mark);
                t.track = f(t.track);
                t.success = f(t.success);
                t.warn = f(t.warn);
                t.error = f(t.error);
                t.bg = t.bg.map(f);
                let k = &mut t.kinds;
                for c in [
                    &mut k.code,
                    &mut k.markup,
                    &mut k.text,
                    &mut k.doc,
                    &mut k.image,
                    &mut k.vector,
                    &mut k.audio,
                    &mut k.video,
                    &mut k.archive,
                    &mut k.data,
                    &mut k.config,
                    &mut k.binary,
                    &mut k.other,
                    &mut k.folder,
                    &mut k.link,
                ] {
                    *c = f(*c);
                }
                t.depth = ColorDepth::Ansi256;
                t
            }
        }
    }

    // ------------------------------------------------------------------ style helpers

    pub fn base(&self) -> Style {
        self.on(Style::default().fg(self.text))
    }

    pub fn dim(&self) -> Style {
        self.on(Style::default().fg(self.text_dim))
    }

    pub fn faint(&self) -> Style {
        self.on(Style::default().fg(self.muted))
    }

    pub fn fg(&self, c: Color) -> Style {
        self.on(Style::default().fg(c))
    }

    pub fn accent_style(&self) -> Style {
        self.on(Style::default().fg(self.accent))
    }

    pub fn title(&self) -> Style {
        self.on(Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD))
    }

    /// A key name in a hint.
    pub fn key(&self) -> Style {
        self.on(Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD))
    }

    /// Text on the accent colour (buttons, the plan window's operation pill).
    pub fn pill(&self) -> Style {
        Style::default()
            .fg(self.on_accent)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// The row under the cursor.
    pub fn selected(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.selection)
        }
    }

    pub fn marked(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(self.mark)
        }
    }

    /// Apply the theme's own background, if it has one.
    pub fn on(&self, s: Style) -> Style {
        match self.bg {
            Some(bg) => s.bg(bg),
            None => s,
        }
    }

    /// Every colour the interface may put on screen with this theme: the tokens, and the
    /// tints derived from them. Tests use it to prove nothing is hand-coloured.
    pub fn all_colors(&self) -> Vec<Color> {
        let k = &self.kinds;
        let mut v = vec![
            self.text,
            self.text_dim,
            self.muted,
            self.accent,
            self.on_accent,
            self.selection,
            self.mark,
            self.track,
            self.success,
            self.warn,
            self.error,
            k.code,
            k.markup,
            k.text,
            k.doc,
            k.image,
            k.vector,
            k.audio,
            k.video,
            k.archive,
            k.data,
            k.config,
            k.binary,
            k.other,
            k.folder,
            k.link,
        ];
        v.extend(self.bg);
        for c in [self.accent, self.success, self.warn, self.error] {
            v.push(self.tint(c));
        }
        v
    }

    /// A tinted background for a badge of colour `c`.
    pub fn tint(&self, c: Color) -> Color {
        match (self.depth, c) {
            (ColorDepth::True, Color::Rgb(r, g, b)) => {
                // 22% of the colour over a dark base.
                let mix = |v: u8, base: u8| (v as f32 * 0.22 + base as f32 * 0.78) as u8;
                rgb(mix(r, 20), mix(g, 22), mix(b, 32))
            }
            (ColorDepth::Ansi256, Color::Indexed(_)) => self.mark,
            _ => Color::Reset,
        }
    }
}

// ---------------------------------------------------------------------------- colour maths

/// Nearest entry of the xterm 256-colour palette (6x6x6 cube plus the grey ramp).
fn to_256(c: Color) -> Color {
    let Color::Rgb(r, g, b) = c else { return c };
    let level = |v: u8| -> (u8, u8) {
        const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let (i, s) = STEPS
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| (v as i32 - **s as i32).abs())
            .unwrap();
        (i as u8, *s)
    };
    let (ri, rs) = level(r);
    let (gi, gs) = level(g);
    let (bi, bs) = level(b);
    let cube = 16 + 36 * ri + 6 * gi + bi;
    let dist = |a: (u8, u8, u8)| -> i32 {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    let cube_d = dist((rs, gs, bs));
    // Grey ramp 232..=255: 8, 18, ... 238.
    let avg = ((r as u32 + g as u32 + b as u32) / 3) as i32;
    let gi = ((avg - 8 + 5) / 10).clamp(0, 23) as u8;
    let gv = 8 + 10 * gi;
    let grey_d = dist((gv, gv, gv));
    Color::Indexed(if grey_d < cube_d { 232 + gi } else { cube })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_resolves_at_every_depth() {
        for n in Theme::NAMES {
            for d in [ColorDepth::True, ColorDepth::Ansi256, ColorDepth::Ansi16] {
                let t = Theme::named(n, d).unwrap_or_else(|| panic!("{n}"));
                assert_eq!(t.depth, d);
            }
        }
        assert!(Theme::named("nope", ColorDepth::True).is_none());
    }

    #[test]
    fn indexed_themes_contain_no_rgb_colours() {
        let t = Theme::named("tokyo-night", ColorDepth::Ansi256).unwrap();
        let all = [
            t.text,
            t.text_dim,
            t.muted,
            t.accent,
            t.on_accent,
            t.selection,
            t.mark,
            t.success,
            t.warn,
            t.error,
            t.kinds.code,
            t.kinds.image,
            t.kinds.folder,
            t.kinds.link,
        ];
        assert!(
            all.iter().all(|c| matches!(c, Color::Indexed(_))),
            "{all:?}"
        );
        let t16 = Theme::named("catppuccin", ColorDepth::Ansi16).unwrap();
        assert!(!matches!(t16.accent, Color::Rgb(..)) && !matches!(t16.kinds.code, Color::Rgb(..)));
    }

    #[test]
    fn nearest_256_colour() {
        assert_eq!(to_256(rgb(0, 0, 0)), Color::Indexed(16));
        assert_eq!(to_256(rgb(255, 255, 255)), Color::Indexed(231));
        assert_eq!(to_256(rgb(255, 0, 0)), Color::Indexed(196));
        // A mid grey prefers the grey ramp.
        assert!(matches!(to_256(rgb(128, 128, 128)), Color::Indexed(n) if n >= 232));
    }
}
