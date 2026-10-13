//! The details pane: the name of the item under the cursor with its icon, its type and size,
//! a big preview (a picture, a PDF's first page, text, an archive's summary), its properties
//! and the buttons Open, Open with… and the full-screen preview.
//!
//! The pane is a column on a wide terminal and an overlay on the right of the list when the
//! user asks for it on a narrower one (`Alt+P`).

use rada_core::display;
use rada_core::i18n::{Key, tf};
use rada_core::preview::Preview;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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
fn properties(app: &App) -> Vec<(Key, String)> {
    let Some(e) = app.current() else {
        return Vec::new();
    };
    let when = |t| fmt::date_with_day(t, app.now(), app.date_format, &app.tz);
    let mut rows: Vec<(Key, String)> = Vec::new();
    if let Some(img) = &app.preview.image
        && app.preview.name == e.display
        && let (Some(w), Some(h)) = (img.info.width, img.info.height)
    {
        rows.push((Key::PropDimensions, format!("{w} × {h}")));
    }
    if let Some(img) = &app.preview.image
        && app.preview.name == e.display
        && let Some(d) = &img.info.doc
    {
        let pages = match d.pages {
            Some(1) => Key::PageOne.tr().to_string(),
            Some(n) => tf(Key::PageMany, &[&n]),
            None => Key::PagesUnknown.tr().to_string(),
        };
        rows.push((Key::PropPages, pages));
        if let Some(t) = &d.title {
            rows.push((Key::PropTitle, t.clone()));
        }
        if let Some(a) = &d.author {
            rows.push((Key::PropAuthor, a.clone()));
        }
    }
    if e.is_dir() {
        if let Some(Preview::Dir(d)) = &app.preview.content
            && app.preview.name == e.display
        {
            let n = d.entries.len() as u64;
            let more = if d.truncated { "+" } else { "" };
            rows.push((
                Key::PropContains,
                format!("{}{more}", fmt::count(n, Key::ItemOne, Key::ItemMany)),
            ));
        }
    } else {
        rows.push((
            Key::PropSize,
            tf(
                Key::PropSizeBytes,
                &[&fmt::size(e.size), &fmt::thousands(e.size)],
            ),
        ));
    }
    if !e.tags.is_empty() {
        // Drawn with its coloured dots (see `draw_details`); the text is for the width.
        rows.push((
            Key::PropTags,
            e.tags
                .iter()
                .map(|t| format!("● {}", t.name))
                .collect::<Vec<_>>()
                .join("  "),
        ));
    }
    rows.push((Key::PropModified, when(e.mtime)));
    if e.created.is_some() {
        rows.push((Key::PropCreated, when(e.created)));
    }
    rows.push((
        Key::PropLocation,
        fmt::short_path(e.path.parent().unwrap_or(&app.cwd), app.home()),
    ));
    if let Some(m) = e.mode {
        rows.push((Key::PropPermissions, rada_core::preview::mode_string(m)));
    } else {
        // Windows has attributes where Unix has permissions.
        let mut flags: Vec<&str> = Vec::new();
        if e.readonly {
            flags.push(Key::FlagReadOnly.tr());
        }
        if e.hidden {
            flags.push(Key::FlagHidden.tr());
        }
        if e.system {
            flags.push(Key::FlagSystem.tr());
        }
        rows.push((
            Key::PropAttributes,
            if flags.is_empty() {
                Key::FlagNormal.tr().to_string()
            } else {
                flags.join(", ")
            },
        ));
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
                fmt::selected_items(app.marked.len() as u64),
                th.base().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(tf(Key::InFiles, &[&fmt::size(size)]), th.dim())),
        ];
        f.render_widget(Paragraph::new(lines), Rect { height: 2, ..inner });
        return;
    }
    let Some(e) = app.current().cloned() else {
        f.render_widget(
            Paragraph::new(Span::styled(Key::SelectToSee.tr(), th.dim())),
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
    app.want_folder_sizes(app.current_index().into_iter());
    let sub = if e.is_dir() {
        app.folder_summary(&e)
            .unwrap_or_else(|| rada_core::i18n::type_label(&e.type_label).into_owned())
    } else {
        format!("{} · {}", rada_core::i18n::type_label(&e.type_label), fmt::size(e.size))
    };
    f.render_widget(
        Paragraph::new(Span::styled(display::truncate(&sub, w), th.dim())),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
    );

    // Below the header: the preview, the buttons, then the properties, with a blank row
    // between each and one more under the last property, before the status bar.
    let mut props = properties(app);
    let quick_look = app.quick_look_available();
    let fixed_top = 3u16;
    let bottom_gap = 1u16;
    let fit = |buttons_h: u16| {
        let avail = inner
            .height
            .saturating_sub(fixed_top + 1 + buttons_h + 1 + bottom_gap);
        let n = (avail as usize).min(props.len());
        let gaps = if n > 0 { 2 } else { 1 };
        let preview_h = inner
            .height
            .saturating_sub(fixed_top + buttons_h + n as u16 + gaps + bottom_gap);
        (n, preview_h)
    };
    // Open, a blank row, Open with… and Preview side by side; on a Mac Quick Look under them
    // after another blank row, or in the place of Preview when the pane is too short.
    let mut buttons_h = if quick_look { 5u16 } else { 3u16 };
    let mut quick_look_below = quick_look;
    let (mut n, mut preview_h) = fit(buttons_h);
    if quick_look && preview_h < 3 {
        buttons_h = 3;
        quick_look_below = false;
        (n, preview_h) = fit(buttons_h);
    }
    props.truncate(n);
    let props_h = props.len() as u16;

    if preview_h >= 3 {
        let body = Rect {
            y: inner.y + fixed_top,
            height: preview_h,
            ..inner
        };
        app.hits.add(body, Target::Preview);
        preview::content(f, app, body, false);
    }

    // Buttons, right under the preview. All are pills: rounded where the font allows, plain
    // rectangles otherwise.
    let by = inner.y + fixed_top + preview_h + 1;
    let end = inner.y + inner.height;
    if by < end {
        let set = app.icons;
        let rounded = set == icons::IconSet::Nerd;
        let primary = (
            th.accent,
            Style::default()
                .fg(th.on_accent)
                .bg(th.accent)
                .add_modifier(Modifier::BOLD),
        );
        let secondary = (th.button, th.base().patch(Style::default().bg(th.button)));
        let row = |y: u16| Rect {
            y,
            height: 1,
            ..inner
        };
        let open = format!("{} {}", icons::ui(set, Glyph::Open), Key::BtnOpen.tr());
        let open_rect = row(by);
        draw_pill(f, open_rect, open.trim(), primary, rounded);
        app.hits.add(open_rect, Target::Act(Action::Open));

        if by + 2 < end {
            let row2 = row(by + 2);
            let preview_label = format!("{} {}", icons::ui(set, Glyph::Eye), Key::BtnPreview.tr());
            let preview_label = preview_label.trim().to_string();
            // What takes the second place: Preview, or Quick Look when it has no row of its own.
            let (second, second_target) = if quick_look && !quick_look_below {
                (Key::BtnQuickLook.tr().to_string(), Target::Act(Action::QuickLook))
            } else {
                (preview_label, Target::FullPreview)
            };
            if e.is_navigable_dir() {
                draw_pill(f, row2, &second, secondary, rounded);
                app.hits.add(row2, second_target);
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
                draw_pill(f, left, Key::BtnOpenWith.tr(), secondary, rounded);
                draw_pill(f, right, &second, secondary, rounded);
                app.hits.add(left, Target::Act(Action::OpenWith));
                app.hits.add(right, second_target);
            }
        }
        // macOS: the Finder's own preview, under the others.
        if quick_look_below && by + 4 < end {
            let row3 = row(by + 4);
            draw_pill(f, row3, Key::BtnQuickLook.tr(), secondary, rounded);
            app.hits.add(row3, Target::Act(Action::QuickLook));
        }
    }

    // Properties, at the bottom, a blank row above the status bar.
    let props_y = inner.y + inner.height.saturating_sub(props_h + bottom_gap);
    let tags = app.current().map(|e| e.tags.clone()).unwrap_or_default();
    let lines: Vec<Line> = props
        .iter()
        .map(|(k, v)| {
            let room = w.saturating_sub(KEY_W);
            let mut spans = vec![Span::styled(format!("{:<KEY_W$}", k.tr()), th.dim())];
            if *k == Key::PropTags {
                // A dot in each tag's colour, then its name; what does not fit is cut.
                let mut left = room;
                for (i, t) in tags.iter().enumerate() {
                    if left < 3 {
                        break;
                    }
                    if i > 0 {
                        spans.push(Span::raw("  "));
                        left = left.saturating_sub(2);
                    }
                    spans.push(Span::styled(
                        "●",
                        Style::default().fg(th.tag_color(t.color)),
                    ));
                    let name = display::truncate(&t.name, left.saturating_sub(2));
                    left = left.saturating_sub(2 + name.width());
                    spans.push(Span::styled(format!(" {name}"), th.base()));
                }
            } else {
                spans.push(Span::styled(display::truncate(v, room), th.base()));
            }
            Line::from(spans)
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

/// A button: `label` centred on `fill.0`, with `fill.1` as its text style; the ends are
/// rounded when `rounded` (Nerd Font glyphs), else the button is a plain rectangle.
fn draw_pill(f: &mut Frame, area: Rect, label: &str, fill: (Color, Style), rounded: bool) {
    let w = area.width as usize;
    let (color, text_style) = fill;
    let line = if rounded && w > 2 {
        let edge = Style::default().fg(color);
        Line::from(vec![
            Span::styled("\u{e0b6}", edge),
            Span::styled(centered_text(label, w - 2), text_style),
            Span::styled("\u{e0b4}", edge),
        ])
    } else {
        Line::from(Span::styled(centered_text(label, w), text_style))
    };
    f.render_widget(Paragraph::new(line), area);
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
