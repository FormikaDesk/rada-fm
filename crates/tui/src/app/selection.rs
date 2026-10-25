//! Where the cursor is in a tab, what the list is scrolled to, and what is marked.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::ops::RangeInclusive;

use super::App;
use super::Viewport;
use crate::view::ViewMode;

/// The cursor, the marks and the part of the list in view, of one tab.
#[derive(Default)]
pub struct Selection {
    /// The position of the cursor among the entries shown.
    pub cursor: usize,
    /// The first entry shown; in the icon view, the first tile, always the start of a row.
    /// It sits here and not with the viewport because it only ever follows the cursor.
    pub scroll: usize,
    pub marked: BTreeSet<OsString>,
    /// Where an extended selection started and what was marked before it did.
    sel_anchor: Option<(usize, BTreeSet<OsString>)>,
}

impl Selection {
    /// Put the cursor on `to`, or on the last of `count` entries if that is past them. It
    /// tells whether the cursor moved.
    pub fn move_to(&mut self, to: usize, count: usize) -> bool {
        let to = to.min(count.saturating_sub(1));
        let moved = to != self.cursor;
        self.cursor = to;
        moved
    }

    /// Scroll as little as it takes for the cursor to be in view among `count` entries.
    pub fn keep_visible(&mut self, view: ViewMode, viewport: Viewport, count: usize) {
        if view == ViewMode::Icons {
            self.keep_tile_visible(viewport, count);
            return;
        }
        let rows = viewport.rows.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + rows {
            self.scroll = self.cursor + 1 - rows;
        }
        self.scroll = self.scroll.min(count.saturating_sub(rows));
    }

    fn keep_tile_visible(&mut self, viewport: Viewport, count: usize) {
        let cols = viewport.grid_cols.max(1);
        let rows = viewport.grid_rows.max(1);
        let cursor_row = self.cursor / cols;
        let mut first = self.scroll / cols;
        if cursor_row < first {
            first = cursor_row;
        } else if cursor_row >= first + rows {
            first = cursor_row + 1 - rows;
        }
        let last_row = count.saturating_sub(1) / cols;
        first = first.min(last_row.saturating_sub(rows - 1));
        self.scroll = first * cols;
    }

    /// Mark `name`, or take its mark off if it has one.
    pub fn toggle(&mut self, name: OsString) {
        if !self.marked.remove(&name) {
            self.marked.insert(name);
        }
    }

    /// Mark all of `names`, or none if they all are marked already.
    pub fn toggle_all(&mut self, names: Vec<OsString>) {
        if self.marked.len() == names.len() {
            self.clear_marks();
        } else {
            self.marked = names.into_iter().collect();
        }
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
        self.sel_anchor = None;
    }

    /// The next Shift move starts over from the cursor.
    pub fn drop_anchor(&mut self) {
        self.sel_anchor = None;
    }

    /// A selection extended from now on starts at `at`, on top of what is marked now.
    pub fn anchor_at(&mut self, at: usize) {
        self.sel_anchor = Some((at, self.marked.clone()));
    }

    /// A selection extended from now on starts at `at`, with nothing marked before it.
    pub fn anchor_alone(&mut self, at: usize) {
        self.sel_anchor = Some((at, BTreeSet::new()));
    }

    /// Start extending a selection from the cursor, unless one is being extended already.
    pub fn begin_extend(&mut self) {
        if self.sel_anchor.is_none() {
            self.anchor_at(self.cursor);
        }
    }

    /// The entries between the anchor and the cursor, both included.
    pub fn span(&self) -> Option<RangeInclusive<usize>> {
        let (anchor, _) = self.sel_anchor.as_ref()?;
        Some(*anchor.min(&self.cursor)..=*anchor.max(&self.cursor))
    }

    /// Mark `names`, the ones in [`span`](Self::span), on top of what was marked before the
    /// anchor was set.
    pub fn mark_span(&mut self, names: impl IntoIterator<Item = OsString>) {
        let Some((_, base)) = &self.sel_anchor else {
            return;
        };
        let mut marked = base.clone();
        marked.extend(names);
        self.marked = marked;
    }
}

impl App {
    pub(super) fn set_cursor(&mut self, to: usize) {
        if self.tab.sel.move_to(to, self.tab.visible.len()) {
            self.preview.scroll = 0;
        }
        self.tab.ensure_visible();
        self.request_preview();
        self.redraw.dirty = true;
    }

    pub(super) fn toggle_mark(&mut self) {
        if let Some(e) = self.tab.current() {
            let name = e.name.clone();
            self.tab.sel.toggle(name);
        }
        self.set_cursor(self.tab.sel.cursor + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn names(of: &[&str]) -> Vec<OsString> {
        of.iter().map(OsString::from).collect()
    }

    fn view(rows: usize) -> Viewport {
        Viewport {
            rows,
            grid_cols: 4,
            grid_rows: 2,
        }
    }

    #[test]
    fn the_cursor_stops_at_the_last_entry_and_says_when_it_moved() {
        let mut s = Selection::default();
        assert!(s.move_to(99, 5));
        assert_eq!(s.cursor, 4);
        assert!(!s.move_to(4, 5));
        assert!(s.move_to(0, 0));
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_the_cursor_needs() {
        let mut s = Selection {
            cursor: 12,
            ..Default::default()
        };
        s.keep_visible(ViewMode::Details, view(10), 50);
        assert_eq!(s.scroll, 3);
        s.cursor = 5;
        s.keep_visible(ViewMode::Details, view(10), 50);
        assert_eq!(s.scroll, 3);
        s.cursor = 2;
        s.keep_visible(ViewMode::Details, view(10), 50);
        assert_eq!(s.scroll, 2);
    }

    #[test]
    fn a_short_list_is_not_scrolled_past_its_end() {
        let mut s = Selection {
            cursor: 9,
            scroll: 8,
            ..Default::default()
        };
        s.keep_visible(ViewMode::Details, view(10), 12);
        assert_eq!(s.scroll, 2);
    }

    #[test]
    fn tiles_scroll_by_whole_rows() {
        let mut s = Selection {
            cursor: 13,
            ..Default::default()
        };
        s.keep_visible(ViewMode::Icons, view(10), 20);
        assert_eq!(s.scroll, 8);
        s.cursor = 1;
        s.keep_visible(ViewMode::Icons, view(10), 20);
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn toggling_marks_and_unmarks() {
        let mut s = Selection::default();
        s.toggle("a".into());
        assert!(s.marked.contains(OsStr::new("a")));
        s.toggle("a".into());
        assert!(s.marked.is_empty());
    }

    #[test]
    fn toggle_all_marks_everything_then_nothing() {
        let mut s = Selection::default();
        s.toggle("a".into());
        s.toggle_all(names(&["a", "b"]));
        assert_eq!(s.marked.len(), 2);
        s.toggle_all(names(&["a", "b"]));
        assert!(s.marked.is_empty());
    }

    #[test]
    fn an_extended_selection_adds_to_what_was_marked_before_it_started() {
        let mut s = Selection::default();
        s.toggle("x".into());
        s.cursor = 1;
        s.begin_extend();
        s.cursor = 3;
        assert_eq!(s.span(), Some(1..=3));
        s.mark_span(names(&["b", "c", "d"]));
        assert_eq!(s.marked.len(), 4);
        s.cursor = 2;
        s.mark_span(names(&["b", "c"]));
        assert_eq!(s.marked, names(&["x", "b", "c"]).into_iter().collect());
    }

    #[test]
    fn without_an_anchor_there_is_no_span_and_marking_a_span_does_nothing() {
        let mut s = Selection::default();
        assert_eq!(s.span(), None);
        s.mark_span(names(&["a"]));
        assert!(s.marked.is_empty());
        s.begin_extend();
        s.drop_anchor();
        assert_eq!(s.span(), None);
    }

    #[test]
    fn clearing_the_marks_drops_the_anchor_too() {
        let mut s = Selection::default();
        s.toggle("a".into());
        s.anchor_at(0);
        s.clear_marks();
        assert!(s.marked.is_empty());
        assert_eq!(s.span(), None);
    }
}
