//! The boxes of the panels view: a rounded border with `[ TITLE ]` on its top edge, the
//! border in the accent colour while the keyboard is in the box and a quiet one otherwise.
//!
//! The cells of the border and the space between boxes keep the theme's background; the inside
//! is filled with the theme's `inner` tone, a step away from it.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::theme::{ColorDepth, Theme};

/// What goes on the top edge of a box besides its name: spans on the right (the git branch of
/// the folder).
pub struct Edge {
    pub right: Vec<Span<'static>>,
}

impl Edge {
    pub const NONE: Edge = Edge { right: Vec::new() };
}

/// Draw a box in `area` and return what is inside it.
pub fn draw(
    f: &mut Frame,
    area: Rect,
    th: &Theme,
    title: &str,
    edge: &Edge,
    focused: bool,
) -> Rect {
    if area.width < 4 || area.height < 3 {
        return Rect::default();
    }
    let buf = f.buffer_mut();
    let colour = if focused { th.accent } else { th.muted };
    // The line, on the background around the box.
    let line = th.on(Style::default()).fg(colour);
    let fill = Style::default().bg(th.inner);
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.right() - 1, area.bottom() - 1);
    for x in x0 + 1..x1 {
        put(buf, x, y0, "─", line);
        put(buf, x, y1, "─", line);
    }
    for y in y0 + 1..y1 {
        put(buf, x0, y, "│", line);
        put(buf, x1, y, "│", line);
        for x in x0 + 1..x1 {
            put(buf, x, y, " ", fill);
        }
    }
    put(buf, x0, y0, "╭", line);
    put(buf, x1, y0, "╮", line);
    put(buf, x0, y1, "╰", line);
    put(buf, x1, y1, "╯", line);

    // `[ NAME ]` after the corner and one cell of line.
    let room = (area.width as usize).saturating_sub(6);
    let name = rada_core::display::truncate(title, room.saturating_sub(4));
    let name_style = if focused {
        th.accent_style().add_modifier(Modifier::BOLD)
    } else {
        th.dim().add_modifier(Modifier::BOLD)
    };
    let mut x = x0 + 2;
    for (text, style) in [("[ ", line), (name.as_str(), name_style), (" ]", line)] {
        x = text_at(buf, x, y0, text, style);
    }
    let used = x - (x0 + 2);

    // The right side: a space, the spans, a space, then line up to the corner.
    let right_w: usize = edge.right.iter().map(|s| s.content.width()).sum();
    if right_w > 0 {
        let left_end = x0 + 2 + used + 1;
        let want = right_w as u16 + 2;
        let start = x1.saturating_sub(1 + want);
        if start >= left_end {
            let mut x = start;
            x = text_at(buf, x, y0, " ", th.on(Style::default()));
            for s in &edge.right {
                x = text_at(
                    buf,
                    x,
                    y0,
                    &s.content,
                    th.on(Style::default()).patch(s.style),
                );
            }
            text_at(buf, x, y0, " ", th.on(Style::default()));
        }
    }
    Rect {
        x: x0 + 1,
        y: y0 + 1,
        width: area.width - 2,
        height: area.height - 2,
    }
}

/// What the text of a box may use: one empty column on each side of `inner` and, in a tall
/// window (`airy`), one empty row under the title. Nothing is left at the bottom.
pub fn pad(inner: Rect, airy: bool) -> Rect {
    let top = u16::from(airy).min(inner.height);
    let side = u16::from(inner.width >= 3);
    Rect {
        x: inner.x + side,
        y: inner.y + top,
        width: inner.width - 2 * side,
        height: inner.height - top,
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, symbol: &str, style: Style) {
    let cell = &mut buf[(x, y)];
    cell.set_symbol(symbol);
    cell.set_style(style);
}

/// Write `text` at (x, y) and return the cell after it.
fn text_at(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) -> u16 {
    let (end, _) = buf.set_stringn(x, y, text, usize::MAX >> 1, style);
    end
}

/// Give the cells of `area` that still have the theme's own background the inside tone of the
/// boxes. The text drawn in a box takes its style from the theme and so carries the outer
/// background; this puts it right once the box is done. Cells with a colour of their own (the
/// selected row, a pill, the pixels of a picture) are left alone.
pub fn settle(buf: &mut Buffer, area: Rect, th: &Theme) {
    // A theme that leaves the terminal's background alone has none of its own around the
    // boxes: what is on the terminal's (the end of a pill) is what the inside replaces.
    let outer = th.bg.unwrap_or(ratatui::style::Color::Reset);
    if th.depth == ColorDepth::Ansi16 || th.inner == outer {
        return;
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            if cell.bg == outer {
                cell.bg = th.inner;
            }
        }
    }
}
