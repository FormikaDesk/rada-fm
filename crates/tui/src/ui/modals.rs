//! Windows in front of the interface: the plan, the jump palette, errors, prompts.
//! One window at a time, centred, with everything behind it dimmed.

use rada_core::display;
use rada_core::error::Denied;
use rada_core::health::{Health, Unsure};
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
use super::sheet::{self, Sheet};
use super::widgets::{SPIN, button, centered, field_parts, key_name, pad, pad_left, tail};
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

/// Lines of one style, each text wrapped to `width`, as the body of a [`Sheet`].
fn plain(
    texts: impl Iterator<Item = String>,
    width: usize,
    style: Style,
) -> Vec<(Line<'static>, Option<Target>)> {
    texts
        .flat_map(|t| fmt::wrap(&t, width))
        .map(|l| (Line::from(Span::styled(l, style)), None))
        .collect()
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
    let tw = sheet::text_width(area, 80);
    let key_w = p.rows.iter().map(|(k, _)| k.width()).max().unwrap_or(8) + 2;
    let value_w = tw.saturating_sub(key_w).max(10);
    let mut body = Vec::new();
    for (k, v) in &p.rows {
        for (i, part) in fmt::wrap(v, value_w).into_iter().enumerate() {
            let line = Line::from(vec![
                Span::styled(pad(if i == 0 { k } else { "" }, key_w), th.dim()),
                Span::styled(part, th.base()),
            ]);
            body.push((line, None));
        }
    }
    let sheet = Sheet {
        title: titled(
            ctx,
            Key::ChipProperties.tr(),
            Tone::Accent,
            &display::truncate(&p.title, tw / 2),
        ),
        body,
        scroll: p.scroll,
        buttons: vec![(
            button(th, "Esc", Key::BtnClose.tr(), None, true),
            Target::Key(KeyCode::Esc),
        )],
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 80, sheet, hits);
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

/// The window that lists what went wrong in an operation that finished.
fn draw_result(f: &mut Frame, ctx: &Ctx, r: &ResultView, area: Rect, hits: &mut Hits) {
    let th = ctx.th;
    let tw = sheet::text_width(area, 90);
    let mut body = Vec::new();
    for (k, t) in &r.lines {
        let c = if *k == ToastKind::Error {
            th.error
        } else {
            th.warn
        };
        for (i, l) in fmt::wrap(t, tw.saturating_sub(2)).into_iter().enumerate() {
            let bullet = if i == 0 { "• " } else { "  " };
            body.push((
                Line::from(vec![
                    Span::styled(bullet, th.fg(c)),
                    Span::styled(l, th.fg(c)),
                ]),
                None,
            ));
        }
    }
    let title = display::truncate(&r.title, tw.saturating_sub(10));
    let sheet = Sheet {
        title: titled(ctx, Key::ChipDone.tr(), Tone::Warn, &title),
        body,
        scroll: r.scroll,
        buttons: vec![
            (
                button(th, "u", Key::BtnUndoDone.tr(), Some(th.accent), true),
                Target::Key(KeyCode::Char('u')),
            ),
            (
                button(th, "Esc", Key::BtnClose.tr(), None, true),
                Target::Key(KeyCode::Esc),
            ),
        ],
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 90, sheet, hits);
}

/// The question asked when quitting while an operation runs.
fn draw_confirm_quit(f: &mut Frame, ctx: &Ctx, area: Rect, hits: &mut Hits) {
    let th = ctx.th;
    let tw = sheet::text_width(area, 66);
    let warn = [Key::QuitWarn1.tr(), Key::QuitWarn2.tr()];
    let sheet = Sheet {
        title: titled(
            ctx,
            Key::ChipRunning.tr(),
            Tone::Warn,
            Key::OpInProgress.tr(),
        ),
        body: plain(warn.iter().map(|w| w.to_string()), tw, th.dim()),
        buttons: vec![
            (
                button(th, "y", Key::BtnQuitAnyway.tr(), Some(th.warn), true),
                Target::Key(KeyCode::Char('y')),
            ),
            (
                button(th, "Esc", Key::BtnStay.tr(), None, true),
                Target::Key(KeyCode::Esc),
            ),
        ],
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 66, sheet, hits);
}

/// What the window of a scan shows.
struct ScanLine<'a> {
    what: &'a str,
    spinner: &'a str,
    current: &'a std::path::Path,
    files: u64,
    dirs: u64,
    bytes: u64,
}

/// The window of a scan or a plan being made: what for, how far, and which path it is on.
fn draw_scanning(f: &mut Frame, ctx: &Ctx, area: Rect, scan: &ScanLine, hits: &mut Hits) {
    let th = ctx.th;
    let ScanLine {
        what,
        spinner,
        current,
        files,
        dirs,
        bytes,
    } = *scan;
    let tw = sheet::text_width(area, 66);
    let totals = tf(
        Key::ScanTotals,
        &[
            &fmt::count(files, Key::FileOne, Key::FileMany),
            &fmt::count(dirs, Key::FolderOne, Key::FolderMany),
            &fmt::size(bytes),
        ],
    );
    let sheet = Sheet {
        title: Line::from(vec![
            Span::styled(format!("{spinner} "), th.accent_style()),
            Span::styled(what.to_string(), th.base().add_modifier(Modifier::BOLD)),
        ]),
        body: vec![
            (Line::from(Span::styled(totals, th.base())), None),
            (
                Line::from(Span::styled(tail(&display::path(current), tw), th.dim())),
                None,
            ),
        ],
        after: vec![Line::from(vec![
            Span::styled("Esc", th.key()),
            Span::styled(format!(" {}", Key::HintCancel.tr()), th.dim()),
        ])],
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 66, sheet, hits);
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
            let scan = ScanLine {
                what,
                spinner,
                current,
                files: *files,
                dirs: *dirs,
                bytes: *bytes,
            };
            draw_scanning(f, &ctx, area, &scan, &mut app.frame.hits);
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
        Modal::Input(iv) => draw_input(f, &ctx, iv, area, &mut app.frame.hits),
        Modal::History(h) => draw_history(f, &ctx, h, area, spinner, &mut app.frame.hits),
        Modal::Result(r) => draw_result(f, &ctx, r, area, &mut app.frame.hits),
        Modal::ConfirmQuit => draw_confirm_quit(f, &ctx, area, &mut app.frame.hits),
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
    let tw = sheet::text_width(area, 88);
    let headline = if info.journal {
        Key::JournalFailTitle.tr()
    } else {
        Key::WentWrong.tr()
    };
    let mut body = plain(std::iter::once(info.message.clone()), tw, th.fg(th.error));
    body.extend(failure_notes(th, info, tw));
    if info.attempt > 1 {
        let attempt = tf(Key::AttemptN, &[&info.attempt]);
        body.extend(plain(std::iter::once(attempt), tw, th.dim()));
    }
    let after = if info.journal {
        Vec::new()
    } else {
        vec![Line::from(Span::styled(Key::AbortKeeps.tr(), th.faint()))]
    };
    let sheet = Sheet {
        title: titled(ctx, Key::ChipError.tr(), Tone::Danger, headline),
        body,
        buttons: failure_buttons(th, info.journal, info.denied.is_some()),
        after,
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 88, sheet, hits);
}

/// What is said under the message of a failure: that the journal could not be written, or in
/// words what a refusal of the system means and what to do instead.
fn failure_notes(
    th: &Theme,
    info: &rada_core::events::FailureInfo,
    tw: usize,
) -> Vec<(Line<'static>, Option<Target>)> {
    let mut out = Vec::new();
    if info.journal {
        let note = Key::JournalFailNote.tr().to_string();
        out.extend(plain(std::iter::once(note), tw, th.dim()));
    }
    let advice = info.denied.map(|d| match d {
        Denied::NoRight => Key::FailNoRight,
        Denied::NotPermitted => Key::FailNotPermitted,
    });
    let advice = advice.map(|k| k.tr().to_string());
    out.extend(plain(advice.into_iter(), tw, th.base()));
    out
}

/// The buttons of a failure. When the system refused for want of a right, trying again
/// changes nothing: Retry is still there, last and faint.
fn failure_buttons(th: &Theme, journal: bool, denied: bool) -> Vec<(Span<'static>, Target)> {
    let keys: Vec<(char, Key, bool)> = if journal {
        vec![
            ('a', Key::BtnAbort, true),
            ('c', Key::BtnContinueNoUndo, false),
        ]
    } else if denied {
        vec![
            ('s', Key::BtnSkip, true),
            ('S', Key::BtnSkipAll, false),
            ('a', Key::BtnAbort, false),
            ('r', Key::BtnRetry, false),
        ]
    } else {
        vec![
            ('s', Key::BtnSkip, true),
            ('S', Key::BtnSkipAll, false),
            ('r', Key::BtnRetry, false),
            ('a', Key::BtnAbort, false),
        ]
    };
    keys.into_iter()
        .map(|(c, label, main)| {
            let key = c.to_string();
            let span = if denied && c == 'r' {
                Span::styled(format!(" {}  {} ", key_name(&key), label.tr()), th.faint())
            } else {
                button(th, &key, label.tr(), main.then_some(th.accent), true)
            };
            (span, Target::Key(KeyCode::Char(c)))
        })
        .collect()
}

/// The title of the window and the hint under it, by what is being asked.
fn input_texts(kind: &InputKind) -> (&'static str, String) {
    match kind {
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
    }
}

/// What a bulk rename would do to the first names: from, and to (or the problem). The rows
/// are made in the background; until they come, those of the pattern before stay.
fn input_preview(iv: &InputView) -> Vec<(String, String)> {
    let InputKind::BulkRename { preview, .. } = &iv.kind else {
        return Vec::new();
    };
    if rada_core::ops::Pattern::parse(&iv.text).is_err() {
        return Vec::new();
    }
    preview
        .rows
        .iter()
        .take(12)
        .map(|pv| {
            let from = pv.from.file_name().map(display::name).unwrap_or_default();
            let to = match &pv.to {
                Ok(n) => display::name(n),
                Err(e) => format!("✖ {e}"),
            };
            (from, to)
        })
        .collect()
}

/// Under the field of a bulk rename, when the pattern is good: how far the reading of the
/// files is, or how many items cannot be renamed and how to take them out.
fn bulk_state(th: &Theme, iv: &InputView) -> Option<Span<'static>> {
    let InputKind::BulkRename { preview, .. } = &iv.kind else {
        return None;
    };
    if let Some((done, total)) = preview.reading {
        let text = tf(Key::InBulkReading, &[&done, &total]);
        return Some(Span::styled(text, th.dim()));
    }
    match preview.refused_now(&iv.text) {
        0 => None,
        n => Some(Span::styled(
            format!("✖ {}", tf(Key::InBulkRefused, &[&n])),
            th.fg(th.error),
        )),
    }
}

/// The lines under the title: the hint, the field with its cursor, and what is wrong.
fn input_lead(th: &Theme, iv: &InputView, hint: &str, w: usize) -> Vec<Line<'static>> {
    let (before, at, after) = field_parts(&iv.text, iv.cursor);
    let at = if at.is_empty() { " ".into() } else { at };
    let problem = match &iv.error {
        Some(e) => Some(e.to_string()),
        None if matches!(iv.kind, InputKind::BulkRename { .. }) => {
            rada_core::ops::Pattern::parse(&iv.text)
                .err()
                .map(|e| e.to_string())
        }
        None => None,
    };
    vec![
        Line::from(Span::styled(display::truncate(hint, w), th.dim())),
        Line::from(vec![
            Span::styled("› ", th.accent_style().add_modifier(Modifier::BOLD)),
            Span::styled(before, th.base()),
            Span::styled(at, Style::default().add_modifier(Modifier::REVERSED)),
            Span::styled(after, th.base()),
        ]),
        match problem {
            Some(e) => Line::from(Span::styled(format!("✖ {e}"), th.fg(th.error))),
            None => bulk_state(th, iv).map_or_else(|| Line::raw(""), Line::from),
        },
    ]
}

fn draw_input(f: &mut Frame, ctx: &Ctx, iv: &InputView, area: Rect, hits: &mut Hits) {
    let th = ctx.th;
    let (title, hint) = input_texts(&iv.kind);
    let w = sheet::text_width(area, 88);
    let half = w.saturating_sub(5) / 2;
    let body = input_preview(iv)
        .into_iter()
        .map(|(a, b)| {
            let line = Line::from(vec![
                Span::styled(display::truncate(&a, half), th.dim()),
                Span::styled("  →  ", th.accent_style()),
                Span::styled(display::truncate(&b, half), th.base()),
            ]);
            (line, None)
        })
        .collect();
    let go = if matches!(iv.kind, InputKind::OpenWith { .. }) {
        Key::BtnOpen.tr()
    } else {
        Key::BtnPlan.tr()
    };
    let sheet = Sheet {
        title: Line::from(chip(ctx, title, Tone::Accent)),
        lead: input_lead(th, iv, &hint, w),
        body,
        buttons: vec![
            (
                button(th, "Enter", go, Some(th.accent), true),
                Target::Key(KeyCode::Enter),
            ),
            (
                button(th, "Esc", Key::BtnCancel.tr(), None, true),
                Target::Key(KeyCode::Esc),
            ),
        ],
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 88, sheet, hits);
}

/// The rows of the history, and the row where its entries begin (a line may come first).
fn history_body(
    th: &Theme,
    h: &HistoryView,
    spinner: &str,
    w: usize,
) -> (Vec<(Line<'static>, Option<Target>)>, usize) {
    let mut body: Vec<(Line<'static>, Option<Target>)> = Vec::new();
    if h.loading {
        let reading = tf(Key::ReadingJournal, &[&spinner]);
        body.push((Line::from(Span::styled(reading, th.dim())), None));
    } else if h.entries.is_empty() {
        body.push((Line::from(Span::styled(Key::NoOpsYet.tr(), th.dim())), None));
    }
    let first_entry = body.len();
    for (i, e) in h.entries.iter().enumerate() {
        let health = h.health.get(&e.id);
        let row = history_row(th, e, health, i == h.selected, w);
        body.push((row, Some(Target::HistoryRow(i))));
        // Under the row chosen, what its mark means for this operation.
        if i == h.selected
            && let Some(say) = health.map(|h| health_says(*h))
        {
            let say = plain(std::iter::once(say.into()), w.saturating_sub(4), th.dim());
            body.extend(say.into_iter().map(|(mut line, _)| {
                line.spans.insert(0, Span::raw("    "));
                (line, None)
            }));
        }
    }
    // A list never shrinks to nothing: three rows at least.
    while body.len() < 3 {
        body.push((Line::raw(""), None));
    }
    (body, first_entry)
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
    let w = sheet::text_width(area, 104);
    let (body, first_entry) = history_body(th, h, spinner, w);
    let sheet = Sheet {
        title: Line::from(chip(ctx, Key::ChipHistory.tr(), Tone::Accent)),
        body,
        body_max: Some(16),
        keep: Some(first_entry + h.selected),
        buttons: vec![
            (
                button(
                    th,
                    "Enter",
                    Key::BtnUndoSelected.tr(),
                    Some(th.accent),
                    true,
                ),
                Target::Key(KeyCode::Enter),
            ),
            (
                button(th, "Esc", Key::BtnClose.tr(), None, true),
                Target::Key(KeyCode::Esc),
            ),
        ],
        ..Sheet::default()
    };
    sheet::draw(f, th, area, 104, sheet, hits);
}

/// The sentence under the row chosen: what its mark means for that operation.
fn health_says(health: Health) -> &'static str {
    match health {
        Health::Undoable => Key::HealthSayUndoable.tr(),
        Health::ToCheck { .. } => Key::HealthSayChanged.tr(),
        Health::TooMany => Key::HealthSayTooMany.tr(),
        Health::NotGuaranteed(Unsure::Action) => Key::HealthSayAction.tr(),
        Health::NotGuaranteed(Unsure::Device) => Key::HealthSayDevice.tr(),
        Health::NotGuaranteed(Unsure::Gaps) => Key::HealthSayGaps.tr(),
    }
}

/// The mark of an opinion on an undo (● can be undone, ◐ to check, ○ not guaranteed), what
/// it says, and in which colour. `undo` is what the row says without an opinion.
fn health_tag(th: &Theme, health: Health, undo: String) -> (String, Color) {
    match health {
        Health::Undoable => (format!("● {undo}"), th.accent),
        Health::ToCheck { changed } => {
            let n = i18n_count(changed, Key::HealthToCheckOne, Key::HealthToCheckMany);
            (format!("◐ {n}"), th.warn)
        }
        Health::TooMany => (format!("◐ {}", Key::HealthTooMany.tr()), th.warn),
        Health::NotGuaranteed(_) => (format!("○ {}", Key::HealthNotGuaranteed.tr()), th.warn),
    }
}

/// `one` for 1, `many` (with the number) for the others.
fn i18n_count(n: usize, one: Key, many: Key) -> String {
    if n == 1 {
        one.tr().to_string()
    } else {
        tf(many, &[&n])
    }
}

/// What the journal says about undoing an entry, and in which colour; with an opinion on
/// the undo, its mark.
fn history_tag(
    th: &Theme,
    e: &rada_core::journal::JournalEntry,
    health: Option<&Health>,
) -> (String, Color) {
    let (tag, color) = journal_tag(th, e);
    match health {
        Some(h) => health_tag(th, *h, tag),
        None => (tag, color),
    }
}

/// What the journal says about undoing an entry, and in which colour.
fn journal_tag(th: &Theme, e: &rada_core::journal::JournalEntry) -> (String, Color) {
    match (&e.undo_state, e.reversible) {
        (UndoState::Undone, _) => (Key::HistUndone.tr().to_string(), th.muted),
        (UndoState::Partial { remaining }, _) => (tf(Key::HistPartly, &[remaining]), th.muted),
        _ if e.reversible && e.unrecorded_from.is_some() => {
            (Key::HistPartlyUndoable.tr().to_string(), th.warn)
        }
        (_, false) => (Key::HistPermanent.tr().to_string(), th.error),
        _ if e.undo_steps == 0 => (Key::HistNothing.tr().to_string(), th.muted),
        _ => (Key::HistCanUndo.tr().to_string(), th.accent),
    }
}

/// One entry of the journal: its state, when, what, and whether it can still be undone.
fn history_row(
    th: &Theme,
    e: &rada_core::journal::JournalEntry,
    health: Option<&Health>,
    sel: bool,
    w: usize,
) -> Line<'static> {
    let row = if sel { th.selected() } else { Style::default() };
    let (g, gc) = match e.status {
        EntryStatus::Finished(RunStatus::Completed) => ("✔", th.success),
        EntryStatus::Finished(RunStatus::CompletedWithProblems) => ("▲", th.warn),
        EntryStatus::Finished(RunStatus::Aborted) | EntryStatus::Finished(RunStatus::Cancelled) => {
            ("■", th.warn)
        }
        EntryStatus::Finished(RunStatus::Interrupted) => ("?", th.warn),
        EntryStatus::Interrupted => ("?", th.error),
    };
    let (tag, tag_color) = history_tag(th, e, health);
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
    Line::from(spans)
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
