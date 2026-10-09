//! Drawing. A pure function of the application state: no I/O, no blocking.
//!
//! The look: a calm list with no boxes around it, a breadcrumb on top, one line of hints
//! at the bottom, and temporary things (progress, notifications, windows) that appear only
//! when there is something to say. Colours come from the theme, and the terminal's own
//! background is left alone.

mod list;
mod modals;
mod preview;
pub mod widgets;

use std::path::Path;

use rada_core::display;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::*;
use crate::fmt;
use crate::hits::Target;
use crate::keymap::Action;
use crate::theme::Density;
use widgets::{SPIN, dim_backdrop, hit_spans, pad, progress_spans};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width < 24 || area.height < 8 {
        f.render_widget(Paragraph::new("terminal too small"), area);
        return;
    }
    let th = app.th.clone();
    app.hits.clear();
    if let Some(bg) = th.bg {
        f.render_widget(Block::default().style(Style::default().bg(bg)), area);
    }
    let margin = match th.density {
        Density::Airy => 3,
        Density::Balanced => 2,
        Density::Dense => 1,
    }
    .min(area.width / 12);
    let airy = th.density == Density::Airy && area.height >= 24;
    let progress_h = if app.running.is_some() { 4 } else { 0 };
    let [header, gap, body, progress, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(airy)),
        Constraint::Min(3),
        Constraint::Length(progress_h),
        Constraint::Length(1),
    ])
    .areas(area);
    let side = |r: Rect| Rect {
        x: r.x + margin,
        width: r.width.saturating_sub(margin * 2),
        ..r
    };
    let _ = gap;

    draw_header(f, app, side(header));

    let body = side(body);
    // The preview needs room: it hides itself on narrow terminals.
    if body.width >= 88 {
        let pw = ((body.width as u32 * 36 / 100) as u16).max(30);
        let [left, divider, right] = Layout::horizontal([
            Constraint::Min(30),
            Constraint::Length(1),
            Constraint::Length(pw),
        ])
        .areas(body);
        list::draw_list(
            f,
            app,
            Rect {
                width: left.width.saturating_sub(1),
                ..left
            },
        );
        let rule: Vec<Line> = (0..divider.height)
            .map(|_| Line::from(Span::styled("│", th.faint())))
            .collect();
        f.render_widget(Paragraph::new(rule), divider);
        app.hits.add(right, Target::Preview);
        preview::draw_preview(f, app, right);
    } else {
        list::draw_list(f, app, body);
    }
    if app.running.is_some() {
        draw_progress(
            f,
            app,
            Rect {
                x: area.x + margin,
                width: area.width.saturating_sub(margin * 2),
                ..progress
            },
        );
    }
    draw_footer(
        f,
        app,
        Rect {
            x: area.x + margin,
            width: area.width.saturating_sub(margin * 2),
            ..footer
        },
    );
    draw_toast(f, app, area, footer);

    if app.modal.is_some() {
        // Only the window is clickable while it is open.
        app.hits.clear();
        if !matches!(app.modal, Some(Modal::Menu(_))) {
            dim_backdrop(f.buffer_mut(), area, th.light);
        }
        modals::draw_modal(f, app, area);
    }
}

// ----------------------------------------------------------------------------- header

/// `~ › projects › rada`, the current folder in the accent colour. Each segment comes
/// with the folder it stands for, so a click can go there.
fn breadcrumb<'a>(app: &App, max: usize) -> Vec<(Span<'a>, Option<std::path::PathBuf>)> {
    let th = &app.th;
    let home = app.home();
    let mut acc: std::path::PathBuf;
    let (root, rest): (String, Vec<String>) = match app.cwd.strip_prefix(home) {
        Ok(r) => {
            acc = home.to_path_buf();
            (
                "~".to_string(),
                r.components()
                    .map(|c| display::name(c.as_os_str()))
                    .collect(),
            )
        }
        Err(_) => {
            acc = std::path::PathBuf::from(std::path::MAIN_SEPARATOR_STR);
            (
                std::path::MAIN_SEPARATOR.to_string(),
                app.cwd
                    .components()
                    .filter(|c| matches!(c, std::path::Component::Normal(_)))
                    .map(|c| display::name(c.as_os_str()))
                    .collect(),
            )
        }
    };
    // The real path of every segment (names are shown lossy, paths are exact).
    let mut paths = vec![acc.clone()];
    let tail_components: Vec<std::ffi::OsString> = app
        .cwd
        .strip_prefix(&acc)
        .map(|r| {
            r.components()
                .map(|c| c.as_os_str().to_os_string())
                .collect()
        })
        .unwrap_or_default();
    for c in tail_components {
        acc.push(c);
        paths.push(acc.clone());
    }
    let mut segs: Vec<String> = vec![root];
    segs.extend(rest);
    // Drop from the left until it fits.
    let width = |v: &[String], ell: bool| {
        v.iter().map(|s| s.width()).sum::<usize>()
            + v.len().saturating_sub(1) * 3
            + if ell { 4 } else { 0 }
    };
    let mut start = 0;
    while start + 1 < segs.len() && width(&segs[start..], start > 0) > max {
        start += 1;
    }
    let mut spans: Vec<(Span, Option<std::path::PathBuf>)> = Vec::new();
    if start > 0 {
        spans.push((Span::styled("…", th.faint()), None));
        spans.push((Span::styled(" › ", th.faint()), None));
    }
    let last = segs.len() - 1;
    for (i, s) in segs.iter().enumerate().skip(start) {
        if i > start {
            spans.push((Span::styled(" › ", th.faint()), None));
        }
        let text = if i == last {
            display::truncate(s, max.saturating_sub(2).max(8))
        } else {
            s.clone()
        };
        let path = paths.get(i).cloned();
        if i == last {
            spans.push((
                Span::styled(text, th.accent_style().add_modifier(Modifier::BOLD)),
                path,
            ));
        } else {
            spans.push((Span::styled(text, th.dim()), path));
        }
    }
    spans
}

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let th = &th;
    // The right side is a row of small groups. When the terminal is narrow the least
    // important ones go first (help, then the jump button, then the rest), so the
    // breadcrumb on the left always keeps some room.
    type Group<'a> = (Vec<Span<'a>>, Vec<(usize, Target)>);
    let mut groups: Vec<Group> = Vec::new();

    let mut status: Vec<Span> = Vec::new();
    if app.is_loading() {
        status.push(Span::styled(
            format!("{} ", SPIN[app.spinner % SPIN.len()]),
            th.fg(th.warn),
        ));
    }
    if app.show_hidden {
        status.push(Span::styled("hidden shown   ", th.fg(th.warn)));
    }
    if let Some(fl) = &app.filter {
        status.push(Span::styled("▽ ", th.accent_style()));
        status.push(Span::styled(
            fl.text.clone(),
            th.base().add_modifier(Modifier::BOLD),
        ));
        status.push(Span::styled(
            if fl.editing { "▏" } else { "" },
            th.accent_style(),
        ));
        status.push(Span::styled(
            if fl.editing {
                "  Enter keep · Esc clear"
            } else {
                "  Esc clear"
            },
            th.faint(),
        ));
        status.push(Span::raw("   "));
    }
    if !status.is_empty() {
        groups.push((status, Vec::new()));
    }
    let position = if app.visible.is_empty() {
        "0 items".to_string()
    } else {
        format!("{}/{} items", app.cursor + 1, app.visible.len())
    };
    groups.push((vec![Span::styled(position, th.dim())], Vec::new()));
    if let Some(k) = app.keymap.hint(Action::Palette) {
        let t = Target::Act(Action::Palette);
        groups.push((
            vec![
                Span::styled("   ", th.dim()),
                Span::styled("⌕ ", th.dim()),
                Span::styled(k, th.key()),
            ],
            vec![(1, t.clone()), (2, t)],
        ));
    }
    if let Some(k) = app.keymap.hint(Action::Help) {
        let t = Target::Act(Action::Help);
        groups.push((
            vec![
                Span::styled("   ", th.dim()),
                Span::styled(k, th.key()),
                Span::styled(" help", th.dim()),
            ],
            vec![(1, t.clone()), (2, t)],
        ));
    }

    let natural: usize = breadcrumb(app, usize::MAX / 4)
        .iter()
        .map(|(s, _)| s.content.width())
        .sum();
    let width_of = |g: &Group| g.0.iter().map(|s| s.content.width()).sum::<usize>();
    let budget = (area.width as usize).saturating_sub(natural.min(28) + 6);
    let mut kept = groups.len();
    while kept > 1 && groups[..kept].iter().map(width_of).sum::<usize>() > budget {
        kept -= 1;
    }
    groups.truncate(kept);
    let mut right: Vec<Span> = Vec::new();
    let mut right_hits: Vec<(usize, Target)> = Vec::new();
    for (spans, hits) in groups {
        let base = right.len();
        right_hits.extend(hits.into_iter().map(|(i, t)| (base + i, t)));
        right.extend(spans);
    }
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();

    let mut spans = vec![Span::styled("▍", th.accent_style()), Span::raw(" ")];
    let mut hit_list: Vec<(usize, Target)> = Vec::new();
    for (span, path) in breadcrumb(app, (area.width as usize).saturating_sub(right_w + 6)) {
        if let Some(p) = path {
            hit_list.push((spans.len(), Target::Crumb(p)));
        }
        spans.push(span);
    }
    hit_spans(&mut app.hits, area.x, area.y, &spans, &hit_list);
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    let gap = (area.width as usize).saturating_sub(used + right_w);
    spans.push(Span::raw(" ".repeat(gap)));
    let right_x = area.x + (used + gap) as u16;
    hit_spans(&mut app.hits, right_x, area.y, &right, &right_hits);
    spans.extend(right);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ----------------------------------------------------------------------------- footer

fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let th = &th;
    let w = area.width as usize;
    // What is worth reminding, in order of importance; each is clickable.
    let wanted: &[(Action, &str)] = if app.marked.is_empty() {
        &[
            (Action::Open, "open"),
            (Action::ToggleMark, "mark"),
            (Action::Copy, "copy"),
            (Action::Cut, "cut"),
            (Action::Paste, "paste"),
            (Action::Trash, "trash"),
            (Action::Rename, "rename"),
            (Action::Undo, "undo"),
        ]
    } else {
        &[
            (Action::ToggleMark, "mark"),
            (Action::Copy, "copy"),
            (Action::Cut, "cut"),
            (Action::Trash, "trash"),
            (Action::DeletePermanently, "delete"),
            (Action::BulkRename, "bulk rename"),
            (Action::ClearSelection, "clear"),
        ]
    };
    let hints: Vec<(Action, String, &str)> = wanted
        .iter()
        .filter_map(|(a, d)| app.keymap.hint(*a).map(|k| (*a, k, *d)))
        .collect();

    // Right side: what is selected, what is on the clipboard, how much room is left.
    let mut right: Vec<Span> = Vec::new();
    if !app.marked.is_empty() {
        let size: u64 = app
            .listing
            .all()
            .iter()
            .filter(|e| app.marked.contains(&e.name))
            .map(|e| e.size)
            .sum();
        right.push(Span::styled(
            format!("● {} selected · {}", app.marked.len(), fmt::size(size)),
            th.accent_style().add_modifier(Modifier::BOLD),
        ));
        right.push(Span::raw("   "));
    }
    if let Some(c) = &app.clipboard {
        let verb = if c.mode == rada_core::ops::TransferMode::Copy {
            "copied"
        } else {
            "cut"
        };
        right.push(Span::styled(
            format!("⎘ {} {verb}", c.paths.len()),
            th.fg(th.kinds.vector),
        ));
        right.push(Span::raw("   "));
    }
    if let Some(v) = app
        .volumes
        .iter()
        .filter(|v| app.cwd.starts_with(&v.mount_point))
        .max_by_key(|v| v.mount_point.as_os_str().len())
        && let Some(a) = v.available
    {
        right.push(Span::styled(format!("{} free", fmt::size(a)), th.dim()));
    }
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();

    let mut left: Vec<Span> = Vec::new();
    let mut used = 0;
    let mut foot_hits: Vec<(usize, Target)> = Vec::new();
    for (action, k, d) in &hints {
        let chunk = k.width() + 1 + d.width() + 3;
        if used + chunk + right_w + 2 > w {
            break;
        }
        foot_hits.push((left.len(), Target::Act(*action)));
        foot_hits.push((left.len() + 1, Target::Act(*action)));
        left.push(Span::styled(k.clone(), th.key()));
        left.push(Span::styled(format!(" {d}   "), th.dim()));
        used += chunk;
    }
    hit_spans(&mut app.hits, area.x, area.y, &left, &foot_hits);
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(w.saturating_sub(used + right_w))));
    spans.extend(right);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A notification: a small pill that comes and goes by itself. Long messages (an error
/// with a full path) are wrapped in a box, never truncated.
fn draw_toast(f: &mut Frame, app: &App, area: Rect, footer: Rect) {
    let Some(t) = &app.toast else { return };
    let th = &app.th;
    let color = match t.kind {
        ToastKind::Info => th.accent,
        ToastKind::Ok => th.success,
        ToastKind::Warn => th.warn,
        ToastKind::Error => th.error,
    };
    let glyph = match t.kind {
        ToastKind::Info => "●",
        ToastKind::Ok => "✔",
        ToastKind::Warn => "▲",
        ToastKind::Error => "✖",
    };
    let one_line = t.text.replace('\n', " ");
    let max_line = (area.width as usize).saturating_sub(10);
    if footer.y == 0 {
        return;
    }
    if one_line.width() + 6 <= max_line.min(90) {
        let text = format!(" {glyph}  {one_line} ");
        let w = text.width() as u16;
        let r = Rect {
            x: area.x + area.width.saturating_sub(w + 2),
            y: footer.y - 1,
            width: w,
            height: 1,
        };
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(Span::styled(
                text,
                Style::default()
                    .fg(color)
                    .bg(th.tint(color))
                    .add_modifier(Modifier::BOLD),
            )),
            r,
        );
        return;
    }
    let inner_w = max_line.clamp(10, 96);
    let mut lines = fmt::wrap(&t.text, inner_w);
    lines.truncate(6);
    let h = lines.len() as u16 + 2;
    if footer.y < h {
        return;
    }
    let widest = lines.iter().map(|l| l.width()).max().unwrap_or(10) as u16 + 6;
    let r = Rect {
        x: area.x + area.width.saturating_sub(widest + 2),
        y: footer.y - h,
        width: widest,
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

// ----------------------------------------------------------------------------- progress

/// The temporary bar at the bottom while an operation runs.
fn draw_progress(f: &mut Frame, app: &App, area: Rect) {
    let Some(r) = &app.running else { return };
    let th = &app.th;
    let p = &r.progress;
    let frac = p.fraction();
    let pct = (frac * 100.0) as u32;
    let w = area.width as usize;
    if area.height < 3 {
        return;
    }

    let mut right = String::new();
    if p.bytes_total > 0 {
        right.push_str(&format!(
            "{} / {}",
            fmt::size(p.bytes_done),
            fmt::size(p.bytes_total)
        ));
        if r.rate() > 1.0 {
            right.push_str(&format!("  ·  {}", fmt::rate(r.rate())));
        }
        if let Some(eta) = r.eta() {
            right.push_str(&format!("  ·  {} left", fmt::duration(eta)));
        }
    } else {
        right.push_str(&format!(
            "{} / {}",
            p.steps_done,
            fmt::count(p.steps_total, "step", "steps")
        ));
    }
    let head_right = format!("{pct:>3}%   {right}   Esc cancel");
    // The title names a folder with its full path; show it the way the rest of the
    // interface does.
    let full_title = r.title.replace(&display::path(app.home()), "~");
    let title = display::truncate(&full_title, w.saturating_sub(head_right.width() + 6));
    let gap = w.saturating_sub(title.width() + head_right.width() + 3);
    let line1 = Line::from(vec![
        Span::styled(
            format!("{} ", SPIN[app.spinner % SPIN.len()]),
            th.accent_style(),
        ),
        Span::styled(title, th.base().add_modifier(Modifier::BOLD)),
        Span::raw(" ".repeat(gap + 1)),
        Span::styled(
            format!("{pct:>3}%"),
            th.accent_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("   {right}   "), th.dim()),
        Span::styled("Esc", th.key()),
        Span::styled(" cancel", th.dim()),
    ]);
    let y = area.y + 1;
    f.render_widget(
        Paragraph::new(line1),
        Rect {
            y,
            height: 1,
            ..area
        },
    );

    // The bar moves in eighths of a cell: smooth even on a slow copy.
    let bar_w = w;
    f.render_widget(
        Paragraph::new(Line::from(progress_spans(th, frac, bar_w))),
        Rect {
            y: y + 1,
            height: 1,
            ..area
        },
    );

    let current = if p.current.as_os_str().is_empty() {
        String::new()
    } else {
        widgets::tail(&fmt::short_path(&p.current, app.home()), w)
    };
    f.render_widget(
        Paragraph::new(Span::styled(pad(&current, w), th.faint())),
        Rect {
            y: y + 2,
            height: 1,
            ..area
        },
    );
    let _ = Alignment::Left;
    let _: &Path = app.home();
}
