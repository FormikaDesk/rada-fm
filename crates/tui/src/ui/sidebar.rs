//! The navigation pane: Home, the pinned folders (with a pin), the disks (a name, a bar of
//! ten segments for how full each is and what is free), the tags and the trash, which sits at
//! the bottom. Narrow, it is a column of icons.
//!
//! The place you are in is a rounded pill, the item the keyboard is on is highlighted more
//! strongly (so focus is visible), and the one under the mouse gets a light highlight.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look, RowTone};
use super::widgets::{pad, pad_left, segment_bar, segment_bar_width};
use crate::app::App;
use crate::fmt;
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::sidebar::{Density, Item, Mode, Origin, Section, current_index};

/// What is drawn on one screen row.
enum Row {
    Title(&'static str),
    Gap,
    Item(usize),
    /// The second line of a disk: its bar and what is free.
    Usage(usize),
}

/// How much empty space there is between the entries, from none to a line after each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Air {
    /// What the pane has always been: a gap between the groups, no more.
    Compact,
    /// A line between Home and Recent, and two between one group and the next.
    Groups,
    /// And a line after every second entry of a group.
    Half,
    /// And a line after every entry. A disk's bar stays under its disk.
    Full,
}

fn layout(items: &[Item], rail: bool, air: Air) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut last: Option<Section> = None;
    let mut in_group = 0usize;
    for (i, item) in items.iter().enumerate() {
        if last != Some(item.section) {
            if last.is_some() {
                rows.push(Row::Gap);
                if air >= Air::Groups {
                    rows.push(Row::Gap);
                }
            }
            if let Some(t) = item.section.title() {
                rows.push(Row::Title(t));
            }
            last = Some(item.section);
            in_group = 0;
        }
        rows.push(Row::Item(i));
        if !rail && item.usage.is_some() {
            rows.push(Row::Usage(i));
        }
        in_group += 1;
        let more_in_group = items
            .get(i + 1)
            .is_some_and(|next| next.section == item.section);
        if more_in_group && !rail {
            let gap = match air {
                Air::Compact => false,
                Air::Groups => item.section == Section::Home,
                Air::Half => item.section == Section::Home || in_group.is_multiple_of(2),
                Air::Full => true,
            };
            if gap {
                rows.push(Row::Gap);
            }
        }
    }
    rows
}

/// The arrangement for a pane `height` rows tall. Only `Comfortable` asks for air: the roomiest
/// arrangement that shows every entry, or the lightest roomy one, with the list scrolling,
/// when none does. `Auto` and `Compact` are the pane as it has always been, and the column of
/// icons is always compact.
fn choose_air(items: &[Item], rail: bool, height: usize, density: Density) -> Air {
    if rail || density != Density::Comfortable {
        return Air::Compact;
    }
    for air in [Air::Full, Air::Half, Air::Groups] {
        if layout(items, rail, air).len() <= height {
            return air;
        }
    }
    Air::Groups
}

pub fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect, mode: Mode) {
    let th = app.th.clone();
    let look = Look::of(app);
    let items = app.side_items();
    if items.is_empty() || area.height == 0 {
        return;
    }
    let rail = mode == Mode::Rail;
    let current = current_index(&items, &app.cwd);
    let focused = app
        .side_focus
        .then_some(app.side_cursor.min(items.len() - 1));
    let height = area.height as usize;
    let air = choose_air(&items, rail, height, app.sidebar_density);
    let mut rows = layout(&items, rail, air);
    // The trash sits at the bottom of the pane when there is room to spare above it.
    let mut trash_row: Option<Row> = None;
    if rows.len() < height
        && let Some(Row::Item(i)) = rows.last()
        && items[*i].section == Section::Trash
    {
        trash_row = rows.pop();
        while matches!(rows.last(), Some(Row::Gap)) {
            rows.pop();
        }
    }

    // Keep the focused (or current) item on screen when the list is longer than the pane.
    let anchor = focused.or(current).unwrap_or(0);
    let anchor_row = rows
        .iter()
        .rposition(|r| matches!(r, Row::Item(i) | Row::Usage(i) if *i == anchor))
        .unwrap_or(0);
    let skip = (anchor_row + 1).saturating_sub(height);

    let w = area.width as usize;
    let pin = icons::ui(app.icons, Glyph::Pin);
    let mut placed: Vec<(u16, &Row)> = rows
        .iter()
        .skip(skip)
        .take(height)
        .enumerate()
        .map(|(y, r)| (y as u16, r))
        .collect();
    if let Some(t) = &trash_row {
        placed.push((area.height - 1, t));
    }
    for (screen_y, row) in placed {
        let r = Rect {
            y: area.y + screen_y,
            height: 1,
            ..area
        };
        match row {
            Row::Gap => {}
            Row::Title(t) => {
                if !rail {
                    f.render_widget(
                        Paragraph::new(Span::styled(
                            format!(" {t}"),
                            th.faint().add_modifier(Modifier::BOLD),
                        )),
                        r,
                    );
                }
            }
            Row::Item(i) | Row::Usage(i) => {
                let item = &items[*i];
                let is_current = current == Some(*i);
                let is_focus = focused == Some(*i);
                let is_hover = app.hover.as_ref() == Some(&item.path);
                let row_style = if is_focus {
                    Style::default().bg(th.cursor)
                } else if is_current {
                    th.current_place()
                } else if is_hover {
                    Style::default().bg(th.mark)
                } else {
                    Style::default()
                };
                let edge = if (is_focus || is_current) && !look.caps {
                    "▎"
                } else {
                    " "
                };
                let marked = is_focus || is_current;
                let mut spans = vec![Span::styled(edge, th.accent_style().patch(row_style))];
                if let Row::Usage(_) = row {
                    // Under the name: the bar and what is free.
                    let free = item
                        .usage
                        .as_ref()
                        .map(|u| fmt::size(u.free))
                        .unwrap_or_default();
                    let free_w = free.width().max(7);
                    let bar_w = segment_bar_width(app.basic);
                    // Under the name of the disk, where the name starts.
                    let indent = if item.glyph(app.icons).is_empty() {
                        4
                    } else {
                        5
                    };
                    spans.push(Span::styled(" ".repeat(indent), row_style));
                    if let Some(u) = &item.usage {
                        spans.extend(segment_bar(&th, u.used, app.basic, row_style.bg));
                    }
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    // What is free is pushed to the right edge, with a column of air before the
                    // border, when the bar leaves room.
                    let room = w.saturating_sub(used + 2);
                    let free_shown = pad_left(&free, room.max(free_w));
                    let _ = bar_w;
                    spans.push(Span::styled(" ", row_style));
                    spans.push(Span::styled(free_shown, th.dim().patch(row_style)));
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    spans.push(Span::styled(" ".repeat(w.saturating_sub(used)), row_style));
                    app.hits.add(r, Target::Place(item.path.clone()));
                    f.render_widget(Paragraph::new(Line::from(spans)), r);
                    if is_focus {
                        soft::tint_row(f.buffer_mut(), r, &th, Look::FLAT, RowTone::Focus);
                    } else if is_current {
                        soft::tint_row(f.buffer_mut(), r, &th, Look::FLAT, RowTone::Place);
                    }
                    continue;
                }
                let icon_color = match item.origin {
                    // A tag's dot keeps its colour (a tag with none is a dim dot).
                    Origin::Tag(color) => th.tag_color(color),
                    _ if marked => th.accent,
                    _ => th.text_dim,
                };
                let glyph = item.glyph(app.icons);
                let mark = if glyph.is_empty() && rail {
                    item.initial()
                } else {
                    glyph.to_string()
                };
                if rail {
                    spans.push(Span::styled(
                        format!(" {mark} "),
                        th.fg(icon_color).patch(row_style),
                    ));
                    let used: usize = spans.iter().map(|s| s.content.width()).sum();
                    spans.push(Span::styled(" ".repeat(w.saturating_sub(used)), row_style));
                } else {
                    // The entries stand one column further in than the titles of the groups,
                    // which are what sets the groups apart. A space on each side of the icon and
                    // one more after it: a wide symbol of a Nerd Font overflows its cell, and
                    // must not touch the name.
                    let icon = if glyph.is_empty() {
                        "  ".to_string()
                    } else {
                        format!("  {glyph}  ")
                    };
                    spans.push(Span::styled(icon, th.fg(icon_color).patch(row_style)));
                    let name_style = if marked {
                        th.base().add_modifier(Modifier::BOLD)
                    } else {
                        th.base()
                    };
                    let head: usize = spans.iter().map(|s| s.content.width()).sum();
                    // A pin at the right edge for what is pinned (the user's folders and the
                    // ones pinned by hand).
                    let pinned = matches!(item.section, Section::Pinned)
                        && !matches!(item.origin, Origin::Device(_));
                    let lock = match app.icons {
                        icons::IconSet::Nerd => "\u{f023}",
                        icons::IconSet::UnicodeIcons | icons::IconSet::UnicodeStd => "⊘",
                        icons::IconSet::None => "!",
                    };
                    let pin_w = if item.locked || (pinned && !pin.is_empty()) {
                        3
                    } else {
                        0
                    };
                    // What follows the name (a device that is not mounted) keeps its place: it is
                    // the name that is cut short.
                    let note = item.note().map(|n| format!(" {n}")).unwrap_or_default();
                    let note_w = note.width();
                    let room = w.saturating_sub(head + pin_w + 1 + note_w);
                    let name = rada_core::display::truncate(&item.name, room);
                    if note_w == 0 {
                        spans.push(Span::styled(
                            pad(&name, w.saturating_sub(head + pin_w)),
                            name_style.patch(row_style),
                        ));
                    } else {
                        let name_w = name.width();
                        spans.push(Span::styled(name, name_style.patch(row_style)));
                        spans.push(Span::styled(note, th.faint().patch(row_style)));
                        spans.push(Span::styled(
                            " ".repeat(w.saturating_sub(head + pin_w + name_w + note_w)),
                            name_style.patch(row_style),
                        ));
                    }
                    if item.locked {
                        // The system has not allowed this place yet: the same words as in
                        // the window that opens when it is clicked.
                        spans.push(Span::styled(
                            format!(" {lock} "),
                            th.fg(th.warn).patch(row_style),
                        ));
                    } else if pin_w > 0 {
                        spans.push(Span::styled(
                            format!(" {pin} "),
                            th.faint().patch(row_style),
                        ));
                    }
                }
                app.hits.add(r, Target::Place(item.path.clone()));
                f.render_widget(Paragraph::new(Line::from(spans)), r);
                // The place you are in is a rounded pill; the item the keyboard is on is
                // stronger.
                if is_focus {
                    soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Focus);
                } else if is_current {
                    soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Place);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidebar::Usage;
    use rada_core::platform::{PlaceKind, VolumeKind};
    use std::path::PathBuf;

    fn item(section: Section, origin: Origin, name: &str, usage: bool) -> Item {
        Item {
            section,
            origin,
            name: name.into(),
            path: PathBuf::from(format!("/{name}")),
            usage: usage.then_some(Usage { used: 0.5, free: 1 }),
            locked: false,
        }
    }

    /// Home and Recent, two pinned folders, two disks (each with its bar), three tags, the trash.
    fn items() -> Vec<Item> {
        let place = |k| Origin::Place(k);
        vec![
            item(Section::Home, place(PlaceKind::Home), "Home", false),
            item(Section::Home, Origin::Recent, "Recent", false),
            item(Section::Pinned, place(PlaceKind::Documents), "Docs", false),
            item(Section::Pinned, place(PlaceKind::Downloads), "Down", false),
            item(
                Section::Devices,
                Origin::Device(VolumeKind::Fixed),
                "Sys",
                true,
            ),
            item(
                Section::Devices,
                Origin::Device(VolumeKind::Removable),
                "Usb",
                true,
            ),
            item(Section::Tags, Origin::Tag(None), "Red", false),
            item(Section::Tags, Origin::Tag(None), "Blue", false),
            item(Section::Tags, Origin::Tag(None), "Gray", false),
            item(Section::Trash, place(PlaceKind::Trash), "Trash", false),
        ]
    }

    /// The rows as letters: `T`itle, `.` gap, item number, `u` for a disk's bar.
    fn shape(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| match r {
                Row::Title(_) => "T".to_string(),
                Row::Gap => ".".to_string(),
                Row::Item(i) => i.to_string(),
                Row::Usage(_) => "u".to_string(),
            })
            .collect()
    }

    #[test]
    fn the_compact_layout_is_the_one_the_pane_always_had() {
        assert_eq!(
            shape(&layout(&items(), false, Air::Compact)),
            ".01.T23.T4u5u.T678.9".replacen('.', "", 1),
            "a gap between groups, a title over each group but Home's, nothing between entries"
        );
    }

    #[test]
    fn each_step_of_air_adds_only_blank_lines_and_never_takes_an_entry_away() {
        let items = items();
        let strip = |air: Air| -> String { shape(&layout(&items, false, air)).replace('.', "") };
        let compact = strip(Air::Compact);
        for air in [Air::Groups, Air::Half, Air::Full] {
            assert_eq!(
                strip(air),
                compact,
                "{air:?}: the same rows, with blanks between"
            );
        }
        let lens: Vec<usize> = [Air::Compact, Air::Groups, Air::Half, Air::Full]
            .iter()
            .map(|a| layout(&items, false, *a).len())
            .collect();
        assert!(lens.windows(2).all(|w| w[0] < w[1]), "{lens:?}");
    }

    #[test]
    fn groups_get_a_line_between_home_and_recent_and_two_between_one_group_and_the_next() {
        let s = shape(&layout(&items(), false, Air::Groups));
        assert_eq!(s, "0.1..T23..T4u5u..T678..9", "{s}");
    }

    #[test]
    fn half_air_is_a_line_after_every_second_entry_and_full_air_after_each() {
        let half = shape(&layout(&items(), false, Air::Half));
        assert_eq!(half, "0.1..T23..T4u5u..T67.8..9", "{half}");
        let full = shape(&layout(&items(), false, Air::Full));
        assert_eq!(full, "0.1..T2.3..T4u.5u..T6.7.8..9", "{full}");
    }

    #[test]
    fn a_disks_bar_stays_under_its_disk_at_every_step() {
        for air in [Air::Compact, Air::Groups, Air::Half, Air::Full] {
            let rows = layout(&items(), false, air);
            for (k, r) in rows.iter().enumerate() {
                if let Row::Item(i) = r
                    && items()[*i].usage.is_some()
                {
                    assert!(
                        matches!(rows.get(k + 1), Some(Row::Usage(j)) if j == i),
                        "{air:?}: the bar is right under {i}: {}",
                        shape(&rows)
                    );
                }
            }
        }
    }

    #[test]
    fn only_comfortable_asks_for_air_and_gets_the_roomiest_that_fits() {
        let items = items();
        let need = |air| layout(&items, false, air).len();
        // The default is the compact pane, at any height.
        for height in [3, 40, 200] {
            assert_eq!(
                choose_air(&items, false, height, Density::Auto),
                Air::Compact
            );
            assert_eq!(
                choose_air(&items, false, height, Density::Compact),
                Air::Compact
            );
        }
        let comfortable = |height| choose_air(&items, false, height, Density::Comfortable);
        assert_eq!(comfortable(200), Air::Full);
        assert_eq!(comfortable(need(Air::Full)), Air::Full);
        assert_eq!(comfortable(need(Air::Full) - 1), Air::Half);
        assert_eq!(comfortable(need(Air::Half) - 1), Air::Groups);
        // Never tighter than a gap between groups, even if the list scrolls.
        assert_eq!(comfortable(need(Air::Groups) - 1), Air::Groups);
        assert_eq!(comfortable(3), Air::Groups);
        // The column of icons is always compact.
        assert_eq!(
            choose_air(&items, true, 200, Density::Comfortable),
            Air::Compact
        );
    }

    #[test]
    fn the_density_names_parse() {
        assert_eq!(Density::parse("auto"), Some(Density::Auto));
        assert_eq!(Density::parse(" Comfortable "), Some(Density::Comfortable));
        assert_eq!(Density::parse("compact"), Some(Density::Compact));
        assert_eq!(Density::parse("roomy"), None);
        assert_eq!(Density::default(), Density::Auto);
    }
}
