//! The bottom of the screen: a status bar (how many items, what is selected, what waits on
//! the clipboard, the free space of the disk and the Details / Icons / Columns switch) and
//! below it a row of key hints drawn as little keys.
//!
//! The hints come from the real keymap, so they follow the scheme in use and whatever the
//! user rebound, and each one can be clicked.

use rada_core::i18n::{Key, tf};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look};
use super::widgets::{Bar, key_name, segment_bar};
use crate::app::{App, BarMode, Modal};
use crate::fmt;
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;
use crate::view::{LayoutKind, ViewMode};

/// One key hint: the key as written on the cap, what it does, and what a click does.
struct Hint {
    key: String,
    what: Key,
    click: Option<Target>,
}

impl Hint {
    fn act(app: &App, action: Action, what: Key) -> Option<Hint> {
        app.settings.keymap.hint(action).map(|key| Hint {
            key,
            what,
            click: Some(Target::Act(action)),
        })
    }

    fn fixed(key: &str, what: Key) -> Hint {
        Hint {
            key: key_name(key).to_string(),
            what,
            click: None,
        }
    }
}

/// The hints that fit the situation, most useful first; the row shows as many as fit.
fn hints(app: &App) -> Vec<Hint> {
    use Action::*;
    let acts = |list: &[(Action, Key)]| -> Vec<Hint> {
        list.iter()
            .filter_map(|(a, w)| Hint::act(app, *a, *w))
            .collect()
    };
    if let Some(modal) = &app.modal {
        return match modal {
            Modal::Plan(_) => vec![
                Hint::fixed("Enter", Key::HRun),
                Hint::fixed("Esc", Key::HCancel),
            ],
            Modal::Input(_) => vec![
                Hint::fixed("Enter", Key::HOk),
                Hint::fixed("Esc", Key::HCancel),
            ],
            Modal::Palette(_) => vec![
                Hint::fixed("Enter", Key::HGo),
                Hint::fixed("Esc", Key::HClose),
            ],
            Modal::OpenWith(v) => {
                let mut h = vec![
                    Hint::fixed("↑↓", Key::HChoose),
                    Hint::fixed("Enter", Key::HOpen),
                    Hint::fixed("Tab", Key::HAlways),
                ];
                // Opened from a menu, Left gives the menu back.
                if v.under.is_some() && v.filter.is_empty() {
                    h.push(Hint::fixed("←", Key::HBack));
                }
                h.push(Hint::fixed("Esc", Key::HClose));
                h
            }
            // Only the keys that do something now: Right where an entry opens a menu, Left
            // where there is a menu to go back to.
            Modal::Menu(m) => {
                let mut h = Vec::new();
                if m.items.iter().filter(|i| i.selectable()).count() > 1 {
                    h.push(Hint::fixed("↑↓", Key::HChoose));
                }
                if m.items
                    .get(m.selected)
                    .is_some_and(|i| i.enabled && i.opens_menu())
                {
                    h.push(Hint::fixed("→", Key::HOpen));
                }
                if m.parent.is_some() {
                    h.push(Hint::fixed("←", Key::HBack));
                }
                h.push(Hint::fixed("Enter", Key::HConfirm));
                h.push(Hint::fixed("Esc", Key::HClose));
                h
            }
            Modal::ConfirmQuit => vec![
                Hint::fixed("Enter", Key::HQuit),
                Hint::fixed("Esc", Key::HStay),
            ],
            Modal::Scanning { .. } => vec![Hint::fixed("Esc", Key::HStop)],
            _ => vec![Hint::fixed("Esc", Key::HClose)],
        };
    }
    if let Some(a) = &app.address {
        return if a.mode() == BarMode::Search {
            vec![
                Hint::fixed("Enter", Key::HGo),
                Hint::fixed("↑↓", Key::HSuggestions),
                Hint::fixed("Esc", Key::HCancel),
            ]
        } else {
            vec![
                Hint::fixed("Enter", Key::HGo),
                Hint::fixed("Tab", Key::HComplete),
                Hint::fixed("↑↓", Key::HSuggestions),
                Hint::fixed("Esc", Key::HCancel),
            ]
        };
    }
    if let Some(f) = &app.filter
        && f.editing
    {
        return vec![
            Hint::fixed("Enter", Key::HKeep),
            Hint::fixed("Esc", Key::HClear),
        ];
    }
    if app.side_nav.focus {
        let mut v = vec![Hint::fixed("Enter", Key::HOpen)];
        v.extend(Hint::act(app, SwitchPane, Key::HList));
        v.extend(Hint::act(app, Bookmark, Key::HPin));
        v.push(Hint::fixed("Del", Key::HUnpin));
        v.extend(Hint::act(app, ToggleSidebar, Key::HHide));
        return v;
    }
    let mut v = if app.marked.is_empty() && app.in_archive() {
        acts(&[
            (Open, Key::HOpen),
            (Copy, Key::HCopyOut),
            (ExtractHere, Key::HExtractAll),
            (ToggleMark, Key::HSelect),
            (Parent, Key::HBack),
            (Undo, Key::HUndo),
            (Help, Key::HHelp),
        ])
    } else if app.marked.is_empty() {
        let mut first = if app.on_archive_item() {
            acts(&[
                (ExtractHere, Key::HExtract),
                (ExtractToFolder, Key::HExtractTo),
            ])
        } else {
            Vec::new()
        };
        first.extend(acts(&[
            (Open, Key::HOpen),
            (ToggleMark, Key::HSelect),
            (Copy, Key::HCopy),
            (Paste, Key::HPaste),
            (Trash, Key::HTrash),
            (Rename, Key::HRename),
            (Undo, Key::HUndo),
            (NewTab, Key::HNewTab),
            (Palette, Key::HCommands),
            // Extras, shown when there is room.
            (Cut, Key::HCut),
            (NewFolder, Key::HNewFolder),
            (FocusAddress, Key::HAddress),
            (Compress, Key::HCompress),
            (Help, Key::HHelp),
        ]));
        first
    } else {
        let mut v = acts(&[
            (Copy, Key::HCopy),
            (Cut, Key::HCut),
            (Trash, Key::HTrash),
            (DeletePermanently, Key::HDelete),
            (BulkRename, Key::HRenameAll),
            (Compress, Key::HCompress),
        ]);
        if app.ops.clipboard.is_some() {
            v.extend(acts(&[(Paste, Key::HPaste)]));
        }
        v.extend(acts(&[(ClearSelection, Key::HDeselect)]));
        v
    };
    if app.filter.is_some() && app.marked.is_empty() {
        v.insert(0, Hint::fixed("Esc", Key::HClearSearch));
    }
    v
}

/// "19 items", "3 of 19 items".
pub(super) fn count_text(app: &App) -> String {
    let total = app.listing.all().len();
    if app.filter.is_some() {
        tf(
            Key::ItemsOfTotal,
            &[
                &app.visible.len(),
                &fmt::count(total as u64, Key::ItemOne, Key::ItemMany),
            ],
        )
    } else {
        fmt::count(app.visible.len() as u64, Key::ItemOne, Key::ItemMany)
    }
}

fn status_left<'a>(app: &App) -> Bar<'a> {
    let th = &app.settings.th;
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
    if app.settings.layout == LayoutKind::Compact {
        b.push(Span::styled(
            match app.view_title() {
                Some(title) => title,
                None => fmt::short_path(&app.cwd, app.home()),
            },
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
        let tick = icons::ui(app.settings.icons, Glyph::Checked);
        b.push(Span::styled("  │  ", th.faint().patch(band)));
        b.push(Span::styled(
            format!(
                "{tick} {}",
                if app.marked.len() == 1 {
                    tf(Key::SelectedSizeOne, &[&fmt::size(size)])
                } else {
                    tf(
                        Key::SelectedSizeMany,
                        &[&app.marked.len(), &fmt::size(size)],
                    )
                }
            ),
            th.fg(th.check).add_modifier(Modifier::BOLD).patch(band),
        ));
    }
    if let Some(c) = &app.ops.clipboard {
        let icon = icons::ui(app.settings.icons, Glyph::Clipboard);
        let verb = if c.mode == rada_core::ops::TransferMode::Copy {
            Key::ReadyPaste
        } else {
            Key::ReadyMove
        };
        b.push(Span::styled("  │  ", th.faint().patch(band)));
        b.push(Span::styled(
            format!(
                "{icon} {}",
                tf(
                    verb,
                    &[&fmt::count(
                        c.paths.len() as u64,
                        Key::ItemOne,
                        Key::ItemMany
                    )]
                )
            ),
            th.fg(th.kinds.vector).patch(band),
        ));
    }
    if app.settings.show_hidden {
        b.push(Span::styled("  │  ", th.faint().patch(band)));
        b.push(Span::styled(
            Key::HiddenShown.tr(),
            th.fg(th.warn).patch(band),
        ));
    }
    b
}

/// The update notice, in the longest wording that fits in `room` beside `left_width` cells of
/// the rest of the status (which gives way, down to a few cells).
pub(super) fn update_notice_bar<'a>(app: &App, room: usize, left_width: usize) -> Option<Bar<'a>> {
    let (variants, clickable) = app.update_notice()?;
    let th = &app.settings.th;
    let band = th.band();
    let failed = matches!(app.updates.update, crate::app::UpdateStatus::Failed { .. });
    let keep_for_count = 12.min(left_width);
    let text = variants
        .iter()
        .find(|v| v.width() + 6 + keep_for_count <= room)
        .or(variants.last())?;
    let style = if failed {
        th.fg(th.error)
    } else {
        th.accent_style()
    }
    .add_modifier(Modifier::BOLD)
    .patch(band);
    let mut b = Bar::default();
    b.push(Span::styled("  │  ", th.faint().patch(band)));
    let span = Span::styled(format!("↑ {text}"), style);
    if clickable {
        b.push_hit(span, Target::Act(Action::Update));
    } else {
        b.push(span);
    }
    Some(b)
}

/// The free space of the disk the current folder is on, with a bar of ten segments for how
/// full it is.
pub(super) fn free_space<'a>(app: &App, with_bar: bool) -> Bar<'a> {
    let th = &app.settings.th;
    let band = th.band();
    let mut b = Bar::default();
    let Some(v) = app
        .devices
        .volumes
        .iter()
        .filter(|v| app.cwd.starts_with(&v.mount_point))
        .max_by_key(|v| v.mount_point.as_os_str().len())
    else {
        return b;
    };
    let Some(avail) = v.available else { return b };
    b.push(Span::styled(
        format!("{} ", tf(Key::FreeSpace, &[&fmt::size(avail)])),
        th.dim().patch(band),
    ));
    if with_bar && let Some(total) = v.total.filter(|t| *t > 0) {
        let used = 1.0 - avail as f64 / total as f64;
        b.spans
            .extend(segment_bar(th, used, app.settings.basic, band.bg));
        b.push(Span::styled(" ", band));
    }
    b
}

/// The buttons that switch between Details, Icons and Columns: the one in use is a pill.
fn view_buttons<'a>(app: &App) -> Bar<'a> {
    let th = &app.settings.th;
    let band = th.band();
    let look = Look::of(app);
    let mut b = Bar::default();
    for (mode, glyph) in [
        (ViewMode::Details, Glyph::Details),
        (ViewMode::Icons, Glyph::Icons),
        (ViewMode::Columns, Glyph::Columns),
    ] {
        let g = icons::ui(app.settings.icons, glyph);
        if app.view == mode {
            b.push_hits(
                soft::pill(th, look, g, th.accent, th.pill(), band),
                Target::ViewMode(mode),
            );
        } else {
            b.push_hit(
                Span::styled(format!(" {g} "), th.dim().patch(band)),
                Target::ViewMode(mode),
            );
        }
    }
    b.push(Span::styled(" ", band));
    b
}

pub fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let band = th.band();
    f.render_widget(Block::default().style(band), area);
    let w = area.width as usize;

    let buttons = view_buttons(app);
    // The running version, always there; a click asks whether there is a newer one.
    let version = || {
        let mut v = Bar::default();
        if w >= 70 {
            v.push_hit(
                Span::styled(format!("v{} ⟳", app.version()), th.dim().patch(band)),
                Target::CheckUpdates,
            );
            v.push(Span::styled("  ", band));
        }
        v
    };
    let mut right = version();
    right.append(free_space(app, true));
    right.append(view_buttons(app));
    if right.width() > w / 2 {
        right = version();
        right.append(free_space(app, false));
        right.append(view_buttons(app));
    }
    if right.width() > w.saturating_sub(12) {
        right = buttons;
    }
    let mut left = status_left(app);
    let mut room = w.saturating_sub(right.width() + 1);
    // A newer version (or an update at work) is worth more than the item count: it keeps its
    // place, in the longest wording that leaves the count room to show.
    if let Some(notice) = update_notice_bar(app, room, left.width()) {
        room = room.saturating_sub(notice.width());
        left.clip(room);
        left.append(notice);
    } else {
        left.clip(room);
    }
    let gap = w.saturating_sub(left.width() + right.width());
    left.push(Span::styled(" ".repeat(gap), band));
    left.append(right);
    left.render(f, &mut app.frame.hits, area);
}

/// The strip of key hints: a background of its own set apart from the status bar by a row of
/// ▄ above and ▀ below in the strip's colour (when there are three rows), each key a pill —
/// rounded with Nerd Font glyphs, a plain rectangle with a space on each side otherwise —
/// and its description faint after it.
pub fn draw_hints(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.settings.th.clone();
    let strip = Style::default().bg(th.panel);
    let w = area.width as usize;
    let inner = if area.height >= 3 {
        // The basic profile has ASCII only: there the edge rows are the strip's colour itself.
        let basic = app.settings.basic;
        let edge = |glyph: &str| {
            let (glyph, style) = if basic {
                (" ", strip)
            } else {
                (glyph, Style::default().fg(th.panel))
            };
            Paragraph::new(Line::from(Span::styled(glyph.repeat(w), style)))
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
    let look = Look::of(app);
    let all = hints(app);

    // As many as fit, two spaces from the left edge and four between groups.
    // A rounded pill has a glyph on each side, a plain one a space: two cells either way.
    let pill_w = |h: &Hint| h.key.width() + 2;
    let chunk = |h: &Hint| pill_w(h) + 1 + h.what.tr().width();
    // Four cells between hints; three, then two when that lets more of them in.
    let fit = |gap: usize| -> Vec<&Hint> {
        let mut shown: Vec<&Hint> = Vec::new();
        let mut used = 2;
        for h in &all {
            let need = chunk(h) + if shown.is_empty() { 0 } else { gap };
            if used + need > w {
                break;
            }
            used += need;
            shown.push(h);
        }
        shown
    };
    let mut gap = 4;
    let mut shown = fit(4);
    for g in [3, 2] {
        let more = fit(g);
        if more.len() > shown.len() {
            shown = more;
            gap = g;
        }
    }

    let mut bar = Bar::default();
    bar.push(Span::styled("  ", strip));
    for (i, h) in shown.iter().enumerate() {
        if i > 0 {
            bar.push(Span::styled(" ".repeat(gap), strip));
        }
        let pill = soft::pill(
            &th,
            look,
            &h.key,
            th.key_bg,
            Style::default()
                .fg(th.key_text)
                .add_modifier(Modifier::BOLD),
            strip,
        );
        let what = Span::styled(format!(" {}", h.what.tr()), th.dim().patch(strip));
        match &h.click {
            Some(t) => {
                bar.push_hits(pill, t.clone());
                bar.push_hit(what, t.clone());
            }
            None => {
                for sp in pill {
                    bar.push(sp);
                }
                bar.push(what);
            }
        }
    }
    bar.render(f, &mut app.frame.hits, inner);
}
