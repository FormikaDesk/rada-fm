//! The command bar: New, Cut, Copy, Paste, Rename, Compress, Delete, then Sort and Tag, and
//! Undo with a few words about what it would undo.
//!
//! The bar has a background of its own and, when there is height to spare, a row of ▄ above
//! and ▀ below in the bar's colour: a vertical padding made of half blocks. Every button is
//! a pill; New, the one that makes something, is filled with the accent colour.
//!
//! Every button runs the action the keyboard runs. One that cannot be used right now is
//! drawn faint and is not clickable. When the terminal narrows the bar gives up room in
//! steps: first the description after Undo, then the generous spacing, then the words.

use rada_core::i18n::Key;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look};
use super::widgets::Bar;
use crate::app::{App, MenuKind};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// How much of each button is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    /// Icon and word, a space of air inside each pill and between pills.
    Roomy,
    /// The same, tighter.
    Words,
    /// Words only, the icons given up (Italian words are longer).
    Text,
    /// Icons only.
    Icons,
}

impl Level {
    fn between(self) -> &'static str {
        match self {
            Level::Roomy => "  ",
            Level::Words | Level::Text | Level::Icons => " ",
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
    let look = Look::of(app);
    let bar = Style::default().bg(th.cmdbar);
    let set = app.icons;
    let ro = app.archive.is_some();
    let nowhere = ro || app.tag_view().is_some();
    let has_target = !app.marked.is_empty() || app.current().is_some();
    let mut b = Bar::default();
    b.push(Span::styled(" ", bar));

    // New ▾: the one filled button, its text in the colour made for the accent.
    let new_icon = icons::ui(set, Glyph::New);
    let new_text = match level {
        Level::Icons => format!("{new_icon}▾"),
        Level::Text => format!("{} ▾", Key::MenuNew.tr()),
        _ => format!("{new_icon} {} ▾", Key::MenuNew.tr()),
    };
    if nowhere {
        b.push_hits(
            soft::pill(th, look, &new_text, th.button, th.faint(), bar),
            Target::Window,
        );
    } else {
        b.push_hits(
            soft::pill(th, look, &new_text, th.accent, th.pill(), bar),
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
            (_, true) | (Level::Text, _) => word.to_string(),
            _ => format!("{icon} {word}"),
        };
        if enabled {
            b.push_hits(
                soft::pill(th, look, &text, th.button, th.base(), bar),
                target,
            );
        } else {
            b.push_hits(
                soft::pill(th, look, &text, th.button, th.faint(), bar),
                Target::Window,
            );
        }
    };

    if !reduced {
        button(
            &mut b,
            &mut first,
            Glyph::Cut,
            Key::LCut.tr(),
            has_target && !ro,
            Target::Act(Action::Cut),
        );
    }
    button(
        &mut b,
        &mut first,
        Glyph::Copy,
        Key::LCopy.tr(),
        has_target,
        Target::Act(Action::Copy),
    );
    button(
        &mut b,
        &mut first,
        Glyph::Paste,
        Key::LPaste.tr(),
        app.clipboard.is_some() && !nowhere,
        Target::Act(Action::Paste),
    );
    if !reduced {
        button(
            &mut b,
            &mut first,
            Glyph::Rename,
            Key::LRename.tr(),
            app.current().is_some() && !ro,
            Target::Act(Action::Rename),
        );
        button(
            &mut b,
            &mut first,
            Glyph::Compress,
            Key::BtnCompress.tr(),
            has_target && !ro,
            Target::Act(Action::Compress),
        );
    }
    button(
        &mut b,
        &mut first,
        Glyph::Delete,
        Key::BtnDelete.tr(),
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
            &format!("{} ▾", Key::BtnSort.tr()),
            true,
            Target::Menu(MenuKind::Sort),
        );
        if app.tags_available() {
            button(
                &mut b,
                &mut first,
                Glyph::Tag,
                &format!("{} ▾", Key::MenuTags.tr()),
                has_target,
                Target::Menu(MenuKind::Tags),
            );
        }
    }
    sep(&mut b);
    first = true;
    button(
        &mut b,
        &mut first,
        Glyph::Undo,
        Key::LUndo.tr(),
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
        (Level::Text, false),
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
