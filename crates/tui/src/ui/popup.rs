//! Popups: the surface, the rounded border and the shadow that every menu, window and
//! notification card shares, and the small parts they are made of — pills, buttons, badges,
//! strips of a status colour, rules.
//!
//! A card is a surface a little lighter than the window behind it (`popup`), a thin rounded
//! border one step lighter again (`popup_border`), and a shadow of one cell on the right and
//! at the bottom: the colours underneath, darkened (true colour only). The four corner cells
//! take the colour of what is behind them, so that the corners read as round. The basic
//! profile draws the same, and its backend turns the border into `+-|`.
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

/// Darken what lies right of and below `r` (one cell): the shadow. True colour only; a cell
/// with no colour of its own (the terminal's) is taken as the theme's canvas.
pub fn shadow(buf: &mut Buffer, r: Rect, th: &Theme) {
    if th.depth != ColorDepth::True || r.width == 0 || r.height == 0 {
        return;
    }
    let area = buf.area;
    let mut dim = |x: u16, y: u16| {
        if x >= area.right() || y >= area.bottom() {
            return;
        }
        let cell = &mut buf[(x, y)];
        let bg = if cell.bg == Color::Reset {
            th.canvas
        } else {
            cell.bg
        };
        cell.set_bg(th.shadow(bg));
        if cell.fg != Color::Reset {
            cell.set_fg(th.shadow(cell.fg));
        }
    };
    for y in r.y + 1..=r.bottom() {
        dim(r.right(), y);
    }
    for x in r.x + 1..r.right() {
        dim(x, r.bottom());
    }
}

/// Paint a card over `r`: shadow, surface, rounded border. Returns what is inside the border.
pub fn card(buf: &mut Buffer, r: Rect, th: &Theme) -> Rect {
    let r = r.intersection(buf.area);
    if r.width < 2 || r.height < 2 {
        return Rect::default();
    }
    // What the four corners sit on: they take it, so that they read as round.
    let corner_bg = [
        (r.x, r.y),
        (r.right() - 1, r.y),
        (r.x, r.bottom() - 1),
        (r.right() - 1, r.bottom() - 1),
    ]
    .map(|(x, y)| buf[(x, y)].bg);
    shadow(buf, r, th);
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let c = &mut buf[(x, y)];
            c.reset();
            c.set_bg(th.popup);
        }
    }
    let edge = Style::default().fg(th.popup_border).bg(th.popup);
    for x in r.x + 1..r.right() - 1 {
        buf[(x, r.y)].set_symbol("─").set_style(edge);
        buf[(x, r.bottom() - 1)].set_symbol("─").set_style(edge);
    }
    for y in r.y + 1..r.bottom() - 1 {
        buf[(r.x, y)].set_symbol("│").set_style(edge);
        buf[(r.right() - 1, y)].set_symbol("│").set_style(edge);
    }
    for (i, (x, y, glyph)) in [
        (r.x, r.y, "╭"),
        (r.right() - 1, r.y, "╮"),
        (r.x, r.bottom() - 1, "╰"),
        (r.right() - 1, r.bottom() - 1, "╯"),
    ]
    .into_iter()
    .enumerate()
    {
        let c = &mut buf[(x, y)];
        c.set_symbol(glyph).set_style(edge);
        c.set_bg(corner_bg[i]);
    }
    Rect {
        x: r.x + 1,
        y: r.y + 1,
        width: r.width - 2,
        height: r.height - 2,
    }
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

    fn painted(th: &Theme) -> Buffer {
        let mut b = Buffer::empty(Rect::new(0, 0, 40, 12));
        for y in 0..12 {
            for x in 0..40 {
                let c = &mut b[(x, y)];
                c.set_symbol("x");
                c.set_bg(th.canvas);
                c.set_fg(th.text);
            }
        }
        b
    }

    #[test]
    fn a_card_has_a_surface_a_rounded_border_and_a_shadow() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let mut b = painted(&th);
        let r = Rect::new(4, 2, 20, 6);
        let inner = card(&mut b, r, &th);
        assert_eq!(inner, Rect::new(5, 3, 18, 4));
        // Surface and border come from the tokens.
        assert_eq!(b[(10, 4)].bg, th.popup);
        assert_eq!(b[(4, 4)].symbol(), "│");
        assert_eq!(b[(4, 4)].fg, th.popup_border);
        // Round corners take the colour of what is behind them.
        assert_eq!(b[(4, 2)].symbol(), "╭");
        assert_eq!(b[(4, 2)].bg, th.canvas);
        assert_eq!(b[(23, 7)].symbol(), "╯");
        // The shadow: one cell right and below, the colours underneath darkened.
        assert_eq!(b[(24, 5)].bg, th.shadow(th.canvas));
        assert_eq!(b[(10, 8)].bg, th.shadow(th.canvas));
        assert_eq!(b[(24, 8)].bg, th.shadow(th.canvas), "the corner");
        // Nothing beyond that.
        assert_eq!(b[(25, 5)].bg, th.canvas);
        assert_eq!(b[(10, 9)].bg, th.canvas);
        // The shadow is darker than what it covers, on a light theme as on a dark one.
        for name in ["rada", "rada-light"] {
            let t = Theme::named(name, ColorDepth::True).unwrap();
            assert_ne!(t.shadow(t.canvas), t.canvas, "{name}");
        }
    }

    #[test]
    fn below_true_colour_there_is_no_shadow() {
        let th = Theme::named("rada", ColorDepth::Ansi256).unwrap();
        let mut b = painted(&th);
        card(&mut b, Rect::new(4, 2, 20, 6), &th);
        assert_eq!(b[(24, 5)].bg, th.canvas);
        assert_eq!(b[(10, 8)].bg, th.canvas);
    }

    #[test]
    fn a_card_at_the_edge_of_the_screen_is_cut_not_broken() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let mut b = painted(&th);
        let inner = card(&mut b, Rect::new(30, 8, 20, 10), &th);
        assert!(inner.right() <= 40 && inner.bottom() <= 12);
        let inner = card(&mut b, Rect::new(0, 0, 1, 1), &th);
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
