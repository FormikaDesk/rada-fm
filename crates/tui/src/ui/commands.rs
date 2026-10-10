//! The command bar: New, Cut, Copy, Paste, Rename, Compress, Delete, then Sort and View,
//! and Undo with a few words about what it would undo.
//!
//! The bar has a background of its own and, when there is height to spare, a row of ▄ above
//! and ▀ below in the bar's colour: a vertical padding made of half blocks.
//!
//! Every button runs the action the keyboard runs. One that cannot be used right now is
//! drawn faint and is not clickable. When the terminal narrows the bar gives up room in
//! steps: first the description after Undo, then the generous spacing, then the words.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::widgets::Bar;
use crate::app::{App, MenuKind};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// How much of each button is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    /// Two spaces between icon and label, five between buttons.
    Roomy,
    /// One space between icon and label, two between buttons.
    Words,
    /// Icons only.
    Icons,
}

impl Level {
    fn inner(self) -> &'static str {
        match self {
            Level::Roomy => "  ",
            _ => " ",
        }
    }

    fn between(self) -> &'static str {
        match self {
            Level::Roomy => "     ",
            Level::Words => "  ",
            Level::Icons => "   ",
        }
    }

    /// Space on each side of the separators between groups.
    fn around_sep(self) -> &'static str {
        match self {
            Level::Roomy => "  ",
            _ => " ",
        }
    }
}

fn build<'a>(app: &App, level: Level, with_desc: bool, reduced: bool) -> Bar<'a> {
    let th = &app.th;
    let bar = Style::default().bg(th.cmdbar);
    let set = app.icons;
    let ro = app.archive.is_some();
    let has_target = !app.marked.is_empty() || app.current().is_some();
    let mut b = Bar::default();
    b.push(Span::styled(" ", bar));

    // New ▾: the one filled button, its text in the colour made for the accent.
    let new_icon = icons::ui(set, Glyph::New);
    let new_text = match level {
        Level::Icons => format!(" {new_icon}▾ "),
        _ => format!(" {new_icon}{}New ▾ ", level.inner()),
    };
    if ro {
        b.push(Span::styled(new_text, th.faint().patch(bar)));
    } else {
        b.push_hit(
            Span::styled(new_text, th.pill()),
            Target::Menu(MenuKind::New),
        );
    }
    let sep = |b: &mut Bar<'a>| {
        b.push(Span::styled(level.around_sep(), bar));
        b.push(Span::styled("│", th.faint().patch(bar)));
        b.push(Span::styled(level.around_sep(), bar));
    };
    sep(&mut b);

    let mut first = true;
    let button = |b: &mut Bar<'a>,
                  first: &mut bool,
                  glyph: Glyph,
                  word: &str,
                  enabled: bool,
                  target: Target| {
        if !*first {
            b.push(Span::styled(level.between(), bar));
        }
        *first = false;
        let icon = icons::ui(set, glyph);
        let text = match (level, icon.is_empty()) {
            (Level::Icons, false) => icon.to_string(),
            (_, true) => word.to_string(),
            _ => format!("{icon}{}{word}", level.inner()),
        };
        if enabled {
            b.push_hit(Span::styled(text, th.base().patch(bar)), target);
        } else {
            b.push(Span::styled(text, th.faint().patch(bar)));
        }
    };

    if !reduced {
        button(
            &mut b,
            &mut first,
            Glyph::Cut,
            "Cut",
            has_target && !ro,
            Target::Act(Action::Cut),
        );
    }
    button(
        &mut b,
        &mut first,
        Glyph::Copy,
        "Copy",
        has_target,
        Target::Act(Action::Copy),
    );
    button(
        &mut b,
        &mut first,
        Glyph::Paste,
        "Paste",
        app.clipboard.is_some() && !ro,
        Target::Act(Action::Paste),
    );
    if !reduced {
        button(
            &mut b,
            &mut first,
            Glyph::Rename,
            "Rename",
            app.current().is_some() && !ro,
            Target::Act(Action::Rename),
        );
        button(
            &mut b,
            &mut first,
            Glyph::Compress,
            "Compress",
            has_target && !ro,
            Target::Act(Action::Compress),
        );
    }
    button(
        &mut b,
        &mut first,
        Glyph::Delete,
        "Delete",
        has_target && !ro,
        Target::Act(Action::Trash),
    );
    if !reduced {
        sep(&mut b);
        first = true;
        button(
            &mut b,
            &mut first,
            Glyph::Sort,
            "Sort ▾",
            true,
            Target::Menu(MenuKind::Sort),
        );
        button(
            &mut b,
            &mut first,
            Glyph::View,
            "View ▾",
            true,
            Target::Menu(MenuKind::View),
        );
    }
    sep(&mut b);
    first = true;
    button(
        &mut b,
        &mut first,
        Glyph::Undo,
        "Undo",
        app.undo_label.is_some(),
        Target::Act(Action::Undo),
    );
    if with_desc && let Some(label) = &app.undo_label {
        b.push(Span::styled(format!("  · {label}"), th.dim().patch(bar)));
    }
    b
}

pub fn draw_commands(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let bar = Style::default().bg(th.cmdbar);
    let w = area.width as usize;
    // With three rows the outer two are half blocks in the bar's colour.
    let inner = if area.height >= 3 {
        let edge = |glyph: &str| {
            Paragraph::new(Line::from(Span::styled(
                glyph.repeat(w),
                Style::default().fg(th.cmdbar),
            )))
        };
        f.render_widget(edge("▄"), Rect { height: 1, ..area });
        f.render_widget(
            edge("▀"),
            Rect {
                y: area.y + area.height - 1,
                height: 1,
                ..area
            },
        );
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        }
    } else {
        area
    };
    f.render_widget(Block::default().style(bar), inner);

    let more = icons::ui(app.icons, Glyph::More);
    let more_w = more.width() + 1;
    // The fullest version that fits, with room kept for the "more" button.
    let tries = [
        (Level::Roomy, true),
        (Level::Roomy, false),
        (Level::Words, true),
        (Level::Words, false),
        (Level::Icons, false),
    ];
    let mut chosen = None;
    for (level, desc) in tries {
        if build(app, level, desc, false).width() + more_w <= w {
            chosen = Some((level, desc));
            break;
        }
    }
    // Not even the icons fit: the buttons used most, and the rest by keyboard or by menu.
    let mut bar_line = match chosen {
        Some((level, desc)) => build(app, level, desc, false),
        None => build(app, Level::Icons, false, true),
    };
    let gap = w.saturating_sub(bar_line.width() + more_w);
    bar_line.push(Span::styled(" ".repeat(gap), bar));
    bar_line.push_hit(
        Span::styled(format!(" {more}"), th.base().patch(bar)),
        Target::Menu(MenuKind::More),
    );
    bar_line.render(f, &mut app.hits, inner);
}
