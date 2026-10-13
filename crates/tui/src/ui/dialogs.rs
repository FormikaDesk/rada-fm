//! Small windows about the system and its disks: a folder the system has not let the
//! terminal open yet, and ejecting a disk.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph};

use super::widgets::{button, centered, hit_spans};
use crate::app::{EjectStage, EjectView, FontView};
use crate::fmt;
use crate::hits::{Hits, Target};
use crate::theme::Theme;
use crossterm::event::KeyCode;

fn chip<'a>(th: &Theme, text: &str, color: Color) -> Span<'a> {
    Span::styled(
        format!(" {text} "),
        Style::default()
            .fg(th.on_accent)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

/// The window that explains a folder the system has not let the terminal open yet, with the
/// way to open the page of the settings where that is changed.
pub fn draw_access(
    f: &mut Frame,
    th: &Theme,
    d: &rada_core::platform::AccessDenied,
    area: Rect,
    hits: &mut Hits,
) {
    let w = 84.min(area.width.saturating_sub(4));
    let text_w = (w as usize).saturating_sub(6);
    let mut lines = vec![
        Line::from(vec![
            chip(th, "No access", th.warn),
            Span::styled(
                format!("  {}", d.message()),
                th.base().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(""),
    ];
    for l in fmt::wrap(
        "macOS asks you once, for each program, before it lets it into the Desktop, Documents, Downloads, iCloud Drive and external or network disks.",
        text_w,
    ) {
        lines.push(Line::from(Span::styled(l, th.dim())));
    }
    lines.push(Line::raw(""));
    for (i, step) in d.explanation().iter().enumerate() {
        let wrapped = fmt::wrap(step, text_w.saturating_sub(3));
        for (j, l) in wrapped.into_iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(
                    if j == 0 {
                        format!("{}. ", i + 1)
                    } else {
                        "   ".into()
                    },
                    th.accent_style(),
                ),
                Span::styled(l, th.base()),
            ]));
        }
    }
    lines.push(Line::raw(""));
    let spans = vec![
        button(
            th,
            "Enter",
            "Open Files & Folders settings",
            Some(th.accent),
            true,
        ),
        Span::raw("  "),
        button(th, "f", "Full Disk Access", None, true),
        Span::raw("  "),
        button(th, "Esc", "Close", None, true),
    ];
    let h = (lines.len() as u16 + 1 + 4).min(area.height.saturating_sub(2));
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(th.warn))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = blk.inner(rect);
    f.render_widget(blk, rect);
    hit_spans(
        hits,
        inner.x,
        inner.y + lines.len() as u16,
        &spans,
        &[
            (0, Target::Key(KeyCode::Enter)),
            (2, Target::Key(KeyCode::Char('f'))),
            (4, Target::Key(KeyCode::Esc)),
        ],
    );
    lines.push(Line::from(spans));
    f.render_widget(Paragraph::new(lines), inner);
}

/// Ejecting a disk: the question, the wait, or why it did not work.
pub fn draw_eject(f: &mut Frame, th: &Theme, e: &EjectView, area: Rect, hits: &mut Hits) {
    let w = 76.min(area.width.saturating_sub(4));
    let text_w = (w as usize).saturating_sub(6);
    let mut lines = vec![
        Line::from(vec![
            chip(th, "Eject", th.accent),
            Span::styled(
                format!("  {}", e.name),
                th.base().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(""),
    ];
    let (spans, targets, tone): (Vec<Span>, Vec<(usize, Target)>, Color) = match &e.stage {
        EjectStage::Confirm => {
            for l in fmt::wrap(
                "Everything in progress on this disk is finished first. After this you can unplug it.",
                text_w,
            ) {
                lines.push(Line::from(Span::styled(l, th.dim())));
            }
            (
                vec![
                    button(th, "Enter", "Eject", Some(th.accent), true),
                    Span::raw("  "),
                    button(th, "Esc", "Cancel", None, true),
                ],
                vec![
                    (0, Target::Key(KeyCode::Enter)),
                    (2, Target::Key(KeyCode::Esc)),
                ],
                th.accent,
            )
        }
        EjectStage::Working => {
            lines.push(Line::from(Span::styled("Ejecting…", th.base())));
            (
                vec![button(th, "Esc", "Hide", None, true)],
                vec![(0, Target::Key(KeyCode::Esc))],
                th.accent,
            )
        }
        EjectStage::Failed(why) => {
            for l in fmt::wrap(&format!("It was not ejected: {why}"), text_w) {
                lines.push(Line::from(Span::styled(l, th.fg(th.error))));
            }
            (
                vec![
                    button(th, "Enter", "Try again", Some(th.accent), true),
                    Span::raw("  "),
                    button(th, "Esc", "Close", None, true),
                ],
                vec![
                    (0, Target::Key(KeyCode::Enter)),
                    (2, Target::Key(KeyCode::Esc)),
                ],
                th.error,
            )
        }
    };
    lines.push(Line::raw(""));
    let h = (lines.len() as u16 + 1 + 4).min(area.height.saturating_sub(2));
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(tone))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = blk.inner(rect);
    f.render_widget(blk, rect);
    hit_spans(
        hits,
        inner.x,
        inner.y + lines.len() as u16,
        &spans,
        &targets,
    );
    lines.push(Line::from(spans));
    f.render_widget(Paragraph::new(lines), inner);
}

/// The offer of a Nerd Font, made once.
pub fn draw_font(f: &mut Frame, th: &Theme, v: &FontView, area: Rect, hits: &mut Hits) {
    use crate::app::FontStage;
    let w = 80.min(area.width.saturating_sub(4));
    let text_w = (w as usize).saturating_sub(6);
    let mut lines = vec![
        Line::from(vec![
            chip(th, "Icons", th.accent),
            Span::styled(
                "  Your terminal cannot draw rada's icons yet",
                th.base().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(""),
    ];
    let (spans, targets): (Vec<Span>, Vec<(usize, Target)>) = match v.stage {
        FontStage::Ask => {
            let body = format!(
                "File-type icons need a Nerd Font, and this terminal is not set to use one. rada can install {} for you: about {:.0} MB, taken from the Nerd Fonts project at a fixed release and checked against its checksums, for your account only (no administrator rights). Afterwards you choose it in your terminal's font setting. This is asked only once.",
                v.offer.family, v.offer.megabytes
            );
            for l in fmt::wrap(&body, text_w) {
                lines.push(Line::from(Span::styled(l, th.dim())));
            }
            (
                vec![
                    button(th, "Enter", "Install the font", Some(th.accent), true),
                    Span::raw("  "),
                    button(th, "Esc", "Not now", None, true),
                ],
                vec![
                    (0, Target::Key(KeyCode::Enter)),
                    (2, Target::Key(KeyCode::Esc)),
                ],
            )
        }
        FontStage::Working => {
            lines.push(Line::from(Span::styled(
                "Downloading and installing…",
                th.base(),
            )));
            (
                vec![button(th, "Esc", "Hide", None, true)],
                vec![(0, Target::Key(KeyCode::Esc))],
            )
        }
    };
    lines.push(Line::raw(""));
    let h = (lines.len() as u16 + 1 + 4).min(area.height.saturating_sub(2));
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(th.accent))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = blk.inner(rect);
    f.render_widget(blk, rect);
    hit_spans(
        hits,
        inner.x,
        inner.y + lines.len() as u16,
        &spans,
        &targets,
    );
    lines.push(Line::from(spans));
    f.render_widget(Paragraph::new(lines), inner);
}
