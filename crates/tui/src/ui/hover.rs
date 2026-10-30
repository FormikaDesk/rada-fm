//! The light highlight of whatever the pointer is over.
//!
//! It is painted last, over the finished frame, from the map of clicks: the thing under the
//! pointer is the target the map has there, and its rectangle is lit. Nothing is drawn twice
//! and no widget knows about it. A row the cursor is on, or that is selected, is left as it
//! is; so is anything already filled with a stronger tone. The colour is the theme's `mark`,
//! the tone its tokens call "a row under the mouse".

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use super::soft::{CAP_LEFT, CAP_RIGHT};
use crate::app::App;
use crate::hits::Target;
use crate::theme::{ColorDepth, Theme};

/// Light what is under the pointer, and note it as what the screen shows.
pub fn paint(f: &mut Frame, app: &mut App) {
    let lit = under_pointer(app);
    // The layout may have moved under a pointer that did not: what the next move compares
    // with is what is drawn here.
    if lit.as_ref().map(|(_, t)| t) != app.hover.over.as_ref() {
        app.hover.over = lit.as_ref().map(|(_, t)| t.clone());
        app.redraw.dirty = true;
    }
    if let Some((area, target)) = lit
        && !is_current(app, &target)
    {
        shade(f.buffer_mut(), area, &app.settings.th);
    }
}

/// What is lit, and where: the target under the pointer in the frame just drawn. A checkbox
/// lights its row and a tree arrow its place.
fn under_pointer(app: &App) -> Option<(Rect, Target)> {
    let (x, y) = app.hover.at.filter(|_| app.settings.mouse)?;
    let (rect, under) = app.frame.hits.rect_at(x, y)?;
    let target = under.lit()?;
    let area = if &target == under {
        rect
    } else {
        app.frame.hits.find(&target).unwrap_or(rect)
    };
    Some((area, target))
}

/// The row of the list is already marked out: the cursor is on it or it is selected.
fn is_current(app: &App, target: &Target) -> bool {
    let Target::Row(i) = target else {
        return false;
    };
    app.tab.sel.cursor == *i
        || app
            .tab
            .entry_at(*i)
            .is_some_and(|e| app.tab.sel.marked.contains(&e.name))
}

/// Light `area`: what lies on a plain ground or on a button goes to the hover tone of that
/// ground (`Theme::hover_over`); a field
/// (which is that tone already) and the main button (whose text would not read on it) are
/// underlined instead; what is filled with a stronger tone (the cursor, a selection, the tab
/// in front) is left alone. With 16 colours there are no tones to tell apart, so everything is
/// underlined unless it is in reverse video already.
fn shade(buf: &mut Buffer, area: Rect, th: &Theme) {
    let area = area.intersection(buf.area);
    if th.depth == ColorDepth::Ansi16 {
        if !cells(area).any(|p| buf[p].modifier.contains(Modifier::REVERSED)) {
            underline(buf, area);
        }
        return;
    }
    let base = dominant_bg(buf, area);
    if th.hoverable_grounds().contains(&base) {
        let tone = th.hover_over(base);
        for p in cells(area) {
            if buf[p].bg == base {
                buf[p].set_bg(tone);
            }
        }
        recolour_caps(buf, area, base, tone);
    } else if [th.field, th.accent].contains(&base) {
        underline(buf, area);
    }
}

/// The half circles at the ends of a pill are drawn in its colour: they follow it.
fn recolour_caps(buf: &mut Buffer, area: Rect, from: Color, to: Color) {
    for p in cells(area) {
        let cell = &mut buf[p];
        if (cell.symbol() == CAP_LEFT || cell.symbol() == CAP_RIGHT) && cell.fg == from {
            cell.set_fg(to);
        }
    }
}

/// Every cell of `area`.
fn cells(area: Rect) -> impl Iterator<Item = (u16, u16)> {
    (area.y..area.bottom()).flat_map(move |y| (area.x..area.right()).map(move |x| (x, y)))
}

/// The background most of `area` has.
fn dominant_bg(buf: &Buffer, area: Rect) -> Color {
    let mut seen: Vec<(Color, usize)> = Vec::new();
    for p in cells(area) {
        let bg = buf[p].bg;
        match seen.iter_mut().find(|(c, _)| *c == bg) {
            Some((_, n)) => *n += 1,
            None => seen.push((bg, 1)),
        }
    }
    seen.into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or(Color::Reset, |(c, _)| c)
}

/// Underline all of `area` but the half circles at the ends of a pill.
fn underline(buf: &mut Buffer, area: Rect) {
    for p in cells(area) {
        let cell = &mut buf[p];
        if cell.symbol() != CAP_LEFT && cell.symbol() != CAP_RIGHT {
            cell.modifier.insert(Modifier::UNDERLINED);
        }
    }
}
