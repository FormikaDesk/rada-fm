//! Drawing. A pure function of the application state: no I/O, no blocking.
//!
//! The look: a calm list with no boxes around it, a breadcrumb on top, one line of hints
//! at the bottom, and temporary things (progress, notifications, windows) that appear only
//! when there is something to say. Colours come from the theme, and the terminal's own
//! background is left alone.

mod footer;
mod header;
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
use crate::theme::Density;
use widgets::{SPIN, dim_backdrop, pad, progress_spans};

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
    let progress_h = if app.running.is_some() { 4 } else { 0 };
    let [header, body, progress, footer] = Layout::vertical([
        Constraint::Length(1),
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

    header::draw_header(f, app, header);

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
        draw_progress(f, app, side(progress));
    }
    footer::draw_footer(f, app, footer);
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
