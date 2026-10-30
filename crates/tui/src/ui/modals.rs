//! Windows in front of the interface: the plan, the jump palette, errors, prompts.
//! One window at a time, centred, with everything behind it dimmed.

use rada_core::display;
use rada_core::i18n::{Key, tf};
use rada_core::journal::{EntryStatus, UndoState};
use rada_core::ops::RunStatus;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::popup::{Ctx, Tone};
use super::widgets::{SPIN, button, centered, field_parts, hit_spans, pad, pad_left, tail};
use crate::app::*;
use crate::fmt;
use crate::hits::{Hits, Target};
use crate::keymap::{Action, Keymap, Scheme};
use crate::palette::{PaletteKind, PaletteView};
use crate::theme::Theme;
use crossterm::event::KeyCode;

/// The card of a window: its own tone and a rounded border (see [`super::popup`]); returns
/// the usable area inside it, with air of two cells at the sides and one above and below. The
/// colour a window used to give its border now says itself in the mark at its head.
pub(super) fn frame(f: &mut Frame, th: &Theme, r: Rect, _tone: Color) -> Rect {
    let inner = super::popup::card(f.buffer_mut(), r, th, super::popup::Edge::Own);
    Rect {
        x: inner.x + 2,
        y: inner.y + 1,
        width: inner.width.saturating_sub(4),
        height: inner.height.saturating_sub(2),
    }
}

/// The label at the head of a window: a pill in the tint of its tone (see [`super::popup`]),
/// the title follows it.
pub(super) fn chip(ctx: &Ctx, text: &str, tone: Tone) -> Vec<Span<'static>> {
    let th = ctx.th;
    let c = tone.color(th);
    let fill = th.tint_share(th.popup, c, if th.light { 0.18 } else { 0.28 });
    super::popup::pill(
        ctx,
        text,
        fill,
        Style::default().fg(c).add_modifier(Modifier::BOLD),
    )
}

/// A line that starts with [`chip`] and goes on with a title in bold.
pub(super) fn titled(ctx: &Ctx, label: &str, tone: Tone, title: &str) -> Line<'static> {
    let mut spans = chip(ctx, label, tone);
    spans.push(Span::styled(
        format!("  {title}"),
        ctx.base().add_modifier(Modifier::BOLD),
    ));
    Line::from(spans)
}

/// The preview of the item under the cursor over the whole screen.
fn draw_full_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    f.render_widget(Clear, area);
    if let Some(bg) = th.bg {
        f.render_widget(Block::default().style(Style::default().bg(bg)), area);
    }
    let body = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(2),
        ..area
    };
    super::preview::draw_full(f, app, body);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Esc ", th.chip().add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}   ", Key::HintClose.tr()), th.dim()),
            Span::styled(" ↑↓ ", th.chip().add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}", Key::HintScroll.tr()), th.dim()),
            if app.quick_look_available() {
                Span::styled("   ", th.dim())
            } else {
                Span::raw("")
            },
            if app.quick_look_available() {
                Span::styled(" Q ", th.chip().add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            },
            if app.quick_look_available() {
                Span::styled(format!(" {}", Key::BtnQuickLook.tr()), th.dim())
            } else {
                Span::raw("")
            },
        ])),
        Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        },
    );
    app.frame.hits.add(area, Target::Window);
}

fn draw_properties(f: &mut Frame, ctx: &Ctx, p: &PropsView, area: Rect, hits: &mut Hits) {
    let th = ctx.th;
    let key_w = p.rows.iter().map(|(k, _)| k.width()).max().unwrap_or(8) + 2;
    let w = 80.min(area.width.saturating_sub(4));
    let value_w = (w as usize).saturating_sub(6 + key_w).max(10);
    let mut lines: Vec<Line> = Vec::new();
    for (k, v) in &p.rows {
        for (i, part) in fmt::wrap(v, value_w).into_iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(pad(if i == 0 { k } else { "" }, key_w), th.dim()),
                Span::styled(part, th.base()),
            ]));
        }
    }
    let h = (lines.len() as u16 + 6).min(area.height.saturating_sub(2));
    let r = centered(area, w, h);
    let inner = frame(f, th, r, th.accent);
    let rows = inner.height.saturating_sub(3) as usize;
    let scroll = p.scroll.min(lines.len().saturating_sub(rows));
    let mut out = vec![
        titled(
            ctx,
            Key::ChipProperties.tr(),
            Tone::Accent,
            &display::truncate(&p.title, inner.width as usize / 2),
        ),
        Line::raw(""),
    ];
    out.extend(lines.into_iter().skip(scroll).take(rows));
    out.push(Line::raw(""));
    out.push(Line::from(vec![button(
        th,
        "Esc",
        Key::BtnClose.tr(),
        None,
        true,
    )]));
    f.render_widget(Paragraph::new(out), inner);
    hits.add(
        Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            width: 14.min(inner.width),
            ..inner
        },
        Target::Key(KeyCode::Esc),
    );
}

/// Whether the window in front dims what is behind it: a menu does not, nor the list of
/// programs that sits beside one, nor the preview that takes the whole screen.
pub(super) fn dims_backdrop(app: &App) -> bool {
    let beside_menu = matches!(&app.windows.modal, Some(Modal::OpenWith(v)) if v.under.is_some());
    app.windows.modal.is_some()
        && !beside_menu
        && !matches!(
            app.windows.modal,
            Some(Modal::Menu(_)) | Some(Modal::FullPreview)
        )
}

/// Where a menu may be: the screen without the status bar and the key hints, which it stays
/// above (a menu opened near the bottom moves up instead), and without the row over the status
/// bar, so that the line of the menu is never against the colour of the bar.
pub(super) fn popup_room(app: &App, area: Rect) -> Rect {
    let explorer = app.settings.layout == crate::view::LayoutKind::Explorer;
    let hints = super::hints_height(explorer && app.settings.show_hints, area.height);
    Rect {
        height: area.height.saturating_sub(2 + hints),
        ..area
    }
}

pub fn draw_modal(f: &mut Frame, app: &mut App, area: Rect) {
    if matches!(app.windows.modal, Some(Modal::FullPreview)) {
        draw_full_preview(f, app, area);
        return;
    }
    let mut th = app.settings.th.clone();
    if dims_backdrop(app) {
        // A window over the dimmed screen is a tone of its own, and so is everything in it.
        th.popup = th.modal_surface();
    }
    let room = popup_room(app, area);
    let ctx = super::popup::Ctx {
        th: &th,
        look: super::soft::Look::of(app),
        set: app.settings.icons,
        basic: app.settings.basic,
    };
    let home = app.home().to_path_buf();
    let spinner = SPIN[app.redraw.spinner % SPIN.len()];
    let Some(modal) = &app.windows.modal else {
        return;
    };
    match modal {
        Modal::Scanning {
            what,
            files,
            dirs,
            bytes,
            current,
            ..
        } => {
            let r = centered(area, 66, 9);
            let inner = frame(f, &th, r, th.accent);
            let lines = vec![
                Line::from(vec![
                    Span::styled(format!("{spinner} "), th.accent_style()),
                    Span::styled(what.clone(), th.base().add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(Span::styled(
                    tf(
                        Key::ScanTotals,
                        &[
                            &fmt::count(*files, Key::FileOne, Key::FileMany),
                            &fmt::count(*dirs, Key::FolderOne, Key::FolderMany),
                            &fmt::size(*bytes),
                        ],
                    ),
                    th.base(),
                )),
                Line::from(Span::styled(
                    tail(&display::path(current), inner.width as usize),
                    th.dim(),
                )),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Esc", th.key()),
                    Span::styled(format!(" {}", Key::HintCancel.tr()), th.dim()),
                ]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Plan(pv) => super::plan::draw(
            f,
            &ctx,
            pv,
            &home,
            &app.settings.keymap,
            area,
            &mut app.frame.hits,
        ),
        Modal::Palette(p) => draw_palette(f, &th, p, area, &mut app.frame.hits),
        Modal::OpenWith(v) => {
            // Opened from the context menu: the menu stays in view and the list sits beside it.
            let anchor = v.under.as_deref().map(|m| {
                super::menu::draw_behind(f, &ctx, &app.settings.keymap, m, room);
                super::menu::beside(&ctx, &app.settings.keymap, m, room)
            });
            let at = if anchor.is_some() { room } else { area };
            super::openwith::draw(
                f,
                &ctx,
                &app.settings.keymap,
                v,
                at,
                anchor,
                &mut app.frame.hits,
            );
        }
        Modal::Failure { info, .. } => draw_failure(f, &ctx, info, area, &mut app.frame.hits),
        Modal::Input(iv) => draw_input(f, &ctx, iv, area),
        Modal::History(h) => draw_history(f, &ctx, h, area, spinner, &mut app.frame.hits),
        Modal::Result(r) => {
            let w = 90.min(area.width.saturating_sub(4));
            let wrapped: Vec<(ToastKind, String, bool)> = r
                .lines
                .iter()
                .flat_map(|(k, t)| {
                    fmt::wrap(t, (w as usize).saturating_sub(8))
                        .into_iter()
                        .enumerate()
                        .map(move |(i, l)| (*k, l, i == 0))
                })
                .collect();
            let h = (wrapped.len() as u16 + 8).min(area.height.saturating_sub(2));
            let rect = centered(area, w, h);
            let inner = frame(f, &th, rect, th.warn);
            let title = display::truncate(&r.title, (w as usize).saturating_sub(10));
            let mut lines = vec![
                titled(&ctx, Key::ChipDone.tr(), Tone::Warn, &title),
                Line::raw(""),
            ];
            let rows = inner.height.saturating_sub(5) as usize;
            for (k, t, first) in wrapped.iter().skip(r.scroll).take(rows) {
                let c = if *k == ToastKind::Error {
                    th.error
                } else {
                    th.warn
                };
                lines.push(Line::from(vec![
                    Span::styled(if *first { "• " } else { "  " }, th.fg(c)),
                    Span::styled(t.clone(), th.fg(c)),
                ]));
            }
            lines.push(Line::raw(""));
            let btns = vec![
                button(&th, "u", Key::BtnUndoDone.tr(), Some(th.accent), true),
                Span::raw("  "),
                button(&th, "Esc", Key::BtnClose.tr(), None, true),
            ];
            hit_spans(
                &mut app.frame.hits,
                inner.x,
                inner.y + lines.len() as u16,
                &btns,
                &[
                    (0, Target::Key(KeyCode::Char('u'))),
                    (2, Target::Key(KeyCode::Esc)),
                ],
            );
            lines.push(Line::from(btns));
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::ConfirmQuit => {
            let r = centered(area, 66, 9);
            let inner = frame(f, &th, r, th.warn);
            let lines = vec![
                titled(
                    &ctx,
                    Key::ChipRunning.tr(),
                    Tone::Warn,
                    Key::OpInProgress.tr(),
                ),
                Line::raw(""),
                Line::from(Span::styled(Key::QuitWarn1.tr(), th.dim())),
                Line::from(Span::styled(Key::QuitWarn2.tr(), th.dim())),
                Line::raw(""),
                Line::from(vec![
                    button(&th, "y", Key::BtnQuitAnyway.tr(), Some(th.warn), true),
                    Span::raw("  "),
                    button(&th, "Esc", Key::BtnStay.tr(), None, true),
                ]),
            ];
            let btns = vec![
                button(&th, "y", Key::BtnQuitAnyway.tr(), Some(th.warn), true),
                Span::raw("  "),
                button(&th, "Esc", Key::BtnStay.tr(), None, true),
            ];
            hit_spans(
                &mut app.frame.hits,
                inner.x,
                inner.y + 5,
                &btns,
                &[
                    (0, Target::Key(KeyCode::Char('y'))),
                    (2, Target::Key(KeyCode::Esc)),
                ],
            );
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Help => draw_help(
            f,
            &ctx,
            area,
            &app.settings.keymap,
            app.settings.mouse,
            app.windows.help_scroll,
        ),
        Modal::Menu(m) => {
            super::menu::draw(f, &ctx, &app.settings.keymap, m, room, &mut app.frame.hits)
        }
        Modal::Properties(p) => draw_properties(f, &ctx, p, area, &mut app.frame.hits),
        Modal::Update(u) => super::updates::draw_update(f, &ctx, u, area, &mut app.frame.hits),
        Modal::Access(d) => super::dialogs::draw_access(f, &ctx, d, area, &mut app.frame.hits),
        Modal::Eject(e) => super::dialogs::draw_eject(f, &ctx, e, area, &mut app.frame.hits),
        Modal::Font(v) => super::dialogs::draw_font(f, &ctx, v, area, &mut app.frame.hits),
        Modal::FullPreview => {}
    }
}

// -------------------------------------------------------------------------------- palette

fn draw_palette(f: &mut Frame, th: &Theme, p: &PaletteView, area: Rect, hits: &mut Hits) {
    let w = 78.min(area.width.saturating_sub(6));
    let rows_wanted = p.hits.len().clamp(3, 12) as u16;
    let h = (rows_wanted + 7).min(area.height.saturating_sub(2));
    // A little above the middle, like a command palette.
    let mut r = centered(area, w, h);
    r.y = r.y.saturating_sub(area.height / 8);
    let inner = frame(f, th, r, th.accent);
    let iw = inner.width as usize;

    // Input line.
    let (before, at, after) = field_parts(&p.query, p.cursor);
    let at = if at.is_empty() { " ".into() } else { at };
    let mut input = vec![Span::styled(
        "⌕  ",
        th.accent_style().add_modifier(Modifier::BOLD),
    )];
    if p.query.is_empty() {
        input.push(Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        ));
        input.push(Span::styled(Key::PaletteHint.tr(), th.faint()));
    } else {
        input.push(Span::styled(before, th.base()));
        input.push(Span::styled(
            at,
            Style::default().add_modifier(Modifier::REVERSED),
        ));
        input.push(Span::styled(after, th.base()));
    }
    f.render_widget(
        Paragraph::new(Line::from(input)),
        Rect { height: 1, ..inner },
    );
    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(iw), th.faint())),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
    );

    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(4),
        ..inner
    };
    let visible = list.height as usize;
    let start = p.selected.saturating_sub(visible.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    if p.hits.is_empty() {
        lines.push(Line::from(Span::styled(Key::PaletteNone.tr(), th.dim())));
    }
    for (i, hit) in p.hits.iter().enumerate().skip(start).take(visible) {
        let it = &p.items[hit.item];
        let sel = i == p.selected;
        let row = if sel { th.selected() } else { Style::default() };
        let (glyph, gcolor) = match it.kind {
            PaletteKind::Bookmark => ("★", th.warn),
            PaletteKind::Recent => ("↻", th.text_dim),
            PaletteKind::Place => ("⌂", th.kinds.folder),
            PaletteKind::Disk => ("◉", th.kinds.data),
            PaletteKind::Parent => ("↑", th.text_dim),
            PaletteKind::Folder => ("▸", th.kinds.folder),
            PaletteKind::Typed => ("→", th.accent),
        };
        let tag = it.kind.tag();
        let tag_w = 9;
        let label_w = (iw / 3).clamp(14, 28);
        let detail_w = iw.saturating_sub(4 + label_w + 2 + tag_w + 1);
        let label = display::truncate(&it.label, label_w);
        let mut spans: Vec<Span> = vec![
            Span::styled(if sel { "▎" } else { " " }, th.accent_style().patch(row)),
            Span::styled(format!(" {glyph} "), Style::default().fg(gcolor).patch(row)),
        ];
        // The matched characters of the label are picked out in the accent colour.
        let mut used = 0;
        for (ci, c) in label.chars().enumerate() {
            let hl = hit.label_pos.contains(&ci);
            let st = if hl {
                th.accent_style().add_modifier(Modifier::BOLD)
            } else {
                th.base()
            };
            let st = if sel && !hl {
                st.add_modifier(Modifier::BOLD)
            } else {
                st
            };
            spans.push(Span::styled(c.to_string(), st.patch(row)));
            used += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        }
        spans.push(Span::styled(
            " ".repeat(label_w.saturating_sub(used) + 2),
            row,
        ));
        let detail = if it.detail.width() > detail_w {
            tail(&it.detail, detail_w)
        } else {
            it.detail.clone()
        };
        spans.push(Span::styled(pad(&detail, detail_w), th.dim().patch(row)));
        spans.push(Span::styled(
            pad_left(tag, tag_w + 1),
            th.faint().patch(row),
        ));
        let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
        if drawn < iw {
            spans.push(Span::styled(" ".repeat(iw - drawn), row));
        }
        lines.push(Line::from(spans));
        hits.add(
            Rect {
                y: list.y + (i - start) as u16,
                height: 1,
                ..list
            },
            Target::PaletteRow(i),
        );
    }
    f.render_widget(Paragraph::new(lines), list);

    let foot = Line::from(vec![
        Span::styled("↑↓", th.key()),
        Span::styled(format!(" {}   ", Key::HintMove.tr()), th.dim()),
        Span::styled(super::widgets::key_name("Enter"), th.key()),
        Span::styled(format!(" {}   ", Key::HintGo.tr()), th.dim()),
        Span::styled("Esc", th.key()),
        Span::styled(format!(" {}   ", Key::HintClose.tr()), th.dim()),
        Span::styled(
            fmt::count(p.hits.len() as u64, Key::ResultOne, Key::ResultMany),
            th.faint(),
        ),
    ]);
    f.render_widget(
        Paragraph::new(foot),
        Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        },
    );
}

// -------------------------------------------------------------------------------- others

/// The window of a step that failed (skip, skip all, retry, abort) or of a journal that
/// cannot be written (abort, which is the default, or go on without a way back).
fn draw_failure(
    f: &mut Frame,
    ctx: &Ctx,
    info: &rada_core::events::FailureInfo,
    area: Rect,
    hits: &mut Hits,
) {
    let th = ctx.th;
    let w = 88.min(area.width.saturating_sub(4));
    let msg = fmt::wrap(&info.message, (w as usize).saturating_sub(6));
    let note = if info.journal {
        fmt::wrap(Key::JournalFailNote.tr(), (w as usize).saturating_sub(6))
    } else {
        Vec::new()
    };
    let h = ((msg.len() + note.len()) as u16 + 9).min(area.height.saturating_sub(2));
    let inner = frame(f, th, centered(area, w, h), th.error);
    let mut lines = failure_text(ctx, info, msg, note);
    let (btns, targets) = failure_buttons(th, info.journal);
    hit_spans(hits, inner.x, inner.y + lines.len() as u16, &btns, &targets);
    lines.push(Line::from(btns));
    if !info.journal {
        lines.push(Line::from(Span::styled(Key::AbortKeeps.tr(), th.faint())));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn failure_text(
    ctx: &Ctx,
    info: &rada_core::events::FailureInfo,
    msg: Vec<String>,
    note: Vec<String>,
) -> Vec<Line<'static>> {
    let th = ctx.th;
    let headline = if info.journal {
        Key::JournalFailTitle.tr()
    } else {
        Key::WentWrong.tr()
    };
    let mut lines = vec![
        titled(ctx, Key::ChipError.tr(), Tone::Danger, headline),
        Line::raw(""),
    ];
    for l in msg {
        lines.push(Line::from(Span::styled(l, th.fg(th.error))));
    }
    for l in note {
        lines.push(Line::from(Span::styled(l, th.dim())));
    }
    if info.attempt > 1 {
        lines.push(Line::from(Span::styled(
            tf(Key::AttemptN, &[&info.attempt]),
            th.dim(),
        )));
    }
    lines.push(Line::raw(""));
    lines
}

fn failure_buttons(th: &Theme, journal: bool) -> (Vec<Span<'static>>, Vec<(usize, Target)>) {
    let keys: Vec<(char, Key, bool)> = if journal {
        vec![
            ('a', Key::BtnAbort, true),
            ('c', Key::BtnContinueNoUndo, false),
        ]
    } else {
        vec![
            ('s', Key::BtnSkip, true),
            ('S', Key::BtnSkipAll, false),
            ('r', Key::BtnRetry, false),
            ('a', Key::BtnAbort, false),
        ]
    };
    let mut spans = Vec::new();
    let mut targets = Vec::new();
    for (i, (c, label, main)) in keys.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        targets.push((spans.len(), Target::Key(KeyCode::Char(c))));
        spans.push(button(
            th,
            &c.to_string(),
            label.tr(),
            main.then_some(th.accent),
            true,
        ));
    }
    (spans, targets)
}

fn draw_input(f: &mut Frame, ctx: &Ctx, iv: &InputView, area: Rect) {
    let th = ctx.th;
    let (title, hint): (&str, String) = match &iv.kind {
        InputKind::Rename { .. } => (Key::VRename.tr(), Key::InNewName.tr().into()),
        InputKind::NewDir => (Key::VNewFolder.tr(), Key::InName.tr().into()),
        InputKind::SaveAs => (Key::ChooseTitleSave.tr(), Key::ChooseSaveName.tr().into()),
        InputKind::NewFile => (Key::VNewFile.tr(), Key::InNewFileHint.tr().into()),
        InputKind::NewTag { .. } => (Key::ChipNewTag.tr(), Key::InNewTagHint.tr().into()),
        InputKind::OpenWith { .. } => (Key::ChipOpenWith.tr(), Key::InOpenWithHint.tr().into()),
        InputKind::ExtractTo { .. } => (Key::VExtract.tr(), Key::InExtractHint.tr().into()),
        InputKind::Compress { format, .. } => {
            use rada_core::archive::ArchiveKind::*;
            let all = [
                (Zip, "zip"),
                (TarGz, "tar.gz"),
                (TarZst, "tar.zst"),
                (TarXz, "tar.xz"),
            ];
            let formats = all
                .iter()
                .map(|(k, name)| {
                    if k == format {
                        format!("[{name}]")
                    } else {
                        (*name).to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("  ");
            (Key::VCompress.tr(), tf(Key::InArchiveHint, &[&formats]))
        }
        InputKind::BulkRename { .. } => (Key::VerbBulkRename.tr(), Key::InBulkHint.tr().into()),
    };
    let preview: Vec<(String, String)> = match &iv.kind {
        InputKind::BulkRename { items, rules } => match rada_core::ops::Pattern::parse(&iv.text) {
            Ok(p) => p
                .preview(items, rules)
                .into_iter()
                .take(12)
                .map(|pv| {
                    let from = pv.from.file_name().map(display::name).unwrap_or_default();
                    let to = match pv.to {
                        Ok(n) => display::name(&n),
                        Err(e) => format!("✖ {e}"),
                    };
                    (from, to)
                })
                .collect(),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };
    let h = 10 + preview.len() as u16;
    let r = centered(area, 88, h);
    let inner = frame(f, th, r, th.accent);
    let w = inner.width as usize;
    let (before, at, after) = field_parts(&iv.text, iv.cursor);
    let at = if at.is_empty() { " ".into() } else { at };
    let mut lines = vec![
        Line::from(chip(ctx, title, Tone::Accent)),
        Line::raw(""),
        Line::from(Span::styled(display::truncate(&hint, w), th.dim())),
        Line::from(vec![
            Span::styled("› ", th.accent_style().add_modifier(Modifier::BOLD)),
            Span::styled(before, th.base()),
            Span::styled(at, Style::default().add_modifier(Modifier::REVERSED)),
            Span::styled(after, th.base()),
        ]),
    ];
    match &iv.error {
        Some(e) => lines.push(Line::from(Span::styled(format!("✖ {e}"), th.fg(th.error)))),
        None => {
            if let InputKind::BulkRename { .. } = &iv.kind {
                if let Err(e) = rada_core::ops::Pattern::parse(&iv.text) {
                    lines.push(Line::from(Span::styled(format!("✖ {e}"), th.fg(th.error))));
                } else {
                    lines.push(Line::raw(""));
                }
            } else {
                lines.push(Line::raw(""));
            }
        }
    }
    for (a, b) in preview {
        let half = w.saturating_sub(5) / 2;
        lines.push(Line::from(vec![
            Span::styled(display::truncate(&a, half), th.dim()),
            Span::styled("  →  ", th.accent_style()),
            Span::styled(display::truncate(&b, half), th.base()),
        ]));
    }
    lines.push(Line::raw(""));
    let go = if matches!(iv.kind, InputKind::OpenWith { .. }) {
        Key::BtnOpen.tr()
    } else {
        Key::BtnPlan.tr()
    };
    lines.push(Line::from(vec![
        button(th, "Enter", go, Some(th.accent), true),
        Span::raw("  "),
        button(th, "Esc", Key::BtnCancel.tr(), None, true),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_history(
    f: &mut Frame,
    ctx: &Ctx,
    h: &HistoryView,
    area: Rect,
    spinner: &str,
    hits: &mut Hits,
) {
    let th = ctx.th;
    let rows = h.entries.len().clamp(3, 16) as u16;
    let r = centered(area, 104, rows + 9);
    let inner = frame(f, th, r, th.accent);
    let w = inner.width as usize;
    let mut lines: Vec<Line> = vec![
        Line::from(chip(ctx, Key::ChipHistory.tr(), Tone::Accent)),
        Line::raw(""),
    ];
    if h.loading {
        lines.push(Line::from(Span::styled(
            tf(Key::ReadingJournal, &[&spinner]),
            th.dim(),
        )));
    } else if h.entries.is_empty() {
        lines.push(Line::from(Span::styled(Key::NoOpsYet.tr(), th.dim())));
    }
    let visible = inner.height.saturating_sub(5) as usize;
    let start = h.selected.saturating_sub(visible.saturating_sub(1));
    for (i, e) in h.entries.iter().enumerate().skip(start).take(visible) {
        let sel = i == h.selected;
        let row = if sel { th.selected() } else { Style::default() };
        let (g, gc) = match e.status {
            EntryStatus::Finished(RunStatus::Completed) => ("✔", th.success),
            EntryStatus::Finished(RunStatus::CompletedWithProblems) => ("▲", th.warn),
            EntryStatus::Finished(RunStatus::Aborted)
            | EntryStatus::Finished(RunStatus::Cancelled) => ("■", th.warn),
            EntryStatus::Finished(RunStatus::Interrupted) => ("?", th.warn),
            EntryStatus::Interrupted => ("?", th.error),
        };
        let (tag, tag_color) = match (&e.undo_state, e.reversible) {
            (UndoState::Undone, _) => (Key::HistUndone.tr().to_string(), th.muted),
            (UndoState::Partial { remaining }, _) => (tf(Key::HistPartly, &[remaining]), th.muted),
            _ if e.reversible && e.unrecorded_from.is_some() => {
                (Key::HistPartlyUndoable.tr().to_string(), th.warn)
            }
            (_, false) => (Key::HistPermanent.tr().to_string(), th.error),
            _ if e.undo_steps == 0 => (Key::HistNothing.tr().to_string(), th.muted),
            _ => (Key::HistCanUndo.tr().to_string(), th.accent),
        };
        let when = fmt::clock(e.time);
        let right = format!(" {tag}");
        let title_w = w.saturating_sub(4 + 13 + right.width());
        let mut spans = vec![
            Span::styled(if sel { "▎" } else { " " }, th.accent_style().patch(row)),
            Span::styled(format!("{g} "), Style::default().fg(gc).patch(row)),
            Span::styled(pad(&when, 13), th.dim().patch(row)),
            Span::styled(
                pad(&display::truncate(&e.title, title_w), title_w),
                th.base().patch(row),
            ),
            Span::styled(right, Style::default().fg(tag_color).patch(row)),
        ];
        let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
        if drawn < w {
            spans.push(Span::styled(" ".repeat(w - drawn), row));
        }
        let row_y = inner.y + lines.len() as u16;
        hits.add(
            Rect {
                y: row_y,
                height: 1,
                ..inner
            },
            Target::HistoryRow(i),
        );
        lines.push(Line::from(spans));
    }
    while lines.len() < inner.height.saturating_sub(2) as usize {
        lines.push(Line::raw(""));
    }
    let btns = vec![
        button(
            th,
            "Enter",
            Key::BtnUndoSelected.tr(),
            Some(th.accent),
            true,
        ),
        Span::raw("  "),
        button(th, "Esc", Key::BtnClose.tr(), None, true),
    ];
    hit_spans(
        hits,
        inner.x,
        inner.y + lines.len() as u16,
        &btns,
        &[
            (0, Target::Key(KeyCode::Enter)),
            (2, Target::Key(KeyCode::Esc)),
        ],
    );
    lines.push(Line::from(btns));
    f.render_widget(Paragraph::new(lines), inner);
}

/// Every action with its keys in both schemes side by side, grouped, scrollable.
fn draw_help(f: &mut Frame, ctx: &Ctx, area: Rect, km: &Keymap, mouse: bool, scroll: usize) {
    use crate::keymap::Group;
    let th = ctx.th;
    let w = 104.min(area.width.saturating_sub(4));
    let h = (area.height.saturating_sub(2)).min(46);
    let r = centered(area, w, h);
    let inner = frame(f, th, r, th.accent);
    let iw = inner.width as usize;
    // The widest label sets the column (Italian ones are longer), within reason.
    let label_w = Action::ALL
        .iter()
        .map(|a| a.label().width())
        .max()
        .unwrap_or(30)
        .clamp(30, 40)
        + 2;
    let vim_w = ((iw.saturating_sub(label_w)) / 2).clamp(14, 30);

    let mut lines: Vec<Line> = Vec::new();
    let col = |on: bool, text: String| -> String {
        if !on {
            "—".to_string()
        } else if text.is_empty() {
            String::new()
        } else {
            text
        }
    };
    let vim_on = matches!(
        km.preset,
        crate::keymap::Preset::VimClassic | crate::keymap::Preset::Vim
    );
    let classic_on = matches!(
        km.preset,
        crate::keymap::Preset::VimClassic | crate::keymap::Preset::Classic
    );
    lines.push(Line::from(vec![
        Span::styled(pad("", label_w), th.dim()),
        Span::styled(
            pad("Vim", vim_w),
            th.accent_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            Key::HelpClassic.tr(),
            th.accent_style().add_modifier(Modifier::BOLD),
        ),
    ]));
    for g in Group::ALL {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            g.title(),
            th.dim().add_modifier(Modifier::BOLD),
        )));
        for a in Action::ALL.iter().filter(|a| a.group() == g) {
            let vim = col(vim_on, km.keys_text(*a, Scheme::Vim));
            let classic = col(classic_on, km.keys_text(*a, Scheme::Classic));
            let style = |t: &str| {
                if t == "—" || t.is_empty() {
                    th.faint()
                } else {
                    th.key()
                }
            };
            lines.push(Line::from(vec![
                Span::styled(
                    pad(&display::truncate(a.label(), label_w - 1), label_w),
                    th.base(),
                ),
                Span::styled(pad(&display::truncate(&vim, vim_w - 1), vim_w), style(&vim)),
                Span::styled(
                    display::truncate(&classic, iw.saturating_sub(label_w + vim_w)),
                    style(&classic),
                ),
            ]));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        Key::HelpMouse.tr(),
        th.dim().add_modifier(Modifier::BOLD),
    )));
    let mouse_lines: Vec<&str> = if mouse {
        vec![
            Key::HelpMouse1.tr(),
            Key::HelpMouse2.tr(),
            Key::HelpMouse3.tr(),
            Key::HelpMouse4.tr(),
            Key::HelpMouse5.tr(),
        ]
    } else {
        vec![Key::HelpMouseOff.tr()]
    };
    for l in mouse_lines {
        lines.push(Line::from(Span::styled(l, th.base())));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        Key::HelpAlso.tr(),
        th.dim().add_modifier(Modifier::BOLD),
    )));
    for l in [
        Key::HelpAlso1.tr(),
        Key::HelpAlso2.tr(),
        Key::HelpAlso3.tr(),
        Key::HelpAlso4.tr(),
        Key::HelpAlso5.tr(),
    ] {
        lines.push(Line::from(Span::styled(l, th.base())));
    }
    lines.push(Line::raw(""));
    let quit = km
        .hint(Action::Quit)
        .unwrap_or_else(|| "Ctrl+Q".to_string());
    for l in [
        Key::HelpEnd1.tr().to_string(),
        Key::HelpEnd2.tr().to_string(),
        tf(Key::HelpEnd3, &[&quit]),
        Key::HelpEnd4.tr().to_string(),
        Key::HelpEnd5.tr().to_string(),
        Key::HelpEnd6.tr().to_string(),
    ] {
        lines.push(Line::from(Span::styled(l, th.dim())));
    }

    let rows = inner.height as usize;
    let max_scroll = lines.len().saturating_sub(rows.saturating_sub(2));
    let scroll = scroll.min(max_scroll);
    let mut out: Vec<Line> = vec![
        {
            let mut head = chip(ctx, Key::ChipKeys.tr(), Tone::Accent);
            head.push(Span::styled(
                if max_scroll > 0 {
                    format!("   {}", Key::HelpScrollClose.tr())
                } else {
                    format!("   {}", Key::HelpClose.tr())
                },
                ctx.faint(),
            ));
            Line::from(head)
        },
        Line::raw(""),
    ];
    out.extend(lines.into_iter().skip(scroll).take(rows.saturating_sub(2)));
    f.render_widget(Paragraph::new(out), inner);
}
