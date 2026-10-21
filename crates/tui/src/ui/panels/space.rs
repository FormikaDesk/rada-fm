//! The SPAZIO box: the main disk with how full it is, what is used of what it holds and what is
//! free, and under it the other volumes, a row each with a bar of twelve cells and a percentage.

use rada_core::i18n::{Key, tf};
use rada_core::platform::{Volume, VolumeKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::fmt;
use crate::hits::Target;
use crate::theme::Theme;
use crate::ui::widgets::{pad, pad_left};

/// Cells of the bar of a volume that is not the main one.
const BAR: usize = 12;

/// The volumes the box is about: the main disk (the one of the system: the root of the file
/// system, or the first fixed disk) and the others, in the order the list gave them.
pub fn volumes(app: &App) -> Option<(&Volume, Vec<&Volume>)> {
    let all: Vec<&Volume> = app
        .volumes
        .iter()
        .filter(|v| v.kind != VolumeKind::Virtual && v.total.is_some_and(|t| t > 0))
        .collect();
    let main = all
        .iter()
        .position(|v| v.mount_point == std::path::Path::new(std::path::MAIN_SEPARATOR_STR))
        .or_else(|| all.iter().position(|v| v.kind == VolumeKind::Fixed))
        .or((!all.is_empty()).then_some(0))?;
    let first = all[main];
    let others = all
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != main)
        .map(|(_, v)| *v)
        .collect();
    Some((first, others))
}

/// Rows the box needs inside its border, for `others` volumes besides the main one; the main
/// disk is four rows, or two when the box is short (`compact`).
pub fn rows_needed(others: usize, compact: bool) -> u16 {
    let main = if compact { 2 } else { 4 };
    main + if others > 0 { 1 + others as u16 } else { 0 }
}

fn used_share(v: &Volume) -> f64 {
    match (v.total, v.available) {
        (Some(t), Some(a)) if t > 0 => (1.0 - a as f64 / t as f64).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

fn percent(v: &Volume) -> String {
    format!("{}%", (used_share(v) * 100.0).round() as u32)
}

/// A bar of `cells` squares: `■■■■■□□□□□□□`.
fn bar(th: &Theme, used: f64, cells: usize, basic: bool) -> Vec<Span<'static>> {
    let on = ((used * cells as f64).round() as usize).min(cells);
    let on = if on == 0 && used > 0.005 { 1 } else { on };
    let mut out = Vec::new();
    if basic {
        out.push(Span::styled("[", th.faint()));
    }
    out.push(Span::styled("■".repeat(on), Style::default().fg(th.disk)));
    out.push(Span::styled(
        "□".repeat(cells - on),
        Style::default().fg(th.muted),
    ));
    if basic {
        out.push(Span::styled("]", th.faint()));
    }
    out
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let Some((main, others)) = volumes(app) else {
        return;
    };
    let (main, others) = (
        main.clone(),
        others.into_iter().cloned().collect::<Vec<_>>(),
    );
    let w = area.width as usize;
    let compact = area.height < rows_needed(others.len(), false);
    let mut lines: Vec<Line> = Vec::new();
    let name_of = |v: &Volume| app.shown_name(&v.mount_point, crate::sidebar::device_name(v));

    // The main disk: its name and how full it is, a bar across the box, used of total, free.
    let pct = percent(&main);
    let name = rada_core::display::truncate(&name_of(&main), w.saturating_sub(pct.width() + 3));
    let gap = w.saturating_sub(2 + name.width() + pct.width());
    lines.push(Line::from(vec![
        Span::styled(" ", th.base()),
        Span::styled(name, th.base().add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(gap), th.base()),
        Span::styled(pct, th.fg(th.hues.cyan).add_modifier(Modifier::BOLD)),
        Span::styled(" ", th.base()),
    ]));
    let across = w.saturating_sub(2);
    let mut spans = vec![Span::styled(" ", th.base())];
    spans.extend(bar(&th, used_share(&main), across, app.basic));
    lines.push(Line::from(spans));
    if !compact {
        let total = main.total.unwrap_or(0);
        let free = main.available.unwrap_or(0);
        lines.push(Line::from(Span::styled(
            format!(
                " {}",
                tf(
                    Key::SpaceUsedOf,
                    &[&fmt::size(total.saturating_sub(free)), &fmt::size(total)]
                )
            ),
            th.dim(),
        )));
        lines.push(Line::from(Span::styled(
            format!(" {}", tf(Key::FreeSpace, &[&fmt::size(free)])),
            th.fg(th.hues.green),
        )));
    }
    if !others.is_empty() {
        lines.push(Line::raw(""));
        // A name, a bar of twelve cells and the percentage: the name gets what is left.
        let pct_w = 4;
        let name_w = w.saturating_sub(1 + 1 + BAR + 1 + pct_w + 1);
        for v in &others {
            let name = pad(
                &rada_core::display::truncate(&name_of(v), name_w.max(1)),
                name_w,
            );
            let mut spans = vec![
                Span::styled(" ", th.base()),
                Span::styled(name, th.dim()),
                Span::styled(" ", th.base()),
            ];
            spans.extend(bar(&th, used_share(v), BAR, false));
            spans.push(Span::styled(
                format!(" {}", pad_left(&percent(v), pct_w)),
                th.dim(),
            ));
            lines.push(Line::from(spans));
        }
    }
    let shown = lines.len().min(area.height as usize);
    f.render_widget(Paragraph::new(lines), area);
    // A click on the box goes to the disk the row is about.
    app.hits.add(
        Rect { height: 1, ..area },
        Target::Place(main.mount_point.clone()),
    );
    let top_others = if compact { 3 } else { 5 };
    for (n, v) in others.iter().enumerate() {
        let y = top_others + n;
        if y < shown {
            app.hits.add(
                Rect {
                    y: area.y + y as u16,
                    height: 1,
                    ..area
                },
                Target::Place(v.mount_point.clone()),
            );
        }
    }
}
