//! The top bar: navigation buttons, the path, and on the right the folder filter, the
//! jump button and how many items the folder holds.
//!
//! Things you can click are drawn as chips (a tone of their own); everything else is plain
//! text. When the terminal is narrow the bar gives way in a fixed order: first the item
//! count goes, then the middle of the path is folded into a clickable "…", and only as a
//! last resort do the key hints of the two buttons go. The words "Filter" and "Go to"
//! stay.

use std::path::PathBuf;

use rada_core::display;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Span;
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthStr;

use super::widgets::{Bar, SPIN};
use crate::app::App;
use crate::fmt;
use crate::hits::Target;
use crate::icons::IconSet;
use crate::keymap::Action;

const SEP: &str = " › ";

/// The home folder's mark in the path: a house where there are icons, `~` where there are
/// none.
fn home_mark(icons: IconSet) -> &'static str {
    match icons {
        IconSet::Nerd => "\u{f015}",
        IconSet::Unicode => "⌂",
        IconSet::None => "~",
    }
}

/// The path as segments, each with the folder it stands for. The first is the home folder
/// (or the root of the disk for a path outside it).
fn segments(app: &App) -> Vec<(String, PathBuf)> {
    use std::path::Component;
    let home = app.home();
    let (first, rest) = match app.cwd.strip_prefix(home) {
        Ok(rest) => ((home_mark(app.icons).to_string(), home.to_path_buf()), rest),
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

/// What the path would take if nothing were folded.
fn natural_width(segs: &[(String, PathBuf)]) -> usize {
    segs.iter().map(|(s, _)| s.width()).sum::<usize>() + SEP.width() * segs.len().saturating_sub(1)
}

/// The path as spans fitting in `budget` cells: all of it, or the first segment, a "…" for
/// the middle (clickable: it lists what it hides) and as many of the last segments as fit.
fn crumbs(app: &App, budget: usize) -> Vec<(Span<'static>, Option<Target>)> {
    let th = &app.th;
    let band = th.band();
    let segs = segments(app);
    let n = segs.len();
    let sep = || (Span::styled(SEP, th.faint().patch(band)), None);
    let seg_span = |i: usize, text: String| {
        let style = if i + 1 == n {
            th.accent_style().add_modifier(Modifier::BOLD)
        } else {
            th.dim()
        };
        (
            Span::styled(text, style.patch(band)),
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
    // First segment, "…", then the longest tail that fits.
    let head = w(0) + sep_w + 1;
    let tail_from = |from: usize| -> usize { (from..n).map(|j| sep_w + w(j)).sum() };
    let mut start = n - 1;
    if head + tail_from(n - 1) > budget {
        // Not even the first segment and the last one: keep only the end of the path.
        out.push((
            Span::styled("…", th.base().patch(band)),
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
        Span::styled("…", th.base().add_modifier(Modifier::BOLD).patch(band)),
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

/// A button-like field: `⌕ Filter…  Ctrl+F`. The words stay; the key may go.
fn field<'a>(app: &App, icon: &str, label: &str, key: Option<String>, target: Target) -> Bar<'a> {
    let th = &app.th;
    let mut b = Bar::default();
    b.push_hit(
        Span::styled(format!(" {icon}{label} "), th.chip()),
        target.clone(),
    );
    if let Some(k) = key {
        b.push_hit(Span::styled(format!("{k} "), th.chip_key()), target);
    }
    b
}

/// The right-hand side for one level of squeezing.
fn right_side<'a>(app: &App, count: bool, keys: bool) -> Bar<'a> {
    let th = &app.th;
    let band = th.band();
    let mut b = Bar::default();
    let key = |a: Action| if keys { app.keymap.hint(a) } else { None };

    if let Some(f) = &app.filter {
        // The field shows what is being typed; its label has nothing to say any more.
        let text = format!(" ▽ {}{} ", f.text, if f.editing { "▏" } else { "" });
        b.push_hit(
            Span::styled(text, th.chip().add_modifier(Modifier::BOLD)),
            Target::Act(Action::Filter),
        );
    } else {
        b.append(field(
            app,
            if app.icons == IconSet::None {
                ""
            } else {
                "⌕ "
            },
            "Filter…",
            key(Action::Filter),
            Target::Act(Action::Filter),
        ));
    }
    b.push(Span::styled(" ", band));
    b.append(field(
        app,
        "",
        "Go to…",
        key(Action::Palette),
        Target::Act(Action::Palette),
    ));
    if count {
        let total = app.listing.all().len();
        let text = if app.filter.is_some() {
            format!("{} of {}", app.visible.len(), total)
        } else {
            fmt::count(app.visible.len() as u64, "item", "items")
        };
        b.push(Span::styled(format!("  {text}"), th.dim().patch(band)));
    }
    if app.is_loading() {
        b.push(Span::styled(
            format!(" {}", SPIN[app.spinner % SPIN.len()]),
            th.fg(th.warn).patch(band),
        ));
    }
    b.push(Span::styled(" ", band));
    b
}

pub fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;

    // Left: back, forward, parent. On a terminal too narrow for anything else the first
    // two give way to the parent button.
    let build_left = |app: &App, all: bool| -> Bar<'static> {
        let mut left = Bar::default();
        left.push(Span::styled(" ", band));
        let buttons = [
            ("‹", app.nav.can_back(), Action::Back, all),
            ("›", app.nav.can_forward(), Action::Forward, all),
            ("↑", app.cwd.parent().is_some(), Action::Parent, true),
        ];
        for (glyph, enabled, action, shown) in buttons {
            if !shown {
                continue;
            }
            let text = format!(" {glyph} ");
            if enabled {
                left.push_hit(Span::styled(text, th.chip()), Target::Act(action));
            } else {
                // Not usable right now: no chip, just a faint glyph.
                left.push(Span::styled(text, th.faint().patch(band)));
            }
            left.push(Span::styled(" ", band));
        }
        left.push(Span::styled(" ", band));
        left
    };
    let mut left = build_left(app, true);

    // Squeeze the right side as little as possible: the count goes first, then the path
    // folds, and the keys of the two buttons are the last thing to give.
    let natural = natural_width(&segments(app));
    let mut right = right_side(app, true, true);
    let fits = |l: &Bar, r: &Bar| w.saturating_sub(l.width() + r.width() + 1) >= natural;
    if !fits(&left, &right) {
        right = right_side(app, false, true);
        if w.saturating_sub(left.width() + right.width() + 1) < 14 {
            right = right_side(app, false, false);
        }
        if w.saturating_sub(left.width() + right.width() + 1) < 8 {
            left = build_left(app, false);
        }
    }
    let budget = w.saturating_sub(left.width() + right.width() + 1);

    let mut bar = left;
    for (span, target) in crumbs(app, budget) {
        match target {
            Some(t) => bar.push_hit(span, t),
            None => bar.push(span),
        }
    }
    let gap = w.saturating_sub(bar.width() + right.width());
    bar.push(Span::styled(" ".repeat(gap), band));
    bar.append(right);
    bar.render(f, &mut app.hits, area);
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
