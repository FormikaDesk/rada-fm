//! Small drawing helpers shared by every part of the interface.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::hits::{Hits, Target};
use crate::theme::{BadgeStyle, BarStyle, ColorDepth, Theme};

pub const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The two ends of a text field, rounded with Nerd Font glyphs: the field's colour on the
/// bar's. Without those glyphs there are none, and the field is a plain rectangle.
pub fn field_ends(
    th: &Theme,
    set: crate::icons::IconSet,
) -> Option<(Span<'static>, Span<'static>)> {
    if set != crate::icons::IconSet::Nerd {
        return None;
    }
    let edge = Style::default().fg(th.field).patch(th.band());
    Some((
        Span::styled("\u{e0b6}", edge),
        Span::styled("\u{e0b4}", edge),
    ))
}

/// The name of a key as the language in use writes it on the cap: `Enter` is `Invio`.
pub fn key_name(key: &str) -> &str {
    use rada_core::i18n::Key;
    match key {
        "Enter" => Key::KeyEnter.tr(),
        "Space" => Key::KeySpace.tr(),
        "Del" => Key::KeyDel.tr(),
        other => other,
    }
}

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
    format!("…{}", rada_core::display::tail_cells(s, max - 1))
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

/// Dim everything already drawn, so that a window in front is the only thing in focus
/// (see [`Theme::dimmed`]: on a light theme "dimmer" means paler). The cells the terminal fills
/// with its own background are dimmed too, as the theme's canvas, so that the whole screen
/// goes down together and the window stands on one tone.
pub fn dim_backdrop(buf: &mut Buffer, area: Rect, th: &Theme) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.modifier.insert(Modifier::DIM);
            // Kitty image cells carry the image id in their colour: leave them alone.
            if cell.symbol().contains('\u{10EEEE}') {
                continue;
            }
            if cell.fg != Color::Reset {
                cell.fg = th.dimmed(cell.fg);
            }
            cell.bg = th.dimmed(cell.bg);
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
    let on_selection = row_bg == Some(th.cursor);
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

/// How many of the ten segments of a disk's bar are filled for `used` (0.0 to 1.0): the
/// nearest, and at least one for a disk that holds anything.
pub fn segments_filled(used: f64) -> usize {
    let used = used.clamp(0.0, 1.0);
    let n = (used * SEGMENTS as f64).round() as usize;
    if n == 0 && used > 0.005 { 1 } else { n }
}

/// Segments in the bar of a disk.
pub const SEGMENTS: usize = 10;

/// How full a disk is, in ten segments: `■■■■■■□□□□`. The basic profile has no such glyphs
/// and gets `[######----]`, the brackets keeping the ends apart.
pub fn segment_bar<'a>(th: &Theme, used: f64, basic: bool, row_bg: Option<Color>) -> Vec<Span<'a>> {
    let on = segments_filled(used);
    let mut fill = Style::default().fg(th.disk);
    let mut rest = Style::default().fg(th.muted);
    let mut edge = Style::default().fg(th.muted);
    if let Some(bg) = row_bg {
        fill = fill.bg(bg);
        rest = rest.bg(bg);
        edge = edge.bg(bg);
    }
    let mut out = Vec::new();
    if basic {
        out.push(Span::styled("[", edge));
    }
    out.push(Span::styled("■".repeat(on), fill));
    out.push(Span::styled("□".repeat(SEGMENTS - on), rest));
    if basic {
        out.push(Span::styled("]", edge));
    }
    out
}

/// The cells a disk's bar takes.
pub fn segment_bar_width(basic: bool) -> usize {
    SEGMENTS + if basic { 2 } else { 0 }
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
    let text = format!(" {}  {label} ", key_name(key));
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

/// Register clickable groups of spans of one rendered line: each `(start, end, target)` is the
/// spans `start..end`, together (a pill is several spans).
pub fn hit_groups(
    hits: &mut Hits,
    x: u16,
    y: u16,
    spans: &[Span],
    groups: &[(usize, usize, Target)],
) {
    for (start, end, target) in groups {
        let before: usize = spans[..(*start).min(spans.len())]
            .iter()
            .map(|s| s.content.width())
            .sum();
        let width: usize = spans[(*start).min(spans.len())..(*end).min(spans.len())]
            .iter()
            .map(|s| s.content.width())
            .sum();
        if width > 0 {
            hits.add(
                Rect {
                    x: x + before as u16,
                    y,
                    width: width as u16,
                    height: 1,
                },
                target.clone(),
            );
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

/// One row of a bar, built from spans, some of them clickable. Positions are worked out
/// from the widths, so what is drawn and what can be clicked cannot disagree.
#[derive(Default)]
pub struct Bar<'a> {
    pub spans: Vec<Span<'a>>,
    targets: Vec<(usize, Target)>,
}

impl<'a> Bar<'a> {
    pub fn push(&mut self, span: Span<'a>) {
        self.spans.push(span);
    }

    pub fn push_hit(&mut self, span: Span<'a>, target: Target) {
        self.targets.push((self.spans.len(), target));
        self.spans.push(span);
    }

    /// Several spans that are one clickable thing.
    pub fn push_hits(&mut self, spans: Vec<Span<'a>>, target: Target) {
        for s in spans {
            self.push_hit(s, target.clone());
        }
    }

    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.content.width()).sum()
    }

    /// Keep only what fits in `width` cells: whole spans, and the last one cut.
    pub fn clip(&mut self, width: usize) {
        let mut used = 0;
        let mut keep = 0;
        for (i, s) in self.spans.iter_mut().enumerate() {
            let w = s.content.width();
            if used + w <= width {
                used += w;
                keep = i + 1;
                continue;
            }
            let room = width - used;
            if room > 0 {
                s.content = rada_core::display::truncate(&s.content, room).into();
                keep = i + 1;
            }
            break;
        }
        self.spans.truncate(keep);
        self.targets.retain(|(i, _)| *i < keep);
    }

    pub fn append(&mut self, other: Bar<'a>) {
        let base = self.spans.len();
        self.targets
            .extend(other.targets.into_iter().map(|(i, t)| (base + i, t)));
        self.spans.extend(other.spans);
    }

    /// Draw it at the left edge of `area` and register what can be clicked.
    pub fn render(self, f: &mut ratatui::Frame, hits: &mut Hits, area: Rect) {
        hit_spans(hits, area.x, area.y, &self.spans, &self.targets);
        f.render_widget(
            ratatui::widgets::Paragraph::new(ratatui::text::Line::from(self.spans)),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn th() -> Theme {
        Theme::named("rada", ColorDepth::True).unwrap()
    }

    #[test]
    fn a_disk_bar_has_ten_segments() {
        for used in [0.0, 0.04, 0.3, 0.55, 0.96, 1.0] {
            let bar = segment_bar(&th(), used, false, None);
            let t = text(&bar);
            assert_eq!(t.chars().count(), 10, "{used}: {t}");
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(t.as_str()),
                segment_bar_width(false)
            );
        }
        assert_eq!(text(&segment_bar(&th(), 0.6, false, None)), "■■■■■■□□□□");
        assert_eq!(text(&segment_bar(&th(), 0.0, false, None)), "□□□□□□□□□□");
        assert_eq!(text(&segment_bar(&th(), 1.0, false, None)), "■■■■■■■■■■");
        // A disk with anything on it shows at least a segment.
        assert_eq!(segments_filled(0.01), 1);
        assert_eq!(segments_filled(0.0), 0);
        assert_eq!(segments_filled(0.5), 5);
    }

    #[test]
    fn the_basic_profile_draws_brackets_and_plain_characters() {
        let bar = segment_bar(&th(), 0.5, true, None);
        let ascii: String = text(&bar)
            .chars()
            .map(|c| crate::compat::ascii_for(c).unwrap_or(c))
            .collect();
        assert_eq!(ascii, "[#####-----]");
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(ascii.as_str()),
            segment_bar_width(true)
        );
    }

    #[test]
    fn the_filled_segments_are_the_disk_colour_and_the_rest_quiet() {
        let t = th();
        let bar = segment_bar(&t, 0.3, false, None);
        assert_eq!(bar[0].style.fg, Some(t.disk));
        assert_eq!(bar[1].style.fg, Some(t.muted));
    }
}
