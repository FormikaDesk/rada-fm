//! Small drawing helpers shared by every part of the interface.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::hits::{Hits, Target};
use crate::theme::{BadgeStyle, BarStyle, ColorDepth, Theme};

pub const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn pad(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

pub fn pad_left(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(width - w))
    }
}

/// Keep the *end* of a long path (the part that matters).
pub fn tail(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: Vec<char> = Vec::new();
    let mut w = 0;
    for c in s.chars().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.reverse();
    format!("…{}", out.into_iter().collect::<String>())
}

pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

/// Dim everything already drawn, so that a window in front is the only thing in focus.
pub fn dim_backdrop(buf: &mut Buffer, area: Rect) {
    fn darken(c: Color, f: f32) -> Color {
        match c {
            Color::Rgb(r, g, b) => Color::Rgb(
                (r as f32 * f) as u8,
                (g as f32 * f) as u8,
                (b as f32 * f) as u8,
            ),
            other => other,
        }
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.modifier.insert(Modifier::DIM);
            // Kitty image cells carry the image id in their colour: leave them alone.
            if cell.symbol().contains('\u{10EEEE}') {
                continue;
            }
            cell.fg = darken(cell.fg, 0.62);
            cell.bg = darken(cell.bg, 0.6);
        }
    }
}

const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// A horizontal bar of `cells` cells filled to `frac` with eighth-block precision.
/// Returns (filled part, empty part) as strings of exactly `cells` cells together.
pub fn bar_parts(frac: f64, cells: usize, style: BarStyle) -> (String, String) {
    let frac = frac.clamp(0.0, 1.0);
    let (fill_ch, track_ch) = match style {
        BarStyle::Half => ('▄', '▄'),
        BarStyle::Thin => ('━', '─'),
    };
    // Thin bars move in half cells (╸), half-height bars too (▖): about 1/(2*cells) precision.
    let halves = (frac * cells as f64 * 2.0).round() as usize;
    let full = (halves / 2).min(cells);
    let mut filled: String = std::iter::repeat_n(fill_ch, full).collect();
    let mut used = full;
    if halves % 2 == 1 && full < cells {
        filled.push(match style {
            BarStyle::Half => '▖',
            BarStyle::Thin => '╸',
        });
        used += 1;
    }
    let empty: String = std::iter::repeat_n(track_ch, cells - used).collect();
    (filled, empty)
}

/// Spans of a bar: the filled part in `fill`, the rest in the theme's track colour.
pub fn bar_spans<'a>(
    th: &Theme,
    frac: f64,
    cells: usize,
    fill: Color,
    row_bg: Option<Color>,
) -> Vec<Span<'a>> {
    let (f, e) = bar_parts(frac, cells, th.bar);
    let mut fill_style = Style::default().fg(fill);
    // The track must stay visible on the highlighted row too, where it would blend in.
    let on_selection = row_bg == Some(th.selection);
    let mut track_style = match th.bar {
        BarStyle::Half if on_selection => Style::default().fg(th.muted),
        BarStyle::Half => Style::default().fg(th.track),
        BarStyle::Thin => Style::default().fg(th.muted),
    };
    if let Some(bg) = row_bg {
        fill_style = fill_style.bg(bg);
        track_style = track_style.bg(bg);
    }
    vec![Span::styled(f, fill_style), Span::styled(e, track_style)]
}

/// The progress bar: moves in eighths of a cell, so it looks continuous even on a slow copy.
pub fn progress_spans<'a>(th: &Theme, frac: f64, cells: usize) -> Vec<Span<'a>> {
    let eighths = (frac.clamp(0.0, 1.0) * cells as f64 * 8.0).round() as usize;
    let full = (eighths / 8).min(cells);
    let rem = eighths % 8;
    let mut filled: String = std::iter::repeat_n('█', full).collect();
    let mut used = full;
    // The partial cell: filled part in accent, the rest of the cell shows the track behind it.
    let mut partial = None;
    if rem > 0 && full < cells {
        partial = Some(EIGHTHS[rem - 1]);
        used += 1;
    }
    let track_ch = if th.depth == ColorDepth::Ansi16 {
        '░'
    } else {
        '█'
    };
    let empty: String = std::iter::repeat_n(track_ch, cells - used).collect();
    let track = Style::default().fg(th.track);
    let mut spans = vec![Span::styled(
        std::mem::take(&mut filled),
        Style::default().fg(th.accent),
    )];
    if let Some(p) = partial {
        spans.push(Span::styled(
            p.to_string(),
            Style::default().fg(th.accent).bg(th.track),
        ));
    }
    spans.push(Span::styled(empty, track));
    spans
}

/// A type badge: ` code `, padded to `width` cells.
pub fn badge<'a>(
    th: &Theme,
    text: &str,
    color: Color,
    width: usize,
    row_bg: Option<Color>,
) -> Vec<Span<'a>> {
    match th.badge {
        BadgeStyle::Filled => {
            let label = format!(" {text} ");
            let lead = width.saturating_sub(label.width());
            let bg = if th.depth == ColorDepth::Ansi16 {
                row_bg.unwrap_or(Color::Reset)
            } else {
                th.tint(color)
            };
            let mut pill = Style::default().fg(color).bg(bg);
            if th.depth == ColorDepth::Ansi16 {
                pill = pill.add_modifier(Modifier::BOLD);
            }
            let mut gap = Style::default();
            if let Some(b) = row_bg {
                gap = gap.bg(b);
            }
            vec![
                Span::styled(label, pill),
                Span::styled(" ".repeat(lead), gap),
            ]
        }
        BadgeStyle::Plain => {
            let label = format!("● {text}");
            let rest = width.saturating_sub(label.width());
            let mut s = Style::default().fg(color);
            let mut gap = Style::default();
            if let Some(b) = row_bg {
                s = s.bg(b);
                gap = gap.bg(b);
            }
            vec![Span::styled(label, s), Span::styled(" ".repeat(rest), gap)]
        }
    }
}

/// A button drawn as a pill: ` Enter  Run `. `Some(colour)` is the primary action.
pub fn button<'a>(
    th: &Theme,
    key: &str,
    label: &str,
    color: Option<Color>,
    enabled: bool,
) -> Span<'a> {
    let text = format!(" {key}  {label} ");
    if !enabled {
        return Span::styled(text, th.faint().add_modifier(Modifier::CROSSED_OUT));
    }
    match color {
        Some(c) => Span::styled(
            text,
            Style::default()
                .fg(th.on_accent)
                .bg(c)
                .add_modifier(Modifier::BOLD),
        ),
        None => {
            let mut st = Style::default().fg(th.text);
            if th.depth == ColorDepth::Ansi16 {
                st = st.add_modifier(Modifier::REVERSED);
            } else {
                st = st.bg(th.selection);
            }
            Span::styled(text, st)
        }
    }
}

/// Register the clickable spans of one rendered line. `targets` pairs a span index with
/// what a click on it does; the x positions come from the widths of the spans before it.
pub fn hit_spans(hits: &mut Hits, x: u16, y: u16, spans: &[Span], targets: &[(usize, Target)]) {
    let mut cx = x;
    for (i, s) in spans.iter().enumerate() {
        let w = s.content.width() as u16;
        if let Some((_, t)) = targets.iter().find(|(idx, _)| *idx == i) {
            hits.add(
                Rect {
                    x: cx,
                    y,
                    width: w,
                    height: 1,
                },
                t.clone(),
            );
        }
        cx = cx.saturating_add(w);
    }
}
