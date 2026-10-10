//! The folder as a table (the Details view): a checkbox, the icon and name, the date, the
//! type in words and the size. The column titles are buttons that sort; the sorted one
//! carries an arrow.
//!
//! What the terminal's width allows decides the columns: below 100 columns the type goes,
//! below 60 the date goes too and only name and size remain.

use rada_core::display;
use rada_core::fs::FileKind;
use rada_core::model::{Entry, SortKey};
use rada_core::ops::LinkState;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::widgets::{SPIN, hit_spans, pad, pad_left};
use crate::app::App;
use crate::fmt::{self, DateStyle};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// Cells of the checkbox column: a space, the box, a space.
const CHECK: usize = 3;

/// Which columns fit, and how wide each is.
pub struct Columns {
    pub icon: usize,
    pub date: usize,
    pub kind: usize,
    pub size: usize,
    pub gap: usize,
    pub name: usize,
}

/// What the caller decides about the table: which optional columns it may have. They follow
/// the width of the terminal, not of the area the table is drawn in, so that every area on a
/// screen agrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListOpts {
    pub show_type: bool,
    pub show_date: bool,
}

impl ListOpts {
    /// Below 100 columns the type goes, below 60 the date goes too.
    pub fn for_terminal(term_width: usize) -> ListOpts {
        ListOpts {
            show_type: term_width >= 100,
            show_date: term_width >= 60,
        }
    }
}

pub fn columns(area_width: usize, opts: ListOpts, icon_w: usize, dates: DateStyle) -> Columns {
    let gap = 2;
    let date_w = match dates {
        DateStyle::Relative => 14,
        DateStyle::Absolute => 16,
    };
    let (mut date, mut kind, mut size) = (date_w, 20, 10);
    if !opts.show_type {
        kind = 0;
    }
    if !opts.show_date || area_width < 46 {
        date = 0;
    }
    if area_width < 24 {
        size = 0;
    }
    let cols = [date, kind, size].iter().filter(|c| **c > 0).count();
    let fixed = CHECK + icon_w + date + kind + size + 1;
    let name = area_width.saturating_sub(fixed + gap * cols);
    Columns {
        icon: icon_w,
        date,
        kind,
        size,
        gap,
        name,
    }
}

/// Draw the table of the front tab in `area` (any area: nothing here knows where it is).
pub fn draw_list(f: &mut Frame, app: &mut App, area: Rect, opts: ListOpts) {
    let th = app.th.clone();
    let count = app.visible.len();
    let cols = columns(area.width as usize, opts, app.icons.width(), app.dates);

    // Column titles, on a band of their own; the sorted one carries an arrow.
    let head_bg = Style::default().bg(th.field);
    let arrow = if app.sort.reverse { "↓" } else { "↑" };
    let label = |text: &str, k: SortKey| {
        let sorted = app.sort.key == k;
        let t = if sorted {
            format!("{text} {arrow}")
        } else {
            text.to_string()
        };
        let st = if sorted {
            th.accent_style().add_modifier(Modifier::BOLD)
        } else {
            th.dim()
        };
        (t, st.patch(head_bg))
    };
    let gap = || Span::styled(" ".repeat(cols.gap), head_bg);
    let mut head: Vec<Span> = Vec::new();
    let mut head_hits: Vec<(usize, Target)> = Vec::new();
    head_hits.push((head.len(), Target::Act(Action::SelectAll)));
    let all_marked = !app.visible.is_empty() && app.marked.len() == app.visible.len();
    head.push(Span::styled(
        format!(
            " {} ",
            icons::ui(
                app.icons,
                if all_marked {
                    Glyph::Checked
                } else {
                    Glyph::Unchecked
                }
            )
        ),
        if all_marked {
            th.fg(th.check).patch(head_bg)
        } else {
            th.faint().patch(head_bg)
        },
    ));
    head.push(Span::styled(" ".repeat(cols.icon), head_bg));
    let (t, st) = label("Name", SortKey::Name);
    head_hits.push((head.len(), Target::SortBy(SortKey::Name)));
    head.push(Span::styled(pad(&t, cols.name), st));
    if cols.date > 0 {
        head.push(gap());
        let (t, st) = label("Date modified", SortKey::Modified);
        head_hits.push((head.len(), Target::SortBy(SortKey::Modified)));
        head.push(Span::styled(pad(&t, cols.date), st));
    }
    if cols.kind > 0 {
        head.push(gap());
        let (t, st) = label("Type", SortKey::Type);
        head_hits.push((head.len(), Target::SortBy(SortKey::Type)));
        head.push(Span::styled(pad(&t, cols.kind), st));
    }
    if cols.size > 0 {
        head.push(gap());
        let (t, st) = label("Size", SortKey::Size);
        head_hits.push((head.len(), Target::SortBy(SortKey::Size)));
        head.push(Span::styled(pad_left(&t, cols.size), st));
    }
    head.push(Span::styled(" ", head_bg));
    let drawn: usize = head.iter().map(|s| s.content.width()).sum();
    if drawn < area.width as usize {
        head.push(Span::styled(
            " ".repeat(area.width as usize - drawn),
            head_bg,
        ));
    }
    if area.height < 2 {
        return;
    }
    hit_spans(&mut app.hits, area.x, area.y, &head, &head_hits);
    f.render_widget(Paragraph::new(Line::from(head)), Rect { height: 1, ..area });

    let body = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    app.viewport.rows = body.height as usize;
    if body.height == 0 {
        return;
    }
    app.hits.add(body, Target::List);

    if app.visible.is_empty() {
        let msg = if app.is_loading() {
            format!("{} loading…", SPIN[app.spinner % SPIN.len()])
        } else if app.filter.is_some() {
            "Nothing here matches the search".to_string()
        } else if app.listing.is_empty() {
            "This folder is empty".to_string()
        } else {
            "Only hidden files here — press . to show them".to_string()
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, th.dim())).alignment(Alignment::Center),
            Rect {
                y: body.y + body.height / 4,
                height: 1,
                ..body
            },
        );
        return;
    }

    app.scroll = app.scroll.min(count.saturating_sub(1));
    let rows = body.height as usize;
    let last = (app.scroll + rows).min(count);
    let shown: Vec<usize> = (app.scroll..last).map(|v| app.visible[v]).collect();
    app.warm_dates(shown.iter().copied());
    let mut lines: Vec<Line> = Vec::with_capacity(rows);
    for vis in app.scroll..last {
        let Some(e) = app.entry_at(vis) else { continue };
        lines.push(row(
            app,
            e,
            app.visible[vis],
            vis == app.cursor,
            &cols,
            area.width as usize,
        ));
    }
    f.render_widget(Paragraph::new(lines), body);
    for vis in app.scroll..last {
        let r = Rect {
            y: body.y + (vis - app.scroll) as u16,
            height: 1,
            ..body
        };
        app.hits.add(r, Target::Row(vis));
        app.hits.add(
            Rect {
                width: (CHECK as u16).min(r.width),
                ..r
            },
            Target::Check(vis),
        );
    }
}

fn row<'a>(
    app: &App,
    e: &Entry,
    listing_index: usize,
    is_cursor: bool,
    cols: &Columns,
    width: usize,
) -> Line<'a> {
    let th = &app.th;
    let marked = app.marked.contains(&e.name);
    // The cursor row is solid with white text; selected rows are a quiet tone; the cursor on
    // a selected row wins, but its tick stays.
    let row_style = if is_cursor {
        th.cursor_row()
    } else if marked {
        th.marked()
    } else {
        Style::default()
    };
    // Patching the row's style over a span's own colour would lose the white text: on the
    // cursor row the text colours are the row's.
    let with = |s: Style| {
        if is_cursor && th.depth != crate::theme::ColorDepth::Ansi16 {
            Style { fg: None, ..s }.patch(row_style)
        } else {
            s.patch(row_style)
        }
    };

    let (glyph, gcolor) = icons::icon(e, app.icons, th);
    let broken = e
        .link
        .as_ref()
        .is_some_and(|l| matches!(l.state, LinkState::Broken | LinkState::Circular));
    let name_color = if e.error.is_some() || broken {
        th.error
    } else if e.hidden {
        th.text_dim
    } else if e.is_dir() {
        th.kinds.folder
    } else {
        th.text
    };
    let mut name_style = Style::default().fg(name_color);
    if e.is_dir() || is_cursor || marked {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }

    let check = icons::ui(
        app.icons,
        if marked {
            Glyph::Checked
        } else {
            Glyph::Unchecked
        },
    );
    // The leftmost cell carries a bar in the accent colour on the cursor row; the tick keeps
    // its accent on a selected row and stays visible under the cursor.
    let tick_style = if is_cursor && marked {
        Style::default()
            .fg(th.cursor_text)
            .add_modifier(Modifier::BOLD)
    } else if marked {
        th.fg(th.check)
    } else {
        th.faint()
    };
    let mut spans: Vec<Span> = vec![
        Span::styled(
            if is_cursor { "▌" } else { " " },
            row_style.patch(Style::default().fg(th.accent)),
        ),
        Span::styled(check.to_string(), with(tick_style)),
        Span::styled(" ", with(Style::default())),
    ];
    // On the cursor row the secondary text turns to the main colour: the stronger
    // background would leave it too faint.
    let dim = if is_cursor {
        Style::default()
    } else {
        th.dim()
    };
    if cols.icon > 0 {
        spans.push(Span::styled(
            format!("{glyph} "),
            with(Style::default().fg(gcolor)),
        ));
    }

    let shown = e.display.clone();
    let name_txt = display::truncate(&shown, cols.name);
    let mut used = name_txt.width();
    spans.push(Span::styled(name_txt, with(name_style)));
    if let Some(l) = &e.link {
        let room = cols.name.saturating_sub(used);
        if room > 5 {
            let n = display::truncate(&format!(" → {}", display::path(&l.target)), room);
            used += n.width();
            spans.push(Span::styled(n, with(dim)));
        }
    }
    spans.push(Span::styled(
        " ".repeat(cols.name.saturating_sub(used)),
        with(Style::default()),
    ));

    let gap = || Span::styled(" ".repeat(cols.gap), with(Style::default()));
    if cols.date > 0 {
        spans.push(gap());
        let text = display::truncate(app.date_text(listing_index), cols.date);
        spans.push(Span::styled(pad(&text, cols.date), with(dim)));
    }
    if cols.kind > 0 {
        spans.push(gap());
        let text = display::truncate(&e.type_label, cols.kind);
        spans.push(Span::styled(pad(&text, cols.kind), with(dim)));
    }
    if cols.size > 0 {
        spans.push(gap());
        let s = if e.kind == FileKind::File {
            fmt::size(e.size)
        } else {
            String::new()
        };
        spans.push(Span::styled(
            pad_left(&s, cols.size),
            with(if is_cursor {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                th.dim()
            }),
        ));
    }
    // Make the highlight reach the right edge.
    let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
    if drawn < width {
        spans.push(Span::styled(
            " ".repeat(width - drawn),
            with(Style::default()),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_terminals_lose_the_least_important_columns_first() {
        let rel = DateStyle::Relative;
        let wide = columns(120, ListOpts::for_terminal(170), 2, rel);
        assert!(wide.kind > 0 && wide.date > 0 && wide.size > 0);
        let mid = columns(90, ListOpts::for_terminal(99), 2, rel);
        assert_eq!(mid.kind, 0, "the type goes below 100 columns");
        assert!(mid.date > 0);
        let narrow = columns(50, ListOpts::for_terminal(59), 2, rel);
        assert_eq!((narrow.date, narrow.kind), (0, 0));
        assert!(narrow.size > 0, "name and size are what stays");
        // Absolute dates need more room than relative ones.
        assert!(columns(120, ListOpts::for_terminal(170), 2, DateStyle::Absolute).date > wide.date);
    }

    #[test]
    fn the_columns_always_add_up_to_the_width() {
        for term in [170usize, 120, 99, 80, 59, 40] {
            let area = term.saturating_sub(30).max(24);
            let c = columns(area, ListOpts::for_terminal(term), 2, DateStyle::Relative);
            let n = [c.date, c.kind, c.size].iter().filter(|x| **x > 0).count();
            assert_eq!(
                CHECK + c.icon + c.name + c.date + c.kind + c.size + c.gap * n + 1,
                area.max(CHECK + c.icon + c.date + c.kind + c.size + c.gap * n + 1),
                "{term}"
            );
        }
    }
}
