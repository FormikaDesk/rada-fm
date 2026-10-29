//! The bar at the top: the buttons back, forward, up and refresh as little round pills; in
//! the middle one field, a pill holding the path as clickable crumbs, "Search files or type a
//! command…" and the key that starts it (Ctrl+P); on the right the pill of the view in use.
//! A thin shaded line runs under the field.
//!
//! The field is also where typing happens: a path is completed, words filter the files and
//! offer commands (see `app/address.rs`). When the terminal narrows the search part shrinks
//! and goes, then the middle of the path folds into a clickable "…".

use std::path::PathBuf;

use crate::fmt;
use rada_core::display;
use rada_core::i18n::Key;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look, RowTone};
use super::widgets::{Bar, SPIN, field_parts, pad};
use crate::app::{App, BarItem, BarMode, MenuKind};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

const SEP: &str = " › ";

/// The first segment of a path: the home folder, or the root of the disk.
fn home_label(app: &App) -> String {
    let mark = icons::ui(app.settings.icons, Glyph::Home);
    if mark.is_empty() {
        Key::HomeLabel.tr().to_string()
    } else {
        format!("{mark} {}", Key::HomeLabel.tr())
    }
}

/// The path as segments, each with the folder it stands for.
fn segments(app: &App) -> Vec<(String, PathBuf)> {
    use std::path::Component;
    // A tag's items are not on a path: one segment, which is the list itself.
    if let Some(title) = app.tab.view_title() {
        return vec![(title, app.tab.cwd.clone())];
    }
    let home = app.home();
    // Inside a phone or a camera the path is the device's name, not the folders of the virtual
    // filesystem that hold it; the real path stays in the details.
    let (first, rest) = if let Some((label, root)) = app.device_head() {
        let rest = app.tab.cwd.strip_prefix(&root).unwrap_or(&app.tab.cwd);
        ((label, root), rest)
    } else {
        match app.tab.cwd.strip_prefix(home) {
            Ok(rest) => ((home_label(app), home.to_path_buf()), rest),
            Err(_) => {
                let mut root = PathBuf::new();
                let mut label = String::new();
                for c in app.tab.cwd.components() {
                    match c {
                        Component::Prefix(_) | Component::RootDir => {
                            root.push(c.as_os_str());
                            label = if label.is_empty() {
                                display::name(c.as_os_str())
                            } else {
                                label
                            };
                        }
                        _ => break,
                    }
                }
                if label.is_empty() || label == std::path::MAIN_SEPARATOR_STR {
                    label = std::path::MAIN_SEPARATOR_STR.to_string();
                }
                let rest = app.tab.cwd.strip_prefix(&root).unwrap_or(&app.tab.cwd);
                ((app.places.shown_name(&root, label), root), rest)
            }
        }
    };
    let mut out = vec![first];
    let mut acc = out[0].1.clone();
    for c in rest.components() {
        if let Component::Normal(n) = c {
            acc.push(n);
            out.push((app.places.shown_name(&acc, display::name(n)), acc.clone()));
        }
    }
    out
}

fn natural_width(segs: &[(String, PathBuf)]) -> usize {
    segs.iter().map(|(s, _)| s.width()).sum::<usize>() + SEP.width() * segs.len().saturating_sub(1)
}

/// The path as spans fitting in `budget` cells: all of it, or the first segment, a "…" for
/// the middle (clickable: it lists what it hides) and as many of the last segments as fit.
fn crumbs(app: &App, budget: usize, field: Style) -> Vec<(Span<'static>, Option<Target>)> {
    let th = &app.settings.th;
    let segs = segments(app);
    let n = segs.len();
    let sep = || (Span::styled(SEP, th.faint().patch(field)), None);
    let seg_span = |i: usize, text: String| {
        let style = if i + 1 == n {
            th.base().add_modifier(Modifier::BOLD)
        } else {
            th.dim()
        };
        (
            Span::styled(text, style.patch(field)),
            Some(Target::Crumb(segs[i].1.clone())),
        )
    };
    let mut out: Vec<(Span<'static>, Option<Target>)> = Vec::new();

    if natural_width(&segs) <= budget || n == 1 {
        for (i, (label, _)) in segs.iter().enumerate() {
            if i > 0 {
                out.push(sep());
            }
            let text = if i + 1 == n {
                display::truncate_name(label, budget.max(4))
            } else {
                label.clone()
            };
            out.push(seg_span(i, text));
        }
        return out;
    }

    let w = |i: usize| segs[i].0.width();
    let sep_w = SEP.width();
    let head = w(0) + sep_w + 1;
    let tail_from = |from: usize| -> usize { (from..n).map(|j| sep_w + w(j)).sum() };
    let mut start = n - 1;
    if head + tail_from(n - 1) > budget {
        out.push((
            Span::styled("…", th.base().patch(field)),
            Some(Target::CrumbMore(
                segs[..n - 1].iter().map(|(_, p)| p.clone()).collect(),
            )),
        ));
        out.push(sep());
        let room = budget.saturating_sub(1 + sep_w).max(4);
        out.push(seg_span(
            n - 1,
            display::truncate_name(&segs[n - 1].0, room),
        ));
        return out;
    }
    while start > 1 && head + tail_from(start - 1) <= budget {
        start -= 1;
    }
    out.push(seg_span(0, segs[0].0.clone()));
    out.push(sep());
    out.push((
        Span::styled("…", th.base().add_modifier(Modifier::BOLD).patch(field)),
        Some(Target::CrumbMore(
            segs[1..start].iter().map(|(_, p)| p.clone()).collect(),
        )),
    ));
    for (i, (label, _)) in segs.iter().enumerate().skip(start) {
        out.push(sep());
        out.push(seg_span(i, label.clone()));
    }
    out
}

/// The view in use, as the pill on the right of the bar says it.
fn view_label(app: &App) -> (Glyph, &'static str) {
    use crate::view::ViewMode;
    match app.tab.view {
        ViewMode::Details => (Glyph::Details, Key::ViewDetails.tr()),
        ViewMode::Icons => (Glyph::Icons, Key::ViewIcons.tr()),
        ViewMode::Columns => (Glyph::Columns, Key::ViewColumns.tr()),
    }
}

/// The path part of the field in its idle state: the crumbs after a leading space, within
/// `budget` cells.
fn idle_crumbs(app: &App, budget: usize, field: Style, bar: &mut Bar<'static>) {
    bar.push(Span::styled(" ", field));
    // A folder being read shows a spinner after the path.
    let spin = if app.tab.is_loading() { 2 } else { 0 };
    for (span, target) in crumbs(app, budget.saturating_sub(1 + spin), field) {
        match target {
            Some(t) => bar.push_hit(span, t),
            None => bar.push(span),
        }
    }
    if spin > 0 {
        bar.push(Span::styled(
            format!(" {}", SPIN[app.redraw.spinner % SPIN.len()]),
            app.settings.th.fg(app.settings.th.warn).patch(field),
        ));
    }
}

/// The key that starts the bar, as a little pill inside the field (the field's colour around).
fn key_chip(
    app: &App,
    look: Look,
    key: &str,
    field: Style,
    target: Option<Target>,
) -> Vec<Span<'static>> {
    let th = &app.settings.th;
    let _ = target;
    soft::pill(
        th,
        look,
        key,
        th.key_bg,
        Style::default()
            .fg(th.key_text)
            .add_modifier(Modifier::BOLD),
        field,
    )
}

/// The field: a pill of exactly `width` cells.
fn field_bar(app: &App, look: Look, width: usize) -> Bar<'static> {
    let th = &app.settings.th;
    let band = th.band();
    let field = Style::default().bg(th.field);
    let mut out = Bar::default();
    if width < 6 {
        return out;
    }
    // The ends: half circles in the field's colour on the strip, or a space of the field.
    let (left_end, right_end) = if look.caps {
        let edge = band.fg(th.field);
        (
            Span::styled(soft::CAP_LEFT, edge),
            Span::styled(soft::CAP_RIGHT, edge),
        )
    } else {
        (Span::styled(" ", field), Span::styled(" ", field))
    };
    let inner = width - 2;
    let icon = icons::ui(app.settings.icons, Glyph::Search);
    let lead = if icon.is_empty() {
        String::new()
    } else {
        format!("{icon} ")
    };
    let mut body = Bar::default();

    if let Some(a) = &app.windows.address {
        // Typing: an icon, the text with its cursor, and the way out.
        let esc = key_chip(app, look, "Esc", field, None);
        let esc_w: usize = esc.iter().map(|s| s.content.width()).sum();
        body.push(Span::styled(format!(" {lead}"), th.dim().patch(field)));
        let room = inner.saturating_sub(1 + lead.width() + esc_w + 2).max(2);
        let (before, at, after) = field_parts(&a.text, a.cursor);
        let after_c = format!("{at}{after}");
        if a.fresh {
            // Everything selected: the first key typed replaces it.
            let shown = display::truncate(&display::text(&a.text), room.saturating_sub(1));
            body.push(Span::styled(
                shown,
                th.base().patch(Style::default().bg(th.selection)),
            ));
            body.push(Span::styled(
                "▏",
                th.accent_style().add_modifier(Modifier::BOLD).patch(field),
            ));
        } else if a.text.is_empty() {
            body.push(Span::styled(
                "▏",
                th.accent_style().add_modifier(Modifier::BOLD).patch(field),
            ));
            body.push(Span::styled(
                display::truncate(Key::BarPlaceholder.tr(), room.saturating_sub(1)),
                th.faint().patch(field),
            ));
        } else {
            // Keep the cursor in view: cut the front when the text is longer than the field.
            let mut head = before.clone();
            while head.width() + 1 > room && !head.is_empty() {
                head.remove(0);
            }
            let tail_room = room.saturating_sub(head.width() + 1);
            body.push(Span::styled(head, th.base().patch(field)));
            body.push(Span::styled(
                "▏",
                th.accent_style().add_modifier(Modifier::BOLD).patch(field),
            ));
            body.push(Span::styled(
                display::truncate(&after_c, tail_room.max(1)),
                th.base().patch(field),
            ));
        }
        let used = body.width();
        body.push(Span::styled(
            " ".repeat(inner.saturating_sub(used + esc_w + 1)),
            field,
        ));
        body.push_hits(esc, Target::Address);
        body.push(Span::styled(" ", field));
    } else {
        // Idle: the crumbs, a thin rule, what to search for, and Ctrl+P.
        let key = app
            .settings
            .keymap
            .hint(Action::Palette)
            .unwrap_or_default();
        let chip = if key.is_empty() {
            Vec::new()
        } else {
            key_chip(app, look, &key, field, None)
        };
        let chip_w: usize = chip.iter().map(|s| s.content.width()).sum();
        let chip_room = if chip_w > 0 { chip_w + 1 } else { 0 };
        let natural = natural_width(&segments(app)) + 1;
        // The words of the search are worth keeping whole before the path is: the path folds
        // its middle into a "…" first, down to its beginning and its last folder.
        let whole = lead.width() + Key::BarPlaceholder.tr().width();
        let sep_w = 3;
        let avail = inner.saturating_sub(chip_room);
        let (crumb_w, search_w) = if avail >= natural + sep_w + whole {
            (natural, avail - natural - sep_w)
        } else if avail >= 34 + sep_w + whole {
            (avail - sep_w - whole, whole)
        } else if avail >= 30 + sep_w + 14 {
            // Narrower still: the words are cut short.
            (avail - sep_w - 14, 14)
        } else {
            (avail, 0)
        };
        idle_crumbs(app, crumb_w, field, &mut body);
        if search_w > 0 {
            body.push(Span::styled(" │ ", th.faint().patch(field)));
            let (text, style) = match &app.tab.filter {
                Some(f) => (
                    format!("{}{}", icons::ui(app.settings.icons, Glyph::Filter), f.text),
                    th.base().add_modifier(Modifier::BOLD),
                ),
                None => (format!("{lead}{}", Key::BarPlaceholder.tr()), th.dim()),
            };
            let target = Target::Act(Action::Filter);
            let used = body.width();
            let shown = display::truncate(&text, search_w);
            let pad_to = crumb_w.max(used.saturating_sub(sep_w)) + sep_w + search_w;
            body.push_hit(
                Span::styled(shown.clone(), style.patch(field)),
                target.clone(),
            );
            let drawn = body.width();
            if drawn < pad_to {
                body.push_hit(Span::styled(" ".repeat(pad_to - drawn), field), target);
            }
        }
        let used = body.width();
        body.push(Span::styled(
            " ".repeat(inner.saturating_sub(used + chip_room)),
            field,
        ));
        if chip_w > 0 {
            body.push_hits(chip, Target::Act(Action::Palette));
            body.push(Span::styled(" ", field));
        }
    }
    // Pad or cut to the inner width exactly.
    let w = body.width();
    if w > inner {
        body.clip(inner);
    } else if w < inner {
        body.push(Span::styled(" ".repeat(inner - w), field));
    }
    out.push(left_end);
    out.append(body);
    out.push(right_end);
    out
}

pub fn draw_address(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let look = Look::of(app);
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;
    let set = app.settings.icons;

    // The line under the field is the second row of the area, when there is one.
    let row = Rect { height: 1, ..area };

    // Buttons: back, forward, up, refresh, as little pills. Unusable ones are not clickable.
    let mut left = Bar::default();
    left.push(Span::styled(" ", band));
    let buttons = [
        (Glyph::Back, app.tab.nav.can_back(), Action::Back),
        (Glyph::Forward, app.tab.nav.can_forward(), Action::Forward),
        (Glyph::Up, app.tab.cwd.parent().is_some(), Action::Parent),
        (Glyph::Refresh, true, Action::Refresh),
    ];
    let narrow = w < 50;
    for (glyph, enabled, action) in buttons {
        if narrow && matches!(glyph, Glyph::Back | Glyph::Forward) {
            continue;
        }
        let g = icons::ui(set, glyph);
        if enabled {
            left.push_hits(
                soft::pill(&th, look, g, th.button, th.base(), band),
                Target::Act(action),
            );
        } else {
            left.push_hits(
                soft::pill(&th, look, g, th.button, th.faint(), band),
                Target::Window,
            );
        }
        left.push(Span::styled(" ", band));
    }
    left.push(Span::styled(" ", band));

    // On the right, the view in use (it opens the menu of views); not on a narrow terminal.
    let mut right = Bar::default();
    if w >= 100 {
        let (glyph, name) = view_label(app);
        let g = icons::ui(set, glyph);
        let label = if g.is_empty() {
            name.to_string()
        } else {
            format!("{g} {name}")
        };
        right.push(Span::styled(" ", band));
        right.push_hits(
            soft::pill(&th, look, &label, th.button, th.base(), band),
            Target::Menu(MenuKind::View),
        );
        right.push(Span::styled(" ", band));
    } else {
        right.push(Span::styled(" ", band));
    }

    let field_w = w.saturating_sub(left.width() + right.width());
    if field_w < 8 {
        left.render(f, &mut app.frame.hits, row);
        return;
    }
    let field_x = area.x + left.width() as u16;
    let field = field_bar(app, look, field_w);
    let field_rect = Rect {
        x: field_x,
        y: area.y,
        width: field_w as u16,
        height: 1,
    };
    app.frame.bar_field = field_rect;
    // A click anywhere in the field that is not a segment starts the typing.
    if app.windows.address.is_none() {
        app.frame.hits.add(field_rect, Target::Address);
    }
    let mut bar = left;
    bar.append(field);
    bar.append(right);
    let total = bar.width();
    if total < w {
        bar.push(Span::styled(" ".repeat(w - total), band));
    }
    bar.render(f, &mut app.frame.hits, row);
}

/// What is offered under the field: the subfolders of a path being typed, or the commands and
/// places that match the words typed.
pub fn draw_suggestions(f: &mut Frame, app: &mut App, row: Rect, screen: Rect) {
    match app.windows.address.as_ref().map(|a| a.mode()) {
        Some(BarMode::Search) => draw_bar_hits(f, app, row, screen),
        _ => draw_paths(f, app, row, screen),
    }
}

fn draw_paths(f: &mut Frame, app: &mut App, row: Rect, screen: Rect) {
    let names = app.address_suggestions();
    if names.is_empty() || row.y + row.height >= screen.height {
        return;
    }
    let th = app.settings.th.clone();
    let widest = names.iter().map(|n| n.width()).max().unwrap_or(8);
    let w = (widest + 4).clamp(24, 60).min(screen.width as usize) as u16;
    // Under the field: after the buttons.
    let x = (app.frame.bar_field.x + 2).min(screen.width.saturating_sub(w));
    let selected = app.windows.address.as_ref().and_then(|a| a.selected);
    let area = Rect {
        x,
        y: row.y + row.height,
        width: w,
        height: (names.len() as u16).min(screen.height.saturating_sub(row.y + row.height)),
    };
    if area.height == 0 {
        return;
    }
    f.render_widget(Clear, area);
    let lines: Vec<Line> = names
        .iter()
        .take(area.height as usize)
        .enumerate()
        .map(|(i, n)| {
            let st = if selected == Some(i) {
                th.selected().patch(th.base().add_modifier(Modifier::BOLD))
            } else {
                Style::default().bg(th.field).patch(th.base())
            };
            Line::from(Span::styled(
                pad(
                    &format!(" {}/", display::truncate(n, w as usize - 4)),
                    w as usize,
                ),
                st,
            ))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
    for i in 0..area.height {
        app.frame.hits.add(
            Rect {
                y: area.y + i,
                height: 1,
                ..area
            },
            Target::Suggest(i as usize),
        );
    }
}

fn draw_bar_hits(f: &mut Frame, app: &mut App, row: Rect, screen: Rect) {
    let th = app.settings.th.clone();
    let look = Look::of(app);
    let Some(a) = &app.windows.address else {
        return;
    };
    if a.hits.is_empty() || row.y + row.height >= screen.height {
        return;
    }
    let rows: Vec<(String, String, String, Vec<usize>)> = a
        .hits
        .iter()
        .map(|h| match &h.item {
            BarItem::Command(action) => (
                action.label().to_string(),
                app.settings.keymap.hint(*action).unwrap_or_default(),
                Key::BarCommand.tr().to_string(),
                h.label_pos.clone(),
            ),
            BarItem::Zoxide(path) => (
                display::name(path.file_name().unwrap_or(path.as_os_str())),
                fmt::short_path(path, app.home()),
                Key::BarZoxide.tr().to_string(),
                h.label_pos.clone(),
            ),
            BarItem::Place(item) => (
                item.label.clone(),
                item.detail.clone(),
                Key::BarGoTo.tr().to_string(),
                h.label_pos.clone(),
            ),
        })
        .collect();
    let selected = a.hit_selected;
    let label_w = rows
        .iter()
        .map(|r| r.0.width())
        .max()
        .unwrap_or(10)
        .clamp(14, 30);
    let detail_w = rows
        .iter()
        .map(|r| r.1.width())
        .max()
        .unwrap_or(0)
        .clamp(0, 32);
    let tag_w = 9usize;
    let w = (label_w + detail_w + tag_w + 10).min(screen.width as usize - 2) as u16;
    let x = (app.frame.bar_field.x + 2).min(screen.width.saturating_sub(w));
    let height = (rows.len() as u16).min(screen.height.saturating_sub(row.y + row.height));
    let area = Rect {
        x,
        y: row.y + row.height,
        width: w,
        height,
    };
    f.render_widget(Clear, area);
    let iw = area.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, (label, detail, tag, pos)) in rows.iter().enumerate().take(height as usize) {
        let sel = selected == Some(i);
        let row_style = Style::default().bg(th.field);
        let mut spans = vec![Span::styled(" ", row_style)];
        let mut used = 1;
        let shown = display::truncate(label, label_w);
        for (ci, c) in shown.chars().enumerate() {
            let hl = pos.contains(&ci);
            let st = if hl {
                th.accent_style().add_modifier(Modifier::BOLD)
            } else {
                th.base()
            };
            let st = if sel {
                st.add_modifier(Modifier::BOLD)
            } else {
                st
            };
            spans.push(Span::styled(c.to_string(), st.patch(row_style)));
            used += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        }
        let gap = (1 + label_w + 2).saturating_sub(used);
        spans.push(Span::styled(" ".repeat(gap), row_style));
        let detail = display::truncate(detail, detail_w);
        spans.push(Span::styled(
            pad(&detail, detail_w + 2),
            th.dim().patch(row_style),
        ));
        let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
        let tail = iw.saturating_sub(drawn + tag.width() + 2);
        spans.push(Span::styled(" ".repeat(tail), row_style));
        spans.push(Span::styled(
            format!("{tag}  "),
            th.faint().patch(row_style),
        ));
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), area);
    for (i, _) in rows.iter().enumerate().take(height as usize) {
        let r = Rect {
            y: area.y + i as u16,
            height: 1,
            ..area
        };
        if selected == Some(i) {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Cursor);
        }
        app.frame.hits.add(r, Target::BarHit(i));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_width_counts_separators() {
        let segs = vec![
            ("~".to_string(), PathBuf::from("/h")),
            ("a".to_string(), PathBuf::from("/h/a")),
            ("bc".to_string(), PathBuf::from("/h/a/bc")),
        ];
        assert_eq!(natural_width(&segs), 1 + 1 + 2 + 2 * SEP.width());
    }
}
