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
    /// Designed for a light terminal background.
    pub light: bool,
    /// The background colour the tokens were chosen against (and tints are mixed
    /// with). The terminal's own background is still what is drawn.
    pub canvas: Color,
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
    /// Highlighted things that are not the cursor: buttons, key caps, the current place.
    pub selection: Color,
    /// The row under the cursor: stronger than `selection`, so that it stands out from
    /// marked rows and from the chips around it.
    pub cursor: Color,
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

/// A named look, with a dark and a light variant.
#[derive(Clone, Copy)]
enum Family {
    Rada,
    Catppuccin,
    TokyoNight,
}

impl Family {
    fn theme(self, light: bool) -> Theme {
        match (self, light) {
            (Family::Rada, false) => Theme::rada(),
            (Family::Rada, true) => Theme::rada_light(),
            (Family::Catppuccin, false) => Theme::catppuccin(),
            (Family::Catppuccin, true) => Theme::catppuccin_latte(),
            (Family::TokyoNight, false) => Theme::tokyo_night(),
            (Family::TokyoNight, true) => Theme::tokyo_night_day(),
        }
    }
}

impl Theme {
    /// Every built-in theme: three for dark terminals, then their light counterparts.
    pub const NAMES: [&'static str; 6] = [
        "rada",
        "catppuccin",
        "tokyo-night",
        "rada-light",
        "catppuccin-latte",
        "tokyo-night-day",
    ];

    /// The family a name belongs to and whether the name asks for the light variant.
    fn family(name: &str) -> Option<(Family, bool)> {
        Some(match name.to_ascii_lowercase().as_str() {
            "rada" | "default" => (Family::Rada, false),
            "rada-light" | "rada-day" => (Family::Rada, true),
            "catppuccin" | "catppuccin-mocha" | "mocha" => (Family::Catppuccin, false),
            "catppuccin-latte" | "latte" => (Family::Catppuccin, true),
            "tokyo-night" | "tokyonight" | "tokyo" => (Family::TokyoNight, false),
            "tokyo-night-day" | "tokyo-day" | "day" => (Family::TokyoNight, true),
            _ => return None,
        })
    }

    /// A theme by name, adapted to the terminal's colour depth.
    pub fn named(name: &str, depth: ColorDepth) -> Option<Theme> {
        let (family, light) = Theme::family(name)?;
        Some(family.theme(light).with_depth(depth))
    }

    /// The theme `name` belongs to, in the variant for a light or a dark background:
    /// `variant("rada-light", false, ..)` is the dark `rada`.
    pub fn variant(name: &str, light: bool, depth: ColorDepth) -> Option<Theme> {
        let (family, _) = Theme::family(name)?;
        Some(family.theme(light).with_depth(depth))
    }

    pub fn detect() -> Theme {
        Theme::named("rada", ColorDepth::detect()).expect("built-in theme")
    }

    /// Base palette of the rada themes (cool neutrals, soft category colours).
    fn rada_base(name: &'static str, accent: Color, selection: Color, mark: Color) -> Theme {
        Theme {
            name,
            light: false,
            canvas: rgb(26, 27, 38),
            bg: None,
            text: rgb(214, 220, 240),
            text_dim: rgb(138, 147, 175),
            muted: rgb(78, 85, 108),
            accent,
            on_accent: rgb(12, 14, 24),
            selection,
            cursor: rgb(48, 72, 124),
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
        t.cursor = rgb(66, 72, 108);
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
        t.cursor = rgb(56, 66, 108);
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

    /// Light themes share the structure of the dark ones; every token is chosen to read on
    /// its `canvas` (see the contrast tests).
    #[allow(clippy::too_many_arguments)]
    fn light_base(
        name: &'static str,
        canvas: Color,
        text: Color,
        text_dim: Color,
        muted: Color,
        accent: Color,
        selection: Color,
        cursor: Color,
        mark: Color,
        track: Color,
    ) -> Theme {
        let mut t = Theme::rada_base(name, accent, selection, mark);
        t.cursor = cursor;
        t.light = true;
        t.canvas = canvas;
        t.text = text;
        t.text_dim = text_dim;
        t.muted = muted;
        t.on_accent = rgb(255, 255, 255);
        t.track = track;
        t
    }

    /// The default theme for a light terminal: the azure accent, deepened to read on white.
    pub fn rada_light() -> Theme {
        let mut t = Theme::light_base(
            "rada-light",
            rgb(255, 255, 255),
            rgb(28, 34, 52),
            rgb(84, 94, 122),
            rgb(150, 158, 178),
            rgb(35, 101, 225),
            rgb(214, 228, 255),
            rgb(186, 210, 252),
            rgb(232, 240, 254),
            rgb(223, 228, 238),
        );
        t.success = rgb(26, 129, 77);
        t.warn = rgb(157, 102, 8);
        t.error = rgb(209, 48, 68);
        t.kinds = KindColors {
            code: rgb(150, 104, 13),
            markup: rgb(137, 82, 210),
            text: rgb(103, 113, 141),
            doc: rgb(187, 75, 50),
            image: rgb(137, 82, 214),
            vector: rgb(17, 120, 180),
            audio: rgb(195, 62, 133),
            video: rgb(205, 55, 91),
            archive: rgb(153, 102, 24),
            data: rgb(16, 125, 106),
            config: rgb(100, 114, 146),
            binary: rgb(40, 129, 57),
            other: rgb(105, 111, 137),
            folder: rgb(35, 101, 225),
            link: rgb(17, 125, 133),
        };
        t
    }

    /// Catppuccin Latte, with the bright accents deepened where the original is too pale
    /// to read as text.
    pub fn catppuccin_latte() -> Theme {
        let mut t = Theme::light_base(
            "catppuccin-latte",
            rgb(239, 241, 245),
            rgb(62, 64, 88),
            rgb(95, 99, 121),
            rgb(146, 149, 167),
            rgb(27, 93, 224),
            rgb(212, 219, 238),
            rgb(190, 202, 232),
            rgb(224, 229, 242),
            rgb(214, 218, 229),
        );
        t.success = rgb(47, 118, 32);
        t.warn = rgb(146, 93, 19);
        t.error = rgb(210, 15, 57);
        t.kinds = KindColors {
            code: rgb(137, 95, 12),
            markup: rgb(129, 77, 198),
            text: rgb(94, 103, 129),
            doc: rgb(176, 71, 47),
            image: rgb(129, 77, 202),
            vector: rgb(16, 110, 165),
            audio: rgb(178, 57, 121),
            video: rgb(187, 50, 83),
            archive: rgb(139, 93, 21),
            data: rgb(15, 118, 100),
            config: rgb(92, 104, 133),
            binary: rgb(37, 118, 52),
            other: rgb(99, 105, 129),
            folder: rgb(27, 93, 224),
            link: rgb(15, 114, 122),
        };
        t
    }

    /// Tokyo Night Day, with the text and accents deepened for contrast.
    pub fn tokyo_night_day() -> Theme {
        let mut t = Theme::light_base(
            "tokyo-night-day",
            rgb(225, 226, 231),
            rgb(52, 59, 88),
            rgb(83, 92, 132),
            rgb(130, 137, 167),
            rgb(34, 92, 172),
            rgb(200, 208, 236),
            rgb(176, 190, 230),
            rgb(212, 217, 238),
            rgb(208, 211, 222),
        );
        t.success = rgb(78, 104, 50);
        t.warn = rgb(117, 90, 52);
        t.error = rgb(181, 31, 74);
        t.kinds = KindColors {
            code: rgb(125, 87, 11),
            markup: rgb(118, 71, 180),
            text: rgb(86, 94, 118),
            doc: rgb(161, 64, 43),
            image: rgb(118, 71, 184),
            vector: rgb(14, 100, 150),
            audio: rgb(162, 52, 111),
            video: rgb(176, 47, 78),
            archive: rgb(131, 87, 20),
            data: rgb(13, 108, 91),
            config: rgb(84, 95, 122),
            binary: rgb(34, 108, 47),
            other: rgb(90, 96, 118),
            folder: rgb(34, 92, 172),
            link: rgb(14, 107, 114),
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
        self.cursor = Color::DarkGray;
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
        if self.light {
            // Pale greys vanish on a light background.
            self.text_dim = Color::DarkGray;
            self.muted = Color::DarkGray;
            self.kinds.text = Color::DarkGray;
            self.kinds.config = Color::DarkGray;
            self.kinds.other = Color::DarkGray;
        }
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
                t.cursor = f(t.cursor);
                t.mark = f(t.mark);
                t.track = f(t.track);
                t.success = f(t.success);
                t.warn = f(t.warn);
                t.error = f(t.error);
                t.bg = t.bg.map(f);
                t.canvas = f(t.canvas);
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
            Style::default().bg(self.cursor)
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
            self.cursor,
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
            (ColorDepth::True, Color::Rgb(r, g, b)) if self.light => {
                // 14% of the colour over the light canvas.
                let Color::Rgb(cr, cg, cb) = self.canvas else {
                    return self.mark;
                };
                let mix = |v: u8, base: u8| (v as f32 * 0.14 + base as f32 * 0.86) as u8;
                rgb(mix(r, cr), mix(g, cg), mix(b, cb))
            }
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
            t.cursor,
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

    // ------------------------------------------------------------------ contrast

    fn channels(c: Color) -> (u8, u8, u8) {
        match c {
            Color::Rgb(r, g, b) => (r, g, b),
            other => panic!("not a true colour: {other:?}"),
        }
    }

    /// Relative luminance (WCAG 2.x).
    fn luminance(c: Color) -> f64 {
        let (r, g, b) = channels(c);
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    /// WCAG contrast ratio, 1.0 (identical) to 21.0 (black on white).
    fn contrast(a: Color, b: Color) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_theme_meets_the_contrast_minimums() {
        // Text that carries information: 7:1 for the main text, 4.5:1 for everything
        // else that is read (secondary text, accents, file kinds, statuses); decorative
        // tokens (separators, empty bars, the cursor row) only have to be visible.
        let mut failures = Vec::new();
        for n in Theme::NAMES {
            let t = Theme::named(n, ColorDepth::True).unwrap();
            let c = t.canvas;
            let mut need = |what: &str, ratio: f64, min: f64| {
                if ratio < min {
                    failures.push(format!("{n}: {what} is {ratio:.2}:1, needs {min}:1"));
                }
            };
            need("text on canvas", contrast(t.text, c), 7.0);
            need("text_dim on canvas", contrast(t.text_dim, c), 4.5);
            need("muted on canvas", contrast(t.muted, c), 1.9);
            need("accent on canvas", contrast(t.accent, c), 4.5);
            need("on_accent on accent", contrast(t.on_accent, t.accent), 4.5);
            need("selection against canvas", contrast(t.selection, c), 1.15);
            need("mark against canvas", contrast(t.mark, c), 1.05);
            need("track against canvas", contrast(t.track, c), 1.1);
            need("cursor against canvas", contrast(t.cursor, c), 1.4);
            need(
                "cursor against selection",
                contrast(t.cursor, t.selection),
                1.15,
            );
            need("cursor against mark", contrast(t.cursor, t.mark), 1.2);
            // The cursor row is bold, which carries the text; its secondary text is
            // drawn in the main colour (see the list).
            need("text on cursor", contrast(t.text, t.cursor), 5.5);
            need("text on selection", contrast(t.text, t.selection), 7.0);
            need(
                "text_dim on selection",
                contrast(t.text_dim, t.selection),
                3.5,
            );
            need("text on mark", contrast(t.text, t.mark), 7.0);
            for (what, col) in [("success", t.success), ("warn", t.warn), ("error", t.error)] {
                need(&format!("{what} on canvas"), contrast(col, c), 4.5);
            }
            let k = &t.kinds;
            for (what, col) in [
                ("code", k.code),
                ("markup", k.markup),
                ("text", k.text),
                ("doc", k.doc),
                ("image", k.image),
                ("vector", k.vector),
                ("audio", k.audio),
                ("video", k.video),
                ("archive", k.archive),
                ("data", k.data),
                ("config", k.config),
                ("binary", k.binary),
                ("other", k.other),
                ("folder", k.folder),
                ("link", k.link),
            ] {
                need(&format!("kind {what} on canvas"), contrast(col, c), 4.5);
                // The pale tinted pill behind a type label must not swallow the label.
                need(
                    &format!("kind {what} on its badge tint"),
                    contrast(col, t.tint(col)),
                    3.0,
                );
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn light_themes_have_a_light_canvas_and_dark_ones_a_dark_canvas() {
        for n in Theme::NAMES {
            let t = Theme::named(n, ColorDepth::True).unwrap();
            let l = luminance(t.canvas);
            assert_eq!(
                t.light,
                n.ends_with("-light") || n.ends_with("latte") || n.ends_with("day")
            );
            assert!(
                if t.light { l > 0.6 } else { l < 0.05 },
                "{n}: canvas luminance {l}"
            );
            // Text goes the other way round.
            assert_eq!(luminance(t.text) < luminance(t.canvas), t.light, "{n}");
        }
    }

    #[test]
    fn a_theme_has_a_variant_for_each_brightness() {
        let d = ColorDepth::True;
        for (dark, light) in [
            ("rada", "rada-light"),
            ("catppuccin", "catppuccin-latte"),
            ("tokyo-night", "tokyo-night-day"),
        ] {
            for name in [dark, light] {
                assert_eq!(Theme::variant(name, false, d).unwrap().name, dark);
                assert_eq!(Theme::variant(name, true, d).unwrap().name, light);
            }
        }
        assert!(Theme::variant("nope", true, d).is_none());
        assert_eq!(Theme::named("latte", d).unwrap().name, "catppuccin-latte");
    }

    #[test]
    fn light_tints_are_pale_and_the_16_colour_light_theme_avoids_pale_greys() {
        let t = Theme::named("rada-light", ColorDepth::True).unwrap();
        assert!(luminance(t.tint(t.accent)) > 0.6);
        let t16 = Theme::named("rada-light", ColorDepth::Ansi16).unwrap();
        assert_eq!(t16.text_dim, Color::DarkGray);
        assert_ne!(t16.kinds.text, Color::Gray);
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
