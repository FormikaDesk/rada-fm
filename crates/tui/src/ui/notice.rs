//! The notification card: a small soft card at the bottom right of the body, in the colour of
//! the panels, with half blocks (▄ above, ▀ below) for rounded top and bottom edges.
//!
//! First line: what happened, with a mark in the colour of its kind. Second line: the ways
//! out, `Undo  Ctrl+Z · Show`, clickable. An error is drawn in the colour of errors and has
//! `Close  Esc` instead. It never takes the keyboard.

use rada_core::i18n::Key;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, NoticeAction, NoticeKind};
use crate::fmt;
use crate::hits::{NoticeHit, Target};
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// The widest a card gets, and the room kept free at its sides.
const MAX_WIDTH: usize = 58;
const MARGIN: u16 = 2;

pub fn draw_notice(f: &mut Frame, app: &mut App, body: Rect) {
    let Some(n) = app.current_notice().cloned() else {
        return;
    };
    let th = app.th.clone();
    if body.width < 24 || body.height < 4 {
        return;
    }
    let (mark, tone) = match n.kind {
        NoticeKind::Ok => (icons::ui(app.icons, Glyph::Done), th.success),
        NoticeKind::Info => ("●", th.accent),
        NoticeKind::Warn => ("▲", th.warn),
        NoticeKind::Error => ("✕", th.error),
    };
    let mark = if mark.is_empty() { "*" } else { mark };
    let panel = if n.kind == NoticeKind::Error && th.tint(th.error) != ratatui::style::Color::Reset
    {
        th.tint(th.error)
    } else {
        th.panel
    };
    let fill = Style::default().bg(panel);

    // The second line: Undo / Redo with its key, then Show (or Close for an error).
    let key_of = |a: Action| app.keymap.hint(a).unwrap_or_default();
    let mut acts: Vec<(String, Style, NoticeHit)> = Vec::new();
    let strong = th.accent_style().add_modifier(Modifier::BOLD).patch(fill);
    let quiet = th.faint().patch(fill);
    match n.action {
        NoticeAction::Undo => {
            acts.push((Action::Undo.label().to_string(), strong, NoticeHit::Undo));
            acts.push((
                format!("  {}", key_of(Action::Undo)),
                quiet,
                NoticeHit::Undo,
            ));
        }
        NoticeAction::Redo => {
            acts.push((Action::Redo.label().to_string(), strong, NoticeHit::Redo));
            acts.push((
                format!("  {}", key_of(Action::Redo)),
                quiet,
                NoticeHit::Redo,
            ));
        }
        NoticeAction::None => {}
    }
    if n.show.is_some() {
        if !acts.is_empty() {
            acts.push(("  ·  ".to_string(), quiet, NoticeHit::Card));
        }
        acts.push((Key::NoticeShow.tr().to_string(), strong, NoticeHit::Show));
    }
    if n.kind == NoticeKind::Error {
        acts.push((Key::NoticeClose.tr().to_string(), strong, NoticeHit::Close));
        acts.push(("  Esc".to_string(), quiet, NoticeHit::Close));
    }
    let waiting = app.notices.waiting();
    if waiting > 0 {
        acts.push((format!("   +{waiting}"), quiet, NoticeHit::Card));
    }
    let acts_w: usize = acts.iter().map(|(t, _, _)| t.width()).sum();

    // The message, wrapped so that a long path is never cut.
    let avail = (body.width as usize)
        .saturating_sub(2 * MARGIN as usize + 4)
        .max(12);
    let text_w = avail.min(MAX_WIDTH).saturating_sub(2);
    let mut lines = fmt::wrap(&n.text.replace('\n', " "), text_w);
    lines.truncate(6);
    let widest = lines.iter().map(|l| l.width()).max().unwrap_or(0);
    let inner_w = (widest + 2).max(acts_w).max(16);
    let width = (inner_w + 4).min(body.width as usize - MARGIN as usize) as u16;
    let rows = lines.len() as u16 + u16::from(!acts.is_empty());
    // Soft top and bottom edges where the glyphs exist.
    let halves = !app.basic;
    let height = rows + if halves { 2 } else { 0 };
    if height > body.height {
        return;
    }
    let card = Rect {
        x: body.x + body.width - width - MARGIN,
        y: body.y + body.height - height,
        width,
        height,
    };
    let w = width as usize;

    let mut out: Vec<Line> = Vec::new();
    if halves {
        out.push(Line::from(Span::styled(
            "▄".repeat(w),
            th.on(Style::default().fg(panel)),
        )));
    }
    for (i, l) in lines.iter().enumerate() {
        let lead = if i == 0 {
            Span::styled(
                format!("  {mark} "),
                th.fg(tone).add_modifier(Modifier::BOLD).patch(fill),
            )
        } else {
            Span::styled("    ", fill)
        };
        let text = Span::styled(
            l.clone(),
            if n.kind == NoticeKind::Error {
                th.fg(th.error).patch(fill)
            } else {
                th.base().add_modifier(Modifier::BOLD).patch(fill)
            },
        );
        let used = 4 + l.width();
        out.push(Line::from(vec![
            lead,
            text,
            Span::styled(" ".repeat(w.saturating_sub(used)), fill),
        ]));
    }
    if !acts.is_empty() {
        let mut spans = vec![Span::styled("    ", fill)];
        for (t, st, _) in &acts {
            spans.push(Span::styled(t.clone(), *st));
        }
        let used = 4 + acts_w;
        spans.push(Span::styled(" ".repeat(w.saturating_sub(used)), fill));
        out.push(Line::from(spans));
    }
    if halves {
        out.push(Line::from(Span::styled(
            "▀".repeat(w),
            th.on(Style::default().fg(panel)),
        )));
    }
    f.render_widget(ratatui::widgets::Clear, card);
    f.render_widget(Paragraph::new(out), card);

    // What can be clicked: each piece of the second line, and the card itself behind them.
    app.hits.add(card, Target::Notice(NoticeHit::Card));
    if !acts.is_empty() {
        let y = card.y + u16::from(halves) + lines.len() as u16;
        let mut x = card.x + 4;
        for (t, _, hit) in &acts {
            let tw = t.width() as u16;
            if *hit != NoticeHit::Card {
                app.hits.add(
                    Rect {
                        x,
                        y,
                        width: tw,
                        height: 1,
                    },
                    Target::Notice(*hit),
                );
            }
            x += tw;
        }
    }
}
