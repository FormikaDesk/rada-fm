//! The notification cards: small rounded cards piled at the bottom right of the list, up to
//! three, the newest at the bottom.
//!
//! A card has the left side of its border in the colour of its state — green for what went
//! well, red for an error, yellow for a warning, the accent for an operation that is running —
//! then the mark of the state, the title in bold and, on the right, quiet details ("42% · 12
//! MB/s · 3 s"). The second line holds what can be done as links in the accent colour
//! (`Annulla  Ctrl+Z · Mostra`); an operation that is running has its progress bar there, shaded
//! like the cursor row. The bottom edge of a card that goes by itself fills with the colour of
//! its state and shortens as its time runs out. A card never takes the keyboard.

use std::time::Instant;

use rada_core::i18n::Key;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::popup::{self, Ctx, Tone};
use super::widgets::SPIN;
use crate::app::{App, MAX_SHOWN, NoticeAction, NoticeKind};
use crate::fmt;
use crate::hits::{NoticeHit, Target};
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// The widest a card gets, and the narrowest.
const MAX_WIDTH: usize = 60;
const MIN_WIDTH: usize = 38;

/// One piece of the second line.
struct Link {
    text: String,
    /// A key name, not a link: quiet.
    quiet: bool,
    hit: NoticeHit,
}

/// A card, worked out and not yet drawn.
pub struct Card {
    /// The id of the notification (0 for the operation that is running).
    id: u64,
    tone: Tone,
    mark: String,
    title: String,
    data: String,
    links: Vec<Link>,
    /// The progress bar of a running operation.
    progress: Option<f64>,
    /// The share of its time a card that goes by itself has left.
    left: Option<f64>,
    /// Lines the title may take (an error says more).
    max_lines: usize,
}

fn link(text: impl Into<String>, quiet: bool, hit: NoticeHit) -> Link {
    Link {
        text: text.into(),
        quiet,
        hit,
    }
}

/// The cards to show now, from the top: the operation that is running, then the notifications
/// (the newest two when something is running).
pub fn cards(app: &App, now: Instant) -> Vec<Card> {
    let mut out: Vec<Card> = Vec::new();
    let key_of = |a: Action| app.keymap.hint(a).unwrap_or_default();
    let done = icons::ui(app.icons, Glyph::Done);
    let mut notes: Vec<Card> = Vec::new();
    for (n, _) in app.notices.shown(now) {
        let (mark, tone) = match n.kind {
            NoticeKind::Ok => (if done.is_empty() { "+" } else { done }, Tone::Ok),
            NoticeKind::Info => ("●", Tone::Info),
            NoticeKind::Warn => ("▲", Tone::Warn),
            NoticeKind::Error => (
                if app.icons == icons::IconSet::None {
                    "x"
                } else {
                    "✕"
                },
                Tone::Danger,
            ),
        };
        let mut links: Vec<Link> = Vec::new();
        match n.action {
            NoticeAction::Undo => {
                links.push(link(Action::Undo.label(), false, NoticeHit::Undo));
                links.push(link(
                    format!("  {}", key_of(Action::Undo)),
                    true,
                    NoticeHit::Undo,
                ));
            }
            NoticeAction::Redo => {
                links.push(link(Action::Redo.label(), false, NoticeHit::Redo));
                links.push(link(
                    format!("  {}", key_of(Action::Redo)),
                    true,
                    NoticeHit::Redo,
                ));
            }
            NoticeAction::None => {}
        }
        if n.show.is_some() {
            if !links.is_empty() {
                links.push(link("  ·  ", true, NoticeHit::Card));
            }
            links.push(link(Key::NoticeShow.tr(), false, NoticeHit::Show));
        }
        if n.kind == NoticeKind::Error {
            links.push(link(Key::NoticeClose.tr(), false, NoticeHit::Close));
            links.push(link("  Esc", true, NoticeHit::Close));
        }
        notes.push(Card {
            id: n.id,
            tone,
            mark: mark.to_string(),
            title: fmt::capitalised(&n.text.replace('\n', " ")),
            data: String::new(),
            links,
            progress: None,
            left: app.notices.left(n.id, now),
            max_lines: if n.kind == NoticeKind::Error { 6 } else { 3 },
        });
    }
    if let Some(r) = &app.running {
        let p = &r.progress;
        let frac = p.fraction();
        let mut data = vec![format!("{}%", (frac * 100.0) as u32)];
        if r.rate() > 1.0 && p.bytes_total > 0 {
            data.push(fmt::rate(r.rate()));
        }
        if let Some(eta) = r.eta() {
            data.push(fmt::duration(eta));
        }
        out.push(Card {
            id: 0,
            tone: Tone::Accent,
            mark: SPIN[app.spinner % SPIN.len()].to_string(),
            title: super::plan::shorten_title(&r.title, app.home()),
            data: data.join(" · "),
            links: vec![
                link(
                    fmt::capitalised(Key::ProgCancel.tr()),
                    false,
                    NoticeHit::CancelRun,
                ),
                link("  Esc", true, NoticeHit::CancelRun),
            ],
            progress: Some(frac),
            left: None,
            max_lines: 2,
        });
        let skip = notes.len().saturating_sub(MAX_SHOWN - 1);
        out.extend(notes.into_iter().skip(skip));
    } else {
        out.extend(notes);
    }
    out
}

/// How wide the cards are on a list `room` cells wide: as wide as the widest needs, within
/// limits.
fn common_width(cards: &[Card], room: usize) -> usize {
    let need = cards
        .iter()
        .map(|c| {
            let data = if c.data.is_empty() {
                0
            } else {
                3 + c.data.width()
            };
            2 + 4 + c.title.width() + data + 2
        })
        .max()
        .unwrap_or(MIN_WIDTH);
    need.clamp(MIN_WIDTH, MAX_WIDTH).min(room.saturating_sub(2))
}

/// The title wrapped to what the card leaves for it.
fn title_lines(c: &Card, width: usize) -> Vec<String> {
    let data_w = if c.data.is_empty() {
        0
    } else {
        3 + c.data.width()
    };
    let room = width.saturating_sub(2 + 4 + data_w + 1).max(12);
    let mut lines = fmt::wrap(&c.title, room);
    lines.truncate(c.max_lines);
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Rows a card takes: its border, the title and the second line.
fn height(c: &Card, width: usize) -> u16 {
    title_lines(c, width).len() as u16 + 1 + 2
}

/// The rows the pile takes on a list `room` cells wide (0 when there is nothing to show).
pub fn stack_height(app: &App, now: Instant, room: usize) -> u16 {
    let cards = cards(app, now);
    let w = common_width(&cards, room);
    cards.iter().map(|c| height(c, w)).sum()
}

/// Draw the pile in the bottom right of `area` (the list), the first card on top.
pub fn draw(f: &mut Frame, app: &mut App, area: Rect, now: Instant) {
    let cards = cards(app, now);
    if cards.is_empty() || area.width < 24 || area.height < 4 {
        return;
    }
    let th = app.th.clone();
    let ctx = Ctx {
        th: &th,
        look: super::soft::Look::of(app),
        set: app.icons,
        basic: app.basic,
    };
    let width = common_width(&cards, area.width as usize);
    let total: u16 = cards.iter().map(|c| height(c, width)).sum();
    if total > area.height {
        return;
    }
    let x = area.right().saturating_sub(width as u16 + 1);
    let mut y = area.bottom() - total;
    let mut targets: Vec<(Rect, Target)> = Vec::new();
    for c in &cards {
        let h = height(c, width);
        let rect = Rect {
            x,
            y,
            width: width as u16,
            height: h,
        };
        draw_card(f, &ctx, c, rect, &mut targets);
        y += h;
    }
    for (r, t) in targets {
        app.hits.add(r, t);
    }
}

fn draw_card(f: &mut Frame, ctx: &Ctx, c: &Card, rect: Rect, targets: &mut Vec<(Rect, Target)>) {
    let th = ctx.th;
    let tone = c.tone.color(th);
    let buf = f.buffer_mut();
    let inner = popup::card(buf, rect, th, popup::Edge::Behind);
    if inner.width < 12 || inner.height < 2 {
        return;
    }
    // The left side of the border in the colour of the state.
    let side = crate::icons::mark(ctx.set, crate::icons::Mark::CardSide);
    for y in rect.y + 1..rect.bottom() - 1 {
        popup::mark(buf, rect.x, y, side, tone);
    }
    let w = inner.width as usize;
    let lines = title_lines(c, rect.width as usize);
    let data_w = c.data.width();
    for (i, line) in lines.iter().enumerate() {
        let lead = if i == 0 {
            Span::styled(
                format!(" {} ", c.mark),
                Style::default()
                    .fg(tone)
                    .bg(th.popup)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("   ", ctx.surface())
        };
        let mut spans = vec![
            lead,
            Span::styled(line.clone(), ctx.base().add_modifier(Modifier::BOLD)),
        ];
        if i == 0 && data_w > 0 {
            let used = 3 + line.width();
            let gap = w.saturating_sub(used + data_w + 1);
            spans.push(Span::styled(" ".repeat(gap), ctx.surface()));
            spans.push(Span::styled(c.data.clone(), ctx.dim()));
        }
        popup::put(
            buf,
            Rect {
                y: inner.y + i as u16,
                height: 1,
                ..inner
            },
            Line::from(spans),
        );
    }
    // The second line: the links, with the progress bar before them for a running operation.
    let row = Rect {
        y: inner.y + lines.len() as u16,
        height: 1,
        ..inner
    };
    let mut spans: Vec<Span> = vec![Span::styled("   ", ctx.surface())];
    let links_w: usize = c.links.iter().map(|l| l.text.width()).sum();
    if let Some(frac) = c.progress {
        let bar_w = w.saturating_sub(3 + links_w + 3).max(6);
        spans.extend(progress_spans(ctx, frac, bar_w));
        spans.push(Span::styled("   ", ctx.surface()));
    }
    let mut x = row.x + spans.iter().map(|s| s.content.width()).sum::<usize>() as u16;
    for l in &c.links {
        let style = if l.quiet {
            ctx.faint()
        } else {
            ctx.accent().add_modifier(Modifier::BOLD)
        };
        let lw = l.text.width() as u16;
        if l.hit != NoticeHit::Card {
            targets.push((
                Rect {
                    x,
                    y: row.y,
                    width: lw,
                    height: 1,
                },
                Target::Notice(c.id, l.hit),
            ));
        }
        x += lw;
        spans.push(Span::styled(l.text.clone(), style));
    }
    popup::put(buf, row, Line::from(spans));
    // The bottom edge: filled with the colour of the state, as long as the time that is left.
    if let Some(left) = c.left {
        let cells = ((rect.width as usize).saturating_sub(2) as f64 * left).round() as u16;
        for i in 0..cells {
            popup::mark(buf, rect.x + 1 + i, rect.bottom() - 1, "━", tone);
        }
    }
    // The card itself behind its parts: a click on it closes it (a running one has no such).
    if c.id != 0 {
        targets.insert(0, (rect, Target::Notice(c.id, NoticeHit::Card)));
    }
}

/// A progress bar `cells` wide, shaded like the cursor row: `━━━━╺───`.
fn progress_spans(ctx: &Ctx, frac: f64, cells: usize) -> Vec<Span<'static>> {
    let th = ctx.th;
    let frac = frac.clamp(0.0, 1.0);
    let halves = (frac * cells as f64 * 2.0).round() as usize;
    let full = (halves / 2).min(cells);
    let mut out: Vec<Span<'static>> = Vec::new();
    for i in 0..full {
        let colour = if th.shades() {
            crate::theme::mix(th.grad_row_start, th.grad_row_end, i, cells.max(2))
        } else {
            th.accent
        };
        out.push(Span::styled("━", Style::default().fg(colour).bg(th.popup)));
    }
    let mut used = full;
    if halves % 2 == 1 && full < cells {
        out.push(Span::styled(
            "╺",
            Style::default().fg(th.accent).bg(th.popup),
        ));
        used += 1;
    }
    if used < cells {
        out.push(Span::styled(
            "━".repeat(cells - used),
            Style::default().fg(th.popup_border).bg(th.popup),
        ));
    }
    out
}
