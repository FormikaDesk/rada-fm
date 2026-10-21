//! The last step before the terminal: what was drawn is adapted to what the terminal can
//! show. The interface is always drawn the same way, with Unicode glyphs and the theme's
//! colours; this backend stands in front of the real one and, cell by cell,
//!
//! - replaces the glyphs of the interface with ASCII ones of the same width (the basic
//!   profile, for terminals without UTF-8 or with a console font),
//! - or only the glyphs that a plain monospace font does not have with ones that every
//!   such font has (the `unicode-std` icon set, for systems without a symbol font),
//! - brings colours that did not come from the theme (the pixels of a picture drawn with
//!   half blocks) down to the terminal's colour depth,
//! - or removes colour altogether (`NO_COLOR`).
//!
//! On a modern terminal with 24-bit colour it does nothing and costs nothing.

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};

use crate::theme::ColorDepth;

/// Which glyphs the terminal's font can be trusted to have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyphs {
    /// Whatever the interface draws.
    Full,
    /// Only what every monospace font has ([`STD_GLYPHS`]): the rest is replaced.
    Std,
    /// Only ASCII, and no attribute beyond bold and reverse.
    Ascii,
}

/// How cells are adapted on their way out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rendering {
    pub glyphs: Glyphs,
    /// Colours are brought down to this depth.
    pub depth: ColorDepth,
    /// No colour at all: bold and reverse video carry the meaning.
    pub no_color: bool,
}

impl Rendering {
    /// Everything as drawn.
    pub const PLAIN: Rendering = Rendering {
        glyphs: Glyphs::Full,
        depth: ColorDepth::True,
        no_color: false,
    };

    /// The basic profile: ASCII and the 16 ANSI colours.
    pub const BASIC: Rendering = Rendering {
        glyphs: Glyphs::Ascii,
        depth: ColorDepth::Ansi16,
        no_color: false,
    };

    fn is_passthrough(self) -> bool {
        self == Rendering::PLAIN
    }

    /// The same, for a font without symbols (the `unicode-std` icon set). ASCII stays ASCII.
    pub fn with_std_glyphs(self) -> Rendering {
        Rendering {
            glyphs: match self.glyphs {
                Glyphs::Full => Glyphs::Std,
                other => other,
            },
            ..self
        }
    }

    /// One cell as the terminal will get it.
    pub fn adapt(self, cell: &Cell) -> Cell {
        let mut out = cell.clone();
        let symbol = cell.symbol();
        // The payload of a graphics protocol (a Sixel sequence, a kitty placeholder whose
        // colour is an image id) is not text: it goes out untouched.
        if symbol.starts_with('\x1b') || symbol.contains('\u{10EEEE}') {
            return out;
        }
        match self.glyphs {
            Glyphs::Full => {}
            Glyphs::Std => {
                if let Some(std) = std_symbol(symbol) {
                    out.set_symbol(&std);
                }
            }
            Glyphs::Ascii => {
                if let Some(plain) = ascii_symbol(symbol) {
                    out.set_symbol(&plain);
                }
                out.modifier.remove(
                    Modifier::DIM | Modifier::CROSSED_OUT | Modifier::ITALIC | Modifier::UNDERLINED,
                );
            }
        }
        if self.no_color {
            out.fg = Color::Reset;
            out.bg = Color::Reset;
        } else {
            out.fg = crate::theme::downgrade(out.fg, self.depth);
            out.bg = crate::theme::downgrade(out.bg, self.depth);
        }
        out
    }
}

/// The ASCII stand-in for a glyph of the interface, or `None` for a character the
/// interface does not draw itself (the letters of a file name, which are the user's own
/// and are passed on as they are).
pub fn ascii_for(ch: char) -> Option<char> {
    Some(match ch {
        // punctuation
        '·' | '•' => '.',
        '×' => 'x',
        '—' | '–' => '-',
        '“' | '”' => '"',
        '‘' | '’' => '\'',
        '…' | '⋯' | '≈' => '~',
        '‹' => '<',
        '›' => '>',
        '\u{00A0}' | '\u{2002}'..='\u{200A}' => ' ',
        // arrows
        '←' | '↶' | '⌫' => '<',
        '→' | '↪' => '>',
        '↑' | '⏏' => '^',
        '↓' => 'v',
        '↻' | '⟳' => '@',
        '⇄' | '≣' => '=',
        '⇅' => ':',
        '\u{2190}'..='\u{21FF}' => '>',
        // symbols
        '⌂' | '⎇' => '~',
        '⌕' | '⚲' => '?',
        '⊘' => '!',
        '☁' => '@',
        '⎘' | '⧉' => '+',
        '✂' => '%',
        '✎' => '/',
        '♪' => '~',
        '★' | '◆' | '◉' | '●' => '*',
        '○' => 'o',
        '☐' => '-',
        '☑' | '✕' | '✖' | '✗' => 'x',
        '✓' | '✔' => '+',
        // triangles
        '▲' | '△' => '^',
        '▶' | '▸' | '▷' | '▹' => '>',
        '▼' | '▽' | '▾' | '▿' | '⌄' => 'v',
        '◀' | '◂' | '◁' | '◃' => '<',
        // squares
        '■' | '▣' | '▤' | '▥' | '▦' => '#',
        '□' => '-',
        '▭' => '=',
        // box drawing: lines stay lines, every corner and joint is a plus
        '─' | '━' | '╌' | '╍' | '┄' | '┅' | '┈' | '┉' | '═' | '╴' | '╶' | '╸' | '╺' | '╼' | '╾' => {
            '-'
        }
        '│' | '┃' | '╎' | '╏' | '┆' | '┇' | '┊' | '┋' | '║' | '╵' | '╷' | '╹' | '╻' | '╽' | '╿' => {
            '|'
        }
        '\u{2500}'..='\u{257F}' => '+',
        // blocks: thin left bars are edges, shades are dots, the rest is solid
        '▏' | '▎' | '▍' | '▕' => '|',
        '░' | '▖' | '▗' | '▘' | '▝' => '.',
        '▒' => ':',
        '\u{2580}'..='\u{259F}' => '#',
        // the spinner's ten braille frames turn as the classic four
        '⠋' | '⠼' | '⠇' => '|',
        '⠙' | '⠴' | '⠏' => '/',
        '⠹' | '⠦' => '-',
        '⠸' | '⠧' => '\\',
        '\u{2800}'..='\u{28FF}' => '*',
        _ => return None,
    })
}

/// The glyphs beyond ASCII that every monospace font has. Measured on the font files:
/// each one is in both Consolas (the classic Windows console) and Cascadia Mono (Windows
/// Terminal), the two default fonts with the smallest repertoires. They belong to the old
/// WGL4 / code page 437 set, plus the box-drawing arcs, which Menlo, DejaVu Sans Mono and
/// their descendants carry too. The `unicode-std` icon set draws with these and nothing
/// else.
pub const STD_GLYPHS: &str = "·×—“”•…‹›←↑→↓↔↕≈≡√⌂─━│╭╮╯╰╸▀▄█▌░▒■▬▲►▼▸▾○●◘◙♦♪";

/// The stand-in from [`STD_GLYPHS`] (or ASCII) for a glyph of the interface that only a
/// symbol font has; `None` for what needs no replacing.
pub fn std_for(ch: char) -> Option<char> {
    Some(match ch {
        // arrows
        '↪' => '→',
        '↶' => '←',
        '↷' => '→',
        '↻' | '⟳' => '○',
        '⇄' => '↔',
        '⇅' => '↕',
        // symbols
        '≣' | '▤' => '≡',
        '⋯' => '…',
        '⌕' | '⚲' => '?',
        '⌫' | '✂' | '✕' | '✖' | '✗' => '×',
        '⏏' => '▲',
        '⊘' => '×',
        '☁' => '○',
        '★' => '*',
        '✎' => '/',
        '✓' | '✔' => '√',
        // squares and the like
        '☐' => '○',
        '⧉' => '+',
        '☑' | '⎘' => '■',
        '▣' => '◘',
        '□' => '░',
        '▥' => '◙',
        '▦' => '▒',
        '▭' => '▬',
        '▶' => '►',
        '▽' => '▼',
        '⌄' => '▾',
        '◆' | '⎇' => '♦',
        '◉' => '●',
        // the eighths of a progress bar round to the half, the thin edges become a line
        '▉' | '▊' => '█',
        '▋' => '▌',
        '▍' | '▎' | '▏' => '│',
        '▖' => '▄',
        '▔' => '▀',
        // the heavy side of a card, the half of a bar's head and the dashes between groups
        '┃' => '│',
        '╺' => '╸',
        '╌' => '─',
        // the spinner's ten braille frames turn as the classic four
        '⠋' | '⠼' | '⠇' => '|',
        '⠙' | '⠴' | '⠏' => '/',
        '⠹' | '⠦' => '-',
        '⠸' | '⠧' => '\\',
        '\u{2800}'..='\u{28FF}' => '*',
        _ => return None,
    })
}

/// A cell's text with every character `stand_in` knows replaced; `None` when nothing
/// changes. Every stand-in is as wide as what it replaces, so the layout is untouched.
fn replaced(symbol: &str, stand_in: fn(char) -> Option<char>) -> Option<String> {
    if symbol.is_ascii() {
        return None;
    }
    let mut changed = false;
    let out: String = symbol
        .chars()
        .map(|c| match stand_in(c) {
            Some(other) => {
                changed = true;
                other
            }
            None => c,
        })
        .collect();
    changed.then_some(out)
}

/// A cell's text with the glyphs of the interface replaced by ASCII ones.
pub fn ascii_symbol(symbol: &str) -> Option<String> {
    replaced(symbol, ascii_for)
}

/// A cell's text with the glyphs only a symbol font has replaced by standard ones.
pub fn std_symbol(symbol: &str) -> Option<String> {
    replaced(symbol, std_for)
}

/// A backend that adapts every cell before handing it to the real one.
pub struct CompatBackend<B: Backend> {
    inner: B,
    how: Rendering,
}

impl<B: Backend> CompatBackend<B> {
    pub fn new(inner: B, how: Rendering) -> CompatBackend<B> {
        CompatBackend { inner, how }
    }

    pub fn rendering(&self) -> Rendering {
        self.how
    }

    pub fn inner(&self) -> &B {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }
}

impl<B: Backend> Backend for CompatBackend<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let how = self.how;
        if how.is_passthrough() {
            return self.inner.draw(content);
        }
        let cells: Vec<(u16, u16, Cell)> = content.map(|(x, y, c)| (x, y, how.adapt(c))).collect();
        self.inner.draw(cells.iter().map(|(x, y, c)| (*x, *y, c)))
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    fn drawn(how: Rendering, line: Line<'static>) -> Terminal<CompatBackend<TestBackend>> {
        let mut term = Terminal::new(CompatBackend::new(TestBackend::new(24, 1), how)).unwrap();
        term.draw(|f| f.render_widget(Paragraph::new(line), f.area()))
            .unwrap();
        term
    }

    fn text(term: &Terminal<CompatBackend<TestBackend>>) -> String {
        let buf = term.backend().inner().buffer();
        (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn the_basic_profile_draws_only_ascii() {
        let term = drawn(Rendering::BASIC, Line::from("╭─╮ │ ▸ a…b · ☑ ⠹ ███▍"));
        assert_eq!(text(&term), "+-+ | > a~b . x - ###|");
    }

    #[test]
    fn stand_ins_are_as_wide_as_what_they_replace() {
        use unicode_width::UnicodeWidthChar;
        for cp in 0x80u32..0x3000 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            if let Some(plain) = ascii_for(c) {
                assert!(plain.is_ascii() && !plain.is_ascii_control(), "{c:?}");
                assert_eq!(c.width().unwrap_or(1), 1, "{c:?} is not one cell wide");
            }
        }
    }

    #[test]
    fn the_std_set_draws_only_what_every_font_has() {
        let term = drawn(
            Rendering::PLAIN.with_std_glyphs(),
            Line::from("╭─╮ ▸ ✔ ✖ ★ ☑ ⇅ ⠹ ██▍"),
        );
        assert_eq!(text(&term), "╭─╮ ▸ √ × * ■ ↕ - ██│");
        // Every stand-in is itself a standard glyph or ASCII, and one cell wide.
        for cp in 0x80u32..0x3000 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            if let Some(std) = std_for(c) {
                assert!(
                    std.is_ascii() || STD_GLYPHS.contains(std),
                    "{c:?} -> {std:?}"
                );
                assert!(!STD_GLYPHS.contains(c), "{c:?} is standard already");
            }
        }
        // The basic profile stays ASCII whatever the icon set.
        assert_eq!(Rendering::BASIC.with_std_glyphs(), Rendering::BASIC);
    }

    #[test]
    fn letters_of_names_are_left_alone() {
        assert_eq!(ascii_for('è'), None);
        assert_eq!(ascii_for('日'), None);
        assert_eq!(ascii_symbol("è"), None);
        assert_eq!(ascii_symbol("plain"), None);
        let term = drawn(Rendering::BASIC, Line::from("perché → qui"));
        assert_eq!(text(&term), "perché > qui");
    }

    #[test]
    fn the_modern_profile_changes_nothing() {
        let term = drawn(Rendering::PLAIN, Line::from("╭─╮ ▸ …"));
        assert_eq!(text(&term), "╭─╮ ▸ …");
    }

    #[test]
    fn colours_follow_the_depth_and_no_color_removes_them() {
        let picture = crate::theme::Theme::rada();
        let style = Style::default().fg(picture.accent).bg(picture.canvas);
        let cell = |how: Rendering| {
            let term = drawn(how, Line::from(Span::styled("x", style)));
            term.backend().inner().buffer()[(0, 0)].clone()
        };
        let plain = cell(Rendering::PLAIN);
        assert_eq!((plain.fg, plain.bg), (picture.accent, picture.canvas));
        let low = cell(Rendering {
            depth: ColorDepth::Ansi256,
            ..Rendering::PLAIN
        });
        assert_eq!(
            low.fg,
            crate::theme::downgrade(picture.accent, ColorDepth::Ansi256)
        );
        assert_ne!(low.fg, picture.accent);
        let none = cell(Rendering {
            no_color: true,
            ..Rendering::BASIC
        });
        assert_eq!((none.fg, none.bg), (Color::Reset, Color::Reset));
    }

    #[test]
    fn attributes_a_plain_terminal_lacks_are_dropped() {
        let style =
            Style::default().add_modifier(Modifier::BOLD | Modifier::DIM | Modifier::CROSSED_OUT);
        let term = drawn(Rendering::BASIC, Line::from(Span::styled("x", style)));
        assert_eq!(
            term.backend().inner().buffer()[(0, 0)].modifier,
            Modifier::BOLD
        );
    }

    #[test]
    fn graphics_payloads_go_out_untouched() {
        let mut sixel = Cell::default();
        sixel.set_symbol("\x1bPq─\x1b\\");
        assert_eq!(Rendering::BASIC.adapt(&sixel).symbol(), "\x1bPq─\x1b\\");
    }
}
