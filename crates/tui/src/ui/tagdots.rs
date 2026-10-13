//! Tags as coloured dots next to a name.

use rada_core::platform::Tag;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

/// The dots for `tags`: `●` in each tag's colour, three at most, then `+N` for the N more.
/// Empty when there are no tags. A tag with no colour is a dim dot.
pub fn dots(th: &Theme, tags: &[Tag]) -> Vec<(String, Color)> {
    let mut out: Vec<(String, Color)> = tags
        .iter()
        .take(3)
        .map(|t| ("●".to_string(), th.tag_color(t.color)))
        .collect();
    if tags.len() > 3 {
        out.push((format!("+{}", tags.len() - 3), th.text_dim));
    }
    out
}

/// The cells the dots take, with the space before them.
pub fn width(dots: &[(String, Color)]) -> usize {
    if dots.is_empty() {
        0
    } else {
        1 + dots.iter().map(|(s, _)| s.width()).sum::<usize>()
    }
}
