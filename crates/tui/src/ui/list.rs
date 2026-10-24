//! The folder as a table (the Details view): a checkbox, the icon and name, the date, the
//! type in words, the size and the tags as coloured dots. The column titles, a pill, are
//! buttons that sort; the sorted one carries an arrow.
//!
//! What the terminal's width allows decides the columns: below 100 columns the type goes,
//! below 60 the date goes too and only name and size remain. The Tag column gives way first,
//! whenever the name would be left too little room.
//!
//! The row under the cursor is shaded from one end to the other, the selected ones are
//! tinted, both with rounded ends (see `soft`).

use rada_core::display;
use rada_core::fs::FileKind;
use rada_core::i18n::{Key, tf};
use rada_core::model::{Entry, SortKey};
use rada_core::ops::LinkState;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::soft::{self, Look, RowTone};
use super::widgets::{SPIN, hit_spans, pad, pad_left};
use crate::app::App;
use crate::fmt::{self, DateStyle};
use crate::hits::Target;
use crate::icons::{self, Glyph};
use crate::keymap::Action;

/// Cells of the checkbox column: a space, the box, a space.
const CHECK: usize = 3;

/// The cells of the Tag column: three dots with a space between, and a "+N".
const TAG_W: usize = 8;
/// The name keeps at least this much room before the Tag column is let in.
const NAME_MIN_WITH_TAGS: usize = 18;
/// Narrower terminals do without the Tag column: the room is the name's.
const TAGS_FROM: usize = 80;

/// Terminals narrower than this do without the date column (the name has the room); the sort by
/// date stays, only the column goes. The list of the panels view follows the same width.
pub const DATE_FROM: usize = 90;

/// Which columns fit, and how wide each is.
pub struct Columns {
    pub icon: usize,
    pub date: usize,
    pub kind: usize,
    pub size: usize,
    /// 0 when the column is not shown.
    pub tags: usize,
    pub gap: usize,
    pub name: usize,
}

/// What the caller decides about the table: which optional columns it may have. They follow
/// the width of the terminal, not of the area the table is drawn in, so that every area on a
/// screen agrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListOpts {
    pub show_type: bool,
    pub show_date: bool,
    /// The Tag column is wanted (it still needs the room).
    pub show_tags: bool,
}

impl ListOpts {
    /// Below 100 columns the type goes, below [`DATE_FROM`] the date goes too, and below 80 the
    /// tags go. `tags` is whether the Tag column has something to show (see
    /// `App::tags_column_wanted`).
    pub fn for_terminal(term_width: usize, tags: bool) -> ListOpts {
        ListOpts {
            show_type: term_width >= 100,
            show_date: term_width >= DATE_FROM,
            show_tags: tags && term_width >= TAGS_FROM,
        }
    }
}

pub fn columns(area_width: usize, opts: ListOpts, icon_w: usize, dates: DateStyle) -> Columns {
    let gap = 2;
    let date_w = match dates {
        DateStyle::Relative => 14,
        DateStyle::Absolute => 16,
    };
    // A title with the arrow of the sort must fit its column ("Dimensione ↑").
    let fit = |base: usize, label: Key| base.max(label.tr().width() + 2);
    let (mut date, mut kind, mut size) = (
        fit(date_w, Key::ColModified).max(fit(date_w, Key::ColUsed)),
        fit(20, Key::ColType),
        fit(10, Key::ColSize),
    );
    if !opts.show_type {
        kind = 0;
    }
    if !opts.show_date || area_width < 46 {
        date = 0;
    }
    if area_width < 24 {
        size = 0;
    }
    let cols = [date, kind, size].iter().filter(|c| **c > 0).count();
    let fixed = CHECK + icon_w + date + kind + size + 1;
    let mut name = area_width.saturating_sub(fixed + gap * cols);
    // The tags come last and go first: only while the name keeps its room.
    let mut tags = 0;
    if opts.show_tags && name >= NAME_MIN_WITH_TAGS + TAG_W + gap {
        tags = TAG_W;
        name -= TAG_W + gap;
    }
    Columns {
        icon: icon_w,
        date,
        kind,
        size,
        tags,
        gap,
        name,
    }
}

/// Draw the table of the front tab in `area` (any area: nothing here knows where it is).
pub fn draw_list(f: &mut Frame, app: &mut App, area: Rect, opts: ListOpts) {
    let th = app.settings.th.clone();
    let look = Look::of(app);
    let count = app.tab.visible.len();
    let cols = columns(
        area.width as usize,
        opts,
        app.settings.icons.width(),
        app.settings.dates,
    );

    // Column titles, in a pill of their own; the sorted one carries an arrow.
    let head_bg = Style::default().bg(th.field);
    let arrow = if app.tab.sort.descending() {
        "↓"
    } else {
        "↑"
    };
    let label = |text: &str, k: SortKey| {
        let sorted = app.tab.sort.key == k;
        let t = if sorted {
            format!("{text} {arrow}")
        } else {
            text.to_string()
        };
        let st = if sorted {
            th.accent_style().add_modifier(Modifier::BOLD)
        } else {
            th.dim()
        };
        (t, st.patch(head_bg))
    };
    let gap = || Span::styled(" ".repeat(cols.gap), head_bg);
    let mut head: Vec<Span> = Vec::new();
    let mut head_hits: Vec<(usize, Target)> = Vec::new();
    head_hits.push((head.len(), Target::Act(Action::SelectAll)));
    let all_marked = !app.tab.visible.is_empty() && app.tab.marked.len() == app.tab.visible.len();
    head.push(Span::styled(
        format!(
            " {} ",
            icons::ui(
                app.settings.icons,
                if all_marked {
                    Glyph::Checked
                } else {
                    Glyph::Unchecked
                }
            )
        ),
        if all_marked {
            th.fg(th.check).patch(head_bg)
        } else {
            th.faint().patch(head_bg)
        },
    ));
    head.push(Span::styled(" ".repeat(cols.icon), head_bg));
    let (t, st) = label(Key::ColName.tr(), SortKey::Name);
    head_hits.push((head.len(), Target::SortBy(SortKey::Name)));
    head.push(Span::styled(pad(&t, cols.name), st));
    if cols.date > 0 {
        head.push(gap());
        // In the list of recent files the date is the one of the last use.
        let title = if app.recents_view() {
            Key::ColUsed
        } else {
            Key::ColModified
        };
        let (t, st) = label(title.tr(), SortKey::Modified);
        head_hits.push((head.len(), Target::SortBy(SortKey::Modified)));
        head.push(Span::styled(pad(&t, cols.date), st));
    }
    if cols.kind > 0 {
        head.push(gap());
        let (t, st) = label(Key::ColType.tr(), SortKey::Type);
        head_hits.push((head.len(), Target::SortBy(SortKey::Type)));
        head.push(Span::styled(pad(&t, cols.kind), st));
    }
    if cols.size > 0 {
        head.push(gap());
        let (t, st) = label(Key::ColSize.tr(), SortKey::Size);
        head_hits.push((head.len(), Target::SortBy(SortKey::Size)));
        head.push(Span::styled(pad_left(&t, cols.size), st));
    }
    if cols.tags > 0 {
        head.push(gap());
        head.push(Span::styled(
            pad(Key::ColTags.tr(), cols.tags),
            th.dim().patch(head_bg),
        ));
    }
    head.push(Span::styled(" ", head_bg));
    let drawn: usize = head.iter().map(|s| s.content.width()).sum();
    if drawn < area.width as usize {
        head.push(Span::styled(
            " ".repeat(area.width as usize - drawn),
            head_bg,
        ));
    }
    if area.height < 2 {
        return;
    }
    hit_spans(&mut app.frame.hits, area.x, area.y, &head, &head_hits);
    let head_row = Rect { height: 1, ..area };
    f.render_widget(Paragraph::new(Line::from(head)), head_row);
    soft::round_ends(f.buffer_mut(), head_row, &th, look, th.field);

    // A row of air under the titles, where there is room for it.
    let air = u16::from(area.height >= 6);
    let body = Rect {
        y: area.y + 1 + air,
        height: area.height - 1 - air,
        ..area
    };
    app.tab.viewport.rows = body.height as usize;
    if body.height == 0 {
        return;
    }
    app.frame.hits.add(body, Target::List);

    if app.tab.visible.is_empty() {
        let msg = if app.is_loading() {
            tf(Key::ListLoading, &[&SPIN[app.redraw.spinner % SPIN.len()]])
        } else if app.tab.filter.is_some() {
            Key::ListNoMatch.tr().to_string()
        } else if app.tab.listing.is_empty() {
            Key::ListEmpty.tr().to_string()
        } else {
            Key::ListOnlyHidden.tr().to_string()
        };
        f.render_widget(
            Paragraph::new(Span::styled(msg, th.dim())).alignment(Alignment::Center),
            Rect {
                y: body.y + body.height / 4,
                height: 1,
                ..body
            },
        );
        return;
    }

    app.tab.scroll = app.tab.scroll.min(count.saturating_sub(1));
    let rows = body.height as usize;
    let last = (app.tab.scroll + rows).min(count);
    let shown: Vec<usize> = (app.tab.scroll..last).map(|v| app.tab.visible[v]).collect();
    app.warm_dates(shown.iter().copied());
    app.want_folder_sizes(shown.iter().copied());
    let mut lines: Vec<Line> = Vec::with_capacity(rows);
    for vis in app.tab.scroll..last {
        let Some(e) = app.entry_at(vis) else { continue };
        lines.push(row(
            app,
            e,
            app.tab.visible[vis],
            vis == app.tab.cursor,
            &cols,
            area.width as usize,
            look,
        ));
    }
    f.render_widget(Paragraph::new(lines), body);
    for vis in app.tab.scroll..last {
        let r = Rect {
            y: body.y + (vis - app.tab.scroll) as u16,
            height: 1,
            ..body
        };
        // The row under the cursor shades from one end to the other, the selected ones are
        // tinted; both have round ends where the font allows.
        let marked = app
            .entry_at(vis)
            .is_some_and(|e| app.tab.marked.contains(&e.name));
        if vis == app.tab.cursor {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Cursor);
        } else if marked {
            soft::tint_row(f.buffer_mut(), r, &th, look, RowTone::Marked);
        }
        app.frame.hits.add(r, Target::Row(vis));
        app.frame.hits.add(
            Rect {
                width: (CHECK as u16).min(r.width),
                ..r
            },
            Target::Check(vis),
        );
    }
}

fn row<'a>(
    app: &App,
    e: &Entry,
    listing_index: usize,
    is_cursor: bool,
    cols: &Columns,
    width: usize,
    look: Look,
) -> Line<'a> {
    let th = &app.settings.th;
    let marked = app.tab.marked.contains(&e.name);
    // The cursor row is solid with white text; selected rows are a quiet tone; the cursor on
    // a selected row wins, but its tick stays.
    let row_style = if is_cursor {
        th.cursor_row()
    } else if marked {
        th.marked()
    } else {
        Style::default()
    };
    // Patching the row's style over a span's own colour would lose the white text: on the
    // cursor row the text colours are the row's.
    let with = |s: Style| {
        if is_cursor && th.depth != crate::theme::ColorDepth::Ansi16 {
            Style { fg: None, ..s }.patch(row_style)
        } else {
            s.patch(row_style)
        }
    };

    let (glyph, gcolor) = icons::icon(e, app.settings.icons, th);
    let broken = e
        .link
        .as_ref()
        .is_some_and(|l| matches!(l.state, LinkState::Broken | LinkState::Circular));
    let name_color = if e.error.is_some() || broken {
        th.error
    } else if e.hidden {
        th.text_dim
    } else if e.is_navigable_dir() {
        th.kinds.folder
    } else {
        th.text
    };
    let mut name_style = Style::default().fg(name_color);
    if e.is_navigable_dir() || is_cursor || marked {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }

    let check = icons::ui(
        app.settings.icons,
        if marked {
            Glyph::Checked
        } else {
            Glyph::Unchecked
        },
    );
    // The tick keeps its accent on a selected row and stays visible under the cursor.
    let tick_style = if is_cursor && marked {
        Style::default()
            .fg(th.cursor_text)
            .add_modifier(Modifier::BOLD)
    } else if marked {
        th.fg(th.check)
    } else {
        th.faint()
    };
    // The leftmost cell carries a bar in the accent colour on the cursor row; with round
    // ends it is the left end itself, drawn after the row (see `soft::tint_row`).
    let mark = if is_cursor && !look.caps { "▌" } else { " " };
    let mut spans: Vec<Span> = vec![
        Span::styled(mark, row_style.patch(Style::default().fg(th.accent))),
        Span::styled(check.to_string(), with(tick_style)),
        Span::styled(" ", with(Style::default())),
    ];
    // On the cursor row the secondary text turns to the main colour: the stronger
    // background would leave it too faint.
    let dim = if is_cursor {
        Style::default()
    } else {
        th.dim()
    };
    if cols.icon > 0 {
        spans.push(Span::styled(
            format!("{glyph} "),
            with(Style::default().fg(gcolor)),
        ));
    }

    let shown = e.display.clone();
    let name_txt = display::truncate_name(&shown, cols.name);
    let mut used = name_txt.width();
    spans.push(Span::styled(name_txt, with(name_style)));
    // In the list of a tag's items, or of recent files, the folder each one is in, dim after
    // the name.
    if app.tag_view().is_some() {
        let room = cols.name.saturating_sub(used);
        if room > 8
            && let Some(parent) = e.path.parent()
        {
            let n = display::truncate(&format!("  {}", fmt::short_path(parent, app.home())), room);
            used += n.width();
            spans.push(Span::styled(n, with(dim)));
        }
    }
    if let Some(l) = &e.link {
        let room = cols.name.saturating_sub(used);
        if room > 5 {
            let n = display::truncate(&format!(" → {}", display::path(&l.target)), room);
            used += n.width();
            spans.push(Span::styled(n, with(dim)));
        }
    }
    spans.push(Span::styled(
        " ".repeat(cols.name.saturating_sub(used)),
        with(Style::default()),
    ));

    let gap = || Span::styled(" ".repeat(cols.gap), with(Style::default()));
    if cols.date > 0 {
        spans.push(gap());
        let text = display::truncate(app.folder_info.date_text(listing_index), cols.date);
        spans.push(Span::styled(pad(&text, cols.date), with(dim)));
    }
    if cols.kind > 0 {
        spans.push(gap());
        let text = display::truncate(&rada_core::i18n::type_label(&e.type_label), cols.kind);
        spans.push(Span::styled(pad(&text, cols.kind), with(dim)));
    }
    if cols.size > 0 {
        spans.push(gap());
        let s = if e.kind == FileKind::File {
            fmt::size(e.size)
        } else {
            app.folder_size_text(e, cols.size)
        };
        spans.push(Span::styled(
            pad_left(&s, cols.size),
            with(if is_cursor {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                th.dim()
            }),
        ));
    }
    if cols.tags > 0 {
        spans.push(gap());
        // A dot in each tag's colour, three at most, then "+N": the colour says it all.
        let dots = super::tagdots::dots(th, &e.tags);
        let mut text_w = 0;
        for (i, (text, color)) in dots.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" ", with(Style::default())));
                text_w += 1;
            }
            text_w += text.width();
            spans.push(Span::styled(
                text.clone(),
                row_style.patch(Style::default().fg(*color)),
            ));
        }
        spans.push(Span::styled(
            " ".repeat(cols.tags.saturating_sub(text_w)),
            with(Style::default()),
        ));
    }
    // Make the highlight reach the right edge.
    let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
    if drawn < width {
        spans.push(Span::styled(
            " ".repeat(width - drawn),
            with(Style::default()),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_terminals_lose_the_least_important_columns_first() {
        let rel = DateStyle::Relative;
        let wide = columns(120, ListOpts::for_terminal(170, false), 2, rel);
        assert!(wide.kind > 0 && wide.date > 0 && wide.size > 0);
        let mid = columns(90, ListOpts::for_terminal(99, false), 2, rel);
        assert_eq!(mid.kind, 0, "the type goes below 100 columns");
        assert!(mid.date > 0);
        let narrow = columns(50, ListOpts::for_terminal(59, false), 2, rel);
        assert_eq!((narrow.date, narrow.kind), (0, 0));
        assert!(narrow.size > 0, "name and size are what stays");
        // Absolute dates need more room than relative ones.
        assert!(
            columns(
                120,
                ListOpts::for_terminal(170, false),
                2,
                DateStyle::Absolute
            )
            .date
                > wide.date
        );
    }

    #[test]
    fn the_tag_column_comes_last_and_goes_first() {
        let rel = DateStyle::Relative;
        let with = |w: usize| columns(w, ListOpts::for_terminal(170, true), 2, rel);
        let without = |w: usize| columns(w, ListOpts::for_terminal(170, false), 2, rel);
        // Plenty of room: the column is there, and the name still has what it had, less the
        // column and its gap.
        let wide = with(120);
        assert_eq!(wide.tags, TAG_W);
        assert_eq!(wide.name + TAG_W + wide.gap, without(120).name);
        // The column is never what squeezes the name: it goes as soon as the name would get
        // too little.
        for w in 40..=140 {
            let c = with(w);
            if c.tags > 0 {
                assert!(c.name >= NAME_MIN_WITH_TAGS, "{w}: {}", c.name);
            }
        }
        assert_eq!(with(60).tags, 0);
        // A narrow terminal does without it, even where the area would have the room.
        assert_eq!(
            columns(120, ListOpts::for_terminal(79, true), 2, rel).tags,
            0
        );
        assert_eq!(
            columns(120, ListOpts::for_terminal(80, true), 2, rel).tags,
            TAG_W
        );
        // Off, it is never there.
        assert_eq!(without(160).tags, 0);
        // The columns still add up to the width with it.
        for w in [170usize, 120, 100, 90] {
            let c = with(w);
            let n = [c.date, c.kind, c.size, c.tags]
                .iter()
                .filter(|x| **x > 0)
                .count();
            assert_eq!(
                CHECK + c.icon + c.name + c.date + c.kind + c.size + c.tags + c.gap * n + 1,
                w,
                "{w}"
            );
        }
    }

    #[test]
    fn the_columns_always_add_up_to_the_width() {
        for term in [170usize, 120, 99, 80, 59, 40] {
            let area = term.saturating_sub(30).max(24);
            let c = columns(
                area,
                ListOpts::for_terminal(term, false),
                2,
                DateStyle::Relative,
            );
            let n = [c.date, c.kind, c.size].iter().filter(|x| **x > 0).count();
            assert_eq!(
                CHECK + c.icon + c.name + c.date + c.kind + c.size + c.gap * n + 1,
                area.max(CHECK + c.icon + c.date + c.kind + c.size + c.gap * n + 1),
                "{term}"
            );
        }
    }
}
