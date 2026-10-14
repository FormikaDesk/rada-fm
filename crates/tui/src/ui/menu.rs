//! Menus: the one of the right button, the dropdowns of the command bar and their submenus.
//!
//! A menu is a card (see [`popup`]) with the name and type of what it is about on top, a thin
//! rule, then the entries in groups set apart by a short dashed line. An entry has its icon on
//! the left, its key on the right (a `›` for one that opens another menu, a `✓` for a choice
//! that is on). The entry under the cursor is a pill with the shade of the list's cursor row;
//! the one that opened the submenu in front stays marked quietly. What takes files away is in
//! the colour of errors, and last.
//!
//! The geometry is a pure function of the menu and the screen, so that the rest of the
//! interface can ask where a menu is (a picture must not be drawn under it).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::popup::{self, Ctx, RowFill};
use super::widgets::pad;
use crate::app::{MenuCmd, MenuIcon, MenuItem, MenuView};
use crate::hits::{Hits, Target};
use crate::icons::{self, Glyph};
use crate::keymap::Keymap;

/// What one row of a menu is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// The name of what the menu is about.
    Title,
    /// The rule under it.
    Rule,
    Item(usize),
    /// The short dashed line between two groups.
    Dashes,
}

/// A menu placed on the screen.
struct Placed {
    rect: Rect,
    rows: Vec<Row>,
    /// Width of the column for the icons (0: there are none).
    icon_w: usize,
    label_w: usize,
}

/// The icon of an entry and its colour; `None` when it has none.
fn icon_of(ctx: &Ctx, it: &MenuItem) -> Option<(String, Color)> {
    let th = ctx.th;
    match it.icon {
        MenuIcon::None => None,
        MenuIcon::Action(a) => {
            let s = icons::action_icon(ctx.set, a);
            (!s.is_empty()).then(|| (s.to_string(), th.accent))
        }
        MenuIcon::Glyph(g) => {
            let s = icons::ui(ctx.set, g);
            (!s.is_empty()).then(|| (s.to_string(), th.accent))
        }
        MenuIcon::Dot(c) => Some(("●".to_string(), th.tag_color(c))),
    }
}

/// What is written on the right of an entry.
fn right_of(ctx: &Ctx, km: &Keymap, it: &MenuItem) -> String {
    match &it.cmd {
        MenuCmd::Sub(_) | MenuCmd::OpenWithList => "›".to_string(),
        _ if it.checked == Some(true) => icons::ui(ctx.set, Glyph::Done).to_string(),
        MenuCmd::Act(a) if it.checked.is_none() => km.hint(*a).unwrap_or_default(),
        _ => String::new(),
    }
}

fn place(
    ctx: &Ctx,
    km: &Keymap,
    m: &MenuView,
    area: Rect,
    parent: Option<(&Placed, u16)>,
) -> Placed {
    let has_icons = m.items.iter().any(|it| icon_of(ctx, it).is_some());
    let icon_w = if has_icons { 2 } else { 0 };
    let label_w = m.items.iter().map(|i| i.label.width()).max().unwrap_or(8);
    let right_w = m
        .items
        .iter()
        .map(|i| right_of(ctx, km, i).width())
        .max()
        .unwrap_or(0);
    let content = 1 + icon_w + label_w + if right_w > 0 { 3 + right_w } else { 0 } + 1;
    let title_w = m.title.as_ref().map_or(0, |t| {
        2 + if t.icon.is_empty() { 0 } else { 2 }
            + t.name.width().min(40)
            + if t.kind.is_empty() {
                0
            } else {
                5 + t.kind.width()
            }
    });
    // Two cells of air on each side of the pills, and never wider than the screen allows.
    let inner = content.max(title_w).max(18) + 2;
    let width = ((inner + 2) as u16).min(area.width);

    let mut rows = Vec::new();
    if m.title.is_some() {
        rows.push(Row::Title);
        rows.push(Row::Rule);
    }
    for (i, it) in m.items.iter().enumerate() {
        if it.gap_before && i > 0 {
            rows.push(Row::Dashes);
        }
        rows.push(Row::Item(i));
    }
    let height = ((rows.len() + 2) as u16).min(area.height);

    // The first row of the menu's items, for a submenu to sit beside the entry that opened it.
    let (mut x, mut y) = match parent {
        None => m.at,
        Some((p, opener_y)) => {
            // Right against its parent, on the right or on the left (see `popup::seam`).
            let right = p.rect.right();
            let x = if right + width <= area.right() {
                right
            } else {
                p.rect.x.saturating_sub(width)
            };
            let head = if m.title.is_some() { 3 } else { 1 };
            (x, opener_y.saturating_sub(head))
        }
    };
    // Inside the room it has: a menu near the bottom moves up, over the bars it stays above.
    x = x.min(area.right().saturating_sub(width)).max(area.x);
    y = y.min(area.bottom().saturating_sub(height)).max(area.y);
    Placed {
        rect: Rect {
            x,
            y,
            width,
            height,
        },
        rows,
        icon_w,
        label_w,
    }
}

/// Where each menu of the chain (outermost first) is.
fn layout(ctx: &Ctx, km: &Keymap, chain: &[&MenuView], area: Rect) -> Vec<Placed> {
    let mut placed: Vec<Placed> = Vec::new();
    for (depth, m) in chain.iter().enumerate() {
        let p = if depth == 0 {
            place(ctx, km, m, area, None)
        } else {
            let (prev_menu, prev) = (chain[depth - 1], &placed[depth - 1]);
            let oy = row_y(prev, prev_menu.selected);
            place(ctx, km, m, area, Some((prev, oy)))
        };
        placed.push(p);
    }
    placed
}

/// The screen row of entry `item` of a placed menu (without scrolling).
fn row_y(p: &Placed, item: usize) -> u16 {
    let top = p.rect.y + 1;
    let at = p
        .rows
        .iter()
        .position(|r| *r == Row::Item(item))
        .unwrap_or(0) as u16;
    top + at
}

/// The rectangles a menu (and the menus it was opened from) covers.
pub fn rects(ctx: &Ctx, km: &Keymap, m: &MenuView, area: Rect) -> Vec<Rect> {
    let chain = m.chain();
    layout(ctx, km, &chain, area)
        .into_iter()
        .map(|p| p.rect)
        .collect()
}

pub fn draw(f: &mut Frame, ctx: &Ctx, km: &Keymap, m: &MenuView, area: Rect, hits: &mut Hits) {
    draw_chain(f, ctx, km, m, area, hits, true);
}

/// A menu that has a window of its own beside it (the list of programs): every level is
/// drawn as a parent, the entry that opened the window marked, nothing clickable.
pub fn draw_behind(f: &mut Frame, ctx: &Ctx, km: &Keymap, m: &MenuView, area: Rect) {
    draw_chain(f, ctx, km, m, area, &mut Hits::default(), false);
}

/// Where the menu in front is, and the row of its entry under the cursor: what a window that
/// opens beside it is placed against.
pub fn beside(ctx: &Ctx, km: &Keymap, m: &MenuView, area: Rect) -> (Rect, u16) {
    let chain = m.chain();
    let placed = layout(ctx, km, &chain, area);
    let last = placed.last().expect("a menu has a box");
    (last.rect, row_y(last, m.selected))
}

fn draw_chain(
    f: &mut Frame,
    ctx: &Ctx,
    km: &Keymap,
    m: &MenuView,
    area: Rect,
    hits: &mut Hits,
    active: bool,
) {
    let chain = m.chain();
    let placed = layout(ctx, km, &chain, area);
    let top = chain.len() - 1;
    for (depth, (menu, p)) in chain.iter().zip(&placed).enumerate() {
        draw_one(f, ctx, km, menu, p, active && depth == top, hits);
        if depth > 0 {
            popup::seam(f.buffer_mut(), placed[depth - 1].rect, p.rect, ctx.th);
        }
    }
}

fn draw_one(
    f: &mut Frame,
    ctx: &Ctx,
    km: &Keymap,
    m: &MenuView,
    p: &Placed,
    is_top: bool,
    hits: &mut Hits,
) {
    let th = ctx.th;
    let buf = f.buffer_mut();
    let inner = popup::card(buf, p.rect, th, popup::Edge::Behind);
    if inner.width < 4 || inner.height == 0 {
        return;
    }
    // The rows that fit, with the cursor in view.
    let visible = inner.height as usize;
    let sel_row = p
        .rows
        .iter()
        .position(|r| *r == Row::Item(m.selected))
        .unwrap_or(0);
    let head = p
        .rows
        .iter()
        .take_while(|r| matches!(r, Row::Title | Row::Rule))
        .count();
    let body_visible = visible.saturating_sub(head).max(1);
    let offset = if p.rows.len() <= visible {
        0
    } else {
        (sel_row + 1).saturating_sub(head + body_visible)
    };
    let shown: Vec<(usize, Row)> = p
        .rows
        .iter()
        .copied()
        .enumerate()
        .filter(|(i, _)| *i < head || *i >= head + offset)
        .take(visible)
        .collect();

    let rw = (inner.width as usize).saturating_sub(2);
    let x0 = inner.x + 1;
    for (n, (_, row)) in shown.iter().enumerate() {
        let y = inner.y + n as u16;
        let region = Rect {
            x: x0,
            y,
            width: rw as u16,
            height: 1,
        };
        match row {
            Row::Title => {
                if let Some(t) = &m.title {
                    let mut spans = vec![Span::styled(" ", ctx.surface())];
                    if !t.icon.is_empty() {
                        spans.push(Span::styled(
                            format!("{} ", t.icon),
                            Style::default().fg(t.icon_color).bg(th.popup),
                        ));
                    }
                    spans.push(Span::styled(
                        rada_core::display::truncate(&t.name, 40),
                        ctx.base().add_modifier(Modifier::BOLD),
                    ));
                    if !t.kind.is_empty() {
                        spans.push(Span::styled("  ·  ", ctx.faint()));
                        spans.push(Span::styled(t.kind.clone(), ctx.dim()));
                    }
                    popup::put(buf, region, Line::from(spans));
                }
            }
            Row::Rule => popup::put(buf, region, Line::from(popup::rule(ctx, rw))),
            Row::Dashes => popup::put(
                buf,
                region,
                Line::from(vec![
                    Span::styled("  ", ctx.surface()),
                    popup::dashes(ctx, rw.saturating_sub(4)),
                ]),
            ),
            Row::Item(i) => {
                let it = &m.items[*i];
                let at_cursor = is_top && *i == m.selected && it.enabled;
                let opener = !is_top && *i == m.selected;
                let fill = if at_cursor && it.danger {
                    Some(RowFill::Danger)
                } else if at_cursor {
                    Some(RowFill::Cursor)
                } else if opener {
                    Some(RowFill::Opener)
                } else {
                    None
                };
                item_row(buf, ctx, km, it, p, region, fill);
                if is_top {
                    hits.add(
                        Rect {
                            x: inner.x,
                            width: inner.width,
                            ..region
                        },
                        Target::MenuItem(*i),
                    );
                }
            }
        }
    }
}

fn item_row(
    buf: &mut ratatui::buffer::Buffer,
    ctx: &Ctx,
    km: &Keymap,
    it: &MenuItem,
    p: &Placed,
    region: Rect,
    fill: Option<RowFill>,
) {
    let th = ctx.th;
    let rw = region.width as usize;
    let on_fill = |c: Style| match fill {
        Some(f) if matches!(f, RowFill::Cursor | RowFill::Danger) => popup::row_text(th, f),
        _ => c,
    };
    let (label_style, icon_color, right_style) = if !it.enabled {
        (ctx.faint(), th.muted, ctx.faint())
    } else if it.danger {
        (
            Style::default().fg(th.error).bg(th.popup),
            th.error,
            ctx.dim(),
        )
    } else {
        (ctx.base(), th.accent, ctx.dim())
    };
    let mut spans: Vec<Span> = vec![Span::styled(" ", ctx.surface())];
    if p.icon_w > 0 {
        match icon_of(ctx, it) {
            Some((s, c)) => {
                let c = if !it.enabled || it.danger {
                    icon_color
                } else {
                    c
                };
                spans.push(Span::styled(
                    format!("{s} "),
                    on_fill(Style::default().fg(c).bg(th.popup)),
                ));
            }
            None => spans.push(Span::styled("  ", ctx.surface())),
        }
    }
    spans.push(Span::styled(
        pad(&it.label, p.label_w),
        on_fill(label_style),
    ));
    let right = right_of(ctx, km, it);
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    let gap = rw.saturating_sub(used + right.width() + 1);
    spans.push(Span::styled(" ".repeat(gap), ctx.surface()));
    let right_style = if it.checked == Some(true) && it.enabled {
        ctx.accent().add_modifier(Modifier::BOLD)
    } else {
        right_style
    };
    spans.push(Span::styled(right, on_fill(right_style)));
    spans.push(Span::styled(" ", ctx.surface()));
    popup::put(buf, region, Line::from(spans));
    if let Some(f) = fill {
        popup::tint_row(buf, region, ctx, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{MenuCmd, MenuItem};
    use crate::icons::IconSet;
    use crate::keymap::{Action, Keymap, Preset};
    use crate::theme::{ColorDepth, Theme};
    use crate::ui::soft::Look;

    fn ctx(th: &Theme, caps: bool) -> Ctx<'_> {
        Ctx {
            th,
            look: Look {
                caps,
                shades: caps && th.shades(),
            },
            set: if caps {
                IconSet::Nerd
            } else {
                IconSet::UnicodeStd
            },
            basic: false,
        }
    }

    fn sample() -> MenuView {
        MenuView::new(
            (10, 5),
            vec![
                MenuItem::act(Action::Open, true, false),
                MenuItem::act(Action::Cut, false, true),
                MenuItem::act(Action::Copy, true, false),
                MenuItem::act(Action::Trash, true, true),
            ],
        )
    }

    #[test]
    fn a_menu_stays_inside_the_room_it_has_and_moves_up_to_fit() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let c = ctx(&th, true);
        let km = Keymap::new(Preset::VimClassic, &std::collections::HashMap::new());
        let area = Rect::new(0, 0, 60, 20);
        for at in [(0, 0), (59, 19), (30, 18), (58, 2)] {
            let mut m = sample();
            m.at = at;
            let r = rects(&c, &km, &m, area);
            assert_eq!(r.len(), 1);
            assert!(
                r[0].right() <= area.right() && r[0].bottom() <= area.bottom(),
                "{at:?}: {:?}",
                r[0]
            );
        }
        // Near the bottom it moves up until its last row is the last row of the room: no row
        // is left empty under it, and none of it is over the bars below.
        let mut m = sample();
        m.at = (30, 18);
        let r = rects(&c, &km, &m, area);
        assert_eq!(r[0].bottom(), area.bottom(), "{:?}", r[0]);
        let mut m = sample();
        m.at = (59, 5);
        let r = rects(&c, &km, &m, area);
        assert_eq!(r[0].right(), area.right(), "{:?}", r[0]);
    }

    #[test]
    fn the_cursor_skips_what_is_disabled() {
        let mut m = sample();
        assert_eq!(m.selected, 0);
        m.step(1);
        assert_eq!(m.selected, 2, "Cut is disabled");
        m.step(-1);
        assert_eq!(m.selected, 0);
        m.step(-1);
        assert_eq!(m.selected, 0, "it does not wrap");
        let first_disabled = MenuView::new(
            (0, 0),
            vec![
                MenuItem::act(Action::Cut, false, false),
                MenuItem::act(Action::Copy, true, false),
            ],
        );
        assert_eq!(
            first_disabled.selected, 1,
            "it starts on the first it can use"
        );
    }

    #[test]
    fn a_submenu_sits_beside_the_entry_that_opened_it() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let c = ctx(&th, true);
        let km = Keymap::new(Preset::VimClassic, &std::collections::HashMap::new());
        let area = Rect::new(0, 0, 100, 30);
        let mut parent = sample();
        parent.items.insert(
            1,
            MenuItem::sub(crate::app::MenuKind::Sort, "Ordina per", true, false),
        );
        parent.selected = 1;
        parent.at = (40, 5);
        let mut child = MenuView::new(
            (0, 0),
            vec![MenuItem::command(MenuCmd::About, "Nome", false)],
        );
        child.parent = Some(Box::new(parent));
        let r = rects(&c, &km, &child, area);
        assert_eq!(r.len(), 2);
        assert!(
            r[1].x >= r[0].right() - 1,
            "to the right of its parent: {r:?}"
        );
        // With no room on the right it opens on the left.
        let narrow = Rect::new(0, 0, r[0].right() + 4, 30);
        let r = rects(&c, &km, &child, narrow);
        assert!(r[1].right() <= r[0].x + 1, "{r:?}");
        // On the left of its parent, at the bottom of the room: flush with it, nothing under.
        let low = Rect::new(0, 0, r[0].right() + 4, r[0].bottom() + 1);
        let r = rects(&c, &km, &child, low);
        assert!(
            r[1].bottom() <= low.bottom() && r[1].right() <= r[0].x + 1,
            "{r:?}"
        );
    }
}
