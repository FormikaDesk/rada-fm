//! The bottom bar: state on the left (what is selected, what is on the clipboard), key
//! hints in the middle drawn as little keys, and the free space of the current disk on the
//! right. The hints depend on what is going on: a selection, the folder filter, an open
//! window. Each one is clickable.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Span;
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthStr;

use super::widgets::{Bar, bar_spans};
use crate::app::{App, Modal};
use crate::fmt;
use crate::hits::Target;
use crate::icons::IconSet;
use crate::keymap::Action;

/// One key hint: the key as written on the cap, what it does, and what a click does.
struct Hint {
    key: String,
    what: &'static str,
    click: Option<Target>,
}

impl Hint {
    fn act(app: &App, action: Action, what: &'static str) -> Option<Hint> {
        app.keymap.hint(action).map(|key| Hint {
            key,
            what,
            click: Some(Target::Act(action)),
        })
    }

    fn fixed(key: &str, what: &'static str) -> Hint {
        Hint {
            key: key.to_string(),
            what,
            click: None,
        }
    }
}

/// The hints that fit the situation, most useful first; the bar shows as many as there is
/// room for.
fn hints(app: &App) -> Vec<Hint> {
    use Action::*;
    let acts = |list: &[(Action, &'static str)]| -> Vec<Hint> {
        list.iter()
            .filter_map(|(a, w)| Hint::act(app, *a, w))
            .collect()
    };
    if let Some(modal) = &app.modal {
        return match modal {
            Modal::Plan(_) => vec![Hint::fixed("Enter", "run"), Hint::fixed("Esc", "cancel")],
            Modal::Input(_) => vec![Hint::fixed("Enter", "ok"), Hint::fixed("Esc", "cancel")],
            Modal::Palette(_) => vec![Hint::fixed("Enter", "go"), Hint::fixed("Esc", "close")],
            Modal::ConfirmQuit => vec![Hint::fixed("Enter", "quit"), Hint::fixed("Esc", "stay")],
            Modal::Scanning { .. } => vec![Hint::fixed("Esc", "cancel")],
            _ => vec![Hint::fixed("Esc", "close")],
        };
    }
    if let Some(f) = &app.filter
        && f.editing
    {
        return vec![Hint::fixed("Enter", "keep"), Hint::fixed("Esc", "clear")];
    }
    let mut v = if app.marked.is_empty() {
        acts(&[
            (Open, "open"),
            (ToggleMark, "mark"),
            (Copy, "copy"),
            (Cut, "cut"),
            (Paste, "paste"),
            (Trash, "trash"),
            (Rename, "rename"),
            (Undo, "undo"),
            // Extras, shown when there is room.
            (NewFolder, "new folder"),
            (BulkRename, "bulk rename"),
            (Bookmark, "bookmark"),
            (ToggleHidden, "hidden"),
            (Help, "help"),
        ])
    } else {
        let mut v = acts(&[
            (Copy, "copy"),
            (Cut, "cut"),
            (Trash, "trash"),
            (DeletePermanently, "delete"),
            (BulkRename, "bulk rename"),
        ]);
        if app.clipboard.is_some() {
            v.extend(acts(&[(Paste, "paste")]));
        }
        v.extend(acts(&[(ClearSelection, "clear")]));
        v
    };
    if app.filter.is_some() && app.marked.is_empty() {
        // A kept filter: the way out is worth a hint.
        v.insert(0, Hint::fixed("Esc", "clear filter"));
    }
    v
}

/// The state on the left.
fn status<'a>(app: &App) -> Bar<'a> {
    let th = &app.th;
    let band = th.band();
    let mut b = Bar::default();
    b.push(Span::styled(" ", band));
    if !app.marked.is_empty() {
        let size: u64 = app
            .listing
            .all()
            .iter()
            .filter(|e| app.marked.contains(&e.name))
            .map(|e| e.size)
            .sum();
        b.push(Span::styled(
            format!("● {} selected · {}", app.marked.len(), fmt::size(size)),
            th.accent_style().add_modifier(Modifier::BOLD).patch(band),
        ));
        b.push(Span::styled("   ", band));
    }
    if let Some(c) = &app.clipboard {
        let icon = if app.icons == IconSet::Nerd {
            "\u{f0ea}"
        } else {
            "⎘"
        };
        let verb = if c.mode == rada_core::ops::TransferMode::Copy {
            "ready to paste"
        } else {
            "ready to move"
        };
        b.push(Span::styled(
            format!(
                "{icon} {} {verb}",
                fmt::count(c.paths.len() as u64, "item", "items")
            ),
            th.fg(th.kinds.vector).patch(band),
        ));
        b.push(Span::styled("   ", band));
    }
    if app.show_hidden {
        b.push(Span::styled(
            "hidden files shown",
            th.fg(th.warn).patch(band),
        ));
        b.push(Span::styled("   ", band));
    }
    b
}

/// The free space of the disk the current folder is on, with a bar of how full it is.
fn free_space<'a>(app: &App) -> Bar<'a> {
    let th = &app.th;
    let band = th.band();
    let mut b = Bar::default();
    let Some(v) = app
        .volumes
        .iter()
        .filter(|v| app.cwd.starts_with(&v.mount_point))
        .max_by_key(|v| v.mount_point.as_os_str().len())
    else {
        return b;
    };
    let Some(avail) = v.available else { return b };
    b.push(Span::styled(
        format!("{} free ", fmt::size(avail)),
        th.dim().patch(band),
    ));
    if let Some(total) = v.total.filter(|t| *t > 0) {
        let used = 1.0 - avail as f64 / total as f64;
        b.spans.extend(bar_spans(th, used, 6, th.accent, band.bg));
    }
    b.push(Span::styled(" ", band));
    b
}

pub fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;

    let left = status(app);
    let right = free_space(app);
    let (lw, rw) = (left.width(), right.width());

    // The hints take what is left, centred between the two sides.
    let mut mid = Bar::default();
    if app.show_hints {
        let room = w.saturating_sub(lw + rw + 4);
        let mut used = 0;
        for h in hints(app) {
            let chunk = h.key.width() + 2 + 1 + h.what.width() + 3;
            if used + chunk > room {
                break;
            }
            used += chunk;
            let cap = Span::styled(
                format!(" {} ", h.key),
                th.chip().add_modifier(Modifier::BOLD),
            );
            let what = Span::styled(format!(" {}   ", h.what), th.dim().patch(band));
            match h.click {
                Some(t) => {
                    mid.push_hit(cap, t.clone());
                    mid.push_hit(what, t);
                }
                None => {
                    mid.push(cap);
                    mid.push(what);
                }
            }
        }
    }
    let free = w.saturating_sub(lw + rw + mid.width());
    let before = free / 2;
    let mut bar = left;
    bar.push(Span::styled(" ".repeat(before), band));
    bar.append(mid);
    let after = w.saturating_sub(bar.width() + rw);
    bar.push(Span::styled(" ".repeat(after), band));
    bar.append(right);
    bar.render(f, &mut app.hits, area);
}
