//! Popups: the rounded border and the surface that every menu, window and notification card
//! share, and the small parts they are made of — pills, buttons, badges, strips of a status
//! colour, rules.
//!
//! A card has no shadow and no surface of its own: the inside of a menu or a notification is
//! the colour of what lies behind it (the theme's background), and so are the cells of its
//! border, whose line is `popup_border`; a cell of the border is never another colour than the
//! cells just outside and just inside it. A window dims the screen behind it and is the dimmed
//! tone a little lighter (`modal_surface`), border and inside alike. The basic profile draws
//! the same, and its backend turns the border into `+-|`.
//!
//! Nothing here picks a colour: every one comes from the theme.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::soft::{CAP_LEFT, CAP_RIGHT, Look};
use crate::icons::IconSet;
use crate::theme::{ColorDepth, Theme};

/// What the parts of a popup need to know to draw themselves.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub th: &'a Theme,
    pub look: Look,
    pub set: IconSet,
    pub basic: bool,
}

impl<'a> Ctx<'a> {
    pub fn of(app: &'a crate::app::App) -> Ctx<'a> {
        Ctx {
            th: &app.th,
            look: Look::of(app),
            set: app.icons,
            basic: app.basic,
        }
    }

    /// The style of the surface itself.
    pub fn surface(&self) -> Style {
        Style::default().bg(self.th.popup)
    }

    pub fn base(&self) -> Style {
        self.th.base().bg(self.th.popup)
    }

    pub fn dim(&self) -> Style {
        self.th.dim().bg(self.th.popup)
    }

    pub fn faint(&self) -> Style {
        self.th.faint().bg(self.th.popup)
    }

    pub fn accent(&self) -> Style {
        self.th.accent_style().bg(self.th.popup)
    }
}

/// What a status colour stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// The accent: what is going on, what is chosen.
    Accent,
    Ok,
    Warn,
    Danger,
    /// Something that is only said.
    Info,
}

impl Tone {
    pub fn color(self, th: &Theme) -> Color {
        match self {
            Tone::Accent | Tone::Info => th.accent,
            Tone::Ok => th.success,
            Tone::Warn => th.warn,
            Tone::Danger => th.error,
        }
    }
}

// ------------------------------------------------------------------------------- the card

/// What the cells of the border of a card are filled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// What is just outside each cell: a menu or a notification is the colour of what lies
    /// behind it, and the line does not stand out of it with a band of another colour.
    Behind,
    /// The colour of the card itself: a window, whose inside is a tone of its own.
    Own,
}

/// Paint a card over `r`: the inside in the popup colour, the rounded border around it.
/// Returns what is inside the border.
pub fn card(buf: &mut Buffer, r: Rect, th: &Theme, edge: Edge) -> Rect {
    let r = r.intersection(buf.area);
    if r.width < 2 || r.height < 2 {
        return Rect::default();
    }
    let area = buf.area;
    // What lies just outside each cell of the border (straight out; a corner, along the
    // diagonal), before anything is drawn: a cell with no colour of its own is the theme's
    // plain background. A cell at the edge of the screen has no outside: its own colour stands.
    let outside = |buf: &Buffer, x: u16, y: u16| -> Color {
        let dx = i32::from(x == r.right() - 1) - i32::from(x == r.x);
        let dy = i32::from(y == r.bottom() - 1) - i32::from(y == r.y);
        let (ox, oy) = (x as i32 + dx, y as i32 + dy);
        let inside_screen = ox >= area.x as i32
            && oy >= area.y as i32
            && ox < area.right() as i32
            && oy < area.bottom() as i32;
        let at = if inside_screen {
            (ox as u16, oy as u16)
        } else {
            (x, y)
        };
        match buf[at].bg {
            Color::Reset => th.popup,
            c => c,
        }
    };
    let on_border =
        |x: u16, y: u16| x == r.x || x == r.right() - 1 || y == r.y || y == r.bottom() - 1;
    let colours: Vec<Color> = (r.y..r.bottom())
        .flat_map(|y| (r.x..r.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| on_border(x, y))
        .map(|(x, y)| outside(buf, x, y))
        .collect();
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let c = &mut buf[(x, y)];
            c.reset();
            c.set_bg(th.popup);
        }
    }
    let line = Style::default().fg(th.popup_border);
    let mut colours = colours.into_iter();
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            if !on_border(x, y) {
                continue;
            }
            let outside = colours.next().unwrap_or(th.popup);
            let bg = match edge {
                Edge::Behind => outside,
                Edge::Own => th.popup,
            };
            let glyph = match (x == r.x, x == r.right() - 1, y == r.y, y == r.bottom() - 1) {
                (true, _, true, _) => "╭",
                (_, true, true, _) => "╮",
                (true, _, _, true) => "╰",
                (_, true, _, true) => "╯",
                (_, _, true, _) | (_, _, _, true) => "─",
                _ => "│",
            };
            buf[(x, y)].set_symbol(glyph).set_style(line.bg(bg));
        }
    }
    Rect {
        x: r.x + 1,
        y: r.y + 1,
        width: r.width - 2,
        height: r.height - 2,
    }
}

/// Draw `symbol` in `fg` over a cell of the border, keeping the colour behind it: the strip
/// of a status colour on the side of a card.
pub fn mark(buf: &mut Buffer, x: u16, y: u16, symbol: &str, fg: Color) {
    buf[(x, y)].set_symbol(symbol).set_fg(fg);
}

/// Write `line` over the cells of `row` (one row of the card), left aligned.
pub fn put(buf: &mut Buffer, row: Rect, line: Line<'_>) {
    if row.width == 0 || row.height == 0 {
        return;
    }
    buf.set_line(row.x, row.y, &line, row.width);
}

// ------------------------------------------------------------------------------ rules

/// A thin rule across `width` cells.
pub fn rule(ctx: &Ctx, width: usize) -> Span<'static> {
    Span::styled(
        "─".repeat(width),
        Style::default().fg(ctx.th.popup_border).bg(ctx.th.popup),
    )
}

/// A short, light dashed line between the groups of a menu.
pub fn dashes(ctx: &Ctx, width: usize) -> Span<'static> {
    Span::styled(
        "╌".repeat(width),
        Style::default().fg(ctx.th.popup_border).bg(ctx.th.popup),
    )
}

// ------------------------------------------------------------------------------- pills

/// A pill on the popup: `text_style` on `fill`, with half circles where the font has them.
pub fn pill(ctx: &Ctx, text: &str, fill: Color, text_style: Style) -> Vec<Span<'static>> {
    let body = text_style.bg(fill);
    if ctx.look.caps {
        let edge = Style::default().fg(fill).bg(ctx.th.popup);
        vec![
            Span::styled(CAP_LEFT, edge),
            Span::styled(text.to_string(), body),
            Span::styled(CAP_RIGHT, edge),
        ]
    } else if ctx.th.depth == ColorDepth::Ansi16 {
        vec![Span::styled(
            format!(" {text} "),
            text_style.add_modifier(Modifier::REVERSED),
        )]
    } else {
        vec![Span::styled(format!(" {text} "), body)]
    }
}

/// How a button is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    /// The main thing to do, filled with the colour of its tone.
    Primary(Tone),
    /// A way out: filled with the quiet key colour.
    Quiet,
    /// Cannot be used now.
    Disabled,
}

/// A button: the label and, inside the same pill, the key that does it (`Annulla  Esc`).
pub fn button(
    ctx: &Ctx,
    label: &str,
    key: Option<&str>,
    kind: Btn,
    focused: bool,
) -> Vec<Span<'static>> {
    let th = ctx.th;
    let (fill, text, keyc) = match kind {
        Btn::Primary(tone) => (tone.color(th), th.on_accent, th.on_accent),
        Btn::Quiet => (th.key_bg, th.text, th.text_dim),
        Btn::Disabled => (th.button, th.muted, th.muted),
    };
    let mut label_style = Style::default().fg(text).add_modifier(Modifier::BOLD);
    if focused && kind != Btn::Disabled {
        label_style = label_style.add_modifier(Modifier::UNDERLINED);
    }
    let body = match key {
        Some(k) => format!(" {label}  {k} "),
        None => format!(" {label} "),
    };
    if th.depth == ColorDepth::Ansi16 && !ctx.look.caps {
        return vec![Span::styled(
            body,
            label_style.add_modifier(Modifier::REVERSED),
        )];
    }
    let mut spans = Vec::new();
    let (cl, cr) = if ctx.look.caps {
        (CAP_LEFT, CAP_RIGHT)
    } else {
        ("", "")
    };
    let edge = Style::default().fg(fill).bg(th.popup);
    if !cl.is_empty() {
        spans.push(Span::styled(cl, edge));
    }
    // The label, the gap and the key as spans of their own: only the label is underlined when
    // the keyboard is on the button.
    let pad = if ctx.look.caps { "" } else { " " };
    spans.push(Span::styled(format!("{pad}{label}"), label_style.bg(fill)));
    match key {
        Some(k) => {
            spans.push(Span::styled("  ", Style::default().bg(fill)));
            spans.push(Span::styled(
                format!("{k}{pad}"),
                Style::default().fg(keyc).bg(fill),
            ));
        }
        None => spans.push(Span::styled(pad.to_string(), Style::default().bg(fill))),
    }
    if !cr.is_empty() {
        spans.push(Span::styled(cr, edge));
    }
    spans
}

/// A small pill that labels a row (`nuovo`, `esiste già`, `predefinito`): the colour of its
/// tone on a light tint of it.
pub fn badge(ctx: &Ctx, text: &str, tone: Tone) -> Vec<Span<'static>> {
    let th = ctx.th;
    let c = tone.color(th);
    let fill = th.tint_share(th.popup, c, if th.light { 0.16 } else { 0.22 });
    pill(ctx, text, fill, Style::default().fg(c))
}

/// A strip across `width` cells in the tint of a status colour, with its mark and a message:
/// the warning of a plan, the note that something cannot be undone.
pub fn strip(ctx: &Ctx, tone: Tone, mark: &str, text: &str, width: usize) -> Vec<Span<'static>> {
    let th = ctx.th;
    let c = tone.color(th);
    let fill = th.tint_on(th.popup, c);
    let style = Style::default().fg(c).bg(fill);
    let lead = if mark.is_empty() {
        " ".to_string()
    } else {
        format!(" {mark} ")
    };
    let caps = ctx.look.caps;
    let inner_w = width.saturating_sub(if caps { 2 } else { 0 });
    let text = crate::ui::widgets::pad(
        &rada_core::display::truncate(text, inner_w.saturating_sub(lead.width() + 1)),
        inner_w.saturating_sub(lead.width()),
    );
    let mut spans = Vec::new();
    if caps {
        spans.push(Span::styled(
            CAP_LEFT,
            Style::default().fg(fill).bg(th.popup),
        ));
    }
    spans.push(Span::styled(lead, style));
    spans.push(Span::styled(text, style));
    if caps {
        spans.push(Span::styled(
            CAP_RIGHT,
            Style::default().fg(fill).bg(th.popup),
        ));
    }
    spans
}

/// The head of a window: a round mark in a tinted circle, the title in bold and a quiet line
/// under it. `mark` may be empty (an icon set without symbols): the circle is then left out.
pub fn headline(
    ctx: &Ctx,
    mark: &str,
    tone: Tone,
    title: &str,
    summary: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let th = ctx.th;
    let c = tone.color(th);
    let mut first: Vec<Span<'static>> = Vec::new();
    let mut lead = 0;
    if !mark.is_empty() {
        let fill = th.tint_share(th.popup, c, if th.light { 0.18 } else { 0.28 });
        let spans = pill(ctx, &format!(" {mark} "), fill, Style::default().fg(c));
        lead = spans.iter().map(|s| s.content.width()).sum::<usize>() + 2;
        first.extend(spans);
        first.push(Span::styled("  ", ctx.surface()));
    }
    first.push(Span::styled(
        rada_core::display::truncate(title, width.saturating_sub(lead)),
        ctx.base().add_modifier(Modifier::BOLD),
    ));
    let mut lines = vec![Line::from(first)];
    if !summary.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" ".repeat(lead), ctx.surface()),
            Span::styled(
                rada_core::display::truncate(summary, width.saturating_sub(lead)),
                ctx.dim(),
            ),
        ]));
    }
    lines
}

// ------------------------------------------------------------------------- tinted rows

/// What a row of a popup is filled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowFill {
    /// The row under the cursor: the shade of the list's cursor row, white text.
    Cursor,
    /// The row that opened the submenu in front: a quiet tint of the accent.
    Opener,
    /// The cursor on something that takes files away: the colour of errors.
    Danger,
}

/// Colour the cells of `row` (one row) as `fill` says, with round ends where the font has
/// them: the half circles are the row's colour on the popup's. The text already in the cells
/// is kept.
pub fn tint_row(buf: &mut Buffer, row: Rect, ctx: &Ctx, fill: RowFill) {
    let th = ctx.th;
    if row.width == 0 || row.height == 0 || th.depth == ColorDepth::Ansi16 {
        return;
    }
    let w = row.width as usize;
    let colour = |x: usize| match fill {
        RowFill::Cursor if ctx.look.shades => th.row_shade(x, w),
        RowFill::Cursor => th.cursor,
        RowFill::Opener => th.tint_share(th.popup, th.accent, if th.light { 0.14 } else { 0.2 }),
        RowFill::Danger => th.error,
    };
    for i in 0..w {
        buf[(row.x + i as u16, row.y)].set_bg(colour(i));
    }
    if ctx.look.caps && w >= 3 {
        for (i, glyph) in [(0, CAP_LEFT), (w - 1, CAP_RIGHT)] {
            let cell = &mut buf[(row.x + i as u16, row.y)];
            cell.set_symbol(glyph);
            cell.set_style(Style::default().fg(colour(i)).bg(th.popup));
        }
    }
}

/// The colour of text on a row filled as `fill`.
pub fn row_text(th: &Theme, fill: RowFill) -> Style {
    match fill {
        RowFill::Cursor => Style::default()
            .fg(th.cursor_text)
            .add_modifier(Modifier::BOLD),
        RowFill::Danger => Style::default()
            .fg(th.on_accent)
            .add_modifier(Modifier::BOLD),
        RowFill::Opener => Style::default().fg(th.text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorDepth;

    fn ctx_for(th: &Theme, caps: bool) -> Ctx<'_> {
        Ctx {
            th,
            look: Look {
                caps,
                shades: caps && th.shades(),
            },
            set: if caps {
                IconSet::Nerd
            } else {
                IconSet::UnicodeStd
            },
            basic: false,
        }
    }

    /// A screen of text on the plain background (what a menu is filled with).
    fn painted(th: &Theme) -> Buffer {
        painted_on(th, th.popup)
    }

    fn painted_on(th: &Theme, bg: Color) -> Buffer {
        let mut b = Buffer::empty(Rect::new(0, 0, 40, 12));
        for y in 0..12 {
            for x in 0..40 {
                let c = &mut b[(x, y)];
                c.set_symbol("x");
                c.set_bg(bg);
                c.set_fg(th.text);
            }
        }
        b
    }

    /// The cells of the border of the card at `r` with the cells just outside and just inside
    /// each of them.
    fn edge_triples(b: &Buffer, r: Rect) -> Vec<((u16, u16), Option<(u16, u16)>, (u16, u16))> {
        let mut out = Vec::new();
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let (left, right, top, bottom) =
                    (x == r.x, x == r.right() - 1, y == r.y, y == r.bottom() - 1);
                if !(left || right || top || bottom) {
                    continue;
                }
                // Straight out of the card and straight into it (a corner: along the diagonal).
                let dx: i32 = if left {
                    -1
                } else if right {
                    1
                } else {
                    0
                };
                let dy: i32 = if top {
                    -1
                } else if bottom {
                    1
                } else {
                    0
                };
                let outside = (x as i32 + dx, y as i32 + dy);
                let inside = ((x as i32 - dx) as u16, (y as i32 - dy) as u16);
                let area = b.area;
                let outside = (outside.0 >= 0
                    && outside.1 >= 0
                    && (outside.0 as u16) < area.right()
                    && (outside.1 as u16) < area.bottom())
                .then_some((outside.0 as u16, outside.1 as u16));
                out.push(((x, y), outside, inside));
            }
        }
        out
    }

    #[test]
    fn a_card_has_a_rounded_border_and_no_shadow() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let mut b = painted(&th);
        let r = Rect::new(4, 2, 20, 6);
        let inner = card(&mut b, r, &th, Edge::Behind);
        assert_eq!(inner, Rect::new(5, 3, 18, 4));
        assert_eq!(b[(4, 4)].symbol(), "│");
        assert_eq!(b[(4, 4)].fg, th.popup_border);
        assert_eq!(b[(4, 2)].symbol(), "╭");
        assert_eq!(b[(23, 2)].symbol(), "╮");
        assert_eq!(b[(4, 7)].symbol(), "╰");
        assert_eq!(b[(23, 7)].symbol(), "╯");
        // Nothing beyond the border: not a cell on the right, nor one below.
        for y in 2..9 {
            assert_eq!(b[(24, y)].bg, th.popup, "right of the card, row {y}");
        }
        for x in 4..25 {
            assert_eq!(b[(x, 8)].bg, th.popup, "under the card, column {x}");
        }
    }

    #[test]
    fn a_cell_of_the_border_is_never_another_colour_than_the_cells_beside_it() {
        // Over a plain screen, in a dark theme and in Carta: the cell of the border, the cell
        // just outside it and the cell just inside it are one colour, corners included.
        for name in ["rada", "rada-light"] {
            let th = Theme::named(name, ColorDepth::True).unwrap();
            let mut b = painted(&th);
            let r = Rect::new(4, 2, 20, 6);
            card(&mut b, r, &th, Edge::Behind);
            for (cell, outside, inside) in edge_triples(&b, r) {
                let bg = b[cell].bg;
                assert_eq!(bg, th.popup, "{name}: {cell:?}");
                if let Some(o) = outside {
                    assert_eq!(b[o].bg, bg, "{name}: outside {cell:?}");
                }
                assert_eq!(b[inside].bg, bg, "{name}: inside {cell:?}");
            }
        }
    }

    #[test]
    fn over_a_coloured_row_the_line_takes_the_colour_of_what_is_outside_it() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let mut b = painted(&th);
        for x in 0..40 {
            b[(x, 4)].set_bg(th.sel);
        }
        let r = Rect::new(10, 2, 20, 6);
        card(&mut b, r, &th, Edge::Behind);
        // On the sides the row goes on outside the line, and so does its colour under it.
        assert_eq!(b[(10, 4)].bg, th.sel);
        assert_eq!(b[(29, 4)].bg, th.sel);
        // Over the top the colour outside is the plain one: the row's colour is not carried
        // into the line (nor, then, a strip of it under the line).
        let mut b = painted(&th);
        for x in 0..40 {
            b[(x, 2)].set_bg(th.sel);
        }
        card(&mut b, r, &th, Edge::Behind);
        assert_eq!(b[(15, 2)].bg, th.popup);
        // The inside is the plain colour.
        assert_eq!(b[(11, 4)].bg, th.popup);
    }

    #[test]
    fn a_window_has_one_tone_for_its_border_and_its_inside() {
        for name in ["rada", "rada-light"] {
            let mut th = Theme::named(name, ColorDepth::True).unwrap();
            let behind = th.dimmed(th.canvas);
            th.popup = th.modal_surface();
            let mut b = painted_on(&th, behind);
            let r = Rect::new(4, 2, 20, 6);
            card(&mut b, r, &th, Edge::Own);
            for (cell, _, inside) in edge_triples(&b, r) {
                assert_eq!(b[cell].bg, th.popup, "{name}: {cell:?}");
                assert_eq!(b[inside].bg, th.popup, "{name}: inside {cell:?}");
            }
            // The screen around it stays the dimmed tone: the window is a little lighter.
            assert_eq!(b[(30, 5)].bg, behind, "{name}");
            assert_ne!(behind, th.popup, "{name}");
        }
    }

    #[test]
    fn the_side_of_a_card_in_a_colour_keeps_the_colour_behind_it() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let mut b = painted(&th);
        let r = Rect::new(4, 2, 20, 6);
        card(&mut b, r, &th, Edge::Behind);
        mark(&mut b, 4, 4, "┃", th.success);
        assert_eq!(b[(4, 4)].symbol(), "┃");
        assert_eq!(b[(4, 4)].fg, th.success);
        assert_eq!(b[(4, 4)].bg, th.popup);
    }

    #[test]
    fn a_card_at_the_edge_of_the_screen_is_cut_not_broken() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let mut b = painted(&th);
        let inner = card(&mut b, Rect::new(30, 8, 20, 10), &th, Edge::Behind);
        assert!(inner.right() <= 40 && inner.bottom() <= 12);
        let inner = card(&mut b, Rect::new(0, 0, 1, 1), &th, Edge::Behind);
        assert_eq!(inner, Rect::default());
    }

    #[test]
    fn a_button_is_as_wide_with_or_without_round_ends() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let width = |caps: bool| -> usize {
            let ctx = ctx_for(&th, caps);
            button(&ctx, "Annulla", Some("Esc"), Btn::Quiet, false)
                .iter()
                .map(|s| s.content.width())
                .sum()
        };
        assert_eq!(width(true), width(false));
    }

    #[test]
    fn buttons_and_badges_use_the_tokens_of_their_tone() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let ctx = ctx_for(&th, true);
        let b = button(
            &ctx,
            "Elimina",
            Some("Invio"),
            Btn::Primary(Tone::Danger),
            false,
        );
        assert!(b.iter().any(|s| s.style.bg == Some(th.error)));
        let b = button(&ctx, "Annulla", Some("Esc"), Btn::Quiet, false);
        assert!(b.iter().any(|s| s.style.bg == Some(th.key_bg)));
        let g = badge(&ctx, "nuovo", Tone::Ok);
        assert!(g.iter().any(|s| s.style.fg == Some(th.success)));
        let s = strip(&ctx, Tone::Warn, "▲", "esiste già", 30);
        let w: usize = s.iter().map(|x| x.content.width()).sum();
        assert_eq!(w, 30);
    }
}
