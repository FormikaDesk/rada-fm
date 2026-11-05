//! Small windows about the system and its disks: a folder the system has not let the
//! terminal open yet, ejecting a disk, and the offer of a font.

use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::modals::titled;
use super::popup::{Ctx, Tone};
use super::sheet::{self, Sheet};
use super::widgets::button;
use crate::app::{EjectStage, EjectView, FontView};
use crate::fmt;
use crate::hits::{Hits, Target};
use crossterm::event::KeyCode;

type Body = Vec<(Line<'static>, Option<Target>)>;
type Buttons = Vec<(Span<'static>, Target)>;

/// A text wrapped to `width`, every line of one style.
fn wrapped(text: &str, width: usize, style: Style) -> Body {
    fmt::wrap(text, width)
        .into_iter()
        .map(|l| (Line::from(Span::styled(l, style)), None))
        .collect()
}

fn key_button(
    ctx: &Ctx,
    key: &str,
    label: &str,
    main: bool,
    code: KeyCode,
) -> (Span<'static>, Target) {
    let th = ctx.th;
    (
        button(th, key, label, main.then_some(th.accent), true),
        Target::Key(code),
    )
}

/// The steps to follow, numbered, each wrapped under its own number.
fn numbered_steps(ctx: &Ctx, steps: &[String], width: usize) -> Body {
    let th = ctx.th;
    let mut out = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        for (j, l) in fmt::wrap(step, width.saturating_sub(3))
            .into_iter()
            .enumerate()
        {
            let num = if j == 0 {
                format!("{}. ", i + 1)
            } else {
                "   ".into()
            };
            let line = Line::from(vec![
                Span::styled(num, th.accent_style()),
                Span::styled(l, th.base()),
            ]);
            out.push((line, None));
        }
    }
    out
}

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
    let text_w = sheet::text_width(area, 84);
    let mut body = wrapped(Key::MacAsksOnce.tr(), text_w, th.dim());
    body.push((Line::raw(""), None));
    body.extend(numbered_steps(ctx, &d.explanation(), text_w));
    let sheet = Sheet {
        title: titled(ctx, Key::ChipNoAccess.tr(), Tone::Warn, &d.message()),
        body,
        buttons: vec![
            key_button(
                ctx,
                "Enter",
                Key::BtnOpenFilesSettings.tr(),
                true,
                KeyCode::Enter,
            ),
            key_button(ctx, "f", Key::BtnFullDisk.tr(), false, KeyCode::Char('f')),
            key_button(ctx, "Esc", Key::BtnClose.tr(), false, KeyCode::Esc),
        ],
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 84, sheet, hits);
}

/// Ejecting a disk: the question, the wait, or why it did not work.
pub fn draw_eject(f: &mut Frame, ctx: &Ctx, e: &EjectView, area: Rect, hits: &mut Hits) {
    let th = ctx.th;
    let text_w = sheet::text_width(area, 76);
    let (body, buttons): (Body, Buttons) = match &e.stage {
        EjectStage::Confirm => (
            wrapped(Key::EjectFinishFirst.tr(), text_w, th.dim()),
            vec![
                key_button(ctx, "Enter", Key::BtnEject.tr(), true, KeyCode::Enter),
                key_button(ctx, "Esc", Key::BtnCancel.tr(), false, KeyCode::Esc),
            ],
        ),
        EjectStage::Working => (
            wrapped(Key::Ejecting.tr(), text_w, th.base()),
            vec![key_button(
                ctx,
                "Esc",
                Key::BtnHide.tr(),
                false,
                KeyCode::Esc,
            )],
        ),
        EjectStage::Failed(why) => (
            wrapped(&tf(Key::NotEjected, &[why]), text_w, th.fg(th.error)),
            vec![
                key_button(ctx, "Enter", Key::BtnTryAgain.tr(), true, KeyCode::Enter),
                key_button(ctx, "Esc", Key::BtnClose.tr(), false, KeyCode::Esc),
            ],
        ),
    };
    let sheet = Sheet {
        title: titled(ctx, Key::BtnEject.tr(), Tone::Accent, &e.name),
        body,
        buttons,
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 76, sheet, hits);
}

/// The offer of a Nerd Font, made once.
pub fn draw_font(f: &mut Frame, ctx: &Ctx, v: &FontView, area: Rect, hits: &mut Hits) {
    use crate::app::FontStage;
    let th = ctx.th;
    let text_w = sheet::text_width(area, 80);
    let hide = key_button(ctx, "Esc", Key::BtnHide.tr(), false, KeyCode::Esc);
    let (body, buttons): (Body, Buttons) = match v.stage {
        FontStage::Ask => {
            let size = format!("{:.0}", v.offer.megabytes);
            let offer = tf(Key::FontOffer, &[&v.offer.family, &size]);
            let install = key_button(ctx, "Enter", Key::BtnInstallFont.tr(), true, KeyCode::Enter);
            let later = key_button(ctx, "Esc", Key::BtnNotNow.tr(), false, KeyCode::Esc);
            (wrapped(&offer, text_w, th.dim()), vec![install, later])
        }
        FontStage::Working => (
            wrapped(Key::FontWorking.tr(), text_w, th.base()),
            vec![hide],
        ),
    };
    let sheet = Sheet {
        title: titled(
            ctx,
            Key::ChipIcons.tr(),
            Tone::Accent,
            Key::FontCannotDraw.tr(),
        ),
        body,
        buttons,
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 80, sheet, hits);
}
