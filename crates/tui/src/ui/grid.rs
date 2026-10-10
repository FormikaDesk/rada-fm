//! The folder as a grid of tiles (the Icons view): a big icon three rows tall and the name
//! under it, cut with "…" when it is too long. A tile is a click target like a row of the
//! table; the cursor tile and the selected ones carry the same tones as rows do.

use rada_core::display;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::widgets::SPIN;
use crate::app::App;
use crate::hits::Target;
use crate::icons::{self, Glyph, IconSet};
use crate::theme::Theme;
use rada_core::model::Entry;

/// Width and height of a tile in cells: the art, the name and a blank line.
pub const TILE_W: usize = 14;
pub const TILE_H: usize = 5;
const ART_W: usize = 5;

/// The big icon: three lines of `ART_W` cells.
fn art(e: &Entry, set: IconSet, glyph: &str) -> [String; 3] {
    if e.is_dir() {
        return [
            "▄▄▄  ".to_string(),
            "█████".to_string(),
            "▀▀▀▀▀".to_string(),
        ];
    }
    // A page, with the file's icon in the middle; with no icons, its extension.
    let mid = if set == IconSet::None || glyph.is_empty() {
        let ext: String = e
            .display
            .rsplit_once('.')
            .filter(|(stem, _)| !stem.is_empty())
            .map(|(_, x)| x.chars().take(3).collect::<String>().to_uppercase())
            .unwrap_or_default();
        let w = ext.width();
        let left = (3 - w.min(3)) / 2;
        format!(
            "{}{}{}",
            " ".repeat(left),
            ext,
            " ".repeat(3 - w.min(3) - left)
        )
    } else {
        format!(" {glyph} ")
    };
    ["╭───╮".to_string(), format!("│{mid}│"), "╰───╯".to_string()]
}

fn centered(text: &str, width: usize) -> String {
    let w = text.width();
    let left = width.saturating_sub(w) / 2;
    format!(
        "{}{}{}",
        " ".repeat(left),
        text,
        " ".repeat(width.saturating_sub(w + left))
    )
}

pub fn draw_grid(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    if area.width < TILE_W as u16 || area.height < TILE_H as u16 - 1 {
        // Too small for a single tile: fall back to a plain message.
        f.render_widget(
            Paragraph::new(Span::styled("too small for icons", th.dim())),
            area,
        );
        return;
    }
    let cols = (area.width as usize / TILE_W).max(1);
    let rows = ((area.height as usize + 1) / TILE_H).max(1);
    app.set_grid(cols, rows);
    app.view_rows = rows;
    app.hits.add(area, Target::List);
    let count = app.visible.len();

    if count == 0 {
        let msg = if app.is_loading() {
            format!("{} loading…", SPIN[app.spinner % SPIN.len()])
        } else if app.filter.is_some() {
            "Nothing here matches the search".to_string()
        } else if app.listing.is_empty() {
            "This folder is empty".to_string()
        } else {
            "Only hidden files here — press . to show them".to_string()
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, th.dim())).alignment(Alignment::Center),
            Rect {
                y: area.y + area.height / 4,
                height: 1,
                ..area
            },
        );
        return;
    }

    let first = app.scroll / cols * cols;
    let x0 = area.x + (area.width as usize - cols * TILE_W).min(2) as u16 / 2;
    let mut lines: Vec<Line> = Vec::with_capacity(rows * TILE_H);
    for r in 0..rows {
        let mut tile_lines: Vec<Vec<Span>> = vec![Vec::new(); TILE_H];
        for c in 0..cols {
            let vis = first + r * cols + c;
            let Some(e) = app.entry_at(vis) else {
                for line in tile_lines.iter_mut() {
                    line.push(Span::raw(" ".repeat(TILE_W)));
                }
                continue;
            };
            let cursor = vis == app.cursor;
            let marked = app.marked.contains(&e.name);
            let bg = if cursor {
                th.selected()
            } else if marked {
                th.marked()
            } else {
                Style::default()
            };
            let (glyph, gcolor) = icons::icon(e, app.icons, &th);
            let color: Color = if e.is_dir() { th.kinds.folder } else { gcolor };
            let lines_of_art = art(e, app.icons, glyph);
            let pad_l = (TILE_W - ART_W) / 2;
            for (i, a) in lines_of_art.iter().enumerate() {
                let mut spans = vec![Span::styled(" ".repeat(pad_l), bg)];
                if i == 0 && marked {
                    // The tick sits in the corner of the art, over its first cell.
                    let tick = icons::ui(app.icons, Glyph::Checked);
                    spans[0] = Span::styled(
                        format!(
                            "{}{}",
                            " ".repeat(pad_l.saturating_sub(2)),
                            format!("{tick} ")
                        ),
                        bg.patch(th.fg(th.check)),
                    );
                }
                spans.push(Span::styled(
                    a.clone(),
                    bg.patch(Style::default().fg(color)),
                ));
                spans.push(Span::styled(" ".repeat(TILE_W - pad_l - ART_W), bg));
                tile_lines[i].extend(spans);
            }
            let name_color = if e.error.is_some() {
                th.error
            } else if e.hidden {
                th.text_dim
            } else if e.is_dir() {
                th.kinds.folder
            } else {
                th.text
            };
            let mut st = Style::default().fg(name_color);
            if e.is_dir() || cursor {
                st = st.add_modifier(Modifier::BOLD);
            }
            let name = display::truncate(&e.display, TILE_W - 2);
            tile_lines[3].push(Span::styled(
                format!(" {} ", centered(&name, TILE_W - 2)),
                bg.patch(st),
            ));
            tile_lines[4].push(Span::raw(" ".repeat(TILE_W)));
            app.hits.add(
                Rect {
                    x: x0 + (c * TILE_W) as u16,
                    y: area.y + (r * TILE_H) as u16,
                    width: TILE_W as u16,
                    height: (TILE_H - 1) as u16,
                },
                Target::Row(vis),
            );
        }
        for l in tile_lines {
            lines.push(Line::from(l));
        }
    }
    let _: &Theme = &th;
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            x: x0,
            width: area.width.saturating_sub(x0 - area.x),
            ..area
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_centred_in_the_tile() {
        assert_eq!(centered("ab", 6), "  ab  ");
        assert_eq!(centered("abc", 6), " abc  ");
        assert_eq!(centered("toolong", 4), "toolong");
    }
}
