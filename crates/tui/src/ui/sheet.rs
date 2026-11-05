//! A window made of a title, some lines and a row of buttons, as tall as that content needs and
//! no taller than the screen allows.
//!
//! When the screen is too short the buttons keep their place first, then the title, then the
//! lines; what does not fit is left out, and a click is registered only on what was drawn. When
//! the window is too narrow for its buttons side by side they go one under the other.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::popup::{self, Edge};
use super::widgets::centered;
use crate::hits::{Hits, Target};
use crate::theme::Theme;

/// What a window shows. Every part but the title may be empty.
#[derive(Default)]
pub(super) struct Sheet {
    pub title: Line<'static>,
    /// Lines under the title that always stay (a hint, the field being typed in).
    pub lead: Vec<Line<'static>>,
    /// The lines that scroll when they are too many, each with what a click on it does.
    pub body: Vec<(Line<'static>, Option<Target>)>,
    /// The most rows of `body` shown at once.
    pub body_max: Option<usize>,
    pub scroll: usize,
    /// A row of `body` that must be in view (the selected one); it wins over `scroll`.
    pub keep: Option<usize>,
    /// Lines between the body and the buttons that stay.
    pub note: Vec<Line<'static>>,
    pub buttons: Vec<(Span<'static>, Target)>,
    /// Lines under the buttons, the first to go when there is no room.
    pub after: Vec<Line<'static>>,
}

/// How many rows each part gets.
#[derive(Debug, PartialEq, Eq)]
struct Alloc {
    title: usize,
    lead: usize,
    gap_top: usize,
    body: usize,
    note: usize,
    gap_bottom: usize,
    buttons: usize,
    after: usize,
}

/// The rows of a window shared out in the order of what matters: the buttons, the title, what
/// stays under it, the note, the body, the lines under the buttons, then the blank rows.
fn allocate(rows: usize, want: &Alloc) -> Alloc {
    let mut left = rows;
    let mut take = |n: usize| {
        let t = n.min(left);
        left -= t;
        t
    };
    let buttons = take(want.buttons);
    let title = take(want.title);
    let lead = take(want.lead);
    let note = take(want.note);
    let body = take(want.body);
    let after = take(want.after);
    let gap_bottom = if buttons + after > 0 { take(1) } else { 0 };
    let gap_top = if lead + body + note > 0 { take(1) } else { 0 };
    Alloc {
        title,
        lead,
        gap_top,
        body,
        note,
        gap_bottom,
        buttons,
        after,
    }
}

/// The air between the card's edge and its text, sideways: less on a narrow card.
fn side_air(card_width: u16) -> u16 {
    if card_width.saturating_sub(2) >= 30 {
        2
    } else {
        1
    }
}

/// The text area inside a card: air at the sides, and above and below when the card is tall
/// enough for the `need` rows with it.
fn with_air(card: Rect, need: usize) -> Rect {
    let air_y = u16::from(usize::from(card.height) >= need + 2);
    let air_x = side_air(card.width + 2);
    Rect {
        x: card.x + air_x,
        y: card.y + air_y,
        width: card.width.saturating_sub(2 * air_x),
        height: card.height.saturating_sub(2 * air_y),
    }
}

/// The width the text of a window of `wanted` columns has on `area`: what lines are wrapped to.
pub(super) fn text_width(area: Rect, wanted: u16) -> usize {
    let card = wanted.min(area.width.saturating_sub(2));
    usize::from(card.saturating_sub(2 + 2 * side_air(card)))
}

/// A button's rows: as many side by side as fit in `width`, a button too wide for a row cut.
struct Row {
    spans: Vec<Span<'static>>,
    spots: Vec<(usize, usize, Target)>,
    cols: usize,
}

fn lay_out(buttons: Vec<(Span<'static>, Target)>, width: usize) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (mut span, target) in buttons {
        if span.content.width() > width {
            span.content = rada_core::display::truncate(&span.content, width).into();
        }
        let w = span.content.width();
        let fits = |r: &Row| r.cols + 2 + w <= width;
        if rows.last().is_none_or(|r| !fits(r)) {
            rows.push(Row {
                spans: Vec::new(),
                spots: Vec::new(),
                cols: 0,
            });
        }
        let Some(row) = rows.last_mut() else { continue };
        if !row.spans.is_empty() {
            row.spans.push(Span::raw("  "));
            row.cols += 2;
        }
        row.spots.push((row.cols, w, target));
        row.cols += w;
        row.spans.push(span);
    }
    rows
}

/// The card drawn, the area of its text, what each part gets and the rows of buttons.
struct Shape {
    inner: Rect,
    got: Alloc,
    button_rows: Vec<Row>,
}

/// Works out the size of the window from its content and draws its card.
fn shape(f: &mut Frame, th: &Theme, area: Rect, wanted: u16, sheet: &mut Sheet) -> Shape {
    let button_rows = lay_out(std::mem::take(&mut sheet.buttons), text_width(area, wanted));
    let body = sheet
        .body_max
        .map_or(sheet.body.len(), |m| m.min(sheet.body.len()));
    let want = Alloc {
        title: 1,
        lead: sheet.lead.len(),
        gap_top: 1,
        body,
        note: sheet.note.len(),
        gap_bottom: 1,
        buttons: button_rows.len(),
        after: sheet.after.len(),
    };
    let need = want.title
        + want.lead
        + want.gap_top
        + want.body
        + want.note
        + want.gap_bottom
        + want.buttons
        + want.after;
    let card_h = (need + 4).min(usize::from(area.height.saturating_sub(2)));
    let rect = centered(area, wanted, card_h as u16);
    let card = popup::card(f.buffer_mut(), rect, th, Edge::Own);
    let inner = with_air(card, need);
    let got = allocate(usize::from(inner.height), &want);
    Shape {
        inner,
        got,
        button_rows,
    }
}

/// The lines in the order they are drawn, the rows of the body that answer a click, and the
/// row where the buttons begin.
fn compose(sheet: Sheet, got: &Alloc) -> (Vec<Line<'static>>, Vec<(usize, Target)>, usize) {
    let scroll = match sheet.keep {
        Some(k) => k.saturating_sub(got.body.saturating_sub(1)),
        None => sheet.scroll.min(sheet.body.len().saturating_sub(got.body)),
    };
    let blank = |n: usize| (0..n).map(|_| Line::raw(""));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spots: Vec<(usize, Target)> = Vec::new();
    lines.extend(std::iter::once(sheet.title).take(got.title));
    lines.extend(blank(got.gap_top));
    lines.extend(sheet.lead.into_iter().take(got.lead));
    for (line, target) in sheet.body.into_iter().skip(scroll).take(got.body) {
        if let Some(t) = target {
            spots.push((lines.len(), t));
        }
        lines.push(line);
    }
    lines.extend(sheet.note.into_iter().take(got.note));
    lines.extend(blank(got.gap_bottom));
    let buttons_at = lines.len();
    lines.extend(std::iter::repeat_n(Line::raw(""), got.buttons));
    lines.extend(sheet.after.into_iter().take(got.after));
    (lines, spots, buttons_at)
}

/// Draws the window and registers what can be clicked in it; returns the area of its text.
pub(super) fn draw(
    f: &mut Frame,
    th: &Theme,
    area: Rect,
    wanted: u16,
    mut sheet: Sheet,
    hits: &mut Hits,
) -> Rect {
    let Shape {
        inner,
        got,
        button_rows,
    } = shape(f, th, area, wanted, &mut sheet);
    let (mut lines, spots, buttons_at) = compose(sheet, &got);
    for (i, row) in button_rows.iter().take(got.buttons).enumerate() {
        lines[buttons_at + i] = Line::from(row.spans.clone());
    }
    f.render_widget(Paragraph::new(lines), inner);
    for (y, target) in spots {
        let row = Rect {
            y: inner.y + y as u16,
            height: 1,
            ..inner
        };
        hits.add(row, target);
    }
    for (i, row) in button_rows.into_iter().take(got.buttons).enumerate() {
        for (col, w, target) in row.spots {
            let at = Rect {
                x: inner.x + col as u16,
                y: inner.y + (buttons_at + i) as u16,
                width: w as u16,
                height: 1,
            };
            hits.add(at.intersection(inner), target);
        }
    }
    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn want(body: usize, buttons: usize, after: usize) -> Alloc {
        Alloc {
            title: 1,
            lead: 0,
            gap_top: 1,
            body,
            note: 0,
            gap_bottom: 1,
            buttons,
            after,
        }
    }

    #[test]
    fn with_room_everything_is_shown() {
        let got = allocate(8, &want(2, 1, 1));
        assert_eq!((got.title, got.body, got.buttons, got.after), (1, 2, 1, 1));
        assert_eq!((got.gap_top, got.gap_bottom), (1, 1));
    }

    #[test]
    fn the_buttons_are_the_last_thing_to_go() {
        for rows in 0..10 {
            let got = allocate(rows, &want(5, 2, 1));
            assert_eq!(got.buttons, rows.min(2), "{rows} rows");
        }
    }

    #[test]
    fn never_more_rows_than_there_are() {
        for rows in 0..20 {
            let g = allocate(rows, &want(7, 3, 2));
            let sum = g.title + g.lead + g.gap_top + g.body + g.note + g.gap_bottom;
            assert!(sum + g.buttons + g.after <= rows, "{rows}: {g:?}");
        }
    }

    #[test]
    fn narrow_windows_stack_their_buttons() {
        let b = |t: &str| (Span::raw(t.to_string()), Target::Window);
        let rows = lay_out(vec![b("aaaaaaaa"), b("bbbbbbbb"), b("cc")], 18);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].spots.len(), 2);
        assert_eq!(rows[1].spots.len(), 1);
        let one = lay_out(vec![b("a very long button indeed")], 10);
        assert!(one[0].cols <= 10);
    }
}
