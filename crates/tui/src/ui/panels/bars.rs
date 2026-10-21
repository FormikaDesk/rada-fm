//! The bars of the panels view that are not the ones of the classic layout: the status bar
//! (the number of items as a pill, what is selected, what waits on the clipboard, and on the
//! right the version and the free space) and what goes on the top edge of the folder's box.

use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Span;
use ratatui::widgets::Block;

use super::frame::Edge;
use crate::app::App;
use crate::fmt;
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::ui::bottom;
use crate::ui::soft::{self, Look};
use crate::ui::widgets::Bar;

/// What goes on the right of the top edge of the folder's box.
pub fn folder_edge(_app: &App) -> Edge {
    Edge::NONE
}

pub fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let band = th.band();
    let look = Look::of(app);
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;
    let sep = || Span::styled("  │  ", th.faint().patch(band));

    // On the right: the version (a click asks for a newer one) and the free space.
    let mut right = Bar::default();
    if w >= 70 {
        right.push_hit(
            Span::styled(format!("v{} ⟳", app.version()), th.dim().patch(band)),
            Target::CheckUpdates,
        );
        right.push(Span::styled("  ", band));
    }
    right.append(bottom::free_space(app, false));
    if right.width() > w.saturating_sub(12) {
        right = Bar::default();
    }

    let mut left = Bar::default();
    left.push(Span::styled(" ", band));
    left.push_hits(
        soft::pill(
            &th,
            look,
            &bottom::count_text(app),
            th.accent,
            th.pill(),
            band,
        ),
        Target::Act(crate::keymap::Action::SelectAll),
    );
    if !app.marked.is_empty() {
        let size: u64 = app
            .listing
            .all()
            .iter()
            .filter(|e| app.marked.contains(&e.name) && !e.is_dir())
            .map(|e| e.size)
            .sum();
        let tick = icons::ui(app.icons, Glyph::Checked);
        left.push(sep());
        left.push(Span::styled(
            format!(
                "{tick} {}",
                if app.marked.len() == 1 {
                    tf(Key::SelectedSizeOne, &[&fmt::size(size)])
                } else {
                    tf(
                        Key::SelectedSizeMany,
                        &[&app.marked.len(), &fmt::size(size)],
                    )
                }
            ),
            th.fg(th.check).add_modifier(Modifier::BOLD).patch(band),
        ));
    }
    if let Some(c) = &app.clipboard {
        let icon = icons::ui(app.icons, Glyph::Clipboard);
        let verb = if c.mode == rada_core::ops::TransferMode::Copy {
            Key::ReadyPaste
        } else {
            Key::ReadyMove
        };
        left.push(sep());
        left.push(Span::styled(
            format!(
                "{icon} {}",
                tf(
                    verb,
                    &[&fmt::count(
                        c.paths.len() as u64,
                        Key::ItemOne,
                        Key::ItemMany
                    )]
                )
            ),
            th.fg(th.hues.cyan).patch(band),
        ));
    }
    if app.show_hidden {
        left.push(sep());
        left.push(Span::styled(
            Key::HiddenShown.tr(),
            th.fg(th.warn).patch(band),
        ));
    }
    let mut room = w.saturating_sub(right.width() + 1);
    // A newer version keeps its place, in the longest wording that leaves the count room.
    if let Some(notice) = bottom::update_notice_bar(app, room, left.width()) {
        room = room.saturating_sub(notice.width());
        left.clip(room);
        left.append(notice);
    } else {
        left.clip(room);
    }
    let gap = w.saturating_sub(left.width() + right.width());
    left.push(Span::styled(" ".repeat(gap), band));
    left.append(right);
    left.render(f, &mut app.hits, area);
}
