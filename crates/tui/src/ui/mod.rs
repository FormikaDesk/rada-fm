//! Drawing. A pure function of the application state: no I/O, no blocking.
//!
//! The look is the one of a file manager people already know: a row of tabs, an address bar,
//! a command bar, a navigation pane on the left, the folder in the middle, a details pane on
//! the right, a status bar and a row of key hints. Each part gives way as the terminal
//! narrows or shortens. Colours come from the theme, and the terminal's own background is
//! left alone.

mod address;
mod bottom;
mod commands;
mod details;
mod grid;
mod list;
mod modals;
mod preview;
mod sidebar;
mod tabs;
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
use crate::sidebar::Mode;
use crate::view::{LayoutKind, ViewMode};
use widgets::{SPIN, dim_backdrop, pad, progress_spans};

/// How the details pane is shown right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailsMode {
    Hidden,
    /// A column on the right of the list.
    Column,
    /// Over the right part of the list.
    Overlay,
}

/// Terminal width from which the details pane is a column of its own.
const DETAILS_FROM: u16 = 140;
/// Narrowest terminal that can show the details pane as an overlay.
const OVERLAY_FROM: u16 = 60;

/// Whether (and how) the details pane shows on a terminal `width` columns wide, given what
/// the user chose: nothing (follow the width), on, or off.
pub fn details_mode(width: u16, chosen: Option<bool>, layout: LayoutKind) -> DetailsMode {
    match chosen {
        Some(false) => DetailsMode::Hidden,
        None if layout == LayoutKind::Compact => DetailsMode::Hidden,
        None => {
            if width >= DETAILS_FROM {
                DetailsMode::Column
            } else {
                DetailsMode::Hidden
            }
        }
        Some(true) => {
            if width >= DETAILS_FROM {
                DetailsMode::Column
            } else if width >= OVERLAY_FROM {
                DetailsMode::Overlay
            } else {
                DetailsMode::Hidden
            }
        }
    }
}

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
    app.term_width = area.width;
    app.screen = (area.width, area.height);

    // Top to bottom: tabs, address, commands, the body, the progress of a running operation,
    // the status bar and the key hints. The hints go first when the terminal is short, the
    // command bar next; the compact layout has neither, nor the address bar.
    let explorer = app.layout == LayoutKind::Explorer;
    let tabs_h = 1;
    let address_h = u16::from(explorer);
    let commands_h = u16::from(explorer && area.height >= 18);
    let hints_h = u16::from(explorer && app.show_hints && area.height >= 24);
    let progress_h = if app.running.is_some() { 4 } else { 0 };
    let [
        tabs_row,
        address_row,
        commands_row,
        body,
        progress,
        status,
        hints_row,
    ] = Layout::vertical([
        Constraint::Length(tabs_h),
        Constraint::Length(address_h),
        Constraint::Length(commands_h),
        Constraint::Min(3),
        Constraint::Length(progress_h),
        Constraint::Length(1),
        Constraint::Length(hints_h),
    ])
    .areas(area);

    tabs::draw_tabs(f, app, tabs_row);
    if address_h > 0 {
        address::draw_address(f, app, address_row);
    }
    if commands_h > 0 {
        commands::draw_commands(f, app, commands_row);
    }

    // Body: the navigation pane, the folder, and the details pane when there is room.
    app.side_mode = Mode::for_width(area.width, app.side_on);
    if app.side_mode == Mode::Hidden {
        app.side_focus = false;
    }
    let dmode = details_mode(area.width, app.details, app.layout);
    app.details_shown = dmode;
    let mut rest = body;
    if app.side_mode != Mode::Hidden {
        let width = if app.side_mode == Mode::Full {
            crate::sidebar::FULL_WIDTH
        } else {
            crate::sidebar::RAIL_WIDTH
        };
        let [side_area, divider, others] = Layout::horizontal([
            Constraint::Length(width),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(rest);
        let mode = app.side_mode;
        sidebar::draw_sidebar(f, app, side_area, mode);
        let rule: Vec<Line> = (0..divider.height)
            .map(|_| Line::from(Span::styled("│", th.faint())))
            .collect();
        f.render_widget(Paragraph::new(rule), divider);
        rest = others;
    }
    let main = if dmode == DetailsMode::Column {
        let pw = (rest.width * 27 / 100).clamp(34, 50);
        let [left, divider, right] = Layout::horizontal([
            Constraint::Min(30),
            Constraint::Length(1),
            Constraint::Length(pw),
        ])
        .areas(rest);
        let rule: Vec<Line> = (0..divider.height)
            .map(|_| Line::from(Span::styled("│", th.faint())))
            .collect();
        f.render_widget(Paragraph::new(rule), divider);
        details::draw_details(f, app, right);
        left
    } else {
        rest
    };
    match app.view {
        ViewMode::Details => list::draw_list(f, app, main),
        ViewMode::Icons => grid::draw_grid(f, app, main),
    }
    if dmode == DetailsMode::Overlay {
        details::draw_overlay(f, app, main);
    }
    if app.running.is_some() {
        draw_progress(f, app, progress);
    }
    bottom::draw_status(f, app, status);
    if hints_h > 0 {
        bottom::draw_hints(f, app, hints_row);
    }
    let toast_anchor = if hints_h > 0 { hints_row } else { status };
    draw_toast(f, app, area, toast_anchor);
    if app.address.is_some() && address_h > 0 {
        address::draw_suggestions(f, app, address_row, area);
    }

    if app.modal.is_some() {
        // Only the window is clickable while it is open.
        app.hits.clear();
        if !matches!(app.modal, Some(Modal::Menu(_)) | Some(Modal::FullPreview)) {
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
