//! "Open with…": the programs that can open the file, each with its mark and colour, the
//! system's choice labelled, then "Other program…" and the box "always use this for .jpg".
//!
//! It is a card like a menu. Opened from the "Open with ›" entry of the context menu it sits
//! beside that menu, which stays in view with its entry marked; opened with its key it is a
//! window of its own in the middle of the screen, with the screen dimmed behind it.

use rada_core::display;
use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::popup::{self, Ctx, RowFill};
use super::widgets::{key_name, pad};
use crate::app::{OpenRow, OpenWithView};
use crate::hits::{Hits, Target};
use crate::icons::{self, Glyph};
use crate::keymap::Keymap;
use crate::progicons;

/// Rows of programs shown at most; the list scrolls beyond.
const MAX_LIST: usize = 10;

struct Geom {
    rect: Rect,
    /// Rows of the list that fit.
    list_h: usize,
    filter_row: bool,
}

fn geometry(ctx: &Ctx, v: &OpenWithView, area: Rect, anchor: Option<(Rect, u16)>) -> Geom {
    let rows = v.rows();
    let widest = rows
        .iter()
        .map(|r| match r {
            OpenRow::App(a) => 2 + a.name.width() + 2 + badge_text(v, a).width() + 2,
            OpenRow::Other => 2 + Key::OtherProgram.tr().width(),
        })
        .max()
        .unwrap_or(20);
    let title = tf(Key::OpenFileWith, &[&v.file]).width();
    // Wide enough for the note under the box, which is never cut.
    let note = Key::OnlyInRada.tr().width() + 3;
    let inner = widest.max(title).max(note).clamp(40, 66) + 4;
    let width = ((inner + 2) as u16).min(area.width.saturating_sub(2));
    let filter_row = !v.filter.is_empty();
    // The dashed line before "Other program…" is a row of the list too.
    let list_h = (rows.len() + usize::from(rows.len() > 1)).clamp(3, MAX_LIST);
    // title, rule, [filter], list, a dashed line before "Other…" (it is in the list), a row of
    // air, the box, the note.
    let height = (1 + 1 + usize::from(filter_row) + list_h + 1 + 1 + 1 + 1 + 2) as u16;
    let height = height.min(area.height.saturating_sub(2));
    let (mut x, mut y) = match anchor {
        None => (
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
        ),
        Some((menu, oy)) => {
            let x = if menu.right() + width <= area.right() {
                menu.right()
            } else {
                menu.x.saturating_sub(width)
            };
            (x, oy.saturating_sub(3))
        }
    };
    x = x.min(area.right().saturating_sub(width)).max(area.x);
    y = y.min(area.bottom().saturating_sub(height)).max(area.y);
    let _ = ctx;
    Geom {
        rect: Rect {
            x,
            y,
            width,
            height,
        },
        list_h,
        filter_row,
    }
}

/// The label on the right of a program: the system's choice, the one picked for always, one
/// that runs in a terminal.
fn badge_text(v: &OpenWithView, a: &rada_core::platform::OpenApp) -> &'static str {
    if v.always_id.as_deref() == Some(a.id.as_str()) {
        Key::TagAlways.tr()
    } else if a.is_default {
        Key::TagDefault.tr()
    } else if a.terminal {
        Key::TagInTerminal.tr()
    } else {
        ""
    }
}

/// Where the card is (for what must not be drawn under it).
pub fn rect(ctx: &Ctx, v: &OpenWithView, area: Rect, anchor: Option<(Rect, u16)>) -> Rect {
    geometry(ctx, v, area, anchor).rect
}

pub fn draw(
    f: &mut Frame,
    ctx: &Ctx,
    km: &Keymap,
    v: &OpenWithView,
    area: Rect,
    anchor: Option<(Rect, u16)>,
    hits: &mut Hits,
) {
    let _ = km;
    let th = ctx.th;
    let g = geometry(ctx, v, area, anchor);
    let buf = f.buffer_mut();
    // Beside a menu the card is a menu too; on its own it is a window over a dimmed screen.
    let edge = if v.under.is_some() {
        popup::Edge::Behind
    } else {
        popup::Edge::Own
    };
    let inner = popup::card(buf, g.rect, th, edge);
    if inner.width < 10 || inner.height < 6 {
        return;
    }
    let rw = (inner.width as usize).saturating_sub(2);
    let x0 = inner.x + 1;
    let mut y = inner.y;
    let region = |y: u16| Rect {
        x: x0,
        y,
        width: rw as u16,
        height: 1,
    };

    // Title.
    popup::put(
        buf,
        region(y),
        Line::from(Span::styled(
            display::truncate(&tf(Key::OpenFileWith, &[&v.file]), rw),
            ctx.base().add_modifier(Modifier::BOLD),
        )),
    );
    y += 1;
    // What is typed filters the list; the row is there only while something is typed.
    if g.filter_row {
        let before: String = v.filter.chars().take(v.cursor).collect();
        let after: String = v.filter.chars().skip(v.cursor).collect();
        popup::put(
            buf,
            region(y),
            Line::from(vec![
                Span::styled(
                    format!("{} ", icons::ui(ctx.set, Glyph::Search)),
                    ctx.accent(),
                ),
                Span::styled(before, ctx.base()),
                Span::styled("▏", ctx.accent()),
                Span::styled(after, ctx.base()),
            ]),
        );
        y += 1;
    }
    popup::put(buf, region(y), Line::from(popup::rule(ctx, rw)));
    y += 1;

    // The list.
    let rows = v.rows();
    let has_icons = ctx.set != icons::IconSet::None;
    let apps_n = rows.len() - 1;
    let message = if v.loading {
        Some(Key::OpenWithLooking.tr())
    } else if apps_n == 0 && v.apps.is_empty() {
        Some(Key::OpenWithNone.tr())
    } else if apps_n == 0 {
        Some(Key::OpenWithNoMatch.tr())
    } else {
        None
    };
    let mut lines_drawn = 0usize;
    if let Some(m) = message {
        popup::put(
            buf,
            region(y),
            Line::from(Span::styled(format!(" {m}"), ctx.dim())),
        );
        y += 1;
        lines_drawn += 1;
    }
    // Scroll so that the cursor is in view.
    let room = g.list_h.saturating_sub(lines_drawn);
    let total = rows.len() + usize::from(apps_n > 0);
    let start = if total <= room {
        0
    } else {
        (v.selected + 1).saturating_sub(room)
    };
    let mut shown = 0usize;
    for (i, row) in rows.iter().enumerate() {
        // A short dashed line before "Other program…".
        let is_other = matches!(row, OpenRow::Other);
        let slot = i + usize::from(is_other && apps_n > 0);
        if slot < start || shown >= room {
            continue;
        }
        if is_other && apps_n > 0 && slot > start {
            popup::put(
                buf,
                region(y),
                Line::from(vec![
                    Span::styled("  ", ctx.surface()),
                    popup::dashes(ctx, rw.saturating_sub(4)),
                ]),
            );
            y += 1;
            shown += 1;
            if shown >= room {
                break;
            }
        }
        let at_cursor = i == v.selected;
        let row_area = region(y);
        draw_row(buf, ctx, v, row, row_area, has_icons, at_cursor);
        hits.add(
            Rect {
                x: inner.x,
                width: inner.width,
                ..row_area
            },
            Target::OpenWithRow(i),
        );
        y += 1;
        shown += 1;
    }
    // Fill the list to its height, so that the box below stays where it is.
    y = inner.y + 1 + u16::from(g.filter_row) + 1 + g.list_h as u16 + 1;
    // The box.
    let mark = icons::ui(
        ctx.set,
        if v.always {
            Glyph::Checked
        } else {
            Glyph::Unchecked
        },
    );
    let mark = if mark.is_empty() {
        if v.always { "[x]" } else { "[ ]" }
    } else {
        mark
    };
    let ext = std::path::Path::new(&v.file)
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    let label = match &ext {
        Some(e) => tf(Key::AlwaysForExt, &[&e]),
        None => Key::AlwaysForKind.tr().to_string(),
    };
    let usable = v.current().is_some();
    let box_style = if usable { ctx.base() } else { ctx.faint() };
    let mark_style = if v.always && usable {
        ctx.accent()
    } else {
        box_style
    };
    let key_w = 4;
    let label = display::truncate(&label, rw.saturating_sub(4 + key_w));
    let gap = rw.saturating_sub(2 + 2 + label.width() + key_w);
    let box_row = region(y);
    popup::put(
        buf,
        box_row,
        Line::from(vec![
            Span::styled(format!(" {mark} "), mark_style),
            Span::styled(label, box_style),
            Span::styled(" ".repeat(gap), ctx.surface()),
            Span::styled(
                pad(key_name("Tab"), key_w),
                if usable { ctx.dim() } else { ctx.faint() },
            ),
        ]),
    );
    if usable {
        hits.add(
            Rect {
                x: inner.x,
                width: inner.width,
                ..box_row
            },
            Target::OpenWithAlways,
        );
    }
    popup::put(
        buf,
        region(y + 1),
        Line::from(Span::styled(
            format!(
                "   {}",
                display::truncate(Key::OnlyInRada.tr(), rw.saturating_sub(3))
            ),
            ctx.faint(),
        )),
    );
}

fn draw_row(
    buf: &mut ratatui::buffer::Buffer,
    ctx: &Ctx,
    v: &OpenWithView,
    row: &OpenRow,
    area: Rect,
    has_icons: bool,
    at_cursor: bool,
) {
    let th = ctx.th;
    let rw = area.width as usize;
    let on_cursor = |s: Style| {
        if at_cursor {
            popup::row_text(th, RowFill::Cursor)
        } else {
            s
        }
    };
    let mut spans: Vec<Span> = vec![Span::styled(" ", ctx.surface())];
    let mut badge: Vec<Span> = Vec::new();
    match row {
        OpenRow::App(a) => {
            if has_icons {
                let (g, c) = progicons::look(a, v.kind.as_deref(), ctx.set, th);
                spans.push(Span::styled(
                    format!("{} ", if g.is_empty() { " " } else { g }),
                    on_cursor(Style::default().fg(c).bg(th.popup)),
                ));
            }
            let name_w = rw.saturating_sub(1 + usize::from(has_icons) * 2 + 1);
            let text = badge_text(v, a);
            let badge_w = if text.is_empty() {
                0
            } else {
                text.width() + 2 + 1
            };
            spans.push(Span::styled(
                pad(
                    &display::truncate(&a.name, name_w.saturating_sub(badge_w)),
                    name_w.saturating_sub(badge_w),
                ),
                on_cursor(ctx.base()),
            ));
            if !text.is_empty() {
                let tone_fill = if at_cursor {
                    th.tint_share(th.grad_row_end, th.cursor_text, 0.28)
                } else if a.is_default {
                    th.tint_share(th.popup, th.accent, if th.light { 0.16 } else { 0.24 })
                } else {
                    th.key_bg
                };
                let tone_text = if at_cursor {
                    th.cursor_text
                } else if a.is_default {
                    th.accent
                } else {
                    th.text_dim
                };
                badge = popup::pill(
                    ctx,
                    text,
                    tone_fill,
                    Style::default().fg(tone_text).add_modifier(Modifier::BOLD),
                );
                if at_cursor && ctx.look.caps {
                    // The half circles of a pill on a shaded row sit on the row, not the card.
                    for s in badge.iter_mut() {
                        if s.content == super::soft::CAP_LEFT || s.content == super::soft::CAP_RIGHT
                        {
                            s.style = s.style.bg(th.grad_row_end);
                        }
                    }
                }
            }
        }
        OpenRow::Other => {
            if has_icons {
                let mark = icons::ui(ctx.set, Glyph::Open);
                spans.push(Span::styled(
                    format!("{} ", if mark.is_empty() { " " } else { mark }),
                    on_cursor(ctx.dim()),
                ));
            }
            spans.push(Span::styled(
                Key::OtherProgram.tr().to_string(),
                on_cursor(ctx.base()),
            ));
        }
    }
    // The row first (with room left for the label), the shade over it, the label on top: the
    // label keeps its own colour.
    let badge_w: usize = badge.iter().map(|s| s.content.width()).sum();
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    spans.push(Span::styled(
        " ".repeat(rw.saturating_sub(used)),
        ctx.surface(),
    ));
    popup::put(buf, area, Line::from(spans));
    if at_cursor {
        popup::tint_row(buf, area, ctx, RowFill::Cursor);
    }
    if badge_w > 0 && rw > badge_w + 2 {
        let at = Rect {
            x: area.x + (rw - badge_w - 1) as u16,
            width: badge_w as u16,
            ..area
        };
        popup::put(buf, at, Line::from(badge));
    }
}
