//! Soft shapes: pills, rounded cards, rows with rounded ends and the shades behind the
//! cursor.
//!
//! A pill is a run of text on a background with a half circle at each end. The half circles
//! are glyphs of the Nerd Fonts (U+E0B6 and U+E0B4), so they exist only where the icons do;
//! everywhere else a pill is a flat rectangle with a space at each end, which takes the same
//! room. The shades need 24-bit colour; below that the solid colour of the theme stands in.
//! The basic profile has neither.
//!
//! Nothing here picks a colour: every one comes from the theme.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::app::App;
use crate::icons::IconSet;
use crate::theme::{ColorDepth, Theme};

/// The left and right half circles of a pill.
pub const CAP_LEFT: &str = "\u{e0b6}";
pub const CAP_RIGHT: &str = "\u{e0b4}";

/// What the terminal and the person's settings allow of the soft look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// Half circles at the ends of pills and rows.
    pub caps: bool,
    /// Shades (gradients) instead of solid colours.
    pub shades: bool,
}

impl Look {
    pub fn of(app: &App) -> Look {
        Look::new(&app.settings.th, app.settings.icons, app.settings.basic)
    }

    pub fn new(th: &Theme, icons: IconSet, basic: bool) -> Look {
        let caps = icons == IconSet::Nerd && !basic;
        Look {
            caps,
            shades: caps && th.shades(),
        }
    }

    /// No soft shapes at all.
    pub const FLAT: Look = Look {
        caps: false,
        shades: false,
    };
}

/// The style of what lies around a pill: the theme's background, or the strip it sits on.
fn around(surround: Style) -> Style {
    Style {
        fg: None,
        add_modifier: ratatui::style::Modifier::empty(),
        sub_modifier: ratatui::style::Modifier::empty(),
        underline_color: None,
        ..surround
    }
}

/// A pill: `text` in `text_style` on `fill`, rounded at both ends where the font allows, with
/// a space at each end where it does not. `surround` is the style of the strip behind it (its
/// background shows around the half circles).
pub fn pill(
    th: &Theme,
    look: Look,
    text: &str,
    fill: Color,
    text_style: Style,
    surround: Style,
) -> Vec<Span<'static>> {
    let body = text_style.bg(fill);
    if th.depth == ColorDepth::Ansi16 && !look.caps {
        // No colours to fill with: reverse video marks it.
        return vec![Span::styled(
            format!(" {text} "),
            text_style.add_modifier(ratatui::style::Modifier::REVERSED),
        )];
    }
    if look.caps {
        let edge = around(surround).fg(fill);
        vec![
            Span::styled(CAP_LEFT, edge),
            Span::styled(text.to_string(), body),
            Span::styled(CAP_RIGHT, edge),
        ]
    } else {
        vec![Span::styled(format!(" {text} "), body)]
    }
}

/// The cells a pill takes: its text and the two ends.
pub fn pill_width(text_width: usize) -> usize {
    text_width + 2
}

/// The theme's own background, or the terminal's where the theme leaves it alone: set, not
/// left unsaid, so that the end of a pill or of a row stands on it and not on whatever the
/// cell had before.
pub fn outer(th: &Theme) -> Style {
    Style::default().bg(th.bg.unwrap_or(Color::Reset))
}

/// What kind of highlight a row gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowTone {
    /// The row under the cursor: a shade from `grad_row_start` to `grad_row_end`, or the solid
    /// cursor colour.
    Cursor,
    /// A selected row: the quiet tone.
    Marked,
    /// The current place of the sidebar: the quiet tone as well.
    Place,
    /// The place the keyboard is on: the cursor colour.
    Focus,
    /// A row of the panels view that is the current place or selected: the theme's `selection`
    /// tone, which stands out from the inside of a box where the quiet one would not.
    Panel,
}

/// Colour the background of the cells of `row` (`area.height` is 1) as `tone` says, and give
/// the two end cells the half circles where the font allows. The text already in the cells
/// is kept. Rows that must keep a reversed look (16 colours) are left to their own style.
pub fn tint_row(buf: &mut Buffer, row: Rect, th: &Theme, look: Look, tone: RowTone) {
    if row.width == 0 || row.height == 0 || th.depth == ColorDepth::Ansi16 {
        return;
    }
    let w = row.width as usize;
    let colour = |x: usize| match tone {
        // The shade only where the look allows it (true colour and a font with the round
        // ends); the solid cursor colour everywhere else.
        RowTone::Cursor if look.shades => th.row_shade(x, w),
        RowTone::Cursor => th.cursor,
        RowTone::Focus => th.cursor,
        RowTone::Marked | RowTone::Place => th.sel,
        RowTone::Panel => th.selection,
    };
    for i in 0..w {
        buf[(row.x + i as u16, row.y)].set_bg(colour(i));
    }
    if look.caps && w >= 3 {
        let surround = outer(th);
        for (i, glyph) in [(0, CAP_LEFT), (w - 1, CAP_RIGHT)] {
            let cell = &mut buf[(row.x + i as u16, row.y)];
            cell.set_symbol(glyph);
            let style = around(surround).fg(colour(i));
            cell.set_style(style);
        }
    }
}

/// A card: a panel with rounded corners (`╭╮╰╯`) in the theme's quiet line colour, the accent
/// colour while the keyboard is in it. Returns what is inside.
pub fn card(f: &mut ratatui::Frame, area: Rect, th: &Theme, focused: bool) -> Rect {
    use ratatui::widgets::{Block, BorderType, Borders};
    let border = if focused { th.accent } else { th.track };
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(border))
        .style(th.on(Style::default()));
    let inner = blk.inner(area);
    f.render_widget(blk, area);
    inner
}

/// Round the two ends of a row that is already coloured `fill` (a header): the half circles
/// take the cells at the ends, in `fill` on the strip around.
pub fn round_ends(buf: &mut Buffer, row: Rect, th: &Theme, look: Look, fill: Color) {
    if !look.caps || row.width < 3 || row.height == 0 {
        return;
    }
    let surround = outer(th);
    for (x, glyph) in [(row.x, CAP_LEFT), (row.x + row.width - 1, CAP_RIGHT)] {
        let cell = &mut buf[(x, row.y)];
        cell.set_symbol(glyph);
        cell.set_style(around(surround).fg(fill));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorDepth;

    fn buf_of(w: u16) -> Buffer {
        let mut b = Buffer::empty(Rect::new(0, 0, w, 1));
        for x in 0..w {
            b[(x, 0)].set_symbol("x");
        }
        b
    }

    #[test]
    fn a_pill_is_as_wide_with_or_without_the_half_circles() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let text = "Copia";
        let width = |look: Look| -> usize {
            pill(&th, look, text, th.button, th.base(), outer(&th))
                .iter()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                .sum()
        };
        let round = Look {
            caps: true,
            shades: true,
        };
        assert_eq!(width(round), pill_width(5));
        assert_eq!(width(Look::FLAT), pill_width(5));
    }

    #[test]
    fn a_pill_has_half_circles_only_where_the_font_has_them() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let round = pill(
            &th,
            Look {
                caps: true,
                shades: true,
            },
            "x",
            th.button,
            th.base(),
            outer(&th),
        );
        assert_eq!(round.first().unwrap().content, CAP_LEFT);
        assert_eq!(round.last().unwrap().content, CAP_RIGHT);
        // The half circle is the pill's colour on the strip's.
        assert_eq!(round[0].style.fg, Some(th.button));
        let flat = pill(&th, Look::FLAT, "x", th.button, th.base(), outer(&th));
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].content, " x ");
        assert!(flat.iter().all(|s| !s.content.contains('\u{e0b6}')));
    }

    #[test]
    fn the_cursor_row_shades_from_one_end_to_the_other_with_round_ends() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let look = Look {
            caps: true,
            shades: true,
        };
        let mut b = buf_of(20);
        tint_row(&mut b, Rect::new(0, 0, 20, 1), &th, look, RowTone::Cursor);
        // Between the ends the background goes from the first colour to the last.
        assert_eq!(b[(1, 0)].bg, th.row_shade(1, 20));
        assert_eq!(b[(18, 0)].bg, th.row_shade(18, 20));
        assert_ne!(b[(1, 0)].bg, b[(18, 0)].bg);
        // The ends are half circles in the colour of the shade there.
        assert_eq!(b[(0, 0)].symbol(), CAP_LEFT);
        assert_eq!(b[(0, 0)].fg, th.grad_row_start);
        assert_eq!(b[(19, 0)].symbol(), CAP_RIGHT);
        assert_eq!(b[(19, 0)].fg, th.grad_row_end);
        // The text in between is untouched.
        assert_eq!(b[(5, 0)].symbol(), "x");
    }

    #[test]
    fn without_half_circles_the_row_is_tinted_and_keeps_its_cells() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let mut b = buf_of(10);
        tint_row(
            &mut b,
            Rect::new(0, 0, 10, 1),
            &th,
            Look::FLAT,
            RowTone::Marked,
        );
        assert_eq!(b[(0, 0)].symbol(), "x");
        assert_eq!(b[(9, 0)].symbol(), "x");
        assert!((0..10).all(|x| b[(x, 0)].bg == th.sel));
    }

    #[test]
    fn in_256_colours_the_shade_is_one_solid_colour() {
        let th = Theme::named("rada", ColorDepth::Ansi256).unwrap();
        let look = Look::new(&th, IconSet::Nerd, false);
        assert!(look.caps && !look.shades);
        let mut b = buf_of(12);
        tint_row(&mut b, Rect::new(0, 0, 12, 1), &th, look, RowTone::Cursor);
        assert!((1..11).all(|x| b[(x, 0)].bg == th.cursor));
    }

    #[test]
    fn the_basic_profile_and_a_font_without_nerd_glyphs_have_no_soft_shapes() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        assert_eq!(Look::new(&th, IconSet::Nerd, true), Look::FLAT);
        assert_eq!(Look::new(&th, IconSet::UnicodeIcons, false), Look::FLAT);
        assert_eq!(Look::new(&th, IconSet::None, false), Look::FLAT);
        let t16 = Theme::named("rada", ColorDepth::Ansi16).unwrap();
        let mut c = buf_of(8);
        tint_row(
            &mut c,
            Rect::new(0, 0, 8, 1),
            &t16,
            Look::FLAT,
            RowTone::Cursor,
        );
        assert!(
            (0..8).all(|x| c[(x, 0)].bg == Color::Reset),
            "16 colours keep reverse video"
        );
    }
}
