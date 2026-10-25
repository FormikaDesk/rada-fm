//! The preview pane: no frame, just a title and the content, in the same calm style as
//! the list. Images are drawn by the terminal graphics protocol; everything else is text.

use rada_core::display;
use rada_core::i18n::{Key, tf};
use rada_core::ops::LinkState;
use rada_core::preview::Preview;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::widgets::SPIN;
use crate::app::{App, ImageStatus};
use crate::fmt;
use crate::icons;
use crate::theme::{Density, Theme};

/// The preview over the whole screen: the name, a line of facts, then the content.
pub fn draw_full(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let pad = match th.density {
        Density::Airy => 3,
        Density::Balanced => 2,
        Density::Dense => 1,
    };
    let inner = Rect {
        x: area.x + pad,
        width: area.width.saturating_sub(pad * 2),
        ..area
    };
    if inner.width < 8 || inner.height < 4 {
        return;
    }
    let w = inner.width as usize;

    // Title: the file's icon and name, then one line of facts.
    let (glyph, gcolor) = app
        .tab
        .current()
        .map(|e| icons::icon(e, app.settings.icons, &th))
        .unwrap_or(("", th.text_dim));
    let name = display::truncate_name(&app.preview.name, w.saturating_sub(3));
    let mut title = vec![];
    if app.settings.icons != icons::IconSet::None && !glyph.is_empty() {
        title.push(Span::styled(format!("{glyph} "), th.fg(gcolor)));
    }
    title.push(Span::styled(name, th.base().add_modifier(Modifier::BOLD)));
    f.render_widget(
        Paragraph::new(Line::from(title)),
        Rect { height: 1, ..inner },
    );

    let modified = app.tab.current().and_then(|e| e.mtime);
    let ago = fmt::relative(modified, app.now());
    let summary = summary_of(&app.preview.content, app.preview.hex, &ago);
    let mut sum_lines = fmt::wrap(&summary, w);
    sum_lines.truncate(2);
    let facts_rows = sum_lines.len().max(1) as u16;
    if app.preview.image.is_some() {
        f.render_widget(
            Paragraph::new(Span::styled(ago, th.dim())),
            Rect {
                y: inner.y + 1,
                height: 1,
                ..inner
            },
        );
        let top = Rect {
            y: inner.y + 2,
            height: inner.height.saturating_sub(2),
            ..inner
        };
        draw_image_pane(f, app, top, true);
        return;
    }
    if !summary.is_empty() {
        let rows: Vec<Line> = sum_lines
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), th.dim())))
            .collect();
        f.render_widget(
            Paragraph::new(rows),
            Rect {
                y: inner.y + 1,
                height: facts_rows,
                ..inner
            },
        );
    }
    let body = Rect {
        y: inner.y + 1 + facts_rows + 1,
        height: inner.height.saturating_sub(facts_rows + 2),
        ..inner
    };
    content(f, app, body, true);
}

/// What the file holds: text, a folder's items, a picture, an archive's summary. `full` is
/// the whole-screen preview, which also carries the facts the details pane lists itself.
pub fn content(f: &mut Frame, app: &mut App, body: Rect, full: bool) {
    let th = app.settings.th.clone();
    let w = body.width as usize;
    let h = body.height as usize;
    if w < 4 || h == 0 {
        return;
    }
    if app.preview.image.is_some() {
        draw_image_pane(f, app, body, full);
        return;
    }
    let content = app.preview.content.clone();
    let mut lines: Vec<Line> = Vec::new();
    // In the details pane the header already says what and how big; what is left to say
    // is a few words about an empty or special item.
    if !full {
        let note = match &content {
            Some(Preview::Empty) => Some(Key::PvEmptyFile.tr().to_string()),
            Some(Preview::Dir(d)) if d.entries.is_empty() => {
                Some(Key::PvEmptyFolder.tr().to_string())
            }
            Some(Preview::Special(m)) => Some(m.clone()),
            Some(Preview::Text(t)) if t.truncated => Some(Key::PvBeginningOnly.tr().to_string()),
            _ => None,
        };
        if let Some(n) = note {
            for l in fmt::wrap(&n, w) {
                lines.push(Line::from(Span::styled(l, th.dim())));
            }
            lines.push(Line::raw(""));
        }
    }
    match &content {
        None | Some(Preview::Image(_)) => {}
        Some(Preview::Empty) => {}
        Some(Preview::Error(m)) => {
            for l in fmt::wrap(m, w) {
                lines.push(Line::from(Span::styled(l, th.fg(th.error))));
            }
        }
        Some(Preview::Special(_)) => {}
        Some(Preview::Text(t)) => {
            let gw = t.lines.len().max(1).to_string().len();
            app.preview.scroll = app.preview.scroll.min(t.lines.len().saturating_sub(1));
            for (i, l) in t.lines.iter().enumerate().skip(app.preview.scroll).take(h) {
                let mut spans = vec![Span::styled(format!("{:>gw$}  ", i + 1), th.faint())];
                let room = w.saturating_sub(gw + 2);
                match t.syntax.as_ref().and_then(|s| s.get(i)) {
                    Some(tokens) if !tokens.is_empty() => {
                        spans.extend(highlighted(&th, &display::truncate(l, room), tokens));
                    }
                    _ => spans.push(Span::styled(display::truncate(l, room), th.base())),
                }
                lines.push(Line::from(spans));
            }
        }
        Some(Preview::Binary(b)) if app.preview.hex => {
            lines.push(hex_hint(&th, Key::PvBackToSummary.tr(), w));
            lines.push(Line::raw(""));
            app.preview.scroll = app.preview.scroll.min(b.hex.len().saturating_sub(1));
            for l in b
                .hex
                .iter()
                .skip(app.preview.scroll)
                .take(h.saturating_sub(2))
            {
                lines.push(Line::from(Span::styled(display::truncate(l, w), th.dim())));
            }
        }
        Some(Preview::Binary(b)) => {
            let c = &b.card;
            let kv = |lines: &mut Vec<Line>, key: &str, value: &str, st: Style| {
                kv_lines(lines, &th, key, value, st, w);
            };
            if let Some(e) = &c.exec {
                let mut what = vec![e.kind.clone(), e.arch.clone()];
                if let Some(bits) = e.bits {
                    what.push(format!("{bits}-bit"));
                }
                if let Some(en) = e.endian {
                    what.push(en.to_string());
                }
                kv(
                    &mut lines,
                    Key::PvExecutable.tr(),
                    e.format,
                    th.fg(th.kinds.binary).add_modifier(Modifier::BOLD),
                );
                kv(&mut lines, "", &what.join(" · "), th.base());
                if let Some(i) = &e.interpreter {
                    kv(&mut lines, Key::PvInterpreter.tr(), i, th.base());
                }
                lines.push(Line::raw(""));
            }
            if full {
                kv(
                    &mut lines,
                    Key::PvSize.tr(),
                    &tf(
                        Key::PvSizeBytes,
                        &[&fmt::size(c.size), &fmt::thousands(c.size)],
                    ),
                    th.base(),
                );
                kv(
                    &mut lines,
                    Key::PvModified.tr(),
                    &fmt::date(c.modified),
                    th.base(),
                );
                if c.created.is_some() {
                    kv(
                        &mut lines,
                        Key::PvCreated.tr(),
                        &fmt::date(c.created),
                        th.base(),
                    );
                }
                kv(
                    &mut lines,
                    Key::PvAccessed.tr(),
                    &fmt::date(c.accessed),
                    th.base(),
                );
                if let Some(m) = c.mode {
                    kv(
                        &mut lines,
                        Key::PvPermissions.tr(),
                        &format!("{}  {:04o}", rada_core::preview::mode_string(m), m & 0o7777),
                        th.base(),
                    );
                }
                lines.push(Line::raw(""));
            }
            lines.push(hex_hint(&th, Key::PvShowHex.tr(), w));
        }
        Some(Preview::Archive(a)) => {
            let kv = |lines: &mut Vec<Line>, key: &str, value: &str, st: Style| {
                kv_lines(lines, &th, key, value, st, w);
            };
            kv(&mut lines, Key::PvFormat.tr(), &a.format, th.base());
            if let Some(why) = &a.problem {
                lines.push(Line::raw(""));
                for l in fmt::wrap(why, w) {
                    lines.push(Line::from(Span::styled(l, th.fg(th.error))));
                }
            } else {
                let lower = if a.complete { "" } else { Key::PvAtLeast.tr() };
                kv(
                    &mut lines,
                    Key::PvContains.tr(),
                    &format!("{lower}{}", archive_counts(a.files, a.dirs, a.symlinks)),
                    th.base(),
                );
                // The exact number of bytes only where it fits on the line with its label:
                // the pane is narrow, and a value that wraps is hard to read.
                let packed = tf(
                    Key::PvSizeBytes,
                    &[&fmt::size(a.packed), &fmt::thousands(a.packed)],
                );
                let packed = if Key::PvPacked.tr().width() + 2 + packed.width() <= w {
                    packed
                } else {
                    fmt::size(a.packed)
                };
                kv(&mut lines, Key::PvPacked.tr(), &packed, th.base());
                kv(
                    &mut lines,
                    Key::PvUnpacked.tr(),
                    &format!("{lower}{}", fmt::size(a.bytes)),
                    th.base(),
                );
                if a.packed > 0 && a.bytes > a.packed {
                    kv(
                        &mut lines,
                        Key::PvRatio.tr(),
                        &tf(
                            Key::PvRatioValue,
                            &[&rada_core::i18n::decimal(&format!(
                                "{:.1}",
                                a.bytes as f64 / a.packed as f64
                            ))],
                        ),
                        th.dim(),
                    );
                }
                if let Some(n) = &a.note {
                    for l in fmt::wrap(n, w) {
                        lines.push(Line::from(Span::styled(l, th.fg(th.warn))));
                    }
                }
                if a.encrypted > 0 {
                    lines.push(Line::from(Span::styled(
                        if a.encrypted == 1 {
                            Key::PvPwOne.tr().to_string()
                        } else {
                            tf(Key::PvPwMany, &[&fmt::thousands(a.encrypted)])
                        },
                        th.fg(th.warn),
                    )));
                }
                lines.push(Line::raw(""));
                let extract_key = app
                    .key_for(crate::keymap::Action::ExtractHere)
                    .unwrap_or_else(|| "e".into());
                // The two keys on one line where they fit, on two where the pane is narrow.
                let open = (
                    super::widgets::key_name("Enter"),
                    format!("  {}", Key::PvOpenLikeFolder.tr()),
                );
                let extract = (extract_key.clone(), format!("  {}", Key::PvExtract.tr()));
                let one_line =
                    open.0.width() + open.1.width() + 3 + extract.0.width() + extract.1.width();
                if one_line <= w {
                    lines.push(Line::from(vec![
                        Span::styled(open.0, th.key()),
                        Span::styled(format!("{}   ", open.1), th.dim()),
                        Span::styled(extract.0, th.key()),
                        Span::styled(extract.1, th.dim()),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(open.0, th.key()),
                        Span::styled(open.1, th.dim()),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled(extract.0, th.key()),
                        Span::styled(extract.1, th.dim()),
                    ]));
                }
                lines.push(Line::raw(""));
                let room = h.saturating_sub(lines.len());
                app.preview.scroll = app.preview.scroll.min(a.first.len().saturating_sub(1));
                for (name, is_dir) in a.first.iter().skip(app.preview.scroll).take(room) {
                    let (g, c) = if *is_dir {
                        ("▸ ", th.kinds.folder)
                    } else {
                        ("· ", th.muted)
                    };
                    lines.push(Line::from(vec![
                        Span::styled(g, th.fg(c)),
                        Span::styled(
                            display::truncate(name, w.saturating_sub(2)),
                            th.fg(if *is_dir { th.kinds.folder } else { th.text }),
                        ),
                    ]));
                }
                if a.more {
                    lines.push(Line::from(Span::styled("…", th.faint())));
                }
            }
        }
        Some(Preview::Dir(d)) => {
            app.preview.scroll = app.preview.scroll.min(d.entries.len().saturating_sub(1));
            for (name, is_dir) in d.entries.iter().skip(app.preview.scroll).take(h) {
                let (g, c) = if *is_dir {
                    ("▸ ", th.kinds.folder)
                } else {
                    ("· ", th.muted)
                };
                lines.push(Line::from(vec![
                    Span::styled(g, th.fg(c)),
                    Span::styled(
                        display::truncate(name, w.saturating_sub(2)),
                        th.fg(if *is_dir { th.kinds.folder } else { th.text }),
                    ),
                ]));
            }
        }
        Some(Preview::Symlink {
            target,
            state,
            inner: inner_pv,
        }) => {
            lines.push(Line::from(vec![
                Span::styled("→ ", th.fg(th.kinds.link)),
                Span::styled(display::path(target), th.fg(th.kinds.link)),
            ]));
            let (txt, col) = match state {
                LinkState::ToFile => (Key::PvPointsToFile.tr(), th.text_dim),
                LinkState::ToDir => (Key::PvPointsToFolder.tr(), th.text_dim),
                LinkState::Broken => (Key::PvBrokenLink.tr(), th.error),
                LinkState::Circular => (Key::PvCircularLink.tr(), th.error),
            };
            lines.push(Line::from(Span::styled(txt, th.fg(col))));
            lines.push(Line::raw(""));
            match inner_pv.as_deref() {
                Some(Preview::Text(t)) => {
                    for l in t.lines.iter().take(h.saturating_sub(3)) {
                        lines.push(Line::from(Span::styled(display::truncate(l, w), th.base())));
                    }
                }
                Some(Preview::Dir(d)) => {
                    for (n, _) in d.entries.iter().take(h.saturating_sub(3)) {
                        lines.push(Line::from(Span::styled(display::truncate(n, w), th.base())));
                    }
                }
                _ => {}
            }
        }
    }
    f.render_widget(Paragraph::new(lines), body);
}

/// `Label        value`, the value wrapping onto aligned continuation lines.
fn kv_lines(lines: &mut Vec<Line>, th: &Theme, key: &str, value: &str, st: Style, w: usize) {
    const KEY_W: usize = 13;
    let room = w.saturating_sub(KEY_W).max(8);
    for (i, part) in fmt::wrap(value, room).into_iter().enumerate() {
        let label = if i == 0 { key } else { "" };
        lines.push(Line::from(vec![
            Span::styled(format!("{label:<KEY_W$}"), th.dim()),
            Span::styled(part, st),
        ]));
    }
}

/// The one-line (possibly wrapped) description under the file name.
/// The key that switches between the summary of a binary file and its hex dump, and what it does.
fn hex_hint(th: &Theme, what: &str, width: usize) -> Line<'static> {
    let what = display::truncate(&format!("  {what}"), width.saturating_sub(1));
    Line::from(vec![
        Span::styled("H", th.key()),
        Span::styled(what, th.dim()),
    ])
}

fn summary_of(content: &Option<Preview>, hex: bool, ago: &str) -> String {
    match content {
        None | Some(Preview::Image(_)) | Some(Preview::Error(_)) => String::new(),
        Some(Preview::Empty) => Key::PvEmptyFile.tr().into(),
        Some(Preview::Special(m)) => m.clone(),
        Some(Preview::Text(t)) => {
            let mut s = format!("{} · {} · {}", t.encoding, fmt::size(t.size), ago);
            if t.truncated {
                s.push_str(Key::PvBeginningOnlyDot.tr());
            }
            if t.long_lines > 0 {
                s.push_str(&tf(Key::PvLongLinesCut, &[&t.long_lines]));
            }
            s
        }
        Some(Preview::Binary(b)) if hex => tf(Key::PvHexDump, &[&(b.hex.len() * 16)]),
        Some(Preview::Binary(b)) => format!("{} · {}", b.card.kind, fmt::size(b.card.size)),
        Some(Preview::Dir(d)) => {
            if d.entries.is_empty() {
                Key::PvEmptyFolder.tr().into()
            } else {
                tf(
                    if d.entries.len() == 1 {
                        Key::PvShownOne
                    } else {
                        Key::PvShownMany
                    },
                    &[
                        &fmt::count(d.entries.len() as u64, Key::ItemOne, Key::ItemMany),
                        &if d.truncated {
                            Key::PvMoreNotListed.tr()
                        } else {
                            ""
                        },
                    ],
                )
            }
        }
        Some(Preview::Symlink { .. }) => Key::PvSymlink.tr().into(),
        Some(Preview::Archive(a)) if a.problem.is_some() => a.format.clone(),
        Some(Preview::Archive(a)) => format!(
            "{} · {}{} · {}",
            a.format,
            if a.complete { "" } else { Key::PvAtLeast.tr() },
            archive_counts(a.files, a.dirs, a.symlinks),
            fmt::size(a.packed)
        ),
    }
}

/// "12 files, 3 folders" without the parts that are zero.
fn archive_counts(files: u64, dirs: u64, links: u64) -> String {
    let mut parts = Vec::new();
    if files > 0 || (dirs == 0 && links == 0) {
        parts.push(fmt::count(files, Key::FileOne, Key::FileMany));
    }
    if dirs > 0 {
        parts.push(fmt::count(dirs, Key::FolderOne, Key::FolderMany));
    }
    if links > 0 {
        parts.push(fmt::count(links, Key::LinkOne, Key::LinkMany));
    }
    parts.join(", ")
}

fn draw_image_pane(f: &mut Frame, app: &mut App, inner: Rect, with_facts: bool) {
    use ratatui_image::{Resize, StatefulImage};
    let th = app.settings.th.clone();
    let Some(view) = &app.preview.image else {
        return;
    };
    let info = view.info.clone();
    let note = view.note.clone();
    let fallback_text = view.text.clone();
    enum Kind {
        Decoding,
        Shown,
        Plain,
        Warn(String),
        Err(String),
    }
    let kind = match &view.status {
        ImageStatus::Decoding => Kind::Decoding,
        ImageStatus::Shown => Kind::Shown,
        ImageStatus::NoGraphics => Kind::Plain,
        ImageStatus::TooLarge(m) => Kind::Warn(m.clone()),
        ImageStatus::Failed(m) => Kind::Err(m.clone()),
    };
    let w = inner.width as usize;
    // Under the picture: format, pixels (or pages), weight, date.
    let mut facts = vec![if info.format == "PDF" {
        let pages = match info.doc.as_ref().and_then(|d| d.pages) {
            Some(1) => Key::PageOne.tr().to_string(),
            Some(n) => tf(Key::PageMany, &[&n]),
            None => Key::PvPagesUnknown.tr().to_string(),
        };
        tf(Key::PvPdfPages, &[&pages, &fmt::size(info.size)])
    } else {
        format!(
            "{} · {} · {}",
            info.format,
            match (info.width, info.height) {
                (Some(a), Some(b)) => format!("{a} × {b} px"),
                _ => Key::PvSizeUnknown.tr().to_string(),
            },
            fmt::size(info.size)
        )
    }];
    if let Some(d) = &info.doc {
        if let Some(t) = &d.title {
            facts.push(tf(Key::PvTitleLine, &[t]));
        }
        if let Some(a) = &d.author {
            facts.push(tf(Key::PvAuthorLine, &[a]));
        }
    }
    facts.push(tf(Key::PvModifiedLine, &[&fmt::date(info.modified)]));
    // Wrapped, never cut: on a narrow pane "70.7 KB" must not become "70.…".
    // In the details pane the facts are the properties listed below the picture.
    let mut fact_lines: Vec<Line> = if with_facts {
        facts
            .iter()
            .flat_map(|l| fmt::wrap(l, w))
            .map(|l| Line::from(Span::styled(l, th.base())))
            .collect()
    } else {
        Vec::new()
    };
    if let Some(n) = &note {
        for l in fmt::wrap(n, w) {
            fact_lines.push(Line::from(Span::styled(l, th.fg(th.warn))));
        }
    }
    let facts_h = fact_lines.len() as u16;
    let bottom = Rect {
        y: inner.y + inner.height.saturating_sub(facts_h),
        height: facts_h.min(inner.height),
        ..inner
    };
    let pic = Rect {
        height: inner.height.saturating_sub(facts_h),
        ..inner
    };

    match kind {
        Kind::Shown => {
            let mut facts_at = bottom;
            if let Some(ui) = app.preview.image_ui.as_mut() {
                let target = ratatui::layout::Size::new(pic.width, pic.height);
                match ui.proto.size_for(Resize::Fit(None), target) {
                    Some(sz) => {
                        // Flush to the top, right under the name and date.
                        let y0 = inner.y;
                        let r = Rect {
                            x: pic.x,
                            y: y0,
                            width: sz.width.min(pic.width),
                            height: sz.height.min(pic.height),
                        };
                        // A picture the terminal draws lies over the cells: where a popup is
                        // drawn it is left out (the cells under it stay as they are).
                        let covered = ui.is_graphics()
                            && app.frame.popup_covers.iter().any(|c| c.intersects(r));
                        if !covered {
                            f.render_stateful_widget(StatefulImage::default(), r, &mut ui.proto);
                        }
                        // The facts follow the picture directly, no empty row between.
                        facts_at = Rect {
                            x: inner.x,
                            y: r.y + r.height,
                            width: inner.width,
                            height: facts_h,
                        };
                    }
                    None => {
                        f.render_widget(
                            Paragraph::new(Span::styled("…", th.dim())),
                            Rect {
                                y: pic.y + pic.height / 2,
                                height: 1,
                                ..pic
                            },
                        );
                    }
                }
            }
            if facts_at.y + facts_at.height <= inner.y + inner.height {
                f.render_widget(Paragraph::new(fact_lines), facts_at);
            }
        }
        Kind::Decoding => {
            let spin = SPIN[app.redraw.spinner % SPIN.len()];
            f.render_widget(
                Paragraph::new(Span::styled(tf(Key::PvDecoding, &[&spin]), th.dim())),
                Rect {
                    y: pic.y + pic.height / 3,
                    height: 1,
                    ..pic
                },
            );
            f.render_widget(Paragraph::new(fact_lines), bottom);
        }
        Kind::Plain => {
            f.render_widget(
                Paragraph::new(Span::styled(Key::PvImagesOff.tr(), th.dim())),
                pic,
            );
            f.render_widget(Paragraph::new(fact_lines), bottom);
        }
        Kind::Warn(m) => {
            let what = if info.format == "PDF" {
                Key::PvPdfTooLarge.tr()
            } else {
                Key::PvImageTooLarge.tr()
            };
            message(f, &th, pic, what, &m, th.warn);
            f.render_widget(Paragraph::new(fact_lines), bottom);
        }
        Kind::Err(m) => {
            let what = if info.format == "PDF" {
                Key::PvCannotPdf.tr()
            } else {
                Key::PvCannotImage.tr()
            };
            if fallback_text.is_empty() {
                message(f, &th, pic, what, &m, th.error);
            } else {
                // The page could not be drawn, but its words can be read.
                let mut lines = vec![
                    Line::from(Span::styled(tf(Key::PvFirstPageText, &[&m]), th.dim())),
                    Line::raw(""),
                ];
                let room = pic.height.saturating_sub(2) as usize;
                for l in fallback_text.iter().take(room) {
                    lines.push(Line::from(Span::styled(
                        display::truncate(l, pic.width as usize),
                        th.base(),
                    )));
                }
                f.render_widget(Paragraph::new(lines), pic);
            }
            f.render_widget(Paragraph::new(fact_lines), bottom);
        }
    }
}

fn message(f: &mut Frame, th: &Theme, area: Rect, head: &str, text: &str, color: Color) {
    let mut lines = vec![
        Line::from(Span::styled(
            head.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
    ];
    for l in fmt::wrap(text, area.width as usize) {
        lines.push(Line::from(Span::styled(l, th.fg(color))));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// A line of a text file as spans, each coloured part in the style of its kind. `shown` is the
/// line as far as it fits (it may end in an ellipsis that no token covers).
fn highlighted<'a>(
    th: &Theme,
    shown: &str,
    tokens: &[rada_core::preview::syntax::Token],
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let chars: Vec<char> = shown.chars().collect();
    let mut at = 0;
    let text = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    for t in tokens {
        let (start, end) = (t.start.min(chars.len()), t.end.min(chars.len()));
        if start < at || start >= end {
            continue;
        }
        if start > at {
            spans.push(Span::styled(text(at, start), th.base()));
        }
        spans.push(Span::styled(text(start, end), th.syntax(t.kind)));
        at = end;
    }
    if at < chars.len() {
        spans.push(Span::styled(text(at, chars.len()), th.base()));
    }
    spans
}
