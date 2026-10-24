//! The right column: ANTEPRIMA, with the preview of what is under the cursor and under it its
//! name, its type, rounded labels for type, size and resolution and the buttons, and below it a
//! second, compact box. The second box is DETTAGLI for now; the code is ready to put an ASSISTENTE
//! box in its place (see [`Lower`]).

use rada_core::display;
use rada_core::git::FileState;
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
use crate::theme::Theme;
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
    let th = &app.settings.th;
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
    let th = app.settings.th.clone();
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
        app.frame.hits.add(body, Target::Preview);
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
    let name = display::truncate_name(&e.display, name_room.max(1));
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
        // A label that does not fit whole is left out: half a pill is a broken one.
        let push = |bar: &mut Bar, text: String, hue: Color| {
            let chip = chip(app, &text, hue);
            let chip_w: usize = chip.iter().map(|s| s.content.width()).sum();
            if bar.width() + chip_w > w {
                return;
            }
            for s in chip {
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
        bar.render(f, &mut app.frame.hits, row(1));
    }

    if foot_h >= 5 {
        // "Apri" is a full pill but short; the two others are coloured words.
        let set = app.settings.icons;
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
        // One space between the buttons when the box is narrow, so that the last is not cut.
        let between = if w < 36 { " " } else { "  " };
        // The words that do not fit whole are left out (the keys do the same).
        let open_with = Key::BtnOpenWith.tr();
        let preview = format!("{} {}", icons::ui(set, Glyph::Eye), Key::BtnPreview.tr())
            .trim()
            .to_string();
        let mut words = Vec::new();
        if !e.is_navigable_dir() {
            words.push((
                open_with.to_string(),
                th.hues.cyan,
                Target::Act(Action::OpenWith),
            ));
        }
        words.push((preview, th.hues.pink, Target::FullPreview));
        for (text, hue, target) in words {
            if bar.width() + between.width() + text.width() > w {
                break;
            }
            bar.push(Span::styled(between, th.base()));
            bar.push_hit(
                Span::styled(text, th.fg(hue).add_modifier(Modifier::BOLD)),
                target,
            );
        }
        bar.clip(w);
        bar.render(f, &mut app.frame.hits, row(foot_h - 1));
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
    let th = &app.settings.th;
    let Some(e) = app.current() else {
        return vec![Detail {
            label: "",
            value: vec![Span::styled(Key::SelectToSee.tr(), th.dim())],
            click: None,
        }];
    };
    let text = |s: String| vec![Span::styled(s, th.base())];
    let when = |t| app.settings.date_with_day(t, app.now());
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
    // In a git repository: what git says of it.
    if let Some(git) = app.git_here() {
        let word = |letter: &str, what: Key| format!("{letter} {}", what.tr());
        let value = match git.items.get(&e.display) {
            Some(FileState::Modified) => chip(app, &word("M", Key::GitModified), th.warn),
            Some(FileState::New) => chip(app, &word("?", Key::GitNew), th.success),
            None => vec![Span::styled(Key::GitClean.tr(), th.dim())],
        };
        rows.push(Detail {
            label: Key::GitLabel.tr(),
            value,
            click: None,
        });
    }
    // The link to the history, with the key the keymap really gives it.
    let key = app
        .settings
        .keymap
        .hint(Action::History)
        .unwrap_or_default();
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

/// What of a value of DETTAGLI fits in `room` cells: text is cut with an ellipsis, but a rounded
/// label that does not fit whole goes (with the ones after it), and if none is left its words
/// are written as plain text: half a pill is a broken one.
fn fit_value(spans: Vec<Span<'static>>, room: usize, th: &Theme) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| s.content.width()).sum();
    if total <= room {
        return spans;
    }
    // The parts of a label carry a background of their own; the text of a value does not.
    let chip_part = |s: &Span| s.style.bg.is_some() && s.style.bg != th.base().bg;
    let words: String = spans
        .iter()
        .filter(|s| chip_part(s))
        // (Without the round ends of a pill, which are glyphs of a Nerd Font.)
        .map(|s| {
            s.content
                .trim_matches(|c: char| c.is_whitespace() || ('\u{e0b0}'..='\u{e0bf}').contains(&c))
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut out: Vec<Span<'static>> = Vec::new();
    // Up to the last whole label.
    let (mut used, mut safe) = (0, 0);
    for s in spans {
        let w = s.content.width();
        if used + w <= room {
            used += w;
            let boundary = s.content.as_ref() == " ";
            out.push(s);
            if boundary {
                safe = out.len();
            }
            continue;
        }
        if chip_part(&s) || out.last().is_some_and(chip_part) {
            out.truncate(safe);
        } else if room > used {
            let text = display::truncate(&s.content, room - used);
            out.push(Span::styled(text, s.style));
        }
        break;
    }
    if out.is_empty() && !words.is_empty() {
        let text = display::truncate(&words, room);
        return vec![Span::styled(text, th.base())];
    }
    out
}

fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
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
        for s in fit_value(d.value, room, &th) {
            match &click {
                Some(t) => value.push_hit(s, t.clone()),
                None => value.push(s),
            }
        }
        bar.append(value);
        bar.render(f, &mut app.frame.hits, r);
    }
}
