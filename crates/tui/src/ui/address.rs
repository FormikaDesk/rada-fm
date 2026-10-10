//! The address row: back, forward, up and refresh buttons; the path as clickable segments
//! (or, after Ctrl+L, as a text field with suggestions); and on the right the search field
//! that filters the folder.
//!
//! When the terminal narrows the search field shrinks and goes, then the middle of the path
//! folds into a clickable "…".

use std::path::PathBuf;

use rada_core::display;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::widgets::{Bar, SPIN, pad};
use crate::app::App;
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

const SEP: &str = " › ";

/// The first segment of a path: the home folder, or the root of the disk.
fn home_label(app: &App) -> String {
    let mark = icons::ui(app.icons, Glyph::Home);
    if mark.is_empty() {
        "Home".to_string()
    } else {
        format!("{mark} Home")
    }
}

/// The path as segments, each with the folder it stands for.
fn segments(app: &App) -> Vec<(String, PathBuf)> {
    use std::path::Component;
    let home = app.home();
    let (first, rest) = match app.cwd.strip_prefix(home) {
        Ok(rest) => ((home_label(app), home.to_path_buf()), rest),
        Err(_) => {
            let mut root = PathBuf::new();
            let mut label = String::new();
            for c in app.cwd.components() {
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
            let rest = app.cwd.strip_prefix(&root).unwrap_or(&app.cwd);
            ((label, root), rest)
        }
    };
    let mut out = vec![first];
    let mut acc = out[0].1.clone();
    for c in rest.components() {
        if let Component::Normal(n) = c {
            acc.push(n);
            out.push((display::name(n), acc.clone()));
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
    let th = &app.th;
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
                display::truncate(label, budget.max(4))
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
        out.push(seg_span(n - 1, display::truncate(&segs[n - 1].0, room)));
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

/// Width of the search field for a terminal of `w` columns.
fn search_width(w: usize, active: bool) -> usize {
    let natural = if w >= 140 {
        32
    } else if w >= 100 {
        26
    } else if w >= 70 {
        18
    } else {
        0
    };
    if active { natural.max(14) } else { natural }
}

/// The search field: an icon, "Search in demo" and the key, or what is being typed.
fn search_field(app: &App, width: usize) -> Bar<'static> {
    let th = &app.th;
    let field = Style::default().bg(th.field);
    let mut b = Bar::default();
    if width < 8 {
        return b;
    }
    let icon = icons::ui(app.icons, Glyph::Search);
    let lead = if icon.is_empty() {
        " ".to_string()
    } else {
        format!(" {icon} ")
    };
    let (text, style) = if let Some(f) = &app.filter {
        (
            format!("{}{}", f.text, if f.editing { "▏" } else { "" }),
            th.base().add_modifier(Modifier::BOLD),
        )
    } else {
        let name = if app.cwd == app.home() {
            "Home".to_string()
        } else {
            app.cwd
                .file_name()
                .map(display::name)
                .unwrap_or_else(|| display::path(&app.cwd))
        };
        (format!("Search in {name}"), th.dim())
    };
    let key = if app.filter.is_none() {
        app.keymap.hint(Action::Filter).unwrap_or_default()
    } else {
        String::new()
    };
    let key_w = if key.is_empty() { 0 } else { key.width() + 1 };
    let room = width.saturating_sub(lead.width() + key_w + 1);
    // What is typed shows its end; the placeholder shows its beginning.
    let shown = if app.filter.is_some() {
        super::widgets::tail(&text, room)
    } else {
        display::truncate(&text, room)
    };
    let gap = width.saturating_sub(lead.width() + shown.width() + key_w);
    b.push_hit(
        Span::styled(lead, th.dim().patch(field)),
        Target::Act(Action::Filter),
    );
    b.push_hit(
        Span::styled(shown, style.patch(field)),
        Target::Act(Action::Filter),
    );
    b.push_hit(
        Span::styled(" ".repeat(gap), field),
        Target::Act(Action::Filter),
    );
    if key_w > 0 {
        b.push_hit(
            Span::styled(format!("{key} "), th.faint().patch(field)),
            Target::Act(Action::Filter),
        );
    }
    b
}

pub fn draw_address(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;
    let set = app.icons;
    let field = Style::default().bg(th.field);

    // Buttons: back, forward, up, refresh. Unusable ones are not clickable.
    let mut left = Bar::default();
    left.push(Span::styled(" ", band));
    let buttons = [
        (Glyph::Back, app.nav.can_back(), Action::Back),
        (Glyph::Forward, app.nav.can_forward(), Action::Forward),
        (Glyph::Up, app.cwd.parent().is_some(), Action::Parent),
        (Glyph::Refresh, true, Action::Refresh),
    ];
    let narrow = w < 50;
    for (glyph, enabled, action) in buttons {
        if narrow && matches!(glyph, Glyph::Back | Glyph::Forward) {
            continue;
        }
        let text = format!(" {} ", icons::ui(set, glyph));
        if enabled {
            left.push_hit(
                Span::styled(text, th.base().patch(Style::default().bg(th.button))),
                Target::Act(action),
            );
        } else {
            left.push(Span::styled(text, th.faint().patch(band)));
        }
        left.push(Span::styled(" ", band));
    }

    let search_w = search_width(w, app.filter.is_some());
    let search = search_field(app, search_w);
    let after = if search_w > 0 { 2 } else { 1 };
    let field_w = w.saturating_sub(left.width() + search.width() + after);
    if field_w < 6 {
        left.render(f, &mut app.hits, area);
        return;
    }
    let field_x = area.x + left.width() as u16;

    // The path field.
    let mut path_bar = Bar::default();
    let field_rect = Rect {
        x: field_x,
        y: area.y,
        width: field_w as u16,
        height: 1,
    };
    if let Some(a) = &app.address {
        // A text field: what is typed, with the cursor.
        let before: String = a.text.chars().take(a.cursor).collect();
        let after_c: String = a.text.chars().skip(a.cursor).collect();
        let room = field_w.saturating_sub(2);
        // Keep the cursor in view: cut the front when the text is longer than the field.
        let mut head = before.clone();
        while head.width() + 1 > room && !head.is_empty() {
            head.remove(0);
        }
        let tail_room = room.saturating_sub(head.width() + 1);
        let tail = display::truncate(&after_c, tail_room.max(1));
        path_bar.push(Span::styled(" ", field));
        path_bar.push(Span::styled(head, th.base().patch(field)));
        path_bar.push(Span::styled(
            "▏",
            th.accent_style().add_modifier(Modifier::BOLD).patch(field),
        ));
        path_bar.push(Span::styled(tail, th.base().patch(field)));
    } else {
        path_bar.push(Span::styled(" ", field));
        let loading = app.is_loading();
        let spin_w = if loading { 2 } else { 0 };
        for (span, target) in crumbs(app, field_w.saturating_sub(2 + spin_w), field) {
            match target {
                Some(t) => path_bar.push_hit(span, t),
                None => path_bar.push(span),
            }
        }
        if loading {
            path_bar.push(Span::styled(
                format!(" {}", SPIN[app.spinner % SPIN.len()]),
                th.fg(th.warn).patch(field),
            ));
        }
    }
    let used = path_bar.width();
    path_bar.push(Span::styled(
        " ".repeat(field_w.saturating_sub(used)),
        field,
    ));
    // A click anywhere in the field that is not a segment edits the address.
    if app.address.is_none() {
        app.hits.add(field_rect, Target::Address);
    }

    let mut bar = left;
    bar.append(path_bar);
    bar.push(Span::styled(" ", band));
    if search_w > 0 {
        bar.append(search);
        bar.push(Span::styled(" ", band));
    }
    let total = bar.width();
    if total < w {
        bar.push(Span::styled(" ".repeat(w - total), band));
    }
    bar.render(f, &mut app.hits, area);
}

/// The suggestions of the address field, drawn over what is below it.
pub fn draw_suggestions(f: &mut Frame, app: &mut App, row: Rect, screen: Rect) {
    let names = app.address_suggestions();
    if names.is_empty() || row.y + 1 >= screen.height {
        return;
    }
    let th = app.th.clone();
    let widest = names.iter().map(|n| n.width()).max().unwrap_or(8);
    let w = (widest + 4).clamp(24, 60).min(screen.width as usize) as u16;
    // Under the field: after the buttons, which are four cells each.
    let x = (row.x + 12).min(screen.width.saturating_sub(w));
    let selected = app.address.as_ref().and_then(|a| a.selected);
    let area = Rect {
        x,
        y: row.y + 1,
        width: w,
        height: (names.len() as u16).min(screen.height - row.y - 1),
    };
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
        app.hits.add(
            Rect {
                y: area.y + i,
                height: 1,
                ..area
            },
            Target::Suggest(i as usize),
        );
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

    #[test]
    fn the_search_field_shrinks_with_the_terminal() {
        assert!(search_width(170, false) > search_width(120, false));
        assert!(search_width(120, false) > search_width(80, false));
        assert_eq!(search_width(60, false), 0);
        // While it is in use it keeps enough room to type.
        assert!(search_width(60, true) >= 14);
    }
}
