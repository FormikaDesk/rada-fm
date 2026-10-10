//! The details pane: the name of the item under the cursor with its icon, its type and size,
//! a big preview (a picture, a PDF's first page, text, an archive's summary), its properties
//! and the buttons Open, Open with… and the full-screen preview.
//!
//! The pane is a column on a wide terminal and an overlay on the right of the list when the
//! user asks for it on a narrower one (`Alt+P`).

use rada_core::display;
use rada_core::preview::Preview;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::preview;
use crate::app::App;
use crate::fmt;
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// Label width of the properties table.
const KEY_W: usize = 12;

/// The properties of the item under the cursor, as label and value.
fn properties(app: &App) -> Vec<(&'static str, String)> {
    let Some(e) = app.current() else {
        return Vec::new();
    };
    let when = |t| fmt::date_with_day(t, app.now(), app.date_format, &app.tz);
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    if let Some(img) = &app.preview.image
        && app.preview.name == e.display
        && let (Some(w), Some(h)) = (img.info.width, img.info.height)
    {
        rows.push(("Dimensions", format!("{w} × {h}")));
    }
    if e.is_dir() {
        if let Some(Preview::Dir(d)) = &app.preview.content
            && app.preview.name == e.display
        {
            let n = d.entries.len() as u64;
            let more = if d.truncated { "+" } else { "" };
            rows.push((
                "Contains",
                format!("{}{more}", fmt::count(n, "item", "items")),
            ));
        }
    } else {
        rows.push((
            "Size",
            format!("{} ({} bytes)", fmt::size(e.size), fmt::thousands(e.size)),
        ));
    }
    rows.push(("Modified", when(e.mtime)));
    if e.created.is_some() {
        rows.push(("Created", when(e.created)));
    }
    rows.push((
        "Location",
        fmt::short_path(e.path.parent().unwrap_or(&app.cwd), app.home()),
    ));
    if let Some(m) = e.mode {
        rows.push(("Permissions", rada_core::preview::mode_string(m)));
    }
    rows
}

/// Draw the pane in `area`.
pub fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    if area.width < 16 || area.height < 4 {
        return;
    }
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    let w = inner.width as usize;

    // Several items selected: a summary instead of a preview.
    if app.marked.len() > 1 {
        let size: u64 = app
            .listing
            .all()
            .iter()
            .filter(|e| app.marked.contains(&e.name) && !e.is_dir())
            .map(|e| e.size)
            .sum();
        let lines = vec![
            Line::from(Span::styled(
                fmt::count(app.marked.len() as u64, "item", "items") + " selected",
                th.base().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(fmt::size(size) + " in files", th.dim())),
        ];
        f.render_widget(Paragraph::new(lines), Rect { height: 2, ..inner });
        return;
    }
    let Some(e) = app.current().cloned() else {
        f.render_widget(
            Paragraph::new(Span::styled("Select a file to see its details", th.dim())),
            Rect { height: 1, ..inner },
        );
        return;
    };

    // Header: icon and name, then the type and the size.
    let (glyph, gcolor) = icons::icon(&e, app.icons, &th);
    let mut head = Vec::new();
    if !glyph.is_empty() {
        head.push(Span::styled(format!("{glyph} "), th.fg(gcolor)));
    }
    let room = w.saturating_sub(head.iter().map(|s| s.content.width()).sum::<usize>());
    head.push(Span::styled(
        display::truncate(&e.display, room),
        th.base().add_modifier(Modifier::BOLD),
    ));
    f.render_widget(
        Paragraph::new(Line::from(head)),
        Rect { height: 1, ..inner },
    );
    let sub = if e.is_dir() {
        e.type_label.to_string()
    } else {
        format!("{} · {}", e.type_label, fmt::size(e.size))
    };
    f.render_widget(
        Paragraph::new(Span::styled(display::truncate(&sub, w), th.dim())),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
    );

    // Below the header: the preview, the buttons, then the properties.
    let mut props = properties(app);
    let buttons_h = 2u16;
    let fixed_top = 3u16;
    // Blank lines: under the preview and over the properties.
    let avail = inner.height.saturating_sub(fixed_top + 1 + buttons_h + 1);
    props.truncate(avail.min(props.len() as u16) as usize);
    let props_h = props.len() as u16;
    let gaps = if props_h > 0 { 2 } else { 1 };
    let preview_h = inner
        .height
        .saturating_sub(fixed_top + buttons_h + props_h + gaps);

    if preview_h >= 3 {
        let body = Rect {
            y: inner.y + fixed_top,
            height: preview_h,
            ..inner
        };
        app.hits.add(body, Target::Preview);
        preview::content(f, app, body, false);
    }

    // Buttons, right under the preview: Open across the whole width, rounded where the font
    // allows; under it Open with… and Preview side by side.
    let by = inner.y + fixed_top + preview_h + 1;
    if by + 1 < inner.y + inner.height {
        let set = app.icons;
        let rounded = set == icons::IconSet::Nerd;
        let open_rect = Rect {
            y: by,
            height: 1,
            ..inner
        };
        let on_accent = Style::default()
            .fg(th.on_accent)
            .bg(th.accent)
            .add_modifier(Modifier::BOLD);
        let label = format!("{} Open", icons::ui(set, Glyph::Open));
        let label = label.trim().to_string();
        let mid = w.saturating_sub(2);
        let text = centered_text(&label, mid);
        let line = if rounded {
            Line::from(vec![
                Span::styled("\u{e0b6}", Style::default().fg(th.accent)),
                Span::styled(text, on_accent),
                Span::styled("\u{e0b4}", Style::default().fg(th.accent)),
            ])
        } else {
            Line::from(Span::styled(centered_text(&label, w), on_accent))
        };
        f.render_widget(Paragraph::new(line), open_rect);
        app.hits.add(open_rect, Target::Act(Action::Open));

        let row2 = Rect {
            y: by + 1,
            height: 1,
            ..inner
        };
        let secondary = th.base().patch(Style::default().bg(th.button));
        let preview_label = format!("{} Preview", icons::ui(set, Glyph::Eye));
        let preview_label = preview_label.trim().to_string();
        if e.is_dir() {
            f.render_widget(
                Paragraph::new(Span::styled(centered_text(&preview_label, w), secondary)),
                row2,
            );
            app.hits.add(row2, Target::FullPreview);
        } else {
            let half = (w.saturating_sub(1)) / 2;
            let left = Rect {
                width: half as u16,
                ..row2
            };
            let right = Rect {
                x: row2.x + half as u16 + 1,
                width: (w - half - 1) as u16,
                ..row2
            };
            f.render_widget(
                Paragraph::new(Span::styled(centered_text("Open with…", half), secondary)),
                left,
            );
            f.render_widget(
                Paragraph::new(Span::styled(
                    centered_text(&preview_label, w - half - 1),
                    secondary,
                )),
                right,
            );
            app.hits.add(left, Target::Act(Action::OpenWith));
            app.hits.add(right, Target::FullPreview);
        }
    }

    // Properties, at the bottom.
    let props_y = inner.y + inner.height.saturating_sub(props_h);
    let lines: Vec<Line> = props
        .iter()
        .map(|(k, v)| {
            let room = w.saturating_sub(KEY_W);
            Line::from(vec![
                Span::styled(format!("{k:<KEY_W$}"), th.dim()),
                Span::styled(display::truncate(v, room), th.base()),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            y: props_y,
            height: props_h,
            ..inner
        },
    );
}

/// `text` centred in `width` cells (cut with "…" when it does not fit).
fn centered_text(text: &str, width: usize) -> String {
    let t = display::truncate(text, width);
    let w = t.width();
    let left = width.saturating_sub(w) / 2;
    format!(
        "{}{}{}",
        " ".repeat(left),
        t,
        " ".repeat(width.saturating_sub(w + left))
    )
}

/// The pane as an overlay on the right of `main`: it covers part of the list.
pub fn draw_overlay(f: &mut Frame, app: &mut App, main: Rect) {
    let width = (main.width * 55 / 100).clamp(30, 46).min(main.width);
    let area = Rect {
        x: main.x + main.width - width,
        width,
        ..main
    };
    f.render_widget(Clear, area);
    let th = app.th.clone();
    let rule: Vec<Line> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", th.faint())))
        .collect();
    f.render_widget(Paragraph::new(rule), Rect { width: 1, ..area });
    draw_details(
        f,
        app,
        Rect {
            x: area.x + 1,
            width: area.width - 1,
            ..area
        },
    );
}
