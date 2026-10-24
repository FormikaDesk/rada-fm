//! The Columns view, as in the Finder: the folder above (the one you are in marked), the
//! folder you are in, and what the item under the cursor holds, or its preview.
//!
//! Left goes to the folder above, Right goes into the folder under the cursor. On a window
//! too narrow for three columns the folder is drawn as a table instead.

use rada_core::display;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look, RowTone};
use super::widgets::pad;
use crate::app::App;
use crate::hits::Target;
use crate::icons;

/// Narrowest area that is split into three columns.
pub const MIN_WIDTH: u16 = 54;

/// Widths of the three columns for an area of `width` cells, the two rules between them
/// taking a cell each: the folder above is the narrowest, the folder you are in gets the most.
pub fn widths(width: u16) -> (u16, u16, u16) {
    let usable = width.saturating_sub(2);
    let first = (usable * 24 / 100).max(14);
    let second = (usable * 38 / 100).max(20);
    let third = usable.saturating_sub(first + second);
    (first, second, third)
}

pub fn draw_columns(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    if area.width < MIN_WIDTH || area.height < 3 {
        // Too narrow: the table does what it can.
        let opts =
            super::list::ListOpts::for_terminal(area.width as usize, app.tags_column_wanted());
        return super::list::draw_list(f, app, area, opts);
    }
    app.want_parent_column();
    let (w1, w2, w3) = widths(area.width);
    let col = |x: u16, w: u16| Rect {
        x: area.x + x,
        width: w,
        ..area
    };
    let (c1, c2, c3) = (col(0, w1), col(w1 + 1, w2), col(w1 + w2 + 2, w3));
    for x in [c1.x + w1, c2.x + w2] {
        let rule: Vec<Line> = (0..area.height)
            .map(|_| Line::from(Span::styled("│", th.faint())))
            .collect();
        f.render_widget(
            Paragraph::new(rule),
            Rect {
                x,
                width: 1,
                ..area
            },
        );
    }
    draw_parent(f, app, c1);
    draw_current(f, app, c2);
    draw_next(f, app, c3);
}

/// One line of a column: an icon, a name and, for a folder, an arrow; `width` cells.
fn line_of(
    app: &App,
    e: &rada_core::model::Entry,
    width: usize,
    style: Style,
    glyph_style: Option<Style>,
) -> Line<'static> {
    let th = &app.settings.th;
    let (glyph, color) = icons::icon(e, app.settings.icons, th);
    let arrow = if e.is_navigable_dir() { "›" } else { " " };
    let lead = if glyph.is_empty() {
        " ".to_string()
    } else {
        format!(" {glyph} ")
    };
    let room = width.saturating_sub(lead.width() + 2);
    let name = display::truncate(&e.display, room);
    let name_color = if e.is_navigable_dir() {
        th.kinds.folder
    } else {
        th.text
    };
    let mut name_style = Style::default().fg(name_color);
    if e.is_navigable_dir() {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    let pad_to = width.saturating_sub(lead.width() + 2);
    Line::from(vec![
        Span::styled(
            lead,
            glyph_style
                .unwrap_or(Style::default().fg(color))
                .patch(style),
        ),
        Span::styled(pad(&name, pad_to), name_style.patch(style)),
        Span::styled(format!("{arrow} "), th.dim().patch(style)),
    ])
}

/// The first column: the folder above, with the folder you are in picked out.
fn draw_parent(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let look = Look::of(app);
    let entries: Vec<(rada_core::model::Entry, bool)> = app
        .parent_entries()
        .into_iter()
        .map(|e| (e.clone(), e.path == app.tab.cwd))
        .collect();
    let rows = area.height as usize;
    let here = entries
        .iter()
        .position(|(_, is_here)| *is_here)
        .unwrap_or(0);
    let start = (here + 1)
        .saturating_sub(rows / 2 + 1)
        .min(entries.len().saturating_sub(rows));
    let mut lines: Vec<Line> = Vec::new();
    for (i, (e, is_here)) in entries.iter().enumerate().skip(start).take(rows) {
        let style = if *is_here {
            th.current_place()
        } else {
            Style::default()
        };
        let _ = i;
        lines.push(line_of(app, e, area.width as usize, style, None));
    }
    f.render_widget(Paragraph::new(lines), area);
    for (i, (e, is_here)) in entries.iter().enumerate().skip(start).take(rows) {
        let r = Rect {
            y: area.y + (i - start) as u16,
            height: 1,
            ..area
        };
        if *is_here {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Place);
        }
        app.frame.hits.add(r, Target::Place(e.path.clone()));
    }
}

/// The second column: the folder you are in.
fn draw_current(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let look = Look::of(app);
    let rows = area.height as usize;
    app.tab.viewport.rows = rows;
    app.frame.hits.add(area, Target::List);
    let count = app.tab.visible.len();
    if count == 0 {
        f.render_widget(
            Paragraph::new(Span::styled(rada_core::i18n::Key::ListEmpty.tr(), th.dim())),
            Rect {
                y: area.y + area.height / 4,
                height: 1,
                ..area
            },
        );
        return;
    }
    app.tab.scroll = app.tab.scroll.min(count.saturating_sub(1));
    let last = (app.tab.scroll + rows).min(count);
    let mut lines: Vec<Line> = Vec::new();
    for vis in app.tab.scroll..last {
        let Some(e) = app.entry_at(vis) else { continue };
        let marked = app.tab.marked.contains(&e.name);
        let style = if vis == app.tab.cursor {
            th.cursor_row()
        } else if marked {
            th.marked()
        } else {
            Style::default()
        };
        let glyph_style = (vis == app.tab.cursor).then(Style::default);
        lines.push(line_of(app, e, area.width as usize, style, glyph_style));
    }
    f.render_widget(Paragraph::new(lines), area);
    for vis in app.tab.scroll..last {
        let r = Rect {
            y: area.y + (vis - app.tab.scroll) as u16,
            height: 1,
            ..area
        };
        let marked = app
            .entry_at(vis)
            .is_some_and(|e| app.tab.marked.contains(&e.name));
        if vis == app.tab.cursor {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Cursor);
        } else if marked {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Marked);
        }
        app.frame.hits.add(r, Target::Row(vis));
    }
}

/// The third column: the name and what it holds (a folder's items, a file's preview).
fn draw_next(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let Some(e) = app.current().cloned() else {
        return;
    };
    let (glyph, color) = icons::icon(&e, app.settings.icons, &th);
    let mut head = Vec::new();
    if !glyph.is_empty() {
        head.push(Span::styled(format!(" {glyph} "), th.fg(color)));
    } else {
        head.push(Span::raw(" "));
    }
    let room = (area.width as usize)
        .saturating_sub(head.iter().map(|s| s.content.width()).sum::<usize>() + 1);
    head.push(Span::styled(
        display::truncate(&e.display, room),
        th.base().add_modifier(Modifier::BOLD),
    ));
    f.render_widget(Paragraph::new(Line::from(head)), Rect { height: 1, ..area });
    if area.height < 3 {
        return;
    }
    let body = Rect {
        x: area.x + 1,
        y: area.y + 2,
        width: area.width.saturating_sub(2),
        height: area.height - 2,
    };
    app.frame.hits.add(body, Target::Preview);
    super::preview::content(f, app, body, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_columns_share_the_width_with_two_rules_between() {
        for w in [54u16, 60, 80, 100, 140, 200] {
            let (a, b, c) = widths(w);
            assert_eq!(a + b + c + 2, w, "{w}");
            assert!(a >= 14 && b >= 20 && c >= 10, "{w}: {a} {b} {c}");
            assert!(
                a < b,
                "the folder you are in is the widest of the two first"
            );
        }
    }
}
