//! Small windows about the system and its disks: a folder the system has not let the
//! terminal open yet, and ejecting a disk.

use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::modals::titled;
use super::popup::{Ctx, Tone};
use super::widgets::{button, centered, hit_spans};
use crate::app::{EjectStage, EjectView, FontView};
use crate::fmt;
use crate::hits::{Hits, Target};
use crossterm::event::KeyCode;

/// The window that explains a folder the system has not let the terminal open yet, with the
/// way to open the page of the settings where that is changed.
pub fn draw_access(
    f: &mut Frame,
    ctx: &Ctx,
    d: &rada_core::platform::AccessDenied,
    area: Rect,
    hits: &mut Hits,
) {
    let th = ctx.th;
    let w = 84.min(area.width.saturating_sub(4));
    let text_w = (w as usize).saturating_sub(6);
    let mut lines = vec![
        titled(ctx, Key::ChipNoAccess.tr(), Tone::Warn, &d.message()),
        Line::raw(""),
    ];
    for l in fmt::wrap(Key::MacAsksOnce.tr(), text_w) {
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
            Key::BtnOpenFilesSettings.tr(),
            Some(th.accent),
            true,
        ),
        Span::raw("  "),
        button(th, "f", Key::BtnFullDisk.tr(), None, true),
        Span::raw("  "),
        button(th, "Esc", Key::BtnClose.tr(), None, true),
    ];
    let h = (lines.len() as u16 + 1 + 4).min(area.height.saturating_sub(2));
    let rect = centered(area, w, h);
    let inner = super::modals::frame(f, th, rect, th.warn);
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
pub fn draw_eject(f: &mut Frame, ctx: &Ctx, e: &EjectView, area: Rect, hits: &mut Hits) {
    let th = ctx.th;
    let w = 76.min(area.width.saturating_sub(4));
    let text_w = (w as usize).saturating_sub(6);
    let mut lines = vec![
        titled(ctx, Key::BtnEject.tr(), Tone::Accent, &e.name),
        Line::raw(""),
    ];
    let (spans, targets, tone): (Vec<Span>, Vec<(usize, Target)>, Color) = match &e.stage {
        EjectStage::Confirm => {
            for l in fmt::wrap(Key::EjectFinishFirst.tr(), text_w) {
                lines.push(Line::from(Span::styled(l, th.dim())));
            }
            (
                vec![
                    button(th, "Enter", Key::BtnEject.tr(), Some(th.accent), true),
                    Span::raw("  "),
                    button(th, "Esc", Key::BtnCancel.tr(), None, true),
                ],
                vec![
                    (0, Target::Key(KeyCode::Enter)),
                    (2, Target::Key(KeyCode::Esc)),
                ],
                th.accent,
            )
        }
        EjectStage::Working => {
            lines.push(Line::from(Span::styled(Key::Ejecting.tr(), th.base())));
            (
                vec![button(th, "Esc", Key::BtnHide.tr(), None, true)],
                vec![(0, Target::Key(KeyCode::Esc))],
                th.accent,
            )
        }
        EjectStage::Failed(why) => {
            for l in fmt::wrap(&tf(Key::NotEjected, &[why]), text_w) {
                lines.push(Line::from(Span::styled(l, th.fg(th.error))));
            }
            (
                vec![
                    button(th, "Enter", Key::BtnTryAgain.tr(), Some(th.accent), true),
                    Span::raw("  "),
                    button(th, "Esc", Key::BtnClose.tr(), None, true),
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
    let inner = super::modals::frame(f, th, rect, tone);
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
pub fn draw_font(f: &mut Frame, ctx: &Ctx, v: &FontView, area: Rect, hits: &mut Hits) {
    use crate::app::FontStage;
    let th = ctx.th;
    let w = 80.min(area.width.saturating_sub(4));
    let text_w = (w as usize).saturating_sub(6);
    let mut lines = vec![
        titled(
            ctx,
            Key::ChipIcons.tr(),
            Tone::Accent,
            Key::FontCannotDraw.tr(),
        ),
        Line::raw(""),
    ];
    let (spans, targets): (Vec<Span>, Vec<(usize, Target)>) = match v.stage {
        FontStage::Ask => {
            let body = tf(
                Key::FontOffer,
                &[&v.offer.family, &format!("{:.0}", v.offer.megabytes)],
            );
            for l in fmt::wrap(&body, text_w) {
                lines.push(Line::from(Span::styled(l, th.dim())));
            }
            (
                vec![
                    button(th, "Enter", Key::BtnInstallFont.tr(), Some(th.accent), true),
                    Span::raw("  "),
                    button(th, "Esc", Key::BtnNotNow.tr(), None, true),
                ],
                vec![
                    (0, Target::Key(KeyCode::Enter)),
                    (2, Target::Key(KeyCode::Esc)),
                ],
            )
        }
        FontStage::Working => {
            lines.push(Line::from(Span::styled(Key::FontWorking.tr(), th.base())));
            (
                vec![button(th, "Esc", Key::BtnHide.tr(), None, true)],
                vec![(0, Target::Key(KeyCode::Esc))],
            )
        }
    };
    lines.push(Line::raw(""));
    let h = (lines.len() as u16 + 1 + 4).min(area.height.saturating_sub(2));
    let rect = centered(area, w, h);
    let inner = super::modals::frame(f, th, rect, th.accent);
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
