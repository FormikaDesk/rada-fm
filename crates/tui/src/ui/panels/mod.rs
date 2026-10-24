//! The panels view: the whole screen as rounded boxes.
//!
//! From the top: the row of tabs, the row of navigation (back, forward, up, refresh, the path
//! and the search), the row of buttons; then NAV and SPAZIO on the left, the folder in the middle
//! and ANTEPRIMA and DETTAGLI on the right; at the bottom the status bar and the key hints. Each
//! bar is a row of its own, with a blank row between them while the window has the height for it
//! ([`crate::panels::AIRY_FROM`], and in part [`crate::panels::SOME_AIR_FROM`]).
//!
//! Colours come from the theme only. A box's border is the accent colour while the keyboard is
//! in it and a quiet one otherwise; its inside is the theme's `inner` tone.

mod bars;
mod files;
mod frame;
mod nav;
mod side;
mod space;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};

use super::{address, bottom, columns, commands, grid, notice, tabs};
use crate::app::App;
use crate::panels::{AIRY_FROM, SOME_AIR_FROM};
use crate::view::ViewMode;

/// Width of the left column and of the right one for a window `width` columns wide.
fn side_widths(width: u16) -> (u16, u16) {
    let left = if width >= 150 { 30 } else { 28 };
    let right = (width * 27 / 100).clamp(36, 48);
    (left, right)
}

/// Rows the main disk of SPAZIO may take before the box is made shorter: NAV keeps at least
/// this many rows for itself.
const NAV_MIN_ROWS: u16 = 10;

/// Draw the whole screen. Returns the row of the bar at the top, which the suggestions of the
/// command bar hang from.
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) -> Rect {
    let th = app.th.clone();
    let [gap1, gap2, gap3, gap4, gap5] = bar_gaps(area.height);
    let hints_h = u16::from(app.show_hints);
    let [
        tabs_row,
        _,
        nav_row,
        _,
        buttons_row,
        _,
        body,
        _,
        status_row,
        _,
        hints_row,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(gap1),
        Constraint::Length(1),
        Constraint::Length(gap2),
        Constraint::Length(1),
        Constraint::Length(gap3),
        Constraint::Min(8),
        Constraint::Length(gap4),
        Constraint::Length(1),
        Constraint::Length(gap5 * hints_h),
        Constraint::Length(hints_h),
    ])
    .areas(area);

    tabs::draw_tabs(f, app, tabs_row);
    address::draw_address(f, app, nav_row);
    commands::draw_commands(f, app, buttons_row);

    // The columns.
    let two = app.panels_two;
    let (left_w, right_w) = if two {
        let (min, max) = crate::panels::TWO_LEFT_WIDTH;
        (
            (area.width * crate::panels::TWO_LEFT_PERCENT / 100).clamp(min, max),
            0,
        )
    } else {
        side_widths(area.width)
    };
    // Ctrl+B takes the left column away.
    let (left_w, left_gap) = if app.side_on { (left_w, 1) } else { (0, 0) };
    let right_gap = u16::from(!two);
    let [left, _, middle, _, right] = Layout::horizontal([
        Constraint::Length(left_w),
        Constraint::Length(left_gap),
        Constraint::Min(30),
        Constraint::Length(right_gap),
        Constraint::Length(right_w),
    ])
    .areas(body);

    app.want_tree();
    app.want_git();
    if left.width > 0 {
        if two && app.left_preview {
            draw_right(f, app, left);
        } else {
            draw_left(f, app, left);
        }
    } else {
        app.side_focus = false;
        app.preview.focus = false;
    }
    if two && !app.left_preview {
        app.preview.focus = false;
    }
    if two && app.left_preview {
        app.side_focus = false;
    }
    let main = draw_middle(f, app, middle);
    if !two {
        draw_right(f, app, right);
    }

    bars::draw_status(f, app, status_row);
    if hints_h > 0 {
        bottom::draw_hints(f, app, hints_row);
    }
    // Notifications are a card at the bottom of the folder's box.
    let _ = th;
    if app.modal.is_none() {
        let now = app.notice_now();
        notice::draw(f, app, main, now);
    }
    nav_row
}

/// The blank rows between the bars, in the order of the screen: under the tabs, under the
/// navigation row, under the buttons, above the status bar, under it. All of them from
/// [`AIRY_FROM`] rows; then only those under the buttons and under the status bar.
fn bar_gaps(height: u16) -> [u16; 5] {
    match height {
        h if h >= AIRY_FROM => [1; 5],
        h if h >= SOME_AIR_FROM => [0, 0, 1, 0, 1],
        _ => [0; 5],
    }
}

/// Whether the window is tall enough for the blank row under the title of a box.
fn is_airy(f: &Frame) -> bool {
    f.area().height >= AIRY_FROM
}

/// NAV above, SPAZIO below.
fn draw_left(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let airy = is_airy(f);
    // The two columns have SPAZIO only in a tall window.
    let with_space = !app.panels_two || f.area().height >= crate::panels::TWO_SPACE_FROM_ROWS;
    let others = space::volumes(app)
        .filter(|_| with_space)
        .map(|(_, o)| o.len());
    let (nav_area, space_area) = match others {
        None => (area, None),
        Some(others) => {
            // As much of SPAZIO as leaves NAV its rows: the full box, then without the used
            // and free lines of the main disk.
            let ring = space::ring_colors(app).is_some();
            let want =
                |compact, ring| space::rows_needed(others, compact, ring) + 2 + u16::from(airy);
            let mut sizes = vec![want(false, false), want(true, false)];
            if ring {
                sizes.insert(0, want(false, true));
            }
            let h = sizes
                .into_iter()
                .find(|h| area.height.saturating_sub(*h) >= NAV_MIN_ROWS)
                .unwrap_or(want(true, false).min(area.height / 2));
            let [top, bottom] =
                Layout::vertical([Constraint::Min(3), Constraint::Length(h)]).areas(area);
            (top, Some(bottom))
        }
    };
    let focused = app.side_focus;
    let inner = frame::draw(
        f,
        nav_area,
        &th,
        rada_core::i18n::Key::PanelNav.tr(),
        &frame::Edge::NONE,
        focused,
    );
    nav::draw(f, app, frame::pad(inner, airy));
    frame::settle(f.buffer_mut(), inner, &th);
    if let Some(a) = space_area {
        let inner = frame::draw(
            f,
            a,
            &th,
            rada_core::i18n::Key::PanelSpace.tr(),
            &frame::Edge::NONE,
            false,
        );
        space::draw(f, app, frame::pad(inner, airy));
        frame::settle(f.buffer_mut(), inner, &th);
    }
}

/// The folder: its path on the top edge and the list inside. Returns the inside, where the
/// notification cards are drawn.
fn draw_middle(f: &mut Frame, app: &mut App, area: Rect) -> Rect {
    let th = app.th.clone();
    let airy = is_airy(f);
    let title = match app.view_title() {
        Some(t) => t,
        None => crate::fmt::short_path(&app.cwd, app.home()),
    };
    let edge = bars::folder_edge(app);
    let inner = frame::draw(
        f,
        area,
        &th,
        &title,
        &edge,
        !app.side_focus && !app.preview.focus,
    );
    // The cards never cover the text: the list is made shorter while they are there.
    let now = app.notice_now();
    let notice_h = if app.modal.is_none() {
        notice::stack_height(app, now, inner.width as usize)
    } else {
        0
    };
    let padded = frame::pad(inner, airy);
    let list = if notice_h > 0 && list_needs_room(app, padded, notice_h) {
        Rect {
            height: padded.height.saturating_sub(notice_h),
            ..padded
        }
    } else {
        padded
    };
    match app.view {
        ViewMode::Details => files::draw(f, app, list),
        ViewMode::Columns => columns::draw_columns(f, app, list),
        ViewMode::Icons => grid::draw_grid(f, app, list),
    }
    if notice_h > 0 {
        let before = app.scroll;
        app.ensure_visible();
        if app.scroll != before {
            app.redraw.dirty = true;
        }
    }
    frame::settle(f.buffer_mut(), inner, &th);
    inner
}

/// Whether the list has no room to spare for the cards: its rows reach into them.
fn list_needs_room(app: &App, inner: Rect, cards_h: u16) -> bool {
    match app.view {
        ViewMode::Details => {
            // The titles, a blank row and the rows.
            let rows = 2 + app.visible.len();
            rows + cards_h as usize > inner.height as usize
        }
        _ => true,
    }
}

/// ANTEPRIMA above, DETTAGLI (or whatever is in its place) below.
fn draw_right(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let airy = is_airy(f);
    // The lower box takes what its rows need, but ANTEPRIMA keeps most of the column.
    let want = side::lower_rows(app) + 2 + u16::from(airy);
    let h = want
        .min(area.height.saturating_sub(12))
        .max(3.min(area.height));
    let [top, bottom] = Layout::vertical([Constraint::Min(8), Constraint::Length(h)]).areas(area);
    let inner = frame::draw(
        f,
        top,
        &th,
        rada_core::i18n::Key::PanelPreview.tr(),
        &frame::Edge::NONE,
        app.preview.focus,
    );
    side::draw_preview(f, app, frame::pad(inner, airy));
    frame::settle(f.buffer_mut(), inner, &th);
    if bottom.height >= 3 {
        let inner = frame::draw(
            f,
            bottom,
            &th,
            side::lower_title(),
            &frame::Edge::NONE,
            false,
        );
        side::draw_lower(f, app, frame::pad(inner, airy));
        frame::settle(f.buffer_mut(), inner, &th);
    }
}
