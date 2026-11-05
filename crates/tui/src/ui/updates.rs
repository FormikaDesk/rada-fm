//! The window of the update: the release notes and what accepting would do, or why an
//! update did not work.

use rada_core::i18n::{Key, tf};
use rada_core::update::Mode;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::modals::titled;
use super::popup::{Ctx, Tone};
use super::sheet::{self, Sheet};
use super::widgets::button;
use crate::app::UpdateView;
use crate::fmt;
use crate::hits::{Hits, Target};
use crate::theme::Theme;
use crossterm::event::KeyCode;

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
pub fn draw_update(f: &mut Frame, ctx: &Ctx, view: &UpdateView, area: Rect, hits: &mut Hits) {
    let text_w = sheet::text_width(area, 84);
    let sheet = match view {
        UpdateView::Offer {
            notice,
            how,
            scroll,
        } => offer_sheet(ctx, notice, how, *scroll, text_w),
        UpdateView::Failed { message, log } => failed_sheet(ctx, message, log, text_w),
    };
    sheet::draw(f, ctx.th, area, 84, sheet, hits);
}

/// The release notes and the question.
fn offer_sheet(
    ctx: &Ctx,
    notice: &rada_core::update::Notice,
    how: &Result<Mode, String>,
    scroll: usize,
    text_w: usize,
) -> Sheet {
    let th = ctx.th;
    let pre = if notice.prerelease {
        Key::PreRelease.tr()
    } else {
        ""
    };
    let version = tf(Key::UpdateAvailable, &[&notice.version, &pre]);
    let can = matches!(how, Ok(Mode::Binary | Mode::Source { .. }));
    let esc = |label: &str| {
        (
            button(th, "Esc", label, None, true),
            Target::Key(KeyCode::Esc),
        )
    };
    let buttons = if can {
        let now = button(th, "Enter", Key::BtnUpdateNow.tr(), Some(th.accent), true);
        vec![(now, Target::Key(KeyCode::Enter)), esc(Key::BtnLater.tr())]
    } else {
        vec![esc(Key::BtnClose.tr())]
    };
    Sheet {
        title: titled(ctx, Key::ChipUpdate.tr(), Tone::Accent, &version),
        lead: vec![Line::from(Span::styled(Key::WhatIsNew.tr(), th.dim()))],
        body: notes_lines(th, notice.notes.trim(), text_w),
        scroll,
        note: wrapped(&what_happens(how), text_w, th.dim()),
        buttons,
        ..Sheet::default()
    }
}

/// Why the update did not work, and where the log is.
fn failed_sheet(ctx: &Ctx, message: &str, log: &std::path::Path, text_w: usize) -> Sheet {
    let th = ctx.th;
    let tail = crate::ui::widgets::tail(&log.to_string_lossy(), text_w.saturating_sub(5));
    let mut note = wrapped(Key::UpdateUntouched.tr(), text_w, th.dim());
    note.push(Line::from(Span::styled(
        tf(Key::LogLine, &[&tail]),
        th.faint(),
    )));
    let open_log = button(th, "o", Key::BtnOpenLog.tr(), Some(th.accent), true);
    let close = button(th, "Esc", Key::BtnClose.tr(), None, true);
    Sheet {
        title: titled(
            ctx,
            Key::ChipUpdate.tr(),
            Tone::Danger,
            Key::UpdateFailed.tr(),
        ),
        body: wrapped(message, text_w, th.fg(th.error))
            .into_iter()
            .map(|l| (l, None))
            .collect(),
        note,
        buttons: vec![
            (open_log, Target::Key(KeyCode::Char('o'))),
            (close, Target::Key(KeyCode::Esc)),
        ],
        ..Sheet::default()
    }
}

fn wrapped(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    fmt::wrap(text, width)
        .into_iter()
        .map(|l| Line::from(Span::styled(l, style)))
        .collect()
}

/// The release notes: headings in bold, the rest wrapped, blank lines kept.
fn notes_lines(th: &Theme, notes: &str, width: usize) -> Vec<(Line<'static>, Option<Target>)> {
    if notes.is_empty() {
        return vec![(
            Line::from(Span::styled(Key::NoReleaseNotes.tr(), th.faint())),
            None,
        )];
    }
    let mut out = Vec::new();
    for raw in notes.lines() {
        let raw = raw.trim_end();
        if raw.is_empty() {
            out.push((Line::raw(""), None));
            continue;
        }
        let heading = raw.starts_with('#');
        let style = if heading {
            th.base().add_modifier(Modifier::BOLD)
        } else {
            th.base()
        };
        let shown = raw.trim_start_matches('#').trim_start();
        out.extend(wrapped(shown, width, style).into_iter().map(|l| (l, None)));
    }
    out
}
