//! The sidebar: standard places, bookmarks and disks. Full width it has titles, names and
//! a small bar for each disk; narrow it is a column of icons (standard places only).
//! The place you are in is highlighted, the item the keyboard is on is highlighted more
//! strongly (so focus is visible), and the one under the mouse gets a light highlight.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::widgets::{bar_spans, pad};
use crate::app::App;
use crate::hits::Target;
use crate::sidebar::{Item, Mode, Section, current_index};

/// What is drawn on one screen row.
enum Row {
    Title(&'static str),
    Gap,
    Item(usize),
}

fn layout(items: &[Item]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut last: Option<Section> = None;
    for (i, item) in items.iter().enumerate() {
        if last != Some(item.section) {
            if last.is_some() {
                rows.push(Row::Gap);
            }
            rows.push(Row::Title(item.section.title()));
            last = Some(item.section);
        }
        rows.push(Row::Item(i));
    }
    rows
}

pub fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect, mode: Mode) {
    let th = app.th.clone();
    let items = app.side_items();
    if items.is_empty() || area.height == 0 {
        return;
    }
    let current = current_index(&items, &app.cwd);
    let focused = app
        .side_focus
        .then_some(app.side_cursor.min(items.len() - 1));
    let rows = layout(&items);

    // Keep the focused (or current) item on screen when the list is longer than the bar.
    let anchor = focused.or(current).unwrap_or(0);
    let anchor_row = rows
        .iter()
        .position(|r| matches!(r, Row::Item(i) if *i == anchor))
        .unwrap_or(0);
    let height = area.height as usize;
    let skip = (anchor_row + 1).saturating_sub(height);

    let w = area.width as usize;
    for (screen_y, row) in rows.iter().skip(skip).take(height).enumerate() {
        let r = Rect {
            y: area.y + screen_y as u16,
            height: 1,
            ..area
        };
        match row {
            Row::Gap => {}
            Row::Title(t) => {
                if mode == Mode::Full {
                    f.render_widget(
                        Paragraph::new(Span::styled(
                            format!(" {t}"),
                            th.faint().add_modifier(Modifier::BOLD),
                        )),
                        r,
                    );
                }
            }
            Row::Item(i) => {
                let item = &items[*i];
                let is_current = current == Some(*i);
                let is_focus = focused == Some(*i);
                let is_hover = app.hover.as_ref() == Some(&item.path);
                // Focus is the strongest, then the current place, then the mouse.
                let row_style = if is_focus {
                    Style::default().bg(th.cursor)
                } else if is_current || is_hover {
                    th.chip_row()
                } else {
                    Style::default()
                };
                let edge = if is_focus || is_current { "▎" } else { " " };
                let icon_color = if is_focus || is_current {
                    th.accent
                } else {
                    th.text_dim
                };
                let glyph = item.glyph(app.icons);
                let mark = if glyph.is_empty() && mode == Mode::Rail {
                    item.initial()
                } else {
                    glyph.to_string()
                };
                let mut spans = vec![Span::styled(edge, th.accent_style().patch(row_style))];
                match mode {
                    Mode::Rail => {
                        spans.push(Span::styled(
                            format!(" {mark} "),
                            th.fg(icon_color).patch(row_style),
                        ));
                        let used: usize = spans.iter().map(|s| s.content.width()).sum();
                        spans.push(Span::styled(" ".repeat(w.saturating_sub(used)), row_style));
                    }
                    _ => {
                        let icon = if glyph.is_empty() {
                            " ".to_string()
                        } else {
                            format!(" {glyph} ")
                        };
                        spans.push(Span::styled(icon, th.fg(icon_color).patch(row_style)));
                        let name_style = if is_focus || is_current {
                            th.base().add_modifier(Modifier::BOLD)
                        } else {
                            th.base()
                        };
                        let head: usize = spans.iter().map(|s| s.content.width()).sum();
                        match &item.usage {
                            Some(u) => {
                                // Name, then a small bar of how full the disk is.
                                let bar_w = 5;
                                let room = w.saturating_sub(head + bar_w + 2);
                                let name = rada_core::display::truncate(&item.name, room);
                                spans.push(Span::styled(
                                    pad(&name, room),
                                    name_style.patch(row_style),
                                ));
                                spans.push(Span::styled(" ", row_style));
                                spans.extend(bar_spans(
                                    &th,
                                    u.used,
                                    bar_w,
                                    th.accent,
                                    row_style.bg,
                                ));
                                spans.push(Span::styled(" ", row_style));
                            }
                            None => {
                                let room = w.saturating_sub(head + 1);
                                let name = rada_core::display::truncate(&item.name, room);
                                spans.push(Span::styled(
                                    pad(&name, w.saturating_sub(head)),
                                    name_style.patch(row_style),
                                ));
                            }
                        }
                    }
                }
                app.hits.add(r, Target::Place(item.path.clone()));
                f.render_widget(Paragraph::new(Line::from(spans)), r);
            }
        }
    }
}
