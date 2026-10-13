//! The window of the update: the release notes and what accepting would do, or why an
//! update did not work.

use rada_core::i18n::{Key, tf};
use rada_core::update::Mode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph};

use super::widgets::{button, centered, hit_spans};
use crate::app::UpdateView;
use crate::fmt;
use crate::hits::{Hits, Target};
use crate::theme::Theme;
use crossterm::event::KeyCode;
use ratatui::style::{Color, Style};

/// The buttons of the window and the clickable parts among them.
type Buttons<'a> = (Vec<Span<'a>>, Vec<(usize, Target)>);

fn chip<'a>(th: &Theme, text: &str, color: Color) -> Span<'a> {
    Span::styled(
        format!(" {text} "),
        Style::default()
            .fg(th.on_accent)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

/// What accepting would do, in the words of the person.
pub fn what_happens(how: &Result<Mode, String>) -> String {
    match how {
        Ok(Mode::Binary) => Key::UpdateBinary.tr().into(),
        Ok(Mode::Source { dir }) => tf(Key::UpdateSource, &[&dir.display()]),
        Ok(Mode::Package { hint }) => hint.clone(),
        Err(e) => e.clone(),
    }
}

/// The window, drawn over the dimmed interface.
pub fn draw_update(f: &mut Frame, th: &Theme, view: &UpdateView, area: Rect, hits: &mut Hits) {
    let w = 84.min(area.width.saturating_sub(4));
    let text_w = (w as usize).saturating_sub(6);
    let mut head: Vec<Line> = Vec::new();
    let mut body: Vec<Line> = Vec::new();
    let mut foot: Vec<Line> = Vec::new();
    let buttons: Buttons;
    let (tone, scroll) = match view {
        UpdateView::Offer {
            notice,
            how,
            scroll,
        } => {
            head.push(Line::from(vec![
                chip(th, Key::ChipUpdate.tr(), th.accent),
                Span::styled(
                    format!(
                        "  {}",
                        tf(
                            Key::UpdateAvailable,
                            &[
                                &notice.version,
                                &if notice.prerelease {
                                    Key::PreRelease.tr()
                                } else {
                                    ""
                                }
                            ]
                        )
                    ),
                    th.base().add_modifier(Modifier::BOLD),
                ),
            ]));
            head.push(Line::raw(""));
            head.push(Line::from(Span::styled(Key::WhatIsNew.tr(), th.dim())));
            let notes = notice.notes.trim();
            if notes.is_empty() {
                body.push(Line::from(Span::styled(Key::NoReleaseNotes.tr(), th.faint())));
            }
            for raw in notes.lines() {
                let raw = raw.trim_end();
                if raw.is_empty() {
                    body.push(Line::raw(""));
                    continue;
                }
                let heading = raw.starts_with('#');
                let shown = raw.trim_start_matches('#').trim_start();
                for l in fmt::wrap(shown, text_w) {
                    let style = if heading {
                        th.base().add_modifier(Modifier::BOLD)
                    } else {
                        th.base()
                    };
                    body.push(Line::from(Span::styled(l, style)));
                }
            }
            foot.push(Line::raw(""));
            for l in fmt::wrap(&what_happens(how), text_w) {
                foot.push(Line::from(Span::styled(l, th.dim())));
            }
            foot.push(Line::raw(""));
            let can = matches!(how, Ok(Mode::Binary | Mode::Source { .. }));
            let mut spans = Vec::new();
            let mut targets = Vec::new();
            if can {
                spans.push(button(th, "Enter", Key::BtnUpdateNow.tr(), Some(th.accent), true));
                spans.push(Span::raw("  "));
                targets.push((0, Target::Key(KeyCode::Enter)));
                targets.push((2, Target::Key(KeyCode::Esc)));
                spans.push(button(th, "Esc", Key::BtnLater.tr(), None, true));
            } else {
                targets.push((0, Target::Key(KeyCode::Esc)));
                spans.push(button(th, "Esc", Key::BtnClose.tr(), None, true));
            }
            buttons = (spans, targets);
            (th.accent, *scroll)
        }
        UpdateView::Failed { message, log } => {
            head.push(Line::from(vec![
                chip(th, Key::ChipUpdate.tr(), th.error),
                Span::styled(
                    format!("  {}", Key::UpdateFailed.tr()),
                    th.base().add_modifier(Modifier::BOLD),
                ),
            ]));
            head.push(Line::raw(""));
            for l in fmt::wrap(message, text_w) {
                body.push(Line::from(Span::styled(l, th.fg(th.error))));
            }
            foot.push(Line::raw(""));
            foot.push(Line::from(Span::styled(
                Key::UpdateUntouched.tr(),
                th.dim(),
            )));
            foot.push(Line::from(Span::styled(
                tf(
                    Key::LogLine,
                    &[&crate::ui::widgets::tail(
                        &log.to_string_lossy(),
                        text_w.saturating_sub(5),
                    )],
                ),
                th.faint(),
            )));
            foot.push(Line::raw(""));
            buttons = (
                vec![
                    button(th, "o", Key::BtnOpenLog.tr(), Some(th.accent), true),
                    Span::raw("  "),
                    button(th, "Esc", Key::BtnClose.tr(), None, true),
                ],
                vec![
                    (0, Target::Key(KeyCode::Char('o'))),
                    (2, Target::Key(KeyCode::Esc)),
                ],
            );
            (th.error, 0)
        }
    };

    let fixed = head.len() + foot.len() + 1 /* buttons */ + 2 /* padding */ + 2 /* border */;
    let want = body.len() + fixed;
    let h = (want as u16).min(area.height.saturating_sub(2)).max(8);
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(tone))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = blk.inner(rect);
    f.render_widget(blk, rect);

    let rows = (inner.height as usize).saturating_sub(head.len() + foot.len() + 1);
    let scroll = scroll.min(body.len().saturating_sub(rows));
    let mut lines = head;
    lines.extend(body.into_iter().skip(scroll).take(rows));
    // Pad so the footer sits at the bottom of the window.
    while lines.len() + foot.len() + 1 < inner.height as usize {
        lines.push(Line::raw(""));
    }
    lines.extend(foot);
    {
        let (spans, targets) = buttons;
        hit_spans(
            hits,
            inner.x,
            inner.y + lines.len() as u16,
            &spans,
            &targets,
        );
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
