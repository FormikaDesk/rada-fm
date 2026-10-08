//! The folder list: Name, a size bar, Size, Type badge and a relative Modified column.

use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;
use vela_core::display;
use vela_core::fs::FileKind;
use vela_core::model::{Entry, SortKey};
use vela_core::ops::LinkState;

use super::widgets::{SPIN, badge, bar_spans, pad_left};
use crate::app::App;
use crate::fmt;
use crate::icons::{self, Category};
use crate::theme::Density;

/// Which columns fit, and how wide each is.
struct Columns {
    icon: usize,
    bar: usize,
    size: usize,
    kind: usize,
    modified: usize,
    gap: usize,
    name: usize,
}

fn columns(width: usize, icon_w: usize, density: Density) -> Columns {
    let gap = match density {
        Density::Airy => 3,
        Density::Balanced => 2,
        Density::Dense => 1,
    };
    let bar_cells = match density {
        Density::Airy => 8,
        Density::Balanced => 7,
        Density::Dense => 5,
    };
    let (mut bar, mut size, mut kind, mut modified) = (bar_cells + 1, 9, 9, 12);
    // Narrow terminals lose the least important columns first.
    if width < 66 {
        modified = 0;
    }
    if width < 54 {
        kind = 0;
    }
    if width < 44 {
        bar = 0;
    }
    if width < 34 {
        size = 0;
    }
    let fixed = 2 + icon_w + bar + size + kind + modified;
    let cols = [bar, size, kind, modified]
        .iter()
        .filter(|c| **c > 0)
        .count();
    let name = width.saturating_sub(fixed + gap * cols);
    Columns {
        icon: icon_w,
        bar,
        size,
        kind,
        modified,
        gap,
        name,
    }
}

pub fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let count = app.visible.len();
    let cols = columns(area.width as usize, app.icons.width(), th.density);

    // Header: column labels, the sorted one in the accent colour; then a faint rule.
    let sorted = |k: SortKey| app.sort.key == k;
    let arrow = if app.sort.reverse { "↓" } else { "↑" };
    let label = |text: &str, k: SortKey| {
        let t = if sorted(k) {
            format!("{text} {arrow}")
        } else {
            text.to_string()
        };
        let st = if sorted(k) {
            th.accent_style().add_modifier(Modifier::BOLD)
        } else {
            th.dim()
        };
        (t, st)
    };
    let gap = " ".repeat(cols.gap);
    let mut head: Vec<Span> = vec![Span::raw(" ".repeat(2 + cols.icon))];
    let (t, st) = label("Name", SortKey::Name);
    head.push(Span::styled(super::widgets::pad(&t, cols.name), st));
    if cols.bar > 0 {
        head.push(Span::raw(gap.clone()));
        head.push(Span::raw(" ".repeat(cols.bar)));
    }
    if cols.size > 0 {
        head.push(Span::raw(if cols.bar > 0 {
            String::new()
        } else {
            gap.clone()
        }));
        let (t, st) = label("Size", SortKey::Size);
        head.push(Span::styled(pad_left(&t, cols.size), st));
    }
    if cols.kind > 0 {
        head.push(Span::raw(gap.clone()));
        head.push(Span::styled(
            super::widgets::pad("Type", cols.kind),
            th.dim(),
        ));
    }
    if cols.modified > 0 {
        head.push(Span::raw(gap.clone()));
        let (t, st) = label("Modified", SortKey::Modified);
        head.push(Span::styled(pad_left(&t, cols.modified), st));
    }
    if area.height < 3 {
        return;
    }
    f.render_widget(Paragraph::new(Line::from(head)), Rect { height: 1, ..area });
    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(area.width as usize), th.faint())),
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        },
    );

    let body = Rect {
        y: area.y + 2,
        height: area.height - 2,
        ..area
    };
    app.view_rows = body.height as usize;
    if body.height == 0 {
        return;
    }

    if app.visible.is_empty() {
        let msg = if app.is_loading() {
            format!("{} loading…", SPIN[app.spinner % SPIN.len()])
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

    // Bars are proportional to the biggest file of the folder.
    let max_size = app
        .visible
        .iter()
        .filter_map(|&i| app.listing.all().get(i))
        .filter(|e| e.kind == FileKind::File)
        .map(|e| e.size)
        .max()
        .unwrap_or(0);
    let now = SystemTime::now();

    app.scroll = app.scroll.min(count.saturating_sub(1));
    let rows = body.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(rows);
    for vis in app.scroll..(app.scroll + rows).min(count) {
        let Some(e) = app.entry_at(vis) else { continue };
        lines.push(row(
            app,
            e,
            vis == app.cursor,
            &cols,
            max_size,
            now,
            area.width as usize,
        ));
    }
    f.render_widget(Paragraph::new(lines), body);
}

#[allow(clippy::too_many_arguments)]
fn row<'a>(
    app: &App,
    e: &Entry,
    is_cursor: bool,
    cols: &Columns,
    max_size: u64,
    now: SystemTime,
    width: usize,
) -> Line<'a> {
    let th = &app.th;
    let marked = app.marked.contains(&e.name);
    let row_style = if is_cursor {
        th.selected()
    } else if marked {
        th.marked()
    } else {
        Style::default()
    };
    let row_bg = row_style.bg;
    let with = |s: Style| s.patch(row_style);

    let cat = icons::categorize(e);
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
    if e.is_dir() || is_cursor {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }

    let gutter = if marked {
        Span::styled("● ", with(th.accent_style()))
    } else if is_cursor {
        Span::styled("▎ ", with(th.accent_style()))
    } else {
        Span::styled("  ", with(Style::default()))
    };
    let mut spans: Vec<Span> = vec![gutter];
    if cols.icon > 0 {
        spans.push(Span::styled(
            format!("{glyph} "),
            with(Style::default().fg(gcolor)),
        ));
    }

    let mut shown = e.display.clone();
    if e.is_dir() {
        shown.push('/');
    }
    let name_txt = display::truncate(&shown, cols.name);
    let mut used = name_txt.width();
    spans.push(Span::styled(name_txt, with(name_style)));
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

    let gap = || Span::styled(" ".repeat(cols.gap), with(Style::default()));
    if cols.bar > 0 {
        spans.push(gap());
        if e.kind == FileKind::File && max_size > 0 {
            let frac = e.size as f64 / max_size as f64;
            let fill = if is_cursor { th.accent } else { cat.color(th) };
            spans.extend(bar_spans(th, frac, cols.bar - 1, fill, row_bg));
            spans.push(Span::styled(" ", with(Style::default())));
        } else {
            spans.push(Span::styled(" ".repeat(cols.bar), with(Style::default())));
        }
    }
    if cols.size > 0 {
        if cols.bar == 0 {
            spans.push(gap());
        }
        let s = match e.kind {
            FileKind::File => fmt::size(e.size),
            FileKind::Dir => "—".to_string(),
            FileKind::Symlink => "—".to_string(),
            FileKind::Other => "—".to_string(),
        };
        let st = if e.kind == FileKind::File {
            th.base()
        } else {
            th.faint()
        };
        spans.push(Span::styled(pad_left(&s, cols.size), with(st)));
    }
    if cols.kind > 0 {
        spans.push(gap());
        if matches!(cat, Category::Folder | Category::Other) {
            spans.push(Span::styled(
                super::widgets::pad(cat.label(), cols.kind),
                with(th.dim()),
            ));
        } else {
            spans.extend(badge(th, cat.label(), cat.color(th), cols.kind, row_bg));
        }
    }
    if cols.modified > 0 {
        spans.push(gap());
        spans.push(Span::styled(
            pad_left(&fmt::relative(e.mtime, now), cols.modified),
            with(th.dim()),
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
