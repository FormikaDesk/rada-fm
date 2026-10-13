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
        ColorDepth::from_env(&|k| std::env::var(k).ok(), cfg!(windows))
    }

    /// What the environment says about colours. `windows` is whether this is a Windows
    /// console, where `TERM` and `COLORTERM` are normally not set at all.
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>, windows: bool) -> ColorDepth {
        let get = |k: &str| var(k).unwrap_or_default();
        let ct = get("COLORTERM").to_lowercase();
        let term = get("TERM");
        if ct.contains("truecolor")
            || ct.contains("24bit")
            || var("RADA_TRUECOLOR").is_some()
            || term.contains("ghostty")
            || term.contains("kitty")
            // Windows Terminal, GNOME Terminal (and every VTE terminal) and Konsole all
            // draw 24-bit colour, whatever TERM says.
            || !get("WT_SESSION").is_empty()
            || !get("VTE_VERSION").is_empty()
            || !get("KONSOLE_VERSION").is_empty()
        {
            ColorDepth::True
        } else if term.contains("256color") {
            ColorDepth::Ansi256
        } else if windows && term.is_empty() {
            // The Windows console host has drawn 24-bit colour since Windows 10 1703.
            ColorDepth::True
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
    /// Selected rows, and the current place of the navigation pane: a quiet tone.
    pub sel: Color,
    /// Text on the row under the cursor (and on the cursor tile).
    pub cursor_text: Color,
    /// Background of the command bar.
    pub cmdbar: Color,
    /// Background of the strip of key hints.
    pub panel: Color,
    /// The pill behind a key in the hints, and the text on it.
    pub key_bg: Color,
    pub key_text: Color,
    /// Background of the address and search fields.
    pub field: Color,
    /// Background of the command buttons.
    pub button: Color,
    /// The row under the cursor shades from `grad_row_start` on the left to `grad_row_end` on
    /// the right (only on a true-colour terminal: below that the solid `cursor` is used).
    /// The text on it is white, so both ends read at 4.5:1 or better.
    pub grad_row_start: Color,
    pub grad_row_end: Color,
    /// The thin line under the bar of the path and search: the same kind of shade.
    pub grad_line_start: Color,
    pub grad_line_end: Color,
    /// A ticked checkbox.
    pub check: Color,
    /// The filled part of a disk bar.
    pub disk: Color,
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

/// Whether a theme name stands for the whole family or for one variant of it.
#[derive(Clone, Copy)]
enum Pick {
    Adaptive,
    Exact { light: bool },
}

/// A named look, with a dark and a light variant.
#[derive(Clone, Copy)]
enum Family {
    Rada,
    Catppuccin,
    TokyoNight,
    /// Only a light variant of its own; the dark one is rada's.
    Nebbia,
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
            (Family::Nebbia, false) => Theme::rada(),
            (Family::Nebbia, true) => Theme::nebbia_light(),
        }
    }
}

impl Theme {
    /// Every built-in theme: three for dark terminals, then their light counterparts.
    pub const NAMES: [&'static str; 7] = [
        "rada",
        "catppuccin",
        "tokyo-night",
        "rada-light",
        "catppuccin-latte",
        "tokyo-night-day",
        "nebbia-light",
    ];

    /// The family a name belongs to, and whether the name pins a variant: `catppuccin-latte`
    /// is always the light one, `catppuccin` follows the terminal's background.
    fn family(name: &str) -> Option<(Family, Pick)> {
        use Pick::{Adaptive, Exact};
        Some(match name.to_ascii_lowercase().as_str() {
            "auto" | "rada" | "default" => (Family::Rada, Adaptive),
            "rada-dark" => (Family::Rada, Exact { light: false }),
            "rada-light" | "rada-day" => (Family::Rada, Exact { light: true }),
            "catppuccin" => (Family::Catppuccin, Adaptive),
            "catppuccin-mocha" | "mocha" => (Family::Catppuccin, Exact { light: false }),
            "catppuccin-latte" | "latte" => (Family::Catppuccin, Exact { light: true }),
            "tokyo-night" | "tokyonight" | "tokyo" => (Family::TokyoNight, Adaptive),
            "tokyo-night-dark" => (Family::TokyoNight, Exact { light: false }),
            "tokyo-night-day" | "tokyo-day" | "day" => (Family::TokyoNight, Exact { light: true }),
            "nebbia-light" | "nebbia" | "mist" => (Family::Nebbia, Exact { light: true }),
            _ => return None,
        })
    }

    /// A theme by name, adapted to the terminal's colour depth. A name that follows the
    /// terminal's background (`rada`, `catppuccin`, `tokyo-night`, `auto`) gives its dark
    /// variant here; see [`Theme::variant`] to choose.
    pub fn named(name: &str, depth: ColorDepth) -> Option<Theme> {
        let (family, pick) = Theme::family(name)?;
        let light = matches!(pick, Pick::Exact { light: true });
        Some(family.theme(light).with_depth(depth))
    }

    /// Does `name` follow the terminal's background (`Some(true)`), pin one variant
    /// (`Some(false)`), or mean nothing (`None`)?
    pub fn follows_background(name: &str) -> Option<bool> {
        Theme::family(name).map(|(_, p)| matches!(p, Pick::Adaptive))
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
            on_accent: rgb(22, 22, 30),
            selection,
            cursor: rgb(48, 72, 124),
            mark,
            track: rgb(36, 42, 62),
            sel: rgb(34, 42, 68),
            cursor_text: rgb(255, 255, 255),
            cmdbar: rgb(30, 32, 48),
            panel: rgb(21, 22, 32),
            key_bg: rgb(59, 66, 97),
            key_text: rgb(192, 202, 245),
            field: rgb(30, 32, 46),
            button: rgb(36, 40, 59),
            grad_row_start: rgb(0x3d, 0x59, 0xa1),
            grad_row_end: rgb(0x6a, 0x4c, 0x9c),
            grad_line_start: rgb(0x7a, 0xa2, 0xf7),
            grad_line_end: rgb(0xbb, 0x9a, 0xf7),
            check: accent,
            disk: accent,
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
        let mut t = Theme::rada_base("rada", rgb(122, 162, 247), rgb(40, 52, 87), rgb(30, 36, 58));
        t.cursor = rgb(61, 89, 161);
        t.field = rgb(31, 34, 51);
        t.button = rgb(41, 46, 66);
        t.kinds.folder = rgb(122, 162, 247);
        t
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
        t.grad_row_start = rgb(0x45, 0x5e, 0xa8);
        t.grad_row_end = rgb(0x7b, 0x4f, 0xa6);
        t.grad_line_start = rgb(0x89, 0xb4, 0xfa);
        t.grad_line_end = rgb(0xcb, 0xa6, 0xf7);
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
        t.text_dim = rgb(135, 144, 187);
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
        t.field = mark;
        t.button = selection;
        t.sel = mark;
        t.cursor_text = text;
        t.cmdbar = mark;
        t.panel = mark;
        t.key_bg = selection;
        t.key_text = text;
        t.check = accent;
        t.disk = accent;
        t
    }

    /// A light theme painted on a tinted paper instead of white, from the hex values of its
    /// tokens: `[bg, panel, bar, field, border, text, dim, muted, accent, sel, cursor, key,
    /// button, tab (the active tab and the other chips), check]` and the colours of the file
    /// types `[yellow, green, red, purple, cyan, orange]`. Pure white tires the eyes; the
    /// paper keeps the text sharp and the bars a step apart.
    fn paper(name: &'static str, tokens: [u32; 15], types: [u32; 6], gradients: [u32; 4]) -> Theme {
        let c = |v: u32| rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        let [
            bg,
            panel,
            bar,
            field,
            border,
            text,
            dim,
            muted,
            accent,
            sel,
            cursor,
            key,
            button,
            tab,
            check,
        ] = tokens.map(c);
        let [yellow, green, red, purple, cyan, orange] = types.map(c);
        let [row_start, row_end, line_start, line_end] = gradients.map(c);
        let mut t = Theme::light_base(name, bg, text, dim, muted, accent, tab, cursor, bar, border);
        t.bg = Some(bg);
        t.field = field;
        t.button = button;
        t.sel = sel;
        t.cursor_text = rgb(255, 255, 255);
        t.cmdbar = bar;
        t.panel = panel;
        t.key_bg = key;
        t.key_text = text;
        t.check = check;
        t.grad_row_start = row_start;
        t.grad_row_end = row_end;
        t.grad_line_start = line_start;
        t.grad_line_end = line_end;
        t.success = green;
        t.warn = yellow;
        t.error = red;
        t.kinds = KindColors {
            code: orange,
            markup: purple,
            text: dim,
            doc: red,
            image: purple,
            vector: purple,
            audio: red,
            video: red,
            archive: yellow,
            data: yellow,
            config: dim,
            binary: green,
            other: dim,
            folder: accent,
            link: cyan,
        };
        t
    }

    /// The default theme for a light terminal, "paper": warm off-white, a muted blue accent.
    pub fn rada_light() -> Theme {
        Theme::paper(
            "rada-light",
            [
                0xf6f3ee, 0xeeeae3, 0xebe6de, 0xe5e0d7, 0xd9d2c6, 0x3a3632, 0x6a6359, 0xa39a8d,
                0x3b659f, 0xe2e7ee, 0x476fa8, 0xe3ddd3, 0xe5e0d7, 0xf6f3ee, 0x476fa8,
            ],
            [0x8c5f16, 0x4b7438, 0xa8454c, 0x77559e, 0x2c6e7d, 0x9e5826],
            [0x476fa8, 0x7d58a6, 0x3d68a3, 0x8a57a8],
        )
    }

    /// "Mist": the same idea with cool grey-blue paper.
    pub fn nebbia_light() -> Theme {
        Theme::paper(
            "nebbia-light",
            [
                0xeef0f3, 0xe6e9ee, 0xe3e6eb, 0xdde1e7, 0xd0d5dd, 0x343a46, 0x5d6474, 0x9aa3b2,
                0x3e63a8, 0xdfe6f2, 0x4a6fb3, 0xd9dee6, 0xdde1e7, 0xeef0f3, 0x4469b0,
            ],
            [0x86621c, 0x3c7346, 0xa2485a, 0x6f5499, 0x2a6a80, 0x975a2a],
            [0x4a6fb3, 0x7a5aa8, 0x3e63a8, 0x8657a6],
        )
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
        t.grad_row_start = rgb(0x2f, 0x5f, 0xd0);
        t.grad_row_end = rgb(0x88, 0x39, 0xef);
        t.grad_line_start = rgb(0x1e, 0x66, 0xf5);
        t.grad_line_end = rgb(0x88, 0x39, 0xef);
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
        t.grad_row_start = rgb(0x3a, 0x5b, 0xa9);
        t.grad_row_end = rgb(0x78, 0x47, 0xa6);
        t.grad_line_start = rgb(0x2e, 0x7d, 0xe9);
        t.grad_line_end = rgb(0x98, 0x54, 0xf1);
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
        // Sixteen colours have no paper: the terminal's own background stays.
        self.bg = None;
        self.text = Color::Reset;
        self.text_dim = Color::Gray;
        self.muted = Color::DarkGray;
        self.accent = Color::Blue;
        self.on_accent = Color::Black;
        self.selection = Color::DarkGray;
        self.cursor = Color::DarkGray;
        self.mark = Color::DarkGray;
        self.track = Color::DarkGray;
        self.sel = Color::DarkGray;
        self.cursor_text = Color::Reset;
        self.cmdbar = Color::DarkGray;
        self.panel = Color::DarkGray;
        self.key_bg = Color::DarkGray;
        self.key_text = Color::Reset;
        self.field = Color::DarkGray;
        self.button = Color::DarkGray;
        self.check = Color::Blue;
        self.disk = Color::Blue;
        // Sixteen colours have no shades: these are never drawn, the row is reversed.
        self.grad_row_start = Color::Blue;
        self.grad_row_end = Color::Blue;
        self.grad_line_start = Color::Blue;
        self.grad_line_end = Color::Magenta;
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
                t.sel = f(t.sel);
                t.cursor_text = f(t.cursor_text);
                t.cmdbar = f(t.cmdbar);
                t.panel = f(t.panel);
                t.key_bg = f(t.key_bg);
                t.key_text = f(t.key_text);
                t.field = f(t.field);
                t.button = f(t.button);
                t.check = f(t.check);
                t.disk = f(t.disk);
                t.grad_row_start = f(t.grad_row_start);
                t.grad_row_end = f(t.grad_row_end);
                t.grad_line_start = f(t.grad_line_start);
                t.grad_line_end = f(t.grad_line_end);
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

    /// The colour a tag is drawn in: the Finder's seven, in what the terminal can show.
    pub fn tag_color(&self, color: Option<rada_core::platform::TagColor>) -> Color {
        use rada_core::platform::TagColor;
        let Some(c) = color else {
            return self.text_dim;
        };
        match self.depth {
            ColorDepth::Ansi16 => match c {
                TagColor::Red => Color::Red,
                TagColor::Orange => Color::LightRed,
                TagColor::Yellow => Color::Yellow,
                TagColor::Green => Color::Green,
                TagColor::Blue => Color::LightBlue,
                TagColor::Purple => Color::Magenta,
                TagColor::Gray => Color::DarkGray,
            },
            depth => {
                let rgb = match c {
                    TagColor::Red => Color::Rgb(0xff, 0x5f, 0x57),
                    TagColor::Orange => Color::Rgb(0xff, 0x9f, 0x43),
                    TagColor::Yellow => Color::Rgb(0xf7, 0xd1, 0x4c),
                    TagColor::Green => Color::Rgb(0x5f, 0xd0, 0x68),
                    TagColor::Blue => Color::Rgb(0x4c, 0x9a, 0xff),
                    TagColor::Purple => Color::Rgb(0xb0, 0x7c, 0xff),
                    TagColor::Gray => Color::Rgb(0x9a, 0xa0, 0xa6),
                };
                if depth == ColorDepth::Ansi256 {
                    to_256(rgb)
                } else {
                    rgb
                }
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

    /// Selected rows: a quiet background; the name goes bold in the list.
    pub fn marked(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(self.sel)
        }
    }

    /// The current place of the navigation pane.
    pub fn current_place(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.sel)
        }
    }

    /// The row under the cursor with its text: a solid background and white text.
    pub fn cursor_row(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.cursor).fg(self.cursor_text)
        }
    }

    /// The strip behind the top and bottom bars: a tone apart from the content. On a
    /// 16-colour terminal there is no such tone, and the chips carry the structure.
    pub fn band(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            self.on(Style::default())
        } else {
            Style::default().bg(self.mark)
        }
    }

    /// Something clickable in a bar: a button, a field, a key cap.
    pub fn chip(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(self.text).bg(self.selection)
        }
    }

    /// The background of a highlighted row of a list that is not the cursor (the place you
    /// are in, the item under the mouse).
    pub fn chip_row(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.selection)
        }
    }

    /// A key name on a chip.
    pub fn chip_key(&self) -> Style {
        if self.depth == ColorDepth::Ansi16 {
            self.chip().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.accent)
                .bg(self.selection)
                .add_modifier(Modifier::BOLD)
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
            self.sel,
            self.cursor_text,
            self.cmdbar,
            self.panel,
            self.key_bg,
            self.key_text,
            self.field,
            self.button,
            self.check,
            self.disk,
            self.grad_row_start,
            self.grad_row_end,
            self.grad_line_start,
            self.grad_line_end,
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
        // The dots of the seven tag colours, and the dim one of a tag with none.
        for c in rada_core::platform::TagColor::ALL {
            v.push(self.tag_color(Some(c)));
        }
        v.push(self.tag_color(None));
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

impl Theme {
    /// Whether the terminal can show the shades of the gradients: 24-bit colour. Below that
    /// they give way to solid colours.
    pub fn shades(&self) -> bool {
        self.depth == ColorDepth::True
    }

    /// The colour at `x` of a row `width` cells wide that shades from `grad_row_start` to
    /// `grad_row_end`; the solid cursor colour where there are no shades.
    pub fn row_shade(&self, x: usize, width: usize) -> Color {
        if !self.shades() {
            return self.cursor;
        }
        mix(self.grad_row_start, self.grad_row_end, x, width)
    }

    /// The colour at `x` of the thin line under the bar: shaded, or the accent colour.
    pub fn line_shade(&self, x: usize, width: usize) -> Color {
        if !self.shades() {
            return self.accent;
        }
        mix(self.grad_line_start, self.grad_line_end, x, width)
    }
}

/// The colour `x` cells along a run of `width` cells that goes from `a` to `b`. Colours that
/// are not true colour are returned as they are (`a`).
pub fn mix(a: Color, b: Color, x: usize, width: usize) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    if width <= 1 {
        return a;
    }
    let t = x.min(width - 1) as f32 / (width - 1) as f32;
    let lerp = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * t).round() as u8;
    Color::Rgb(lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
}

// ---------------------------------------------------------------------------- colour maths

/// A colour as the terminal can show it. Theme colours are already adapted when the theme
/// is built; this is for colours that come from elsewhere (the pixels of a picture drawn
/// with half blocks), applied where cells are written to the terminal.
pub fn downgrade(c: Color, depth: ColorDepth) -> Color {
    match depth {
        ColorDepth::True => c,
        ColorDepth::Ansi256 => to_256(c),
        ColorDepth::Ansi16 => to_16(c),
    }
}

/// The red, green and blue of a colour the terminal will show: a true colour, or an entry of
/// the 256-colour palette (the cube and the grey ramp). `None` for a named colour, whose
/// shade belongs to the terminal's own palette.
pub fn rgb_of(c: Color) -> Option<[u8; 3]> {
    match c {
        Color::Rgb(r, g, b) => Some([r, g, b]),
        Color::Indexed(n) if n >= 232 => {
            let v = 8 + 10 * (n - 232);
            Some([v, v, v])
        }
        Color::Indexed(n) if n >= 16 => {
            const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let n = n - 16;
            Some([
                STEPS[(n / 36) as usize],
                STEPS[(n / 6 % 6) as usize],
                STEPS[(n % 6) as usize],
            ])
        }
        _ => None,
    }
}

/// The 16 ANSI colours with the values xterm gives them, in palette order.
const ANSI16: [(Color, (u8, u8, u8)); 16] = [
    (Color::Black, (0, 0, 0)),
    (Color::Red, (205, 0, 0)),
    (Color::Green, (0, 205, 0)),
    (Color::Yellow, (205, 205, 0)),
    (Color::Blue, (0, 0, 238)),
    (Color::Magenta, (205, 0, 205)),
    (Color::Cyan, (0, 205, 205)),
    (Color::Gray, (229, 229, 229)),
    (Color::DarkGray, (127, 127, 127)),
    (Color::LightRed, (255, 0, 0)),
    (Color::LightGreen, (0, 255, 0)),
    (Color::LightYellow, (255, 255, 0)),
    (Color::LightBlue, (92, 92, 255)),
    (Color::LightMagenta, (255, 0, 255)),
    (Color::LightCyan, (0, 255, 255)),
    (Color::White, (255, 255, 255)),
];

/// Nearest of the 16 ANSI colours. Named colours and "no colour" are left alone.
fn to_16(c: Color) -> Color {
    let (r, g, b) = match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(n) if n < 16 => return ANSI16[n as usize].0,
        Color::Indexed(n) if n >= 232 => {
            let v = 8 + 10 * (n - 232);
            (v, v, v)
        }
        Color::Indexed(n) => {
            const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let n = n - 16;
            (
                STEPS[(n / 36) as usize],
                STEPS[(n / 6 % 6) as usize],
                STEPS[(n % 6) as usize],
            )
        }
        other => return other,
    };
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
    ANSI16
        .iter()
        .min_by_key(|(_, (ar, ag, ab))| d(*ar, r) + d(*ag, g) + d(*ab, b))
        .map(|(named, _)| *named)
        .unwrap_or(c)
}

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
    fn colours_from_outside_the_theme_are_brought_down_to_the_depth() {
        let red = rgb(250, 10, 10);
        assert_eq!(downgrade(red, ColorDepth::True), red);
        assert_eq!(downgrade(red, ColorDepth::Ansi256), to_256(red));
        assert_eq!(downgrade(red, ColorDepth::Ansi16), Color::LightRed);
        assert_eq!(downgrade(rgb(0, 0, 0), ColorDepth::Ansi16), Color::Black);
        assert_eq!(
            downgrade(rgb(255, 255, 255), ColorDepth::Ansi16),
            Color::White
        );
        // Palette entries too: the cube's pure blue (nearer to blue than to bright blue,
        // which xterm draws pale) and a mid grey.
        assert_eq!(
            downgrade(Color::Indexed(21), ColorDepth::Ansi16),
            Color::Blue
        );
        assert_eq!(
            downgrade(Color::Indexed(244), ColorDepth::Ansi16),
            Color::DarkGray
        );
        assert_eq!(downgrade(Color::Indexed(1), ColorDepth::Ansi16), Color::Red);
        // What is already plain stays as it is.
        for d in [ColorDepth::True, ColorDepth::Ansi256, ColorDepth::Ansi16] {
            assert_eq!(downgrade(Color::Reset, d), Color::Reset);
            assert_eq!(downgrade(Color::Blue, d), Color::Blue);
        }
    }

    #[test]
    fn the_colour_depth_follows_the_environment() {
        let depth = |vars: &[(&str, &str)], windows: bool| {
            let vars: Vec<(String, String)> = vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            ColorDepth::from_env(
                &|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
                windows,
            )
        };
        assert_eq!(
            depth(&[("COLORTERM", "truecolor")], false),
            ColorDepth::True
        );
        assert_eq!(
            depth(&[("TERM", "xterm-256color")], false),
            ColorDepth::Ansi256
        );
        assert_eq!(depth(&[("TERM", "xterm")], false), ColorDepth::Ansi16);
        assert_eq!(depth(&[], false), ColorDepth::Ansi16);
        // Windows sets neither variable.
        assert_eq!(depth(&[("WT_SESSION", "x")], true), ColorDepth::True);
        assert_eq!(depth(&[], true), ColorDepth::True);
        assert_eq!(depth(&[("TERM", "xterm")], true), ColorDepth::Ansi16);
        assert_eq!(
            depth(
                &[("TERM", "xterm-256color"), ("VTE_VERSION", "7600")],
                false
            ),
            ColorDepth::True
        );
    }

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
            // The chips sit on the bars; the active tab of a paper theme is the canvas itself.
            need(
                "selection against its bar",
                contrast(t.selection, t.mark),
                1.05,
            );
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
            need(
                "cursor_text on cursor",
                contrast(t.cursor_text, t.cursor),
                4.5,
            );
            need("text on sel", contrast(t.text, t.sel), 7.0);
            need("sel against canvas", contrast(t.sel, c), 1.05);
            need("key_text on key_bg", contrast(t.key_text, t.key_bg), 4.5);
            need("text on cmdbar", contrast(t.text, t.cmdbar), 7.0);
            need("text_dim on panel", contrast(t.text_dim, t.panel), 4.5);
            need("on_accent on accent", contrast(t.on_accent, t.accent), 4.5);
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
    fn the_white_text_of_the_cursor_row_reads_on_both_ends_of_its_gradient() {
        // Bold white on the shade: 4.5:1 at the left end, at the right end and in between.
        let white = rgb(255, 255, 255);
        let mut failures = Vec::new();
        for n in Theme::NAMES {
            let t = Theme::named(n, ColorDepth::True).unwrap();
            for (what, col) in [("start", t.grad_row_start), ("end", t.grad_row_end)] {
                let r = contrast(white, col);
                if r < 4.5 {
                    failures.push(format!("{n}: white on grad_row {what} is {r:.2}:1"));
                }
            }
            for x in 0..40 {
                let r = contrast(white, t.row_shade(x, 40));
                if r < 4.5 {
                    failures.push(format!("{n}: white on the shade at {x}/40 is {r:.2}:1"));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn the_reference_gradients_are_the_ones_of_the_design() {
        let hex = |c: Color| {
            let (r, g, b) = channels(c);
            format!("#{r:02x}{g:02x}{b:02x}")
        };
        let dark = Theme::named("rada", ColorDepth::True).unwrap();
        assert_eq!(hex(dark.grad_row_start), "#3d59a1");
        assert_eq!(hex(dark.grad_row_end), "#6a4c9c");
        assert_eq!(hex(dark.grad_line_start), "#7aa2f7");
        assert_eq!(hex(dark.grad_line_end), "#bb9af7");
        let paper = Theme::named("rada-light", ColorDepth::True).unwrap();
        assert_eq!(hex(paper.grad_row_start), "#476fa8");
        assert_eq!(hex(paper.grad_row_end), "#7d58a6");
        assert_eq!(hex(paper.grad_line_start), "#3d68a3");
        assert_eq!(hex(paper.grad_line_end), "#8a57a8");
        // The solid cursor colour is where the shade starts.
        assert_eq!(dark.cursor, dark.grad_row_start);
        assert_eq!(paper.cursor, paper.grad_row_start);
    }

    #[test]
    fn the_shade_goes_from_one_end_to_the_other_and_is_solid_below_true_colour() {
        let t = Theme::named("rada", ColorDepth::True).unwrap();
        assert_eq!(t.row_shade(0, 50), t.grad_row_start);
        assert_eq!(t.row_shade(49, 50), t.grad_row_end);
        assert_ne!(t.row_shade(25, 50), t.grad_row_start);
        assert_eq!(
            t.row_shade(3, 1),
            t.grad_row_start,
            "a one-cell row has no shade"
        );
        let t256 = Theme::named("rada", ColorDepth::Ansi256).unwrap();
        assert!(!t256.shades());
        for x in [0, 10, 49] {
            assert_eq!(t256.row_shade(x, 50), t256.cursor, "solid in 256 colours");
            assert_eq!(t256.line_shade(x, 50), t256.accent);
        }
        let t16 = Theme::named("rada", ColorDepth::Ansi16).unwrap();
        assert!(!t16.shades());
        assert!(matches!(t256.grad_row_start, Color::Indexed(_)));
    }

    #[test]
    fn every_theme_reads_on_every_surface_it_is_drawn_on() {
        // WCAG: the main text 7:1 on the background, the secondary text and the accent 4.5:1
        // on the background, the strip of hints, the fields and the selected rows; the text on
        // the cursor row 4.5:1.
        let mut failures = Vec::new();
        for n in Theme::NAMES {
            let t = Theme::named(n, ColorDepth::True).unwrap();
            let surfaces = [
                ("bg", t.canvas),
                ("panel", t.panel),
                ("field", t.field),
                ("sel", t.sel),
            ];
            let r = contrast(t.text, t.canvas);
            if r < 7.0 {
                failures.push(format!("{n}: text on bg is {r:.2}:1, needs 7:1"));
            }
            for (what, color) in [("text_dim", t.text_dim), ("accent", t.accent)] {
                for (on, bg) in surfaces {
                    let r = contrast(color, bg);
                    if r < 4.5 {
                        failures.push(format!("{n}: {what} on {on} is {r:.2}:1, needs 4.5:1"));
                    }
                }
            }
            let r = contrast(t.cursor_text, t.cursor);
            if r < 4.5 {
                failures.push(format!(
                    "{n}: text on the cursor row is {r:.2}:1, needs 4.5:1"
                ));
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
    fn family_names_follow_the_background_and_variant_names_pin_it() {
        for follows in [
            "auto",
            "rada",
            "catppuccin",
            "tokyo-night",
            "default",
            "tokyo",
        ] {
            assert_eq!(Theme::follows_background(follows), Some(true), "{follows}");
        }
        for pinned in [
            "rada-dark",
            "rada-light",
            "catppuccin-mocha",
            "catppuccin-latte",
            "tokyo-night-dark",
            "tokyo-night-day",
            "latte",
            "mocha",
        ] {
            assert_eq!(Theme::follows_background(pinned), Some(false), "{pinned}");
        }
        assert_eq!(Theme::follows_background("nope"), None);
        // A pinned name is that theme whatever the terminal looks like.
        let d = ColorDepth::True;
        assert_eq!(Theme::named("rada-dark", d).unwrap().name, "rada");
        assert!(Theme::named("tokyo-night-day", d).unwrap().light);
        assert!(!Theme::named("tokyo-night-dark", d).unwrap().light);
        // "auto" is the rada family, and `variant` picks its side.
        assert_eq!(Theme::variant("auto", true, d).unwrap().name, "rada-light");
        assert_eq!(Theme::variant("auto", false, d).unwrap().name, "rada");
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
