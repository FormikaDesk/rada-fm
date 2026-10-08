//! Drawing. A pure function of the application state: no I/O, no blocking.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;
use vela_core::display;
use vela_core::fs::FileKind;
use vela_core::journal::{EntryStatus, UndoState};
use vela_core::model::Entry;
use vela_core::ops::LinkState;
use vela_core::ops::{ConflictPolicy, OpKind, RunStatus, Severity};
use vela_core::preview::Preview;

use crate::app::*;
use crate::fmt;
use crate::icons;

const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width < 20 || area.height < 6 {
        f.render_widget(Paragraph::new("terminal too small"), area);
        return;
    }
    let progress_h = if app.running.is_some() { 3 } else { 0 };
    let [header, body, progress, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(progress_h),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(f, app, header);
    if area.width >= 90 {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)])
                .areas(body);
        draw_list(f, app, left);
        draw_preview(f, app, right);
    } else {
        draw_list(f, app, body);
    }
    if app.running.is_some() {
        draw_progress(f, app, progress);
    }
    draw_footer(f, app, footer);
    draw_toast_box(f, app, area, footer);
    draw_modal(f, app, area);
}

fn block<'a>(app: &App, title: Line<'a>, focus: bool) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(app.th.fg(if focus {
            app.th.border_focus
        } else {
            app.th.border
        }))
        .title(title)
}

/// Keep the *end* of a long path (the part that matters).
fn tail(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out: Vec<char> = Vec::new();
    let mut w = 0;
    for c in s.chars().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}

fn pad(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

fn pad_left(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(width - w))
    }
}

// ------------------------------------------------------------------------------ header

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let th = &app.th;
    let path = display::path(&app.cwd);
    let mut right: Vec<Span> = Vec::new();
    if app.is_loading() {
        right.push(Span::styled(
            format!("{} loading ", SPIN[app.spinner % SPIN.len()]),
            th.fg(th.warn),
        ));
    }
    right.push(Span::styled(
        format!(" sort {} ", app.sort_label()),
        th.dim(),
    ));
    if app.show_hidden {
        right.push(Span::styled(" hidden shown ", th.fg(th.accent_soft)));
    }
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();
    let left_max = (area.width as usize).saturating_sub(right_w + 10);
    let mut spans = vec![
        Span::styled(
            " vela ",
            Style::default()
                .fg(th.accent)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw(" "),
        Span::styled(
            tail(&path, left_max),
            th.base().add_modifier(Modifier::BOLD),
        ),
    ];
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    let gap = (area.width as usize).saturating_sub(used + right_w);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ------------------------------------------------------------------------------ list

fn draw_list(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let count = app.visible.len();
    let total_dirs = app
        .visible
        .iter()
        .filter(|&&i| app.listing.all()[i].is_dir())
        .count();
    let title = Line::from(vec![
        Span::styled(" ", th.dim()),
        Span::styled(
            format!(
                "{} · {}",
                fmt::count(count as u64, "item", "items"),
                fmt::count(total_dirs as u64, "folder", "folders")
            ),
            th.dim(),
        ),
        Span::styled(" ", th.dim()),
    ]);
    let mut blk = block(app, title, app.modal.is_none());
    if !app.marked.is_empty() {
        let size: u64 = app
            .listing
            .all()
            .iter()
            .filter(|e| app.marked.contains(&e.name))
            .map(|e| e.size)
            .sum();
        blk = blk.title_bottom(Line::from(Span::styled(
            format!(" {} selected · {} ", app.marked.len(), fmt::size(size)),
            th.fg(th.accent).add_modifier(Modifier::BOLD),
        )));
    }
    if count > 0 {
        blk = blk.title_bottom(
            Line::from(Span::styled(
                format!(" {}/{} ", app.cursor + 1, count),
                th.dim(),
            ))
            .right_aligned(),
        );
    }
    let inner = blk.inner(area);
    f.render_widget(blk, area);
    // One column of breathing room on each side.
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    app.view_rows = inner.height as usize;
    if inner.width < 10 {
        return;
    }

    if app.visible.is_empty() {
        let msg = if app.is_loading() {
            "loading…"
        } else if app.listing.is_empty() {
            "empty folder"
        } else {
            "no visible entries (press . to show hidden files)"
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, th.dim())).alignment(Alignment::Center),
            Rect {
                y: inner.y + inner.height / 3,
                height: 1,
                ..inner
            },
        );
        return;
    }

    let w = inner.width as usize;
    let show_date = w >= 52;
    let show_size = w >= 34;
    let size_w = if show_size { 9 } else { 0 };
    let date_w = if show_date { 17 } else { 0 };
    let icon_w = app.icons.width();
    let name_w = w.saturating_sub(2 + icon_w + size_w + date_w);

    app.scroll = app.scroll.min(count.saturating_sub(1));
    let rows = inner.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(rows);
    for vis in app.scroll..(app.scroll + rows).min(count) {
        let Some(e) = app.entry_at(vis) else { continue };
        lines.push(row(app, e, vis == app.cursor, name_w, size_w, date_w));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn row<'a>(
    app: &App,
    e: &Entry,
    is_cursor: bool,
    name_w: usize,
    size_w: usize,
    date_w: usize,
) -> Line<'a> {
    let th = &app.th;
    let marked = app.marked.contains(&e.name);
    let bg = if is_cursor {
        Some(th.cursor_bg)
    } else if marked {
        Some(th.mark_bg)
    } else {
        None
    };
    let with_bg = |s: Style| if let Some(c) = bg { s.bg(c) } else { s };

    let (glyph, gcolor) = icons::icon(e, app.icons, th);
    let broken = e
        .link
        .as_ref()
        .is_some_and(|l| matches!(l.state, LinkState::Broken | LinkState::Circular));
    let name_color = if e.error.is_some() || broken {
        th.danger
    } else if e.is_dir() {
        th.dir
    } else if e.link.is_some() {
        th.link
    } else if e.executable {
        th.exec
    } else if e.hidden {
        th.muted
    } else {
        th.text
    };
    let mut name_style = Style::default().fg(name_color);
    if e.is_dir() || is_cursor {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }

    let gutter = if marked {
        "● "
    } else if is_cursor {
        "▌ "
    } else {
        "  "
    };
    let gutter_style = th.fg(th.accent);

    let mut shown = e.display.clone();
    if e.is_dir() {
        shown.push('/');
    }
    let link_note = e
        .link
        .as_ref()
        .map(|l| format!(" → {}", display::path(&l.target)));
    let mut spans: Vec<Span> = vec![Span::styled(gutter.to_string(), with_bg(gutter_style))];
    if app.icons != crate::icons::IconSet::None {
        spans.push(Span::styled(
            format!("{glyph} "),
            with_bg(Style::default().fg(gcolor)),
        ));
    }
    let name_len = shown.width();
    let name_txt = display::truncate(&shown, name_w);
    let mut used = name_txt.width();
    spans.push(Span::styled(name_txt, with_bg(name_style)));
    if let Some(note) = link_note {
        let room = name_w.saturating_sub(name_len);
        if room > 4 {
            let n = display::truncate(&note, room);
            used += n.width();
            spans.push(Span::styled(n, with_bg(th.dim())));
        }
    }
    spans.push(Span::styled(
        " ".repeat(name_w.saturating_sub(used)),
        with_bg(Style::default()),
    ));
    if size_w > 0 {
        let s = match e.kind {
            FileKind::File => fmt::size(e.size),
            FileKind::Dir => "—".to_string(),
            FileKind::Symlink => "link".to_string(),
            FileKind::Other => "special".to_string(),
        };
        spans.push(Span::styled(
            pad_left(&format!("{s} "), size_w),
            with_bg(th.fg(th.subtle)),
        ));
    }
    if date_w > 0 {
        spans.push(Span::styled(
            pad_left(&format!("{} ", fmt::date(e.mtime)), date_w),
            with_bg(th.dim()),
        ));
    }
    Line::from(spans)
}

// ------------------------------------------------------------------------------ preview

fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let title = Line::from(vec![Span::styled(
        format!(
            " {} ",
            display::truncate(&app.preview.name, (area.width as usize).saturating_sub(6))
        ),
        th.title(),
    )]);
    let mut blk = block(app, title, false);
    let content = app.preview.content.clone();
    let inner0 = blk.inner(area);
    let inner = Rect {
        x: inner0.x + 1,
        width: inner0.width.saturating_sub(2),
        ..inner0
    };
    let w = inner.width as usize;
    let h = inner.height as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut note = String::new();

    match &content {
        None => lines.push(Line::from(Span::styled("", th.dim()))),
        Some(Preview::Empty) => lines.push(Line::from(Span::styled("empty file", th.dim()))),
        Some(Preview::Error(m)) => {
            for l in fmt::wrap(m, w) {
                lines.push(Line::from(Span::styled(l, th.fg(th.danger))));
            }
        }
        Some(Preview::Special(m)) => {
            lines.push(Line::from(Span::styled(m.clone(), th.fg(th.warn))))
        }
        Some(Preview::Text(t)) => {
            let gw = t.lines.len().max(1).to_string().len();
            app.preview.scroll = app.preview.scroll.min(t.lines.len().saturating_sub(1));
            for (i, l) in t.lines.iter().enumerate().skip(app.preview.scroll).take(h) {
                lines.push(Line::from(vec![
                    Span::styled(format!("{:>gw$} ", i + 1), th.dim()),
                    Span::styled(display::truncate(l, w.saturating_sub(gw + 1)), th.base()),
                ]));
            }
            note = format!(
                "{} · {}{}",
                t.encoding,
                fmt::size(t.size),
                if t.truncated {
                    " · beginning only"
                } else {
                    ""
                }
            );
            if t.long_lines > 0 {
                note.push_str(&format!(" · {} long line(s) cut", t.long_lines));
            }
        }
        Some(Preview::Binary(b)) => {
            lines.push(Line::from(vec![
                Span::styled("binary file", th.fg(th.warn)),
                Span::styled(format!(" · {} · {}", b.kind, fmt::size(b.size)), th.dim()),
            ]));
            lines.push(Line::raw(""));
            for l in b
                .hex
                .iter()
                .skip(app.preview.scroll)
                .take(h.saturating_sub(2))
            {
                lines.push(Line::from(Span::styled(display::truncate(l, w), th.dim())));
            }
        }
        Some(Preview::Dir(d)) => {
            app.preview.scroll = app.preview.scroll.min(d.entries.len().saturating_sub(1));
            for (name, is_dir) in d.entries.iter().skip(app.preview.scroll).take(h) {
                let (g, c) = if *is_dir {
                    ("▸ ", th.dir)
                } else {
                    ("· ", th.muted)
                };
                lines.push(Line::from(vec![
                    Span::styled(g, th.fg(c)),
                    Span::styled(
                        display::truncate(name, w.saturating_sub(2)),
                        th.fg(if *is_dir { th.dir } else { th.text }),
                    ),
                ]));
            }
            if d.entries.is_empty() {
                lines.push(Line::from(Span::styled("empty folder", th.dim())));
            }
            note = format!(
                "{} shown{}",
                d.entries.len(),
                if d.truncated {
                    " · more not listed"
                } else {
                    ""
                }
            );
        }
        Some(Preview::Symlink {
            target,
            state,
            inner: inner_pv,
        }) => {
            lines.push(Line::from(vec![
                Span::styled("→ ", th.fg(th.link)),
                Span::styled(display::path(target), th.fg(th.link)),
            ]));
            let (txt, col) = match state {
                LinkState::ToFile => ("points to a file", th.subtle),
                LinkState::ToDir => ("points to a folder", th.subtle),
                LinkState::Broken => ("broken link: the target does not exist", th.danger),
                LinkState::Circular => ("circular link", th.danger),
            };
            lines.push(Line::from(Span::styled(txt, th.fg(col))));
            lines.push(Line::raw(""));
            match inner_pv.as_deref() {
                Some(Preview::Text(t)) => {
                    for l in t.lines.iter().take(h.saturating_sub(3)) {
                        lines.push(Line::from(Span::styled(display::truncate(l, w), th.base())));
                    }
                }
                Some(Preview::Dir(d)) => {
                    for (n, _) in d.entries.iter().take(h.saturating_sub(3)) {
                        lines.push(Line::from(Span::styled(display::truncate(n, w), th.base())));
                    }
                }
                _ => {}
            }
        }
    }
    if !note.is_empty() {
        blk = blk.title_bottom(Line::from(Span::styled(format!(" {note} "), th.dim())));
    }
    f.render_widget(blk, area);
    f.render_widget(Paragraph::new(lines), inner);
}

// ------------------------------------------------------------------------------ progress & footer

fn draw_progress(f: &mut Frame, app: &App, area: Rect) {
    let Some(r) = &app.running else { return };
    let th = &app.th;
    let p = &r.progress;
    let frac = p.fraction();
    let pct = (frac * 100.0) as u32;
    let mut right = String::new();
    if p.bytes_total > 0 {
        right.push_str(&format!(
            "{} / {}",
            fmt::size(p.bytes_done),
            fmt::size(p.bytes_total)
        ));
        if r.rate() > 1.0 {
            right.push_str(&format!(" · {}", fmt::rate(r.rate())));
        }
        if let Some(eta) = r.eta() {
            right.push_str(&format!(" · ETA {}", fmt::duration(eta)));
        }
    } else {
        right.push_str(&format!("{} / {} steps", p.steps_done, p.steps_total));
    }
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(th.accent))
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", SPIN[app.spinner % SPIN.len()]),
                th.fg(th.accent),
            ),
            Span::styled(
                format!(
                    "{} ",
                    display::truncate(&r.title, (area.width as usize).saturating_sub(24))
                ),
                th.title(),
            ),
        ]))
        .title_bottom(Line::from(Span::styled(" Esc to cancel ", th.dim())).right_aligned());
    let inner = blk.inner(area);
    f.render_widget(blk, area);
    if inner.height == 0 {
        return;
    }
    let w = inner.width as usize;
    let label = format!(" {pct:>3}%  {right}");
    let bar_w = w.saturating_sub(label.width() + 1).max(4);
    let filled = ((bar_w as f64) * frac).round() as usize;
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled("━".repeat(filled), th.fg(th.accent)),
        Span::styled("─".repeat(bar_w - filled.min(bar_w)), th.fg(th.gauge_empty)),
        Span::styled(label, th.base()),
    ]);
    f.render_widget(Paragraph::new(line), Rect { height: 1, ..inner });
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let th = &app.th;
    let w = area.width as usize;
    let mut left: Vec<Span> = Vec::new();
    if let Some(t) = &app.toast {
        let c = match t.kind {
            ToastKind::Info => th.accent_soft,
            ToastKind::Ok => th.ok,
            ToastKind::Warn => th.warn,
            ToastKind::Error => th.danger,
        };
        let glyph = match t.kind {
            ToastKind::Info => "●",
            ToastKind::Ok => "✔",
            ToastKind::Warn => "▲",
            ToastKind::Error => "✖",
        };
        left.push(Span::styled(format!(" {glyph} "), th.fg(c)));
        let one_line = t.text.replace('\n', " ");
        if one_line.width() + 6 <= w {
            left.push(Span::styled(one_line, th.fg(c)));
        } else {
            // Too long for one line: the full text is shown wrapped in a box above.
            left.push(Span::styled("full message above", th.dim()));
        }
    } else {
        let hints: &[(&str, &str)] = if app.marked.is_empty() {
            &[
                ("j/k", "move"),
                ("l", "open"),
                ("h", "back"),
                ("space", "mark"),
                ("y/x/p", "copy·cut·paste"),
                ("d", "trash"),
                ("u", "undo"),
                ("?", "help"),
            ]
        } else {
            &[
                ("space", "mark"),
                ("y", "copy"),
                ("x", "cut"),
                ("d", "trash"),
                ("D", "delete"),
                ("R", "bulk rename"),
                ("Esc", "clear"),
            ]
        };
        left.push(Span::raw(" "));
        for (k, d) in hints {
            left.push(Span::styled((*k).to_string(), th.key()));
            left.push(Span::styled(format!(" {d}   "), th.dim()));
        }
    }
    let mut right: Vec<Span> = Vec::new();
    if let Some(v) = app
        .volumes
        .iter()
        .filter(|v| app.cwd.starts_with(&v.mount_point))
        .max_by_key(|v| v.mount_point.as_os_str().len())
    {
        if let Some(a) = v.available {
            right.push(Span::styled(format!("{} free ", fmt::size(a)), th.dim()));
        }
    }
    if let Some(c) = &app.clipboard {
        let verb = if c.mode == vela_core::ops::TransferMode::Copy {
            "copy"
        } else {
            "cut"
        };
        right.push(Span::styled(
            format!(" {} {verb} ", c.paths.len()),
            th.fg(th.accent_soft),
        ));
    }
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();
    let left_w: usize = left.iter().map(|s| s.content.width()).sum();
    let mut spans = left;
    if left_w + right_w < w {
        spans.push(Span::raw(" ".repeat(w - left_w - right_w)));
        spans.extend(right);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A long message (typically an error with a full path) is wrapped, never truncated.
fn draw_toast_box(f: &mut Frame, app: &App, area: Rect, footer: Rect) {
    let Some(t) = &app.toast else { return };
    let th = &app.th;
    let w = area.width as usize;
    let one_line = t.text.replace('\n', " ");
    if one_line.width() + 6 <= w {
        return;
    }
    let color = match t.kind {
        ToastKind::Info => th.accent_soft,
        ToastKind::Ok => th.ok,
        ToastKind::Warn => th.warn,
        ToastKind::Error => th.danger,
    };
    let inner_w = w.saturating_sub(6).max(10);
    let mut lines = fmt::wrap(&t.text, inner_w);
    lines.truncate(6);
    let h = lines.len() as u16 + 2;
    if footer.y < h {
        return;
    }
    let r = Rect {
        x: area.x + 1,
        y: footer.y - h,
        width: area.width.saturating_sub(2),
        height: h,
    };
    f.render_widget(Clear, r);
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(color))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let inner = blk.inner(r);
    f.render_widget(blk, r);
    let text: Vec<Line> = lines
        .into_iter()
        .map(|l| Line::from(Span::styled(l, th.fg(color))))
        .collect();
    f.render_widget(Paragraph::new(text), inner);
}

// ------------------------------------------------------------------------------ modals

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn modal_block<'a>(
    th: &crate::theme::Theme,
    title: &'a str,
    color: ratatui::style::Color,
) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(color))
        .title(Line::from(Span::styled(
            format!(" {title} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )))
        .padding(ratatui::widgets::Padding::horizontal(1))
}

fn draw_modal(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let spinner = SPIN[app.spinner % SPIN.len()];
    let Some(modal) = &app.modal else { return };
    match modal {
        Modal::Scanning {
            what,
            files,
            dirs,
            bytes,
            current,
            ..
        } => {
            let r = centered(area, 64, 7);
            f.render_widget(Clear, r);
            let blk = modal_block(&th, what, th.accent);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let lines = vec![
                Line::from(vec![
                    Span::styled(format!("{spinner} "), th.fg(th.accent)),
                    Span::styled(
                        format!(
                            "{} files · {} folders · {}",
                            fmt::thousands(*files),
                            fmt::thousands(*dirs),
                            fmt::size(*bytes)
                        ),
                        th.base(),
                    ),
                ]),
                Line::from(Span::styled(
                    tail(&display::path(current), inner.width as usize),
                    th.dim(),
                )),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Esc", th.key()),
                    Span::styled(" cancel", th.dim()),
                ]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Plan(pv) => draw_plan(f, &th, pv, area),
        Modal::Failure { info, .. } => {
            let w = 84.min(area.width.saturating_sub(4));
            let msg = fmt::wrap(&info.message, (w as usize).saturating_sub(4));
            let h = (msg.len() as u16 + 6).min(area.height.saturating_sub(2));
            let r = centered(area, w, h);
            f.render_widget(Clear, r);
            let blk = modal_block(&th, "Something went wrong", th.danger);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let mut lines: Vec<Line> = Vec::new();
            for l in msg {
                lines.push(Line::from(Span::styled(l, th.fg(th.danger))));
            }
            if info.attempt > 1 {
                lines.push(Line::from(Span::styled(
                    format!("(attempt {})", info.attempt),
                    th.dim(),
                )));
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("s", th.key()),
                Span::styled(" skip   ", th.dim()),
                Span::styled("S", th.key()),
                Span::styled(" skip all   ", th.dim()),
                Span::styled("r", th.key()),
                Span::styled(" retry   ", th.dim()),
                Span::styled("a", th.key()),
                Span::styled(" abort (keeps what is done; u undoes it)", th.dim()),
            ]));
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Input(iv) => draw_input(f, &th, iv, area),
        Modal::History(h) => draw_history(f, &th, h, area, spinner),
        Modal::Places(p) => {
            let h = (p.items.len() as u16 + 4).min(area.height.saturating_sub(2));
            let r = centered(area, 70, h);
            f.render_widget(Clear, r);
            let blk = modal_block(&th, "Go to", th.accent);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let w = inner.width as usize;
            let lines: Vec<Line> = p
                .items
                .iter()
                .enumerate()
                .map(|(i, pl)| {
                    let sel = i == p.selected;
                    let st = if sel {
                        Style::default().bg(th.cursor_bg)
                    } else {
                        Style::default()
                    };
                    let label = display::truncate(&pl.label, 30);
                    let left = format!("{} {}", if sel { "▌" } else { " " }, pad(&label, 30));
                    let detail = display::truncate(&pl.detail, w.saturating_sub(34));
                    Line::from(vec![
                        Span::styled(left, st.fg(if sel { th.accent } else { th.text })),
                        Span::styled(pad(&detail, w.saturating_sub(32)), st.fg(th.muted)),
                    ])
                })
                .collect();
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Result(r) => {
            let w = 86.min(area.width.saturating_sub(4));
            let wrapped: Vec<(ToastKind, String, bool)> = r
                .lines
                .iter()
                .flat_map(|(k, t)| {
                    fmt::wrap(t, (w as usize).saturating_sub(6))
                        .into_iter()
                        .enumerate()
                        .map(move |(i, l)| (*k, l, i == 0))
                })
                .collect();
            let h = (wrapped.len() as u16 + 5).min(area.height.saturating_sub(2));
            let rect = centered(area, w, h);
            f.render_widget(Clear, rect);
            let title = display::truncate(&r.title, (w as usize).saturating_sub(6));
            let blk = modal_block(&th, &title, th.warn);
            let inner = blk.inner(rect);
            f.render_widget(blk, rect);
            let rows = inner.height.saturating_sub(2) as usize;
            let mut lines: Vec<Line> = wrapped
                .iter()
                .skip(r.scroll)
                .take(rows)
                .map(|(k, t, first)| {
                    let c = if *k == ToastKind::Error {
                        th.danger
                    } else {
                        th.warn
                    };
                    Line::from(vec![
                        Span::styled(if *first { "• " } else { "  " }, th.fg(c)),
                        Span::styled(t.clone(), th.fg(c)),
                    ])
                })
                .collect();
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("u", th.key()),
                Span::styled(" undo what was done   ", th.dim()),
                Span::styled("any other key", th.key()),
                Span::styled(" close", th.dim()),
            ]));
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::ConfirmQuit => {
            let r = centered(area, 62, 6);
            f.render_widget(Clear, r);
            let blk = modal_block(&th, "Operation in progress", th.warn);
            let inner = blk.inner(r);
            f.render_widget(blk, r);
            let lines = vec![
                Line::from(Span::styled(
                    "Quitting cancels the running operation.",
                    th.base(),
                )),
                Line::from(Span::styled(
                    "Steps already finished stay in the journal and can be undone.",
                    th.dim(),
                )),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("y", th.key()),
                    Span::styled(" quit anyway   ", th.dim()),
                    Span::styled("any other key", th.key()),
                    Span::styled(" stay", th.dim()),
                ]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Help => draw_help(f, &th, area),
    }
}

fn draw_plan(f: &mut Frame, th: &crate::theme::Theme, pv: &PlanView, area: Rect) {
    let plan = &pv.plan;
    let blocked = !plan.is_executable() && plan.blocking().next().is_some();
    let danger = plan.kind == OpKind::Delete;
    let color = if blocked || danger {
        th.danger
    } else {
        th.accent
    };
    let w = 100.min(area.width.saturating_sub(4));
    let inner_w = (w as usize).saturating_sub(4);

    let mut body: Vec<Line> = Vec::new();
    // Summary
    let t = &plan.totals;
    let mut parts: Vec<String> = Vec::new();
    match plan.kind {
        OpKind::Rename | OpKind::BulkRename | OpKind::MakeDir | OpKind::Undo => {
            parts.push(fmt::count(plan.steps.len() as u64, "step", "steps"));
        }
        _ => {
            if t.files > 0 {
                parts.push(fmt::count(t.files, "file", "files"));
            }
            if t.dirs > 0 {
                parts.push(fmt::count(t.dirs, "folder", "folders"));
            }
            if t.symlinks > 0 {
                parts.push(fmt::count(t.symlinks, "link", "links"));
            }
            if t.bytes > 0 || !parts.is_empty() {
                parts.push(fmt::size(t.bytes));
            }
        }
    }
    if parts.is_empty() {
        parts.push("nothing to do".to_string());
    }
    body.push(Line::from(Span::styled(
        parts.join(" · "),
        th.base().add_modifier(Modifier::BOLD),
    )));
    if plan.total_bytes() > 0 && plan.total_bytes() != t.bytes {
        body.push(Line::from(Span::styled(
            format!("{} will be written", fmt::size(plan.total_bytes())),
            th.dim(),
        )));
    }
    body.push(Line::raw(""));

    // Warnings
    for wn in &plan.warnings {
        let (g, c) = match wn.severity {
            Severity::Blocking => ("✖", th.danger),
            Severity::Warning => ("▲", th.warn),
            Severity::Info => ("●", th.subtle),
        };
        for (i, l) in fmt::wrap(&wn.message, inner_w.saturating_sub(3))
            .into_iter()
            .enumerate()
        {
            body.push(Line::from(vec![
                Span::styled(if i == 0 { format!("{g} ") } else { "  ".into() }, th.fg(c)),
                Span::styled(l, th.fg(c)),
            ]));
        }
        for ex in wn.examples.iter().take(3) {
            body.push(Line::from(Span::styled(
                format!(
                    "    {}",
                    tail(&display::path(ex), inner_w.saturating_sub(4))
                ),
                th.dim(),
            )));
        }
    }
    if !plan.warnings.is_empty() {
        body.push(Line::raw(""));
    }

    if matches!(pv.replan, Replan::Transfer { .. }) {
        let label = match plan.policy {
            ConflictPolicy::Skip => "skip existing",
            ConflictPolicy::KeepBoth => "keep both",
            ConflictPolicy::Overwrite => "overwrite (old goes to the trash)",
        };
        body.push(Line::from(vec![
            Span::styled("If a name already exists: ", th.dim()),
            Span::styled(label, th.fg(th.accent_soft).add_modifier(Modifier::BOLD)),
            Span::styled("   (c to change)", th.dim()),
        ]));
        body.push(Line::raw(""));
    }

    // Renames: old → new
    if !plan.renames.is_empty() {
        body.push(Line::from(Span::styled(
            "Renames",
            th.fg(th.subtle).add_modifier(Modifier::BOLD),
        )));
        for (from, to) in plan.renames.iter().take(400) {
            let a = from.file_name().map(display::name).unwrap_or_default();
            let b = to.file_name().map(display::name).unwrap_or_default();
            let half = inner_w.saturating_sub(5) / 2;
            body.push(Line::from(vec![
                Span::styled(display::truncate(&a, half), th.dim()),
                Span::styled(" → ", th.fg(th.accent)),
                Span::styled(display::truncate(&b, half), th.base()),
            ]));
        }
        body.push(Line::raw(""));
    } else if !plan.steps.is_empty() {
        body.push(Line::from(Span::styled(
            format!(
                "What will happen ({})",
                fmt::count(plan.steps.len() as u64, "step", "steps")
            ),
            th.fg(th.subtle).add_modifier(Modifier::BOLD),
        )));
        let base = plan.destination.as_deref();
        let mut shown = 0;
        for s in plan.steps.iter() {
            let Some(label) = step_label(s, base) else {
                continue;
            };
            if shown == 500 {
                body.push(Line::from(Span::styled(
                    "… more steps not listed",
                    th.dim(),
                )));
                break;
            }
            body.push(Line::from(Span::styled(tail(&label, inner_w), th.dim())));
            shown += 1;
        }
    }

    let footer_lines = if danger { 3 } else { 2 };
    let h = (body.len() as u16 + footer_lines + 2).clamp(10, area.height.saturating_sub(2));
    let r = centered(area, w, h);
    f.render_widget(Clear, r);
    let title = display::truncate(&plan.title, w as usize - 6);
    let blk = modal_block(th, &title, color);
    let inner = blk.inner(r);
    f.render_widget(blk, r);
    let content_h = inner.height.saturating_sub(footer_lines) as usize;
    let max_scroll = body.len().saturating_sub(content_h);
    let scroll = pv.scroll.min(max_scroll);
    let shown: Vec<Line> = body.into_iter().skip(scroll).take(content_h).collect();
    f.render_widget(
        Paragraph::new(shown),
        Rect {
            height: content_h as u16,
            ..inner
        },
    );

    let fy = inner.y + content_h as u16;
    let mut foot: Vec<Line> = Vec::new();
    if danger {
        foot.push(Line::from(vec![
            Span::styled(
                "This cannot be undone. ",
                th.fg(th.danger).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Type ", th.dim()),
            Span::styled("yes", th.key()),
            Span::styled(" to confirm: ", th.dim()),
            Span::styled(pv.typed.clone(), th.base()),
            Span::styled("▏", th.fg(th.accent)),
        ]));
    }
    let mut keys: Vec<Span> = Vec::new();
    if pv.replanning.is_some() {
        keys.push(Span::styled("re-planning… ", th.fg(th.warn)));
    }
    if pv.can_run() {
        keys.push(Span::styled("Enter", th.key()));
        keys.push(Span::styled(" run   ", th.dim()));
    } else if blocked {
        keys.push(Span::styled(
            "blocked: nothing will be done   ",
            th.fg(th.danger),
        ));
    }
    if !danger {
        keys.push(Span::styled("↑↓", th.key()));
        keys.push(Span::styled(" scroll   ", th.dim()));
    }
    keys.push(Span::styled("Esc", th.key()));
    keys.push(Span::styled(" cancel", th.dim()));
    foot.push(Line::raw(""));
    foot.push(Line::from(keys));
    f.render_widget(
        Paragraph::new(foot),
        Rect {
            y: fy,
            height: footer_lines,
            ..inner
        },
    );
}

/// One line per step, with paths relative to the destination so they stay readable.
fn step_label(step: &vela_core::ops::Step, base: Option<&std::path::Path>) -> Option<String> {
    use vela_core::ops::Step;
    let rel = |p: &std::path::Path| match base.and_then(|b| p.strip_prefix(b).ok()) {
        Some(r) => display::path(r),
        None => display::path(p),
    };
    Some(match step {
        Step::MakeDir { path, .. } => format!("folder   {}", rel(path)),
        Step::FinishDir { .. } => return None,
        Step::CopyFile {
            src,
            dst,
            remove_source,
            ..
        } => {
            format!(
                "{}   {}",
                if *remove_source { "move  " } else { "copy  " },
                if base.is_some() {
                    rel(dst)
                } else {
                    format!("{} → {}", display::path(src), rel(dst))
                }
            )
        }
        Step::CopySymlink {
            dst,
            target,
            remove_source,
            ..
        } => {
            format!(
                "{}   {} → {}",
                if *remove_source { "link↪ " } else { "link  " },
                rel(dst),
                display::path(target)
            )
        }
        Step::Rename { from, to } => match base {
            Some(_) => format!("move     {} → {}", display::path(from), rel(to)),
            None => format!("rename   {} → {}", display::path(from), display::path(to)),
        },
        Step::TrashItem { path } => format!("trash    {}", display::path(path)),
        Step::RemoveFile { path, .. } => format!("delete   {}", display::path(path)),
        Step::RemoveDir { path } => format!("remove   {}/", display::path(path)),
        Step::Restore { item } => format!("restore  {}", display::path(&item.original)),
    })
}

fn draw_input(f: &mut Frame, th: &crate::theme::Theme, iv: &InputView, area: Rect) {
    let (title, hint) = match &iv.kind {
        InputKind::Rename { .. } => ("Rename", "new name"),
        InputKind::NewDir => ("New folder", "name"),
        InputKind::BulkRename { items } => {
            let _ = items;
            (
                "Bulk rename",
                "pattern  {name} {ext} {n} {n:3} {parent} {name:lower}  or  s/find/replace/",
            )
        }
    };
    let preview: Vec<(String, String)> = match &iv.kind {
        InputKind::BulkRename { items } => match vela_core::ops::Pattern::parse(&iv.text) {
            Ok(p) => p
                .preview(items)
                .into_iter()
                .take(12)
                .map(|pv| {
                    let from = pv.from.file_name().map(display::name).unwrap_or_default();
                    let to = match pv.to {
                        Ok(n) => display::name(&n),
                        Err(e) => format!("✖ {e}"),
                    };
                    (from, to)
                })
                .collect(),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };
    let h = 8 + preview.len() as u16;
    let r = centered(area, 84, h);
    f.render_widget(Clear, r);
    let blk = modal_block(th, title, th.accent);
    let inner = blk.inner(r);
    f.render_widget(blk, r);
    let w = inner.width as usize;
    let mut lines = vec![
        Line::from(Span::styled(display::truncate(hint, w), th.dim())),
        {
            let before: String = iv.text.chars().take(iv.cursor).collect();
            let at: String = iv
                .text
                .chars()
                .nth(iv.cursor)
                .map(String::from)
                .unwrap_or_else(|| " ".to_string());
            let after: String = iv.text.chars().skip(iv.cursor + 1).collect();
            Line::from(vec![
                Span::styled("› ", th.fg(th.accent)),
                Span::styled(before, th.base()),
                Span::styled(at, Style::default().add_modifier(Modifier::REVERSED)),
                Span::styled(after, th.base()),
            ])
        },
    ];
    match &iv.error {
        Some(e) => lines.push(Line::from(Span::styled(format!("✖ {e}"), th.fg(th.danger)))),
        None => {
            if let InputKind::BulkRename { .. } = &iv.kind {
                if let Err(e) = vela_core::ops::Pattern::parse(&iv.text) {
                    lines.push(Line::from(Span::styled(format!("✖ {e}"), th.fg(th.danger))));
                } else {
                    lines.push(Line::raw(""));
                }
            } else {
                lines.push(Line::raw(""));
            }
        }
    }
    for (a, b) in preview {
        let half = w.saturating_sub(5) / 2;
        lines.push(Line::from(vec![
            Span::styled(display::truncate(&a, half), th.dim()),
            Span::styled(" → ", th.fg(th.accent)),
            Span::styled(display::truncate(&b, half), th.base()),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Enter", th.key()),
        Span::styled(" plan   ", th.dim()),
        Span::styled("Esc", th.key()),
        Span::styled(" cancel", th.dim()),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_history(
    f: &mut Frame,
    th: &crate::theme::Theme,
    h: &HistoryView,
    area: Rect,
    spinner: &str,
) {
    let rows = h.entries.len().clamp(3, 18) as u16;
    let r = centered(area, 100, rows + 6);
    f.render_widget(Clear, r);
    let blk = modal_block(th, "History", th.accent);
    let inner = blk.inner(r);
    f.render_widget(blk, r);
    let w = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    if h.loading {
        lines.push(Line::from(Span::styled(
            format!("{spinner} reading the journal…"),
            th.dim(),
        )));
    } else if h.entries.is_empty() {
        lines.push(Line::from(Span::styled("No operations yet.", th.dim())));
    }
    let visible = inner.height.saturating_sub(2) as usize;
    let start = h.selected.saturating_sub(visible.saturating_sub(1));
    for (i, e) in h.entries.iter().enumerate().skip(start).take(visible) {
        let sel = i == h.selected;
        let st = if sel {
            Style::default().bg(th.cursor_bg)
        } else {
            Style::default()
        };
        let (g, gc) = match e.status {
            EntryStatus::Finished(RunStatus::Completed) => ("✔", th.ok),
            EntryStatus::Finished(RunStatus::CompletedWithProblems) => ("▲", th.warn),
            EntryStatus::Finished(RunStatus::Aborted)
            | EntryStatus::Finished(RunStatus::Cancelled) => ("■", th.warn),
            EntryStatus::Interrupted => ("?", th.danger),
        };
        let tag = match (&e.undo_state, e.reversible) {
            (UndoState::Undone, _) => "undone".to_string(),
            (UndoState::Partial { remaining }, _) => format!("partly undone ({remaining} left)"),
            (_, false) => "permanent".to_string(),
            _ if e.undo_steps == 0 => "nothing to undo".to_string(),
            _ => "can undo".to_string(),
        };
        let tag_color = match tag.as_str() {
            "can undo" => th.accent_soft,
            "permanent" => th.danger,
            _ => th.muted,
        };
        let when = fmt::clock(e.time);
        let right = format!(" {tag}");
        let title_w = w.saturating_sub(4 + 13 + right.width());
        lines.push(Line::from(vec![
            Span::styled(if sel { "▌" } else { " " }, st.fg(th.accent)),
            Span::styled(format!("{g} "), st.fg(gc)),
            Span::styled(pad(&when, 13), st.fg(th.muted)),
            Span::styled(
                pad(&display::truncate(&e.title, title_w), title_w),
                st.fg(th.text),
            ),
            Span::styled(right, st.fg(tag_color)),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            height: inner.height.saturating_sub(2),
            ..inner
        },
    );
    let foot = Line::from(vec![
        Span::styled("Enter", th.key()),
        Span::styled(" undo selected   ", th.dim()),
        Span::styled("↑↓", th.key()),
        Span::styled(" move   ", th.dim()),
        Span::styled("Esc", th.key()),
        Span::styled(" close", th.dim()),
    ]);
    f.render_widget(
        Paragraph::new(foot),
        Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        },
    );
}

fn draw_help(f: &mut Frame, th: &crate::theme::Theme, area: Rect) {
    let rows: &[(&str, &str)] = &[
        ("j k / ↑ ↓", "move"),
        ("l → Enter", "open folder / open file"),
        ("h ← Backspace", "parent folder"),
        ("g  G", "top / bottom"),
        ("PgUp PgDn", "page"),
        ("J K", "scroll the preview"),
        ("Space", "mark and move down"),
        ("Ctrl-a", "mark / unmark all"),
        ("y  x  p", "copy · cut · paste (always shows a plan first)"),
        ("d", "move to trash"),
        (
            "D",
            "delete permanently (cannot be undone; asks you to type yes)",
        ),
        ("r  F2", "rename"),
        ("R", "bulk rename with a pattern"),
        ("n", "new folder"),
        ("u", "undo the last operation"),
        ("U", "history of operations"),
        ("s  S", "sort by name/size/date · reverse"),
        (".", "show / hide hidden files"),
        ("m  ~", "volumes and places · home"),
        ("Esc", "cancel running operation / clear marks"),
        ("q", "quit"),
    ];
    let h = rows.len() as u16 + 4;
    let r = centered(area, 78, h);
    f.render_widget(Clear, r);
    let blk = modal_block(th, "Keys", th.accent);
    let inner = blk.inner(r);
    f.render_widget(blk, r);
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(pad(k, 16), th.key()),
                Span::styled((*d).to_string(), th.base()),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Every operation shows a plan first. Every finished operation can be undone with u.",
        th.dim(),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}
