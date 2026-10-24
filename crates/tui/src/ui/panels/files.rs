//! The folder in the middle box: Name, Size (the number alone), Modified and Tag. The row under
//! the cursor is filled with the selection colour and rounded at both ends; selected rows have
//! a quiet tone and a tick.

use rada_core::display;
use rada_core::fs::FileKind;
use rada_core::git::FileState;
use rada_core::i18n::{Key, tf};
use rada_core::model::{Entry, SortKey};
use rada_core::ops::LinkState;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::fmt::{self, DateStyle};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::ui::soft::{self, Look, RowTone};
use crate::ui::widgets::{SPIN, hit_spans, pad, pad_left};

/// The cells of the Tag column: three dots with a space between, and a "+N".
const TAG_W: usize = 8;
const GAP: usize = 2;

/// Which columns the box has room for, and how wide each is.
struct Cols {
    icon: usize,
    name: usize,
    size: usize,
    date: usize,
    tags: usize,
}

fn columns(app: &App, width: usize) -> Cols {
    let icon = app.settings.icons.width();
    let size = Key::ColSize.tr().width() + 2;
    let date = match app.settings.dates {
        DateStyle::Relative => 14,
        DateStyle::Absolute => 16,
    }
    .max(Key::ColModified.tr().width() + 2);
    // The first and the last cell of a row are where the round ends of the cursor row go:
    // a space, the tick and a space come first, and one cell is left at the end.
    let lead = 3 + 1;
    let mut cols = Cols {
        icon,
        name: 0,
        size,
        date,
        tags: 0,
    };
    let fixed = |c: &Cols| lead + c.icon + c.size + c.date + c.tags + GAP * 2;
    if app.tags_column_wanted() && width >= fixed(&cols) + TAG_W + GAP + 22 {
        cols.tags = TAG_W;
    }
    if width < fixed(&cols) + 18 || (app.frame.term_width() as usize) < crate::ui::list::DATE_FROM {
        cols.date = 0;
    }
    let gaps = 1 + usize::from(cols.date > 0) + usize::from(cols.tags > 0);
    cols.name =
        width.saturating_sub(lead + cols.icon + cols.size + cols.date + cols.tags + GAP * gaps);
    cols
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let look = Look::of(app);
    if area.height < 2 || area.width < 24 {
        return;
    }
    let cols = columns(app, area.width as usize);
    let count = app.visible.len();

    // The titles: the sorted one in the accent colour with its arrow.
    let arrow = if app.sort.descending() { "↓" } else { "↑" };
    let title = |text: &str, key: SortKey| {
        if app.sort.key == key {
            (
                format!("{text} {arrow}"),
                th.accent_style().add_modifier(Modifier::BOLD),
            )
        } else {
            (text.to_string(), th.dim().add_modifier(Modifier::BOLD))
        }
    };
    let gap = || Span::styled(" ".repeat(GAP), th.base());
    let mut head: Vec<Span> = vec![Span::styled(" ".repeat(3 + cols.icon), th.base())];
    let mut head_hits: Vec<(usize, Target)> = Vec::new();
    let (t, st) = title(Key::ColName.tr(), SortKey::Name);
    head_hits.push((head.len(), Target::SortBy(SortKey::Name)));
    head.push(Span::styled(pad(&t, cols.name), st));
    head.push(gap());
    let (t, st) = title(Key::ColSize.tr(), SortKey::Size);
    head_hits.push((head.len(), Target::SortBy(SortKey::Size)));
    head.push(Span::styled(pad_left(&t, cols.size), st));
    if cols.date > 0 {
        head.push(gap());
        let name = if app.recents_view() {
            Key::ColUsed
        } else {
            Key::ColModified
        };
        let (t, st) = title(name.tr(), SortKey::Modified);
        head_hits.push((head.len(), Target::SortBy(SortKey::Modified)));
        head.push(Span::styled(pad(&t, cols.date), st));
    }
    if cols.tags > 0 {
        head.push(gap());
        head.push(Span::styled(
            pad(Key::ColTags.tr(), cols.tags),
            th.dim().add_modifier(Modifier::BOLD),
        ));
    }
    hit_spans(&mut app.frame.hits, area.x, area.y, &head, &head_hits);
    f.render_widget(Paragraph::new(Line::from(head)), Rect { height: 1, ..area });

    // A blank row under the titles where there is room for it.
    let air = u16::from(area.height >= 8);
    let body = Rect {
        y: area.y + 1 + air,
        height: area.height - 1 - air,
        ..area
    };
    app.viewport.rows = body.height as usize;
    if body.height == 0 {
        return;
    }
    app.frame.hits.add(body, Target::List);

    if app.visible.is_empty() {
        let msg = if app.is_loading() {
            tf(Key::ListLoading, &[&SPIN[app.redraw.spinner % SPIN.len()]])
        } else if app.filter.is_some() {
            Key::ListNoMatch.tr().to_string()
        } else if app.listing.is_empty() {
            Key::ListEmpty.tr().to_string()
        } else {
            Key::ListOnlyHidden.tr().to_string()
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
    app.want_folder_sizes(shown.iter().copied());
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
        let marked = app
            .entry_at(vis)
            .is_some_and(|e| app.marked.contains(&e.name));
        if vis == app.cursor {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Focus);
        } else if marked {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Panel);
        }
        app.frame.hits.add(r, Target::Row(vis));
    }
}

fn row<'a>(
    app: &App,
    e: &Entry,
    listing_index: usize,
    is_cursor: bool,
    cols: &Cols,
    width: usize,
) -> Line<'a> {
    let th = &app.settings.th;
    let marked = app.marked.contains(&e.name);
    // The selected row is filled with the selection colour and its text is the theme's text
    // for it, in every column (a quieter tone would not read on it).
    let row_style = if is_cursor {
        th.cursor_row()
    } else if marked && th.depth != crate::theme::ColorDepth::Ansi16 {
        Style::default().bg(th.selection)
    } else if marked {
        th.marked()
    } else {
        Style::default()
    };
    let with = |s: Style| {
        if is_cursor && th.depth != crate::theme::ColorDepth::Ansi16 {
            Style { fg: None, ..s }.patch(row_style)
        } else {
            s.patch(row_style)
        }
    };
    let (glyph, gcolor) = icons::icon(e, app.settings.icons, th);
    let broken = e
        .link
        .as_ref()
        .is_some_and(|l| matches!(l.state, LinkState::Broken | LinkState::Circular));
    let name_color = if e.error.is_some() || broken {
        th.error
    } else if e.hidden {
        th.text_dim
    } else if e.is_navigable_dir() {
        th.kinds.folder
    } else {
        th.text
    };
    let mut name_style = Style::default().fg(name_color);
    if e.is_navigable_dir() || is_cursor || marked {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    let tick = icons::ui(app.settings.icons, Glyph::Checked);
    let mut spans: Vec<Span> = vec![
        Span::styled(" ", with(Style::default())),
        Span::styled(
            if marked { tick } else { " " }.to_string(),
            with(Style::default().fg(th.check)),
        ),
        Span::styled(" ", with(Style::default())),
    ];
    // The icon keeps its colour, except on the selected row where only one colour reads.
    if cols.icon > 0 {
        spans.push(Span::styled(
            format!("{glyph} "),
            with(Style::default().fg(gcolor)),
        ));
    }
    // What git says of it, after the name: M for a change, ? for a file that is new.
    let git = app
        .git_here()
        .and_then(|g| g.items.get(&e.display))
        .copied();
    let mark_w = if git.is_some() { 2 } else { 0 };
    let name_txt = display::truncate_name(&e.display, cols.name.saturating_sub(mark_w));
    let mut used = name_txt.width();
    spans.push(Span::styled(name_txt, with(name_style)));
    if let Some(state) = git {
        let (letter, hue) = match state {
            FileState::Modified => ("M", th.warn),
            FileState::New => ("?", th.success),
        };
        // On the selected row the one colour that reads is the row's own.
        spans.push(Span::styled(
            format!(" {letter}"),
            with(Style::default().fg(hue).add_modifier(Modifier::BOLD)),
        ));
        used += 2;
    }
    if let Some(l) = &e.link {
        let room = cols.name.saturating_sub(used);
        if room > 5 {
            let n = display::truncate(&format!(" → {}", display::path(&l.target)), room);
            used += n.width();
            spans.push(Span::styled(n, with(th.dim())));
        }
    }
    spans.push(Span::styled(
        " ".repeat(cols.name.saturating_sub(used)),
        with(Style::default()),
    ));
    let dim = if is_cursor {
        Style::default()
    } else {
        th.dim()
    };
    let gap = || Span::styled(" ".repeat(GAP), with(Style::default()));
    spans.push(gap());
    let size = if e.kind == FileKind::File {
        fmt::size(e.size)
    } else {
        app.folder_size_text(e, cols.size)
    };
    spans.push(Span::styled(pad_left(&size, cols.size), with(dim)));
    if cols.date > 0 {
        spans.push(gap());
        let text = display::truncate(app.folder_info.date_text(listing_index), cols.date);
        spans.push(Span::styled(pad(&text, cols.date), with(dim)));
    }
    if cols.tags > 0 {
        spans.push(gap());
        let dots = crate::ui::tagdots::dots(th, &e.tags);
        let mut text_w = 0;
        for (i, (text, color)) in dots.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" ", with(Style::default())));
                text_w += 1;
            }
            text_w += text.width();
            spans.push(Span::styled(
                text.clone(),
                row_style.patch(Style::default().fg(*color)),
            ));
        }
        spans.push(Span::styled(
            " ".repeat(cols.tags.saturating_sub(text_w)),
            with(Style::default()),
        ));
    }
    // The highlight reaches the right edge, where its round end is.
    let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
    if drawn < width {
        spans.push(Span::styled(
            " ".repeat(width - drawn),
            with(Style::default()),
        ));
    }
    Line::from(spans)
}
