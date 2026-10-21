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
mod menu;
mod modals;
mod notice;
mod openwith;
mod panels;
mod plan;
pub mod popup;
mod preview;
mod sidebar;
pub mod soft;
mod tabs;
mod tagdots;
mod updates;
pub mod widgets;

pub use list::{Columns as ListColumns, ListOpts, columns as list_columns};
pub use plan::policy_step;

use rada_core::display;
use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::*;
use crate::sidebar::Mode;
use crate::view::{LayoutKind, ViewMode};
use widgets::dim_backdrop;

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

/// Whether the one-line advice to enlarge the window is shown on a terminal of this size: only
/// when the person asked for the panels (`layout = "panels"`) and they do not fit, below
/// [`FULL_LAYOUT_MIN`], with room to spare a row for it, and until the person has closed it.
/// With `auto` rada chooses the view and there is nothing to ask; `classic` and `compact` are
/// choices of a small layout.
pub fn size_notice_shown(app: &App, width: u16, height: u16) -> bool {
    app.layout == LayoutKind::Explorer
        && app.layout_pref == crate::LayoutPref::Panels
        && !app.size_notice_dismissed
        && app.panels_wanted(width, height).is_none()
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

/// The bar of a file chooser: what is being asked, and the keys that answer.
fn draw_chooser_bar(f: &mut Frame, app: &App, area: Rect) {
    let Some(ch) = &app.chooser else { return };
    let th = &app.th;
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let title = format!(" {} ", ch.title());
    let room = (area.width as usize).saturating_sub(display::width(&title) + 2);
    let hint = display::truncate(ch.hint(room), room);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                th.accent_style()
                    .add_modifier(ratatui::style::Modifier::BOLD)
                    .patch(band),
            ),
            Span::styled(format!(" {hint}"), th.dim().patch(band)),
        ])),
        area,
    );
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
    app.popup_covers = popup_rects(app, area);
    let columns = app.panels_wanted(area.width, area.height);
    app.panels_shown = columns.is_some();
    app.panels_two = columns == Some(crate::panels::Columns::Two);
    if app.panels_shown {
        // The panels view has its own arrangement of everything; the overlays are the same.
        app.size_notice_visible = false;
        app.side_mode = Mode::Full;
        app.details_shown = DetailsMode::Column;
        let address_row = panels::draw(f, app, area);
        finish(f, app, area, Some(address_row));
        return;
    }
    if app.preview_focus {
        app.preview_focus = false;
    }

    // Top to bottom: tabs, address, commands, the body, the progress of a running operation,
    // the status bar and the key hints. The hints go first when the terminal is short, the
    // command bar next; the compact layout has neither, nor the address bar.
    let explorer = app.layout == LayoutKind::Explorer;
    let tabs_h = 1;
    // The half row of air is made of ▀: the basic profile has no such glyph.
    let gap_h = if app.basic {
        0
    } else {
        top_gap_height(explorer, area.height)
    };
    let address_h = u16::from(explorer);
    // A file chooser has a bar of its own at the top, and none of the buttons or key hints
    // for what it does not do.
    let chooser_h = u16::from(app.chooser.is_some());
    let commands_h = if app.chooser.is_some() {
        0
    } else {
        commands_height(explorer, area.height)
    };
    let hints_h = if app.chooser.is_some() {
        0
    } else {
        hints_height(explorer && app.show_hints, area.height)
    };
    let notice = size_notice_shown(app, area.width, area.height);
    app.size_notice_visible = notice;
    let [
        chooser_row,
        tabs_row,
        gap_row,
        address_row,
        commands_row,
        body,
        notice_row,
        status,
        hints_row,
    ] = Layout::vertical([
        Constraint::Length(chooser_h),
        Constraint::Length(tabs_h),
        Constraint::Length(gap_h),
        Constraint::Length(address_h),
        Constraint::Length(commands_h),
        Constraint::Min(3),
        Constraint::Length(u16::from(notice)),
        Constraint::Length(1),
        Constraint::Length(hints_h),
    ])
    .areas(area);

    if chooser_h > 0 {
        draw_chooser_bar(f, app, chooser_row);
    }
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
    // The cards of the notifications sit in the bottom right of the list. They never cover
    // its text: when the list is full it is made shorter for as long as they are there.
    let now = app.notice_now();
    let notice_h = if app.modal.is_none() {
        notice::stack_height(app, now, main.width as usize)
    } else {
        0
    };
    let notice_area = main;
    let main = if notice_h > 0 && list_needs_room(app, main, notice_h) {
        Rect {
            height: main.height.saturating_sub(notice_h),
            ..main
        }
    } else {
        main
    };
    match app.view {
        ViewMode::Columns => columns::draw_columns(f, app, main),
        ViewMode::Details => list::draw_list(
            f,
            app,
            main,
            list::ListOpts::for_terminal(area.width as usize, app.tags_column_wanted()),
        ),
        ViewMode::Icons => grid::draw_grid(f, app, main),
    }
    // A shorter list may have left the cursor below its last row: bring it back in view.
    if notice_h > 0 {
        let before = app.scroll;
        app.ensure_visible();
        if app.scroll != before {
            app.dirty = true;
        }
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
    bottom::draw_status(f, app, status);
    if hints_h > 0 {
        bottom::draw_hints(f, app, hints_row);
    }
    // Notifications sit at the bottom right of the list; a window in front hides them.
    if app.modal.is_none() {
        notice::draw(f, app, notice_area, now);
    }
    finish(f, app, area, (address_h > 0).then_some(address_row));
}

/// What is drawn over everything else, in either layout: the suggestions under the bar and the
/// window in front.
fn finish(f: &mut Frame, app: &mut App, area: Rect, address_row: Option<Rect>) {
    let th = app.th.clone();
    if let (Some(row), true) = (address_row, app.address.is_some()) {
        address::draw_suggestions(f, app, row, area);
    }
    if app.modal.is_some() {
        // Only the window is clickable while it is open.
        app.hits.clear();
        // A window dims what is behind it; a menu does not, nor the list of programs that sits
        // beside one.
        if modals::dims_backdrop(app) {
            dim_backdrop(f.buffer_mut(), area, &th);
        }
        modals::draw_modal(f, app, area);
    }
}

/// The places popups are drawn at, for what lies over cells and cannot be drawn under them (a
/// picture of the terminal). A window dims the whole screen: all of it.
fn popup_rects(app: &App, area: Rect) -> Vec<Rect> {
    let ctx = popup::Ctx::of(app);
    let room = modals::popup_room(app, area);
    match &app.modal {
        None | Some(Modal::FullPreview) => Vec::new(),
        Some(Modal::Menu(m)) => menu::rects(&ctx, &app.keymap, m, room),
        Some(Modal::OpenWith(v)) => {
            let mut rects = Vec::new();
            let mut anchor = None;
            if let Some(m) = v.under.as_deref() {
                rects.extend(menu::rects(&ctx, &app.keymap, m, room));
                anchor = Some(menu::beside(&ctx, &app.keymap, m, room));
            }
            let at = if anchor.is_some() { room } else { area };
            rects.push(openwith::rect(&ctx, v, at, anchor));
            rects
        }
        Some(_) => vec![area],
    }
}

/// Whether the list has no room to spare for the cards: its rows reach into them. The other
/// views give them the room at once.
fn list_needs_room(app: &App, main: Rect, cards_h: u16) -> bool {
    match app.view {
        ViewMode::Details => {
            // The column titles, a row of air and the rows.
            let rows = 2 + app.visible.len();
            rows + cards_h as usize > main.height as usize
        }
        _ => true,
    }
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
