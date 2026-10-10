//! The bottom of the screen: a status bar (how many items, what is selected, what waits on
//! the clipboard, the free space of the disk and the Details / Icons switch) and below it a
//! row of key hints drawn as little keys.
//!
//! The hints come from the real keymap, so they follow the scheme in use and whatever the
//! user rebound, and each one can be clicked.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::widgets::{Bar, bar_spans};
use crate::app::{App, Modal};
use crate::fmt;
use crate::hits::Target;
use crate::icons::{self, Glyph, IconSet};
use crate::keymap::Action;
use crate::view::{LayoutKind, ViewMode};

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

/// The hints that fit the situation, most useful first; the row shows as many as fit.
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
    if app.address.is_some() {
        return vec![
            Hint::fixed("Enter", "go"),
            Hint::fixed("Tab", "complete"),
            Hint::fixed("↑↓", "suggestions"),
            Hint::fixed("Esc", "cancel"),
        ];
    }
    if let Some(f) = &app.filter
        && f.editing
    {
        return vec![Hint::fixed("Enter", "keep"), Hint::fixed("Esc", "clear")];
    }
    if app.side_focus {
        let mut v = vec![Hint::fixed("Enter", "open")];
        v.extend(Hint::act(app, SwitchPane, "list"));
        v.extend(Hint::act(app, Bookmark, "pin"));
        v.push(Hint::fixed("Del", "unpin"));
        v.extend(Hint::act(app, ToggleSidebar, "hide"));
        return v;
    }
    let mut v = if app.marked.is_empty() && app.in_archive() {
        acts(&[
            (Open, "open"),
            (Copy, "copy out"),
            (ExtractHere, "extract all"),
            (ToggleMark, "select"),
            (Parent, "back"),
            (Undo, "undo"),
            (Help, "help"),
        ])
    } else if app.marked.is_empty() {
        let mut first = if app.on_archive_item() {
            acts(&[(ExtractHere, "extract"), (ExtractToFolder, "extract to…")])
        } else {
            Vec::new()
        };
        first.extend(acts(&[
            (Open, "open"),
            (ToggleMark, "select"),
            (Copy, "copy"),
            (Paste, "paste"),
            (Trash, "trash"),
            (Rename, "rename"),
            (Undo, "undo"),
            (NewTab, "new tab"),
            (Palette, "go to"),
            // Extras, shown when there is room.
            (Cut, "cut"),
            (NewFolder, "new folder"),
            (FocusAddress, "address"),
            (Compress, "compress"),
            (Help, "help"),
        ]));
        first
    } else {
        let mut v = acts(&[
            (Copy, "copy"),
            (Cut, "cut"),
            (Trash, "trash"),
            (DeletePermanently, "delete"),
            (BulkRename, "rename all"),
            (Compress, "compress"),
        ]);
        if app.clipboard.is_some() {
            v.extend(acts(&[(Paste, "paste")]));
        }
        v.extend(acts(&[(ClearSelection, "clear")]));
        v
    };
    if app.filter.is_some() && app.marked.is_empty() {
        v.insert(0, Hint::fixed("Esc", "clear search"));
    }
    v
}

/// "19 items", "3 of 19 items".
fn count_text(app: &App) -> String {
    let total = app.listing.all().len();
    if app.filter.is_some() {
        format!(
            "{} of {}",
            app.visible.len(),
            fmt::count(total as u64, "item", "items")
        )
    } else {
        fmt::count(app.visible.len() as u64, "item", "items")
    }
}

fn status_left<'a>(app: &App) -> Bar<'a> {
    let th = &app.th;
    let band = th.band();
    let mut b = Bar::default();
    b.push(Span::styled(" ", band));
    if let Some(text) = app.side_description() {
        b.push(Span::styled(
            format!("▸ {text}"),
            th.base().add_modifier(Modifier::BOLD).patch(band),
        ));
        b.push(Span::styled("  │  ", th.faint().patch(band)));
    }
    if app.layout == LayoutKind::Compact {
        b.push(Span::styled(
            fmt::short_path(&app.cwd, app.home()),
            th.accent_style().add_modifier(Modifier::BOLD).patch(band),
        ));
        b.push(Span::styled("  │  ", th.faint().patch(band)));
    }
    b.push(Span::styled(count_text(app), th.base().patch(band)));
    if !app.marked.is_empty() {
        let size: u64 = app
            .listing
            .all()
            .iter()
            .filter(|e| app.marked.contains(&e.name) && !e.is_dir())
            .map(|e| e.size)
            .sum();
        let tick = icons::ui(app.icons, Glyph::Checked);
        b.push(Span::styled("  │  ", th.faint().patch(band)));
        b.push(Span::styled(
            format!("{tick} {} selected · {}", app.marked.len(), fmt::size(size)),
            th.fg(th.check).add_modifier(Modifier::BOLD).patch(band),
        ));
    }
    if let Some(c) = &app.clipboard {
        let icon = icons::ui(app.icons, Glyph::Clipboard);
        let verb = if c.mode == rada_core::ops::TransferMode::Copy {
            "ready to paste"
        } else {
            "ready to move"
        };
        b.push(Span::styled("  │  ", th.faint().patch(band)));
        b.push(Span::styled(
            format!(
                "{icon} {} {verb}",
                fmt::count(c.paths.len() as u64, "item", "items")
            ),
            th.fg(th.kinds.vector).patch(band),
        ));
    }
    if app.show_hidden {
        b.push(Span::styled("  │  ", th.faint().patch(band)));
        b.push(Span::styled(
            "hidden files shown",
            th.fg(th.warn).patch(band),
        ));
    }
    b
}

/// The free space of the disk the current folder is on, with a bar of how full it is.
fn free_space<'a>(app: &App, with_bar: bool) -> Bar<'a> {
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
    if with_bar && let Some(total) = v.total.filter(|t| *t > 0) {
        let used = 1.0 - avail as f64 / total as f64;
        b.spans.extend(bar_spans(th, used, 10, th.disk, band.bg));
        b.push(Span::styled(" ", band));
    }
    b
}

/// The two buttons that switch between Details and Icons.
fn view_buttons<'a>(app: &App) -> Bar<'a> {
    let th = &app.th;
    let band = th.band();
    let mut b = Bar::default();
    for (mode, glyph) in [
        (ViewMode::Details, Glyph::Details),
        (ViewMode::Icons, Glyph::Icons),
    ] {
        let style = if app.view == mode {
            th.accent_style()
                .add_modifier(Modifier::BOLD)
                .patch(th.chip_row())
        } else {
            th.dim().patch(band)
        };
        b.push_hit(
            Span::styled(format!(" {} ", icons::ui(app.icons, glyph)), style),
            Target::ViewMode(mode),
        );
    }
    b.push(Span::styled(" ", band));
    b
}

pub fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;

    let buttons = view_buttons(app);
    let mut right = free_space(app, true);
    right.append(view_buttons(app));
    if right.width() > w / 2 {
        right = free_space(app, false);
        right.append(view_buttons(app));
    }
    if right.width() > w.saturating_sub(12) {
        right = buttons;
    }
    let mut left = status_left(app);
    let room = w.saturating_sub(right.width() + 1);
    left.clip(room);
    let gap = w.saturating_sub(left.width() + right.width());
    left.push(Span::styled(" ".repeat(gap), band));
    left.append(right);
    left.render(f, &mut app.hits, area);
}

/// The strip of key hints: a background of its own set apart from the status bar by a row of
/// ▄ above and ▀ below in the strip's colour (when there are three rows), each key a pill —
/// rounded with Nerd Font glyphs, a plain rectangle with a space on each side otherwise —
/// and its description faint after it.
pub fn draw_hints(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let strip = Style::default().bg(th.panel);
    let w = area.width as usize;
    let inner = if area.height >= 3 {
        let edge = |glyph: &str| {
            Paragraph::new(Line::from(Span::styled(
                glyph.repeat(w),
                Style::default().fg(th.panel),
            )))
        };
        f.render_widget(edge("▄"), Rect { height: 1, ..area });
        f.render_widget(
            edge("▀"),
            Rect {
                y: area.y + area.height - 1,
                height: 1,
                ..area
            },
        );
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        }
    } else {
        area
    };
    f.render_widget(Block::default().style(strip), inner);
    let rounded = app.icons == IconSet::Nerd;
    let all = hints(app);

    // As many as fit, two spaces from the left edge and four between groups.
    let pill_w = |h: &Hint| h.key.width() + if rounded { 2 } else { 2 };
    let chunk = |h: &Hint| pill_w(h) + 1 + h.what.width();
    let mut shown: Vec<&Hint> = Vec::new();
    let mut used = 2;
    for h in &all {
        let need = chunk(h) + if shown.is_empty() { 0 } else { 4 };
        if used + need > w {
            break;
        }
        used += need;
        shown.push(h);
    }

    let mut bar = Bar::default();
    bar.push(Span::styled("  ", strip));
    for (i, h) in shown.iter().enumerate() {
        if i > 0 {
            bar.push(Span::styled("    ", strip));
        }
        let key_style = Style::default()
            .fg(th.key_text)
            .bg(th.key_bg)
            .add_modifier(Modifier::BOLD);
        let edge_style = Style::default().fg(th.key_bg).bg(th.panel);
        let (left, text, right) = if rounded {
            (
                Span::styled("\u{e0b6}", edge_style),
                Span::styled(h.key.clone(), key_style),
                Span::styled("\u{e0b4}", edge_style),
            )
        } else {
            (
                Span::styled(" ", key_style),
                Span::styled(h.key.clone(), key_style),
                Span::styled(" ", key_style),
            )
        };
        let what = Span::styled(format!(" {}", h.what), th.dim().patch(strip));
        match &h.click {
            Some(t) => {
                bar.push_hit(left, t.clone());
                bar.push_hit(text, t.clone());
                bar.push_hit(right, t.clone());
                bar.push_hit(what, t.clone());
            }
            None => {
                bar.push(left);
                bar.push(text);
                bar.push(right);
                bar.push(what);
            }
        }
    }
    bar.render(f, &mut app.hits, inner);
}
