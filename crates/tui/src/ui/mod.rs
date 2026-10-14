//! Drawing. A pure function of the application state: no I/O, no blocking.
//!
//! The look is the one of a file manager people already know: a row of tabs, an address bar,
//! a command bar, a navigation pane on the left, the folder in the middle, a details pane on
//! the right, a status bar and a row of key hints. Each part gives way as the terminal
//! narrows or shortens. Colours come from the theme, and the terminal's own background is
//! left alone.

mod address;
mod bottom;
mod columns;
mod commands;
mod details;
mod dialogs;
mod grid;
mod list;
mod modals;
mod notice;
mod preview;
mod sidebar;
pub mod soft;
mod tabs;
mod tagdots;
mod updates;
pub mod widgets;

pub use list::{Columns as ListColumns, ListOpts, columns as list_columns};

use std::path::Path;

use rada_core::display;
use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
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

/// The smallest terminal that shows the whole explorer: the navigation pane in full, the
/// command bar and the key hints.
pub const FULL_LAYOUT_MIN: (u16, u16) = (100, 24);

/// Whether the one-line advice to enlarge the window is shown on a terminal of this size:
/// below [`FULL_LAYOUT_MIN`], with room to spare a row for it, in the explorer layout (the
/// compact one is a choice of a small layout), and until the person has closed it.
pub fn size_notice_shown(app: &App, width: u16, height: u16) -> bool {
    app.layout == LayoutKind::Explorer
        && !app.size_notice_dismissed
        && (width < FULL_LAYOUT_MIN.0 || height < FULL_LAYOUT_MIN.1)
        && width >= 24
        && height >= 12
}

/// Rows between the tabs and the address: half a row of space, made of a ▀ in the colour of
/// the tabs. It is the first thing a short terminal gives up.
pub fn top_gap_height(explorer: bool, height: u16) -> u16 {
    u16::from(explorer && height >= 31)
}

/// Rows of the command bar on a terminal `height` rows tall: three (the bar and a half-block
/// padding above and below) while there is room, then one, then none. The padding of the
/// hints goes before the padding of the command bar, the hints before the command bar.
pub fn commands_height(explorer: bool, height: u16) -> u16 {
    match (explorer, height) {
        (false, _) => 0,
        (true, h) if h >= 28 => 3,
        (true, h) if h >= 18 => 1,
        _ => 0,
    }
}

/// Rows of the strip of hints: three, then one, then none.
pub fn hints_height(wanted: bool, height: u16) -> u16 {
    match (wanted, height) {
        (false, _) => 0,
        (true, h) if h >= 30 => 3,
        (true, h) if h >= 24 => 1,
        _ => 0,
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width < 24 || area.height < 8 {
        f.render_widget(Paragraph::new(Key::TooSmall.tr()), area);
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
    let gap_h = top_gap_height(explorer, area.height);
    let look = soft::Look::of(app);
    // The thin line under the bar of the path takes a row of its own where it can be drawn.
    let address_h = if explorer {
        1 + u16::from(look.caps && area.height >= 26)
    } else {
        0
    };
    let commands_h = commands_height(explorer, area.height);
    let hints_h = hints_height(explorer && app.show_hints, area.height);
    let progress_h = if app.running.is_some() { 4 } else { 0 };
    let notice = size_notice_shown(app, area.width, area.height);
    app.size_notice_visible = notice;
    let [
        tabs_row,
        gap_row,
        address_row,
        commands_row,
        body,
        notice_row,
        progress,
        status,
        hints_row,
    ] = Layout::vertical([
        Constraint::Length(tabs_h),
        Constraint::Length(gap_h),
        Constraint::Length(address_h),
        Constraint::Length(commands_h),
        Constraint::Min(3),
        Constraint::Length(u16::from(notice)),
        Constraint::Length(progress_h),
        Constraint::Length(1),
        Constraint::Length(hints_h),
    ])
    .areas(area);

    tabs::draw_tabs(f, app, tabs_row);
    if let (true, Some(band)) = (gap_h > 0, th.band().bg) {
        // The tabs' colour on the upper half, the window's below: half a row of air.
        f.render_widget(
            Paragraph::new(Span::styled(
                "▀".repeat(area.width as usize),
                Style::default().fg(band),
            )),
            gap_row,
        );
    }
    if address_h > 0 {
        address::draw_address(f, app, address_row);
    }
    if commands_h > 0 {
        commands::draw_commands(f, app, commands_row);
    }

    // Body: the navigation pane, the folder, and the details pane when there is room; in the
    // explorer layout each is a card with rounded corners, a column of air between them.
    app.side_mode = Mode::for_width(area.width, app.side_on);
    if app.side_mode == Mode::Hidden {
        app.side_focus = false;
    }
    let dmode = details_mode(area.width, app.details, app.layout);
    app.details_shown = dmode;
    let carded = explorer && body.height >= 3;
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
        if carded {
            let inner = soft::card(f, side_area, &th, app.side_focus);
            sidebar::draw_sidebar(f, app, inner, mode);
        } else {
            sidebar::draw_sidebar(f, app, side_area, mode);
            let rule: Vec<Line> = (0..divider.height)
                .map(|_| Line::from(Span::styled("│", th.faint())))
                .collect();
            f.render_widget(Paragraph::new(rule), divider);
        }
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
        if carded {
            let inner = soft::card(f, right, &th, false);
            details::draw_details(f, app, inner);
        } else {
            let rule: Vec<Line> = (0..divider.height)
                .map(|_| Line::from(Span::styled("│", th.faint())))
                .collect();
            f.render_widget(Paragraph::new(rule), divider);
            details::draw_details(f, app, right);
        }
        left
    } else {
        rest
    };
    // The folder's own card.
    let main_outer = main;
    let main = if carded {
        soft::card(f, main, &th, false)
    } else {
        main
    };
    let _ = main_outer;
    match app.view {
        ViewMode::Columns => columns::draw_columns(f, app, main),
        ViewMode::Details => list::draw_list(
            f,
            app,
            main,
            list::ListOpts::for_terminal(
                area.width as usize,
                app.show_tags && app.tags_supported(),
            ),
        ),
        ViewMode::Icons => grid::draw_grid(f, app, main),
    }
    if dmode == DetailsMode::Overlay {
        details::draw_overlay(f, app, main);
    }
    if notice {
        let text = format!(" {}", tf(Key::EnlargeNotice, &[&area.width, &area.height]));
        f.render_widget(
            Paragraph::new(Span::styled(
                display::truncate(&text, area.width as usize),
                th.faint(),
            )),
            notice_row,
        );
    }
    if app.running.is_some() {
        draw_progress(f, app, progress);
    }
    bottom::draw_status(f, app, status);
    if hints_h > 0 {
        bottom::draw_hints(f, app, hints_row);
    }
    // Notifications sit at the bottom right of the body; a window in front hides them.
    if app.modal.is_none() {
        notice::draw_notice(f, app, body);
    }
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
            right.push_str(&format!(
                "  ·  {}",
                tf(Key::ProgLeft, &[&fmt::duration(eta)])
            ));
        }
    } else {
        right.push_str(&format!(
            "{} / {}",
            p.steps_done,
            fmt::count(p.steps_total, Key::StepOne, Key::StepMany)
        ));
    }
    let head_right = format!("{pct:>3}%   {right}   Esc {}", Key::ProgCancel.tr());
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
        Span::styled(format!(" {}", Key::ProgCancel.tr()), th.dim()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_terminal_gives_up_the_padding_then_the_hints_then_the_commands() {
        // From tall to short: what is left of the two bars at each height.
        let rows = |h: u16| (commands_height(true, h), hints_height(true, h));
        assert_eq!(rows(35), (3, 3));
        assert_eq!(rows(30), (3, 3));
        assert_eq!(rows(29), (3, 1), "the hints' padding goes first");
        assert_eq!(rows(28), (3, 1));
        assert_eq!(rows(27), (1, 1), "then the command bar's");
        assert_eq!(rows(24), (1, 1));
        assert_eq!(rows(23), (1, 0), "then the hints");
        assert_eq!(rows(18), (1, 0));
        assert_eq!(rows(17), (0, 0), "then the command bar");
        // Never more as the terminal gets shorter.
        for h in (9..40u16).rev() {
            let (c, k) = rows(h);
            let (c2, k2) = rows(h - 1);
            assert!(c2 <= c && k2 <= k);
        }
        assert_eq!(commands_height(false, 40), 0);
        assert_eq!(hints_height(false, 40), 0);
    }

    #[test]
    fn the_space_under_the_tabs_is_the_first_thing_a_short_terminal_gives_up() {
        assert_eq!(top_gap_height(true, 37), 1);
        assert_eq!(top_gap_height(true, 31), 1);
        assert_eq!(top_gap_height(true, 30), 0);
        // While it goes, the bars below still have all their padding.
        assert_eq!((commands_height(true, 30), hints_height(true, 30)), (3, 3));
        assert_eq!(top_gap_height(false, 40), 0, "the compact layout has none");
    }

    #[test]
    fn the_details_pane_follows_the_width_unless_the_user_chose() {
        use crate::view::LayoutKind::*;
        assert_eq!(details_mode(170, None, Explorer), DetailsMode::Column);
        assert_eq!(details_mode(140, None, Explorer), DetailsMode::Column);
        assert_eq!(details_mode(139, None, Explorer), DetailsMode::Hidden);
        assert_eq!(
            details_mode(120, Some(true), Explorer),
            DetailsMode::Overlay
        );
        assert_eq!(details_mode(59, Some(true), Explorer), DetailsMode::Hidden);
        assert_eq!(
            details_mode(170, Some(false), Explorer),
            DetailsMode::Hidden
        );
        assert_eq!(details_mode(170, None, Compact), DetailsMode::Hidden);
    }
}
