//! The command bar: New, Cut, Copy, Paste, Rename, Compress, Delete, then Sort and View,
//! and Undo with a few words about what it would undo.
//!
//! Every button runs the action the keyboard runs. One that cannot be used right now is
//! drawn faint and is not clickable. On a narrow terminal the words go and only the icons
//! stay; before that, the description after Undo goes first.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthStr;

use super::widgets::Bar;
use crate::app::{App, MenuKind};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// How much of each button is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Detail {
    /// Icons, words and the description after Undo.
    Full,
    /// Icons and words.
    Words,
    /// Icons only.
    Icons,
}

fn build<'a>(app: &App, detail: Detail, reduced: bool) -> Bar<'a> {
    let th = &app.th;
    let band = th.band();
    let set = app.icons;
    let ro = app.archive.is_some();
    let has_target = !app.marked.is_empty() || app.current().is_some();
    let mut b = Bar::default();

    // New ▾: the one filled button.
    let new_icon = icons::ui(set, Glyph::New);
    let new_text = match detail {
        Detail::Icons => format!(" {new_icon}▾ "),
        _ => format!(" {new_icon} New ▾ "),
    };
    if ro {
        b.push(Span::styled(new_text, th.faint().patch(band)));
    } else {
        b.push_hit(
            Span::styled(new_text, th.pill()),
            Target::Menu(MenuKind::New),
        );
    }
    b.push(Span::styled("│", th.faint().patch(band)));

    let plain = Style::default().bg(th.mark);
    let button = |b: &mut Bar<'a>, glyph: Glyph, word: &str, enabled: bool, target: Target| {
        let icon = icons::ui(set, glyph);
        let mut text = String::from(" ");
        if !icon.is_empty() {
            text.push_str(icon);
            text.push(' ');
        }
        if detail != Detail::Icons || icon.is_empty() {
            text.push_str(word);
        }
        // Room between buttons, when there is room to spare.
        text.push(' ');
        if detail == Detail::Full {
            text.push(' ');
        }
        if enabled {
            b.push_hit(Span::styled(text, th.base().patch(plain)), target);
        } else {
            b.push(Span::styled(text, th.faint().patch(plain)));
        }
    };

    if !reduced {
        button(
            &mut b,
            Glyph::Cut,
            "Cut",
            has_target && !ro,
            Target::Act(Action::Cut),
        );
    }
    button(
        &mut b,
        Glyph::Copy,
        "Copy",
        has_target,
        Target::Act(Action::Copy),
    );
    button(
        &mut b,
        Glyph::Paste,
        "Paste",
        app.clipboard.is_some() && !ro,
        Target::Act(Action::Paste),
    );
    if !reduced {
        button(
            &mut b,
            Glyph::Rename,
            "Rename",
            app.current().is_some() && !ro,
            Target::Act(Action::Rename),
        );
        button(
            &mut b,
            Glyph::Compress,
            "Compress",
            has_target && !ro,
            Target::Act(Action::Compress),
        );
    }
    button(
        &mut b,
        Glyph::Delete,
        "Delete",
        has_target && !ro,
        Target::Act(Action::Trash),
    );
    b.push(Span::styled(" ", band));
    b.push(Span::styled("│", th.faint().patch(band)));
    if !reduced {
        button(
            &mut b,
            Glyph::Sort,
            "Sort ▾",
            true,
            Target::Menu(MenuKind::Sort),
        );
        button(
            &mut b,
            Glyph::View,
            "View ▾",
            true,
            Target::Menu(MenuKind::View),
        );
        b.push(Span::styled(" ", band));
        b.push(Span::styled("│", th.faint().patch(band)));
    }
    button(
        &mut b,
        Glyph::Undo,
        "Undo",
        app.undo_label.is_some(),
        Target::Act(Action::Undo),
    );
    if detail == Detail::Full
        && let Some(label) = &app.undo_label
    {
        b.push(Span::styled(
            format!("· {label} "),
            th.dim().add_modifier(Modifier::empty()).patch(band),
        ));
    }
    b
}

pub fn draw_commands(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;
    let more = icons::ui(app.icons, Glyph::More);
    let more_w = more.width() + 1;

    // The fullest version that fits, with room kept for the "more" button.
    let wanted = if w >= 100 {
        Detail::Full
    } else {
        Detail::Icons
    };
    let order = [Detail::Full, Detail::Words, Detail::Icons];
    let start = order.iter().position(|d| *d == wanted).unwrap_or(0);
    let mut chosen = None;
    for d in &order[start..] {
        if build(app, *d, false).width() + more_w <= w {
            chosen = Some(*d);
            break;
        }
    }
    // Not even the icons fit: the buttons used most, and the rest by keyboard or by menu.
    let mut bar = match chosen {
        Some(d) => build(app, d, false),
        None => build(app, Detail::Icons, true),
    };
    let gap = w.saturating_sub(bar.width() + more_w);
    bar.push(Span::styled(" ".repeat(gap), band));
    bar.push_hit(
        Span::styled(format!(" {more}"), th.base().patch(band)),
        Target::Menu(MenuKind::More),
    );
    bar.render(f, &mut app.hits, area);
}
