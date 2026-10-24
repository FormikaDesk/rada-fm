//! The NAV box: the tree of folders from Home (an arrow for each folder that has folders in
//! it, a coloured icon, the number of items on the right), the favourites, the seven tag
//! colours as dots in a row, and the trash.
//!
//! The folder you are in is a pill; the row the keyboard is on is a stronger one.

use rada_core::platform::{PlaceKind, TagColor as Hue};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::hits::Target;
use crate::icons::{self, Glyph, Mark};
use crate::panels::NavKind;
use crate::sidebar::Origin;
use crate::theme::Theme;
use crate::ui::soft::{self, Look, RowTone};
use crate::ui::widgets::pad;

/// The colour of the icon of a standard place: each has its own hue.
fn place_hue(th: &Theme, kind: PlaceKind) -> Color {
    match kind {
        PlaceKind::Home | PlaceKind::Documents => th.hues.blue,
        PlaceKind::Desktop => th.hues.cyan,
        PlaceKind::Downloads => th.hues.green,
        PlaceKind::Pictures | PlaceKind::Videos => th.hues.pink,
        PlaceKind::Music => th.hues.orange,
        _ => th.hues.yellow,
    }
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let look = Look::of(app);
    // The rows of the way shown are as many as fit, so the box's height comes first.
    app.frame.nav_area = area;
    let rows = app.nav_rows();
    if rows.is_empty() || area.height == 0 {
        return;
    }
    let items = crate::sidebar::items(app, false);
    let current = App::nav_current(&rows, &app.tab.cwd);
    let cursor = app
        .side_nav
        .focus
        .then_some(app.side_nav.nav_cursor.min(rows.len() - 1));

    // The trash is always the last row of the box: the rest scrolls above it.
    let mut body: Vec<usize> = (0..rows.len()).collect();
    let mut trash = None;
    if area.height >= 2 && rows.last().is_some_and(|r| r.kind == NavKind::Trash) {
        trash = body.pop();
    }
    let height = area.height as usize - usize::from(trash.is_some());
    // Scroll to the row the keyboard is on, or else to the folder shown, when either moves (or
    // the tree grows): not on every frame, or the wheel could not take the box anywhere else.
    let max_skip = body.len().saturating_sub(height);
    if let Some(anchor_at) = cursor
        .or(current)
        .and_then(|a| body.iter().position(|i| *i == a))
    {
        let seen = (app.tab.cwd.clone(), cursor, body.len(), height);
        if app.frame.nav_seen.as_ref() != Some(&seen) {
            app.frame.nav_seen = Some(seen);
            if anchor_at < app.frame.nav_scroll {
                app.frame.nav_scroll = anchor_at;
            } else if anchor_at >= app.frame.nav_scroll + height {
                app.frame.nav_scroll = anchor_at + 1 - height;
            }
        }
    }
    app.frame.nav_scroll = app.frame.nav_scroll.min(max_skip);
    let skip = app.frame.nav_scroll;

    let mut placed: Vec<(u16, usize)> = body
        .iter()
        .skip(skip)
        .take(height)
        .enumerate()
        .map(|(y, i)| (y as u16, *i))
        .collect();
    if let Some(t) = trash {
        placed.push((area.height - 1, t));
    }
    // The thumb has a column of its own: a pill must not end under it.
    let scrolls = body.len() > height && height >= 2;
    let rows_area = Rect {
        width: area.width - u16::from(scrolls && area.width > 1),
        ..area
    };
    let w = rows_area.width as usize;
    let pin = icons::ui(app.settings.icons, Glyph::Pin);
    for (y, i) in placed {
        let r = Rect {
            y: area.y + y,
            height: 1,
            ..rows_area
        };
        let row = &rows[i];
        let is_current = current == Some(i);
        let is_focus = cursor == Some(i);
        let hover =
            !row.path.as_os_str().is_empty() && app.side_nav.hover.as_ref() == Some(&row.path);
        let bg = if is_focus {
            Style::default().bg(th.cursor)
        } else if is_current {
            Style::default().bg(th.selection)
        } else if hover {
            Style::default().bg(th.mark)
        } else {
            Style::default()
        };
        // On the row the keyboard is on the text is the one of the highlight, whatever the
        // theme: dark on a light one.
        let text_style = if is_focus {
            Style::default().fg(th.cursor_text)
        } else {
            th.base()
        };
        match &row.kind {
            NavKind::Gap => continue,
            NavKind::Heading => {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        format!(" {}", row.name),
                        th.faint().add_modifier(Modifier::BOLD),
                    )),
                    r,
                );
                continue;
            }
            NavKind::More => {
                f.render_widget(
                    Paragraph::new(Span::styled(
                        format!(" {}{}", "  ".repeat(row.depth), row.name),
                        th.faint(),
                    )),
                    r,
                );
                continue;
            }
            NavKind::Tags => {
                let mut spans = vec![Span::styled(
                    pad(&format!(" {}", row.name), 6),
                    if is_focus {
                        text_style.add_modifier(Modifier::BOLD).patch(bg)
                    } else {
                        th.faint().add_modifier(Modifier::BOLD).patch(bg)
                    },
                )];
                let mut x = r.x + 6;
                for (n, hue) in Hue::ALL.iter().enumerate() {
                    let on = is_focus && app.side_nav.nav_tag == n;
                    let dot = if on {
                        Mark::TagDotFocused
                    } else {
                        Mark::TagDot
                    };
                    spans.push(Span::styled(
                        format!("{} ", icons::mark(app.settings.icons, dot)),
                        Style::default().fg(th.tag_color(Some(*hue))).patch(bg),
                    ));
                    let path = rada_core::tagsearch::view_path(hue.label(&app.tags.lang));
                    app.frame.hits.add(
                        Rect {
                            x,
                            y: r.y,
                            width: 2,
                            height: 1,
                        },
                        Target::Place(path),
                    );
                    x += 2;
                }
                let used: usize = spans.iter().map(|s| s.content.width()).sum();
                spans.push(Span::styled(" ".repeat(w.saturating_sub(used)), bg));
                f.render_widget(Paragraph::new(Line::from(spans)), r);
                if is_focus {
                    soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Focus);
                }
                continue;
            }
            _ => {}
        }

        // The arrow, the icon, the name, and the number of items on the right.
        let indent = "  ".repeat(row.depth);
        let (arrow, toggles) = match row.kind {
            NavKind::Folder { open: Some(true) } => {
                (icons::mark(app.settings.icons, Mark::TreeOpen), true)
            }
            NavKind::Folder { open: Some(false) } => {
                (icons::mark(app.settings.icons, Mark::TreeClosed), true)
            }
            _ => (" ", false),
        };
        let (glyph, hue) = match &row.kind {
            NavKind::Favorite(n) => {
                let item = &items[*n];
                let hue = match &item.origin {
                    Origin::Place(k) => place_hue(&th, *k),
                    _ => th.hues.yellow,
                };
                (item.glyph(app.settings.icons).to_string(), hue)
            }
            NavKind::Trash => (
                items
                    .iter()
                    .find(|it| it.path == row.path)
                    .map(|it| it.glyph(app.settings.icons).to_string())
                    .unwrap_or_default(),
                th.text_dim,
            ),
            _ if row.depth == 0 && row.path == app.home() => (
                icons::ui(app.settings.icons, Glyph::Home).to_string(),
                th.hues.blue,
            ),
            _ => {
                let kind = app
                    .places
                    .paths
                    .places
                    .iter()
                    .find(|p| p.path == row.path)
                    .map(|p| p.kind);
                // The glyph and the colour of a folder in the list of the middle box.
                let (glyph, folder) = icons::icon_for(
                    &row.name,
                    rada_core::fs::FileKind::Dir,
                    app.settings.icons,
                    &th,
                );
                (
                    glyph.to_string(),
                    kind.map_or(folder, |k| place_hue(&th, k)),
                )
            }
        };
        let mut spans: Vec<Span> = vec![
            Span::styled(" ", bg),
            Span::styled(indent.clone(), bg),
            Span::styled(
                format!("{arrow} "),
                if is_focus {
                    text_style.patch(bg)
                } else {
                    th.dim().patch(bg)
                },
            ),
        ];
        if !glyph.is_empty() {
            let icon_style = if is_focus {
                text_style
            } else {
                Style::default().fg(hue)
            };
            spans.push(Span::styled(format!("{glyph} "), icon_style.patch(bg)));
        }
        let head: usize = spans.iter().map(|s| s.content.width()).sum();
        let mut count = row.count.map(|n| format!("({n})")).unwrap_or_default();
        let bookmark = match row.kind {
            NavKind::Favorite(n) => matches!(items[n].origin, Origin::Bookmark { .. }),
            _ => false,
        };
        let pin_w = if bookmark && !pin.is_empty() { 2 } else { 0 };
        // In a narrow box the numbers go when the name does not fit with them.
        if !count.is_empty()
            && row.name.width() > w.saturating_sub(head + 2 + count.width() + pin_w)
        {
            count.clear();
        }
        let room = w.saturating_sub(head + 2 + count.width() + pin_w);
        let name = rada_core::display::truncate_name(&row.name, room.max(1));
        let name_style = if is_focus || is_current {
            text_style.add_modifier(Modifier::BOLD)
        } else {
            text_style
        };
        let name_w = name.width();
        spans.push(Span::styled(name, name_style.patch(bg)));
        let fill = w.saturating_sub(head + name_w + count.width() + pin_w + 1);
        spans.push(Span::styled(" ".repeat(fill), bg));
        let quiet = if is_focus {
            text_style.patch(bg)
        } else {
            th.faint().patch(bg)
        };
        if pin_w > 0 {
            spans.push(Span::styled(format!("{pin} "), quiet));
        }
        if !count.is_empty() {
            spans.push(Span::styled(
                count,
                if is_focus {
                    text_style.patch(bg)
                } else {
                    th.dim().patch(bg)
                },
            ));
        }
        spans.push(Span::styled(" ", bg));
        f.render_widget(Paragraph::new(Line::from(spans)), r);
        if is_focus {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Focus);
        } else if is_current {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Panel);
        }
        if !row.path.as_os_str().is_empty() {
            app.frame.hits.add(r, Target::Place(row.path.clone()));
        }
        if toggles {
            // The arrow and the cell after it: a click there opens or closes the folder.
            app.frame.hits.add(
                Rect {
                    x: r.x + 1 + indent.len() as u16,
                    width: 2,
                    ..r
                },
                Target::TreeToggle(row.path.clone()),
            );
        }
    }

    // A thumb on the right edge when the tree does not fit.
    if scrolls {
        let x = area.x + area.width.saturating_sub(1);
        let thumb = (height * height / body.len()).clamp(1, height);
        let top = (skip * (height - thumb)).checked_div(max_skip).unwrap_or(0);
        for y in 0..height {
            let on = y >= top && y < top + thumb;
            let (mark, style) = if on {
                (Mark::ScrollThumb, th.dim())
            } else {
                (Mark::ScrollTrack, th.faint())
            };
            let ch = icons::mark(app.settings.icons, mark);
            f.buffer_mut().set_string(x, area.y + y as u16, ch, style);
        }
    }
}
