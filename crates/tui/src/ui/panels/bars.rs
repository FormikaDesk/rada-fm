//! The bars of the panels view that are not the ones of the classic layout: the status bar
//! (the number of items as a pill, what is selected, what waits on the clipboard, and on the
//! right the version and the free space) and what goes on the top edge of the folder's box.

use rada_core::git::GitStatus;
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

/// The branch with its icon, and how far it is from its upstream: `⎇ main ↑2 ↓1`.
pub fn branch_text(app: &App, git: &GitStatus) -> String {
    let icon = icons::ui(app.settings.icons, Glyph::Branch);
    let mut s = String::new();
    if !icon.is_empty() {
        s.push_str(icon);
        s.push(' ');
    }
    s.push_str(&git.branch);
    if git.ahead > 0 {
        s.push_str(&format!(" ↑{}", git.ahead));
    }
    if git.behind > 0 {
        s.push_str(&format!(" ↓{}", git.behind));
    }
    s
}

/// What goes on the right of the top edge of the folder's box: when the folder is in a git
/// repository, its branch with an icon and the number of changed files (●) and new ones (?).
pub fn folder_edge(app: &App) -> Edge {
    let Some(git) = app.git_here() else {
        return Edge::NONE;
    };
    let th = &app.settings.th;
    let mut right = vec![Span::styled(
        branch_text(app, git),
        th.fg(th.hues.pink).add_modifier(Modifier::BOLD),
    )];
    if git.modified > 0 {
        right.push(Span::styled(
            format!(" ●{}", git.modified),
            th.fg(th.warn).add_modifier(Modifier::BOLD),
        ));
    }
    if git.new > 0 {
        right.push(Span::styled(
            format!(" ?{}", git.new),
            th.fg(th.success).add_modifier(Modifier::BOLD),
        ));
    }
    Edge { right }
}

pub fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let band = th.band();
    let look = Look::of(app);
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;
    let sep = || Span::styled("  │  ", th.faint().patch(band));

    // On the right: the version (a click asks for a newer one) and the free space.
    let mut right = Bar::default();
    if w >= 70 {
        right.push_hit(
            Span::styled(
                format!(
                    "v{} {}",
                    app.version(),
                    icons::ui(app.settings.icons, Glyph::Refresh)
                ),
                th.dim().patch(band),
            ),
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
    if let Some(name) = app.nav_tag_name() {
        left.push(Span::styled(
            name,
            th.base().add_modifier(Modifier::BOLD).patch(band),
        ));
        left.push(sep());
    }
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
    if !app.tab.sel.marked.is_empty() {
        let size: u64 = app
            .tab
            .listing
            .all()
            .iter()
            .filter(|e| app.tab.sel.marked.contains(&e.name) && !e.is_dir())
            .map(|e| e.size)
            .sum();
        let tick = icons::ui(app.settings.icons, Glyph::Checked);
        left.push(sep());
        left.push(Span::styled(
            format!(
                "{tick} {}",
                if app.tab.sel.marked.len() == 1 {
                    tf(Key::SelectedSizeOne, &[&fmt::size(size)])
                } else {
                    tf(
                        Key::SelectedSizeMany,
                        &[&app.tab.sel.marked.len(), &fmt::size(size)],
                    )
                }
            ),
            th.fg(th.check).add_modifier(Modifier::BOLD).patch(band),
        ));
    }
    if let Some(c) = &app.ops.clipboard {
        let icon = icons::ui(app.settings.icons, Glyph::Clipboard);
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
    if let Some(git) = app.git_here() {
        left.push(sep());
        left.push(Span::styled(
            branch_text(app, git),
            th.fg(th.hues.pink).patch(band),
        ));
    }
    if app.settings.show_hidden {
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
    left.render(f, &mut app.frame.hits, area);
}
