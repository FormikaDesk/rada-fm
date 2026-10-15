//! The navigation pane: Home, the pinned folders (with a pin), the disks (a name, a bar of
//! ten segments for how full each is and what is free), the tags and the trash, which sits at
//! the bottom. Narrow, it is a column of icons.
//!
//! The place you are in is a rounded pill, the item the keyboard is on is highlighted more
//! strongly (so focus is visible), and the one under the mouse gets a light highlight.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look, RowTone};
use super::widgets::{pad, pad_left, segment_bar, segment_bar_width};
use crate::app::App;
use crate::fmt;
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::sidebar::{Item, Mode, Origin, Section, current_index};

/// What is drawn on one screen row.
enum Row {
    Title(&'static str),
    Gap,
    Item(usize),
    /// The second line of a disk: its bar and what is free.
    Usage(usize),
}

fn layout(items: &[Item], rail: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut last: Option<Section> = None;
    for (i, item) in items.iter().enumerate() {
        if last != Some(item.section) {
            if last.is_some() {
                rows.push(Row::Gap);
            }
            if let Some(t) = item.section.title() {
                rows.push(Row::Title(t));
            }
            last = Some(item.section);
        }
        rows.push(Row::Item(i));
        if !rail && item.usage.is_some() {
            rows.push(Row::Usage(i));
        }
    }
    rows
}

pub fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect, mode: Mode) {
    let th = app.th.clone();
    let look = Look::of(app);
    let items = app.side_items();
    if items.is_empty() || area.height == 0 {
        return;
    }
    let rail = mode == Mode::Rail;
    let current = current_index(&items, &app.cwd);
    let focused = app
        .side_focus
        .then_some(app.side_cursor.min(items.len() - 1));
    let mut rows = layout(&items, rail);
    // The trash sits at the bottom of the pane when there is room to spare above it.
    let height = area.height as usize;
    let mut trash_row: Option<Row> = None;
    if rows.len() < height
        && let Some(Row::Item(i)) = rows.last()
        && items[*i].section == Section::Trash
    {
        trash_row = rows.pop();
        if matches!(rows.last(), Some(Row::Gap)) {
            rows.pop();
        }
    }

    // Keep the focused (or current) item on screen when the list is longer than the pane.
    let anchor = focused.or(current).unwrap_or(0);
    let anchor_row = rows
        .iter()
        .rposition(|r| matches!(r, Row::Item(i) | Row::Usage(i) if *i == anchor))
        .unwrap_or(0);
    let skip = (anchor_row + 1).saturating_sub(height);

    let w = area.width as usize;
    let pin = icons::ui(app.icons, Glyph::Pin);
    let mut placed: Vec<(u16, &Row)> = rows
        .iter()
        .skip(skip)
        .take(height)
        .enumerate()
        .map(|(y, r)| (y as u16, r))
        .collect();
    if let Some(t) = &trash_row {
        placed.push((area.height - 1, t));
    }
    for (screen_y, row) in placed {
        let r = Rect {
            y: area.y + screen_y,
            height: 1,
            ..area
        };
        match row {
            Row::Gap => {}
            Row::Title(t) => {
                if !rail {
                    f.render_widget(
                        Paragraph::new(Span::styled(
                            format!(" {t}"),
                            th.faint().add_modifier(Modifier::BOLD),
                        )),
                        r,
                    );
                }
            }
            Row::Item(i) | Row::Usage(i) => {
                let item = &items[*i];
                let is_current = current == Some(*i);
                let is_focus = focused == Some(*i);
                let is_hover = app.hover.as_ref() == Some(&item.path);
                let row_style = if is_focus {
                    Style::default().bg(th.cursor)
                } else if is_current {
                    th.current_place()
                } else if is_hover {
                    Style::default().bg(th.mark)
                } else {
                    Style::default()
                };
                let edge = if (is_focus || is_current) && !look.caps {
                    "▎"
                } else {
                    " "
                };
                let marked = is_focus || is_current;
                let mut spans = vec![Span::styled(edge, th.accent_style().patch(row_style))];
                if let Row::Usage(_) = row {
                    // Under the name: the bar and what is free.
                    let free = item
                        .usage
                        .as_ref()
                        .map(|u| fmt::size(u.free))
                        .unwrap_or_default();
                    let free_w = free.width().max(7);
                    let bar_w = segment_bar_width(app.basic);
                    spans.push(Span::styled("   ", row_style));
                    if let Some(u) = &item.usage {
                        spans.extend(segment_bar(&th, u.used, app.basic, row_style.bg));
                    }
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    // What is free is pushed to the right edge, with a column of air before the
                    // border, when the bar leaves room.
                    let room = w.saturating_sub(used + 2);
                    let free_shown = pad_left(&free, room.max(free_w));
                    let _ = bar_w;
                    spans.push(Span::styled(" ", row_style));
                    spans.push(Span::styled(free_shown, th.dim().patch(row_style)));
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    spans.push(Span::styled(" ".repeat(w.saturating_sub(used)), row_style));
                    app.hits.add(r, Target::Place(item.path.clone()));
                    f.render_widget(Paragraph::new(Line::from(spans)), r);
                    if is_focus {
                        soft::tint_row(f.buffer_mut(), r, &th, Look::FLAT, RowTone::Focus);
                    } else if is_current {
                        soft::tint_row(f.buffer_mut(), r, &th, Look::FLAT, RowTone::Place);
                    }
                    continue;
                }
                let icon_color = match item.origin {
                    // A tag's dot keeps its colour (a tag with none is a dim dot).
                    Origin::Tag(color) => th.tag_color(color),
                    _ if marked => th.accent,
                    _ => th.text_dim,
                };
                let glyph = item.glyph(app.icons);
                let mark = if glyph.is_empty() && rail {
                    item.initial()
                } else {
                    glyph.to_string()
                };
                if rail {
                    spans.push(Span::styled(
                        format!(" {mark} "),
                        th.fg(icon_color).patch(row_style),
                    ));
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    spans.push(Span::styled(" ".repeat(w.saturating_sub(used)), row_style));
                } else {
                    let icon = if glyph.is_empty() {
                        " ".to_string()
                    } else {
                        format!(" {glyph} ")
                    };
                    spans.push(Span::styled(icon, th.fg(icon_color).patch(row_style)));
                    let name_style = if marked {
                        th.base().add_modifier(Modifier::BOLD)
                    } else {
                        th.base()
                    };
                    let head: usize = spans.iter().map(|s| s.content.width()).sum();
                    // A pin at the right edge for what is pinned (the user's folders and the
                    // ones pinned by hand).
                    let pinned = matches!(item.section, Section::Pinned)
                        && !matches!(item.origin, Origin::Device(_));
                    let lock = match app.icons {
                        icons::IconSet::Nerd => "\u{f023}",
                        icons::IconSet::UnicodeIcons | icons::IconSet::UnicodeStd => "⊘",
                        icons::IconSet::None => "!",
                    };
                    let pin_w = if item.locked || (pinned && !pin.is_empty()) {
                        3
                    } else {
                        0
                    };
                    // What follows the name (a device that is not mounted) keeps its place: it is
                    // the name that is cut short.
                    let note = item.note().map(|n| format!(" {n}")).unwrap_or_default();
                    let note_w = note.width();
                    let room = w.saturating_sub(head + pin_w + 1 + note_w);
                    let name = rada_core::display::truncate(&item.name, room);
                    spans.push(Span::styled(name, name_style.patch(row_style)));
                    if note_w > 0 {
                        spans.push(Span::styled(note, th.faint().patch(row_style)));
                    }
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    spans.push(Span::styled(
                        " ".repeat(w.saturating_sub(pin_w).saturating_sub(used)),
                        row_style,
                    ));
                    if item.locked {
                        // The system has not allowed this place yet: the same words as in
                        // the window that opens when it is clicked.
                        spans.push(Span::styled(
                            format!(" {lock} "),
                            th.fg(th.warn).patch(row_style),
                        ));
                    } else if pin_w > 0 {
                        spans.push(Span::styled(
                            format!(" {pin} "),
                            th.faint().patch(row_style),
                        ));
                    }
                }
                app.hits.add(r, Target::Place(item.path.clone()));
                f.render_widget(Paragraph::new(Line::from(spans)), r);
                // The place you are in is a rounded pill; the item the keyboard is on is
                // stronger.
                if is_focus {
                    soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Focus);
                } else if is_current {
                    soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Place);
                }
            }
        }
    }
}
