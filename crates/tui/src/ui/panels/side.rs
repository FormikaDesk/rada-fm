//! The right column: ANTEPRIMA, with the preview of what is under the cursor and under it its
//! name, its type, rounded labels for type, size and resolution and the buttons, and below it a
//! second, compact box. The second box is DETTAGLI for now; the code is ready to put an ASSISTENTE
//! box in its place (see [`Lower`]).

use rada_core::display;
use rada_core::i18n::{Key, tf};
use rada_core::model::Entry;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::fmt;
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;
use crate::ui::preview;
use crate::ui::soft::{self, Look};
use crate::ui::widgets::Bar;

/// What the box under ANTEPRIMA is. Only [`Lower::Details`] is drawn today: a later version
/// puts an assistant here instead, and only [`lower`] and [`draw_lower`] need to learn of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Lower {
    Details,
    Assistant,
}

/// The box that is under ANTEPRIMA right now.
pub fn lower() -> Lower {
    Lower::Details
}

/// Rows the box under ANTEPRIMA wants inside its border.
pub fn lower_rows(app: &App) -> u16 {
    match lower() {
        Lower::Details => details_rows(app).len() as u16,
        Lower::Assistant => 0,
    }
}

/// The title on the top edge of the box under ANTEPRIMA.
pub fn lower_title() -> &'static str {
    match lower() {
        Lower::Details => Key::PanelDetails.tr(),
        Lower::Assistant => "",
    }
}

pub fn draw_lower(f: &mut Frame, app: &mut App, area: Rect) {
    match lower() {
        Lower::Details => draw_details(f, app, area),
        Lower::Assistant => {}
    }
}

/// A rounded label: `text` in `hue` on a light touch of the same colour.
fn chip(app: &App, text: &str, hue: Color) -> Vec<Span<'static>> {
    let th = &app.th;
    let fill = th.chip_fill(hue);
    soft::pill(
        th,
        Look::of(app),
        text,
        fill,
        Style::default().fg(hue),
        Style::default().bg(th.inner),
    )
}

/// The short word for a file's type on its label: the extension in capitals, or the type in
/// words for what has none.
fn type_word(e: &Entry) -> String {
    if e.is_dir() {
        return rada_core::i18n::type_label(&e.type_label).into_owned();
    }
    match std::path::Path::new(&e.name).extension() {
        Some(x) => x.to_string_lossy().to_uppercase(),
        None => rada_core::i18n::type_label(&e.type_label).into_owned(),
    }
}

pub fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    if area.width < 16 || area.height < 3 {
        return;
    }
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    let w = inner.width as usize;

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
            Line::from(Span::styled(
                tf(Key::InFiles, &[&fmt::size(size)]),
                th.dim(),
            )),
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
    app.want_folder_sizes(app.current_index().into_iter());

    // The foot: name and type, the labels, a blank row, the buttons. A short box gives up the
    // labels and the blank row before the name.
    let foot_h: u16 = match inner.height {
        0..=9 => 2,
        10..=13 => 3,
        _ => 5,
    };
    let body_h = inner.height.saturating_sub(foot_h + 1);
    if body_h >= 3 {
        let body = Rect {
            y: inner.y,
            height: body_h,
            ..inner
        };
        app.hits.add(body, Target::Preview);
        preview::content(f, app, body, false);
    }
    let y0 = inner.y + inner.height.saturating_sub(foot_h);
    let row = |n: u16| Rect {
        y: y0 + n,
        height: 1,
        ..inner
    };

    // Name in bold, type on the right.
    let kind = rada_core::i18n::type_label(&e.type_label).into_owned();
    let kind_w = kind.width().min(w / 2);
    let name_room = w.saturating_sub(kind_w + 2);
    let name = display::truncate(&e.display, name_room.max(1));
    let gap = w.saturating_sub(name.width() + kind_w);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(name.clone(), th.base().add_modifier(Modifier::BOLD)),
            Span::styled(" ".repeat(gap), th.base()),
            Span::styled(display::truncate(&kind, kind_w), th.dim()),
        ])),
        row(0),
    );

    if foot_h >= 3 {
        // Rounded labels: type, size, resolution.
        let mut bar = Bar::default();
        let push = |bar: &mut Bar, text: String, hue: Color| {
            for s in chip(app, &text, hue) {
                bar.push(s);
            }
            bar.push(Span::styled(" ", th.base()));
        };
        push(&mut bar, type_word(&e), th.hues.blue);
        if e.is_dir() {
            if let Some(rada_core::preview::Preview::Dir(d)) = &app.preview.content
                && app.preview.name == e.display
            {
                push(
                    &mut bar,
                    format!(
                        "{}{}",
                        fmt::count(d.entries.len() as u64, Key::ItemOne, Key::ItemMany),
                        if d.truncated { "+" } else { "" }
                    ),
                    th.hues.green,
                );
            }
        } else {
            push(&mut bar, fmt::size(e.size), th.hues.green);
        }
        if let Some(img) = &app.preview.image
            && app.preview.name == e.display
            && let (Some(iw), Some(ih)) = (img.info.width, img.info.height)
        {
            push(&mut bar, format!("{iw} × {ih}"), th.hues.pink);
        }
        bar.clip(w);
        bar.render(f, &mut app.hits, row(1));
    }

    if foot_h >= 5 {
        // "Apri" is a full pill but short; the two others are coloured words.
        let set = app.icons;
        let look = Look::of(app);
        let open = format!("{} {}", icons::ui(set, Glyph::Open), Key::BtnOpen.tr());
        let mut bar = Bar::default();
        bar.push_hits(
            soft::pill(
                &th,
                look,
                &format!(" {} ", open.trim()),
                th.accent,
                th.pill(),
                Style::default().bg(th.inner),
            ),
            Target::Act(Action::Open),
        );
        bar.push(Span::styled("  ", th.base()));
        if !e.is_navigable_dir() {
            bar.push_hit(
                Span::styled(
                    Key::BtnOpenWith.tr(),
                    th.fg(th.hues.cyan).add_modifier(Modifier::BOLD),
                ),
                Target::Act(Action::OpenWith),
            );
            bar.push(Span::styled("  ", th.base()));
        }
        let eye = icons::ui(set, Glyph::Eye);
        bar.push_hit(
            Span::styled(
                format!("{eye} {}", Key::BtnPreview.tr()).trim().to_string(),
                th.fg(th.hues.pink).add_modifier(Modifier::BOLD),
            ),
            Target::FullPreview,
        );
        bar.clip(w);
        bar.render(f, &mut app.hits, row(foot_h - 1));
    }
}

/// One row of DETTAGLI: the label, then what is known.
struct Detail {
    label: &'static str,
    value: Vec<Span<'static>>,
    click: Option<Target>,
}

/// The rows of the box, in the order they are given up when it is short: the last goes first.
fn details_rows(app: &App) -> Vec<Detail> {
    let th = &app.th;
    let Some(e) = app.current() else {
        return vec![Detail {
            label: "",
            value: vec![Span::styled(Key::SelectToSee.tr(), th.dim())],
            click: None,
        }];
    };
    let text = |s: String| vec![Span::styled(s, th.base())];
    let when = |t| fmt::date_with_day(t, app.now(), app.date_format, &app.tz);
    let mut rows = vec![Detail {
        label: Key::PropModified.tr(),
        value: text(when(e.mtime)),
        click: None,
    }];
    if e.created.is_some() {
        rows.push(Detail {
            label: Key::PropCreated.tr(),
            value: text(when(e.created)),
            click: None,
        });
    }
    rows.push(Detail {
        label: Key::PropLocation.tr(),
        value: text(fmt::short_path(
            e.path.parent().unwrap_or(&app.cwd),
            app.home(),
        )),
        click: None,
    });
    let permissions = match e.mode {
        Some(m) => (Key::PropPermissions, rada_core::preview::mode_string(m)),
        None => (
            Key::PropAttributes,
            if e.readonly {
                Key::FlagReadOnly.tr().to_string()
            } else {
                Key::FlagNormal.tr().to_string()
            },
        ),
    };
    rows.push(Detail {
        label: permissions.0.tr(),
        value: chip(app, &permissions.1, th.text),
        click: None,
    });
    let mut tags: Vec<Span<'static>> = Vec::new();
    if e.tags.is_empty() {
        tags.push(Span::styled("—", th.faint()));
    }
    for t in &e.tags {
        tags.extend(chip(app, &t.name, th.tag_color(t.color)));
        tags.push(Span::styled(" ", th.base()));
    }
    rows.push(Detail {
        label: Key::PropTags.tr(),
        value: tags,
        click: None,
    });
    // The link to the history, with the key the keymap really gives it.
    let key = app.keymap.hint(Action::History).unwrap_or_default();
    rows.push(Detail {
        label: "",
        value: vec![
            Span::styled(
                Key::ChipHistory.tr(),
                th.fg(th.hues.cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {key}"), th.faint()),
        ],
        click: Some(Target::Act(Action::History)),
    });
    rows
}

fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let label_w = 12;
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    let rows = details_rows(app);
    for (n, d) in rows.into_iter().take(area.height as usize).enumerate() {
        let r = Rect {
            y: area.y + n as u16,
            height: 1,
            ..inner
        };
        let mut bar = Bar::default();
        let click = d.click.clone();
        bar.push(Span::styled(format!("{:<label_w$}", d.label), th.dim()));
        let room = (inner.width as usize).saturating_sub(label_w);
        let mut value = Bar::default();
        for s in d.value {
            match &click {
                Some(t) => value.push_hit(s, t.clone()),
                None => value.push(s),
            }
        }
        value.clip(room);
        bar.append(value);
        bar.render(f, &mut app.hits, r);
    }
}
