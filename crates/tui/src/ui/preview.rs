//! The preview pane: no frame, just a title and the content, in the same calm style as
//! the list. Images are drawn by the terminal graphics protocol; everything else is text.

use rada_core::display;
use rada_core::ops::LinkState;
use rada_core::preview::Preview;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::widgets::SPIN;
use crate::app::{App, ImageStatus};
use crate::fmt;
use crate::icons;
use crate::theme::{Density, Theme};

pub fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let pad = match th.density {
        Density::Airy => 3,
        Density::Balanced => 2,
        Density::Dense => 1,
    };
    let inner = Rect {
        x: area.x + pad,
        width: area.width.saturating_sub(pad + 1),
        ..area
    };
    if inner.width < 8 || inner.height < 4 {
        return;
    }
    let w = inner.width as usize;

    // Title: the file's icon and name, then one line of facts.
    let (glyph, gcolor) = app
        .current()
        .map(|e| icons::icon(e, app.icons, &th))
        .unwrap_or(("", th.text_dim));
    let name = display::truncate(&app.preview.name, w.saturating_sub(3));
    let mut title = vec![];
    if app.icons != icons::IconSet::None && !glyph.is_empty() {
        title.push(Span::styled(format!("{glyph} "), th.fg(gcolor)));
    }
    title.push(Span::styled(name, th.base().add_modifier(Modifier::BOLD)));
    f.render_widget(
        Paragraph::new(Line::from(title)),
        Rect { height: 1, ..inner },
    );

    let modified = app.current().and_then(|e| e.mtime);
    let ago = fmt::relative(modified, app.now());
    // One or two lines of facts under the name (wrapped, never cut); then the content.
    let summary = summary_of(&app.preview.content, app.preview.hex, &ago);
    let mut sum_lines = fmt::wrap(&summary, w);
    sum_lines.truncate(2);
    let facts_rows = sum_lines.len().max(1) as u16;
    let body = Rect {
        y: inner.y + 1 + facts_rows + 1,
        height: inner.height.saturating_sub(facts_rows + 2),
        ..inner
    };
    let h = body.height as usize;

    if app.preview.image.is_some() {
        f.render_widget(
            Paragraph::new(Span::styled(ago, th.dim())),
            Rect {
                y: inner.y + 1,
                height: 1,
                ..inner
            },
        );
        // The picture sits right under the name and date, flush to the top.
        let top = Rect {
            y: inner.y + 2,
            height: inner.height.saturating_sub(2),
            ..inner
        };
        draw_image_pane(f, app, top);
        return;
    }

    let content = app.preview.content.clone();
    let mut lines: Vec<Line> = Vec::new();
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
                lines.push(Line::from(vec![
                    Span::styled(format!("{:>gw$}  ", i + 1), th.faint()),
                    Span::styled(display::truncate(l, w.saturating_sub(gw + 2)), th.base()),
                ]));
            }
        }
        Some(Preview::Binary(b)) if app.preview.hex => {
            lines.push(Line::from(vec![
                Span::styled("H", th.key()),
                Span::styled("  back to the summary", th.dim()),
            ]));
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
                    "Executable",
                    e.format,
                    th.fg(th.kinds.binary).add_modifier(Modifier::BOLD),
                );
                kv(&mut lines, "", &what.join(" · "), th.base());
                if let Some(i) = &e.interpreter {
                    kv(&mut lines, "Interpreter", i, th.base());
                }
                lines.push(Line::raw(""));
            }
            kv(
                &mut lines,
                "Size",
                &format!("{} · {} bytes", fmt::size(c.size), fmt::thousands(c.size)),
                th.base(),
            );
            kv(&mut lines, "Modified", &fmt::date(c.modified), th.base());
            if c.created.is_some() {
                kv(&mut lines, "Created", &fmt::date(c.created), th.base());
            }
            kv(&mut lines, "Accessed", &fmt::date(c.accessed), th.base());
            if let Some(m) = c.mode {
                kv(
                    &mut lines,
                    "Permissions",
                    &format!("{}  {:04o}", rada_core::preview::mode_string(m), m & 0o7777),
                    th.base(),
                );
            }
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("H", th.key()),
                Span::styled("  show the hex dump", th.dim()),
            ]));
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
                LinkState::ToFile => ("points to a file", th.text_dim),
                LinkState::ToDir => ("points to a folder", th.text_dim),
                LinkState::Broken => ("broken link: the target does not exist", th.error),
                LinkState::Circular => ("circular link", th.error),
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
fn summary_of(content: &Option<Preview>, hex: bool, ago: &str) -> String {
    match content {
        None | Some(Preview::Image(_)) | Some(Preview::Error(_)) => String::new(),
        Some(Preview::Empty) => "Empty file".into(),
        Some(Preview::Special(m)) => m.clone(),
        Some(Preview::Text(t)) => {
            let mut s = format!("{} · {} · {}", t.encoding, fmt::size(t.size), ago);
            if t.truncated {
                s.push_str(" · beginning only");
            }
            if t.long_lines > 0 {
                s.push_str(&format!(" · {} long line(s) cut", t.long_lines));
            }
            s
        }
        Some(Preview::Binary(b)) if hex => format!("hex dump · first {} bytes", b.hex.len() * 16),
        Some(Preview::Binary(b)) => format!("{} · {}", b.card.kind, fmt::size(b.card.size)),
        Some(Preview::Dir(d)) => {
            if d.entries.is_empty() {
                "Empty folder".into()
            } else {
                format!(
                    "{} shown{}",
                    fmt::count(d.entries.len() as u64, "item", "items"),
                    if d.truncated {
                        " · more not listed"
                    } else {
                        ""
                    }
                )
            }
        }
        Some(Preview::Symlink { .. }) => "Symbolic link".into(),
    }
}

fn draw_image_pane(f: &mut Frame, app: &mut App, inner: Rect) {
    use ratatui_image::{Resize, StatefulImage};
    let th = app.th.clone();
    let Some(view) = &app.preview.image else {
        return;
    };
    let info = view.info.clone();
    let note = view.note.clone();
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
    // Under the picture: format, pixels, weight, date.
    let facts = [
        format!(
            "{} · {} · {}",
            info.format,
            match (info.width, info.height) {
                (Some(a), Some(b)) => format!("{a} × {b} px"),
                _ => "size unknown".to_string(),
            },
            fmt::size(info.size)
        ),
        format!("modified {}", fmt::date(info.modified)),
    ];
    // Wrapped, never cut: on a narrow pane "70.7 KiB" must not become "70.…".
    let mut fact_lines: Vec<Line> = facts
        .iter()
        .flat_map(|l| fmt::wrap(l, w))
        .map(|l| Line::from(Span::styled(l, th.base())))
        .collect();
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
            if let Some(ui) = app.image_ui.as_mut() {
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
                        f.render_stateful_widget(StatefulImage::default(), r, &mut ui.proto);
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
            let spin = SPIN[app.spinner % SPIN.len()];
            f.render_widget(
                Paragraph::new(Span::styled(format!("{spin} decoding…"), th.dim())),
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
                Paragraph::new(Span::styled("image rendering is off (--images)", th.dim())),
                pic,
            );
            f.render_widget(Paragraph::new(fact_lines), bottom);
        }
        Kind::Warn(m) => {
            message(f, &th, pic, "image too large to preview", &m, th.warn);
            f.render_widget(Paragraph::new(fact_lines), bottom);
        }
        Kind::Err(m) => {
            message(f, &th, pic, "cannot show this image", &m, th.error);
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
