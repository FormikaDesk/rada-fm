//! The top row: the name of the program, one tab for each open folder with its close mark,
//! the new-tab button, and on the right the notice of a newer version and the help button.
//!
//! The tab in front is a pill; the others are plain text. On a narrow terminal the tabs fold
//! into `‹ 2/3 ›` and the name of the one in front.

use rada_core::display;
use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Span;
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look};
use super::widgets::Bar;
use crate::app::{App, MenuKind};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::{Action, Preset};

/// The key scheme as the right of the row says it (only on a very wide terminal).
pub fn scheme_label(preset: Preset) -> &'static str {
    match preset {
        Preset::VimClassic => "vim + classic keys",
        Preset::Vim => "vim keys",
        Preset::Classic => "classic keys",
    }
}

pub fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let look = Look::of(app);
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;
    let set = app.icons;
    let n = app.tab_count();

    let mut right = Bar::default();
    // A newer version waits: a pill in the accent colour, a click (or the key) opens the notes.
    if w >= 60
        && let crate::app::UpdateStatus::Available { notice, .. } = &app.update
    {
        let up = icons::ui(set, Glyph::Up);
        let text = format!("{up} {}", tf(Key::UpdateTo, &[&notice.version]));
        right.push_hits(
            soft::pill(&th, look, text.trim(), th.accent, th.pill(), band),
            Target::Act(Action::Update),
        );
        right.push(Span::styled(" ", band));
    }
    if w >= 200 {
        right.push(Span::styled(
            format!("{}  ", scheme_label(app.keymap.preset)),
            th.dim().patch(band),
        ));
    }
    if w >= 40 {
        let mark = icons::ui(set, Glyph::Help);
        let label = if mark.is_empty() {
            Key::HelpMenu.tr().to_string()
        } else {
            format!("{mark} {}", Key::HelpMenu.tr())
        };
        right.push_hits(
            soft::pill(
                &th,
                look,
                &label,
                th.button,
                th.accent_style().add_modifier(Modifier::BOLD),
                band,
            ),
            Target::Menu(MenuKind::Help),
        );
        right.push(Span::styled(" ", band));
    }

    let mut left = Bar::default();
    if w >= 30 {
        left.push(Span::styled(
            " rada ",
            th.accent_style().add_modifier(Modifier::BOLD).patch(band),
        ));
    }

    let budget = w.saturating_sub(left.width() + right.width());
    let max_title = if w >= 140 { 22 } else { 16 };
    let folder = icons::ui(set, Glyph::TabFolder);
    let close = icons::ui(set, Glyph::Close);
    let title_of = |app: &App, i: usize| -> String {
        let t = display::truncate(&app.tab_title(i), max_title);
        if folder.is_empty() {
            t
        } else {
            format!("{folder} {t}")
        }
    };

    // The full row, if it fits.
    let mut tabs = Bar::default();
    for i in 0..n {
        let active = i == app.active_tab;
        let title = title_of(app, i);
        if active {
            // A pill: the half circle, the name, the close mark, the half circle.
            let style = th.base().add_modifier(Modifier::BOLD);
            let fill = th.selection;
            let ends = look.caps;
            let edge = band.fg(fill);
            if ends {
                tabs.push_hit(Span::styled(soft::CAP_LEFT, edge), Target::Tab(i));
            }
            tabs.push_hit(
                Span::styled(
                    format!("{}{title} ", if ends { "" } else { " " }),
                    style.bg(fill),
                ),
                Target::Tab(i),
            );
            let mark = if close.is_empty() { " " } else { close };
            tabs.push_hit(
                Span::styled(
                    format!(" {mark}{}", if ends { "" } else { " " }),
                    th.dim().bg(fill),
                ),
                Target::TabClose(i),
            );
            if ends {
                tabs.push_hit(Span::styled(soft::CAP_RIGHT, edge), Target::TabClose(i));
            }
        } else {
            tabs.push_hit(
                Span::styled(format!(" {title} "), th.dim().patch(band)),
                Target::Tab(i),
            );
            let close_label = if close.is_empty() {
                " ".to_string()
            } else {
                format!(" {close} ")
            };
            tabs.push_hit(
                Span::styled(close_label, th.faint().patch(band)),
                Target::TabClose(i),
            );
        }
        tabs.push(Span::styled(" ", band));
    }
    let plus = icons::ui(set, Glyph::New);
    tabs.push_hits(
        soft::pill(&th, look, plus, th.button, th.dim(), band),
        Target::TabNew,
    );

    let mut bar = left;
    if w >= 60 && tabs.width() <= budget {
        bar.append(tabs);
    } else {
        // Folded: ‹ 2/3 › and the tab in front, then +.
        let count = format!(" {}/{} ", app.active_tab + 1, n);
        let arrows = 4 + count.width();
        bar.push_hit(Span::styled(" ‹", th.dim().patch(band)), Target::TabPrev);
        bar.push(Span::styled(count, th.base().patch(band)));
        bar.push_hit(Span::styled("› ", th.dim().patch(band)), Target::TabNext);
        let room = budget.saturating_sub(arrows + 5);
        if room >= 6 {
            let t = display::truncate(&app.tab_title(app.active_tab), room.min(max_title));
            bar.push_hits(
                soft::pill(
                    &th,
                    look,
                    &t,
                    th.selection,
                    th.base().add_modifier(Modifier::BOLD),
                    band,
                ),
                Target::Tab(app.active_tab),
            );
        }
        if budget > arrows + 4 {
            bar.push_hits(
                soft::pill(&th, look, plus, th.button, th.dim(), band),
                Target::TabNew,
            );
        }
    }
    let gap = w.saturating_sub(bar.width() + right.width());
    bar.push(Span::styled(" ".repeat(gap), band));
    bar.append(right);
    bar.render(f, &mut app.hits, area);
}
