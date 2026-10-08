//! The jump palette: one window, fuzzy search over bookmarks, recent folders, places,
//! disks and the folders next to you. Pure data and logic; drawing lives in `ui`.

use std::path::{Path, PathBuf};

use crate::fuzzy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    /// A path typed by the user (`/etc`, `~/src`).
    Typed,
    Bookmark,
    Recent,
    Place,
    Disk,
    Parent,
    Folder,
}

impl PaletteKind {
    pub fn tag(self) -> &'static str {
        match self {
            PaletteKind::Typed => "go to",
            PaletteKind::Bookmark => "bookmark",
            PaletteKind::Recent => "recent",
            PaletteKind::Place => "place",
            PaletteKind::Disk => "disk",
            PaletteKind::Parent => "parent",
            PaletteKind::Folder => "folder",
        }
    }

    fn bonus(self) -> i64 {
        match self {
            PaletteKind::Typed => 1000,
            PaletteKind::Bookmark => 30,
            PaletteKind::Recent => 20,
            PaletteKind::Place => 12,
            PaletteKind::Disk => 8,
            PaletteKind::Parent => 4,
            PaletteKind::Folder => 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PaletteItem {
    pub label: String,
    /// Second line of information: the shortened path, or disk details.
    pub detail: String,
    pub path: PathBuf,
    pub kind: PaletteKind,
}

#[derive(Clone, Debug)]
pub struct PaletteHit {
    pub item: usize,
    /// Matched character positions in `label`, for highlighting.
    pub label_pos: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct PaletteView {
    pub query: String,
    /// Cursor in chars.
    pub cursor: usize,
    pub items: Vec<PaletteItem>,
    pub hits: Vec<PaletteHit>,
    pub selected: usize,
    home: PathBuf,
}

impl PaletteView {
    pub fn new(items: Vec<PaletteItem>, home: PathBuf) -> Self {
        let mut p = PaletteView {
            query: String::new(),
            cursor: 0,
            items,
            hits: Vec::new(),
            selected: 0,
            home,
        };
        p.refilter();
        p
    }

    /// Replace the candidates (new recents arrived) and re-run the current query.
    pub fn set_items(&mut self, items: Vec<PaletteItem>) {
        self.items = items;
        let keep = self.selected;
        self.refilter();
        self.selected = keep.min(self.hits.len().saturating_sub(1));
    }

    pub fn current(&self) -> Option<&PaletteItem> {
        self.hits.get(self.selected).map(|h| &self.items[h.item])
    }

    pub fn refilter(&mut self) {
        let q = self.query.trim();
        let mut scored: Vec<(i64, usize, PaletteHit)> = Vec::new();
        if q.is_empty() {
            for (i, _) in self.items.iter().enumerate() {
                scored.push((
                    -(i as i64),
                    i,
                    PaletteHit {
                        item: i,
                        label_pos: Vec::new(),
                    },
                ));
            }
        } else {
            for (i, it) in self.items.iter().enumerate() {
                let by_label = fuzzy::score(q, &it.label);
                let by_path = fuzzy::score(q, &it.detail);
                let best = match (by_label, by_path) {
                    (Some((a, pos)), Some((b, _))) if a + 25 >= b => Some((a + 25, pos)),
                    (Some((a, pos)), None) => Some((a + 25, pos)),
                    (_, Some((b, _))) => Some((b, Vec::new())),
                    (None, None) => None,
                };
                if let Some((s, pos)) = best {
                    scored.push((
                        s + it.kind.bonus(),
                        i,
                        PaletteHit {
                            item: i,
                            label_pos: pos,
                        },
                    ));
                }
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }
        self.hits = scored.into_iter().map(|(_, _, h)| h).collect();
        // A typed path is always on offer, first.
        if q.starts_with('/') || q.starts_with('~') {
            let path = expand(q, &self.home);
            let typed = PaletteItem {
                label: q.to_string(),
                detail: String::new(),
                path,
                kind: PaletteKind::Typed,
            };
            self.items.retain(|i| i.kind != PaletteKind::Typed);
            self.items.push(typed);
            // Indices shifted only by the retain of an earlier typed item: rebuild hit ids.
            let last = self.items.len() - 1;
            self.hits.retain(|h| h.item < last);
            self.hits.insert(
                0,
                PaletteHit {
                    item: last,
                    label_pos: Vec::new(),
                },
            );
        } else {
            self.items.retain(|i| i.kind != PaletteKind::Typed);
        }
        self.selected = 0;
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.query
            .char_indices()
            .nth(chars)
            .map(|(i, _)| i)
            .unwrap_or(self.query.len())
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.query.insert(at, c);
        self.cursor += 1;
        self.refilter();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let at = self.byte_at(self.cursor - 1);
            self.query.remove(at);
            self.cursor -= 1;
            self.refilter();
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.cursor = 0;
        self.refilter();
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let n = self.hits.len() as isize;
        self.selected = (self.selected as isize + delta).clamp(0, n - 1) as usize;
    }
}

/// `~` and `~/x` expanded.
pub fn expand(q: &str, home: &Path) -> PathBuf {
    if q == "~" {
        home.to_path_buf()
    } else if let Some(rest) = q.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, path: &str, kind: PaletteKind) -> PaletteItem {
        PaletteItem {
            label: label.into(),
            detail: path.into(),
            path: PathBuf::from(path),
            kind,
        }
    }

    fn sample() -> PaletteView {
        PaletteView::new(
            vec![
                item("projects", "~/projects", PaletteKind::Bookmark),
                item("vela", "~/projects/vela", PaletteKind::Recent),
                item("Downloads", "~/Downloads", PaletteKind::Place),
                item("Documents", "~/Documents", PaletteKind::Place),
                item("backup", "/run/media/user/backup", PaletteKind::Disk),
            ],
            PathBuf::from("/home/user"),
        )
    }

    #[test]
    fn an_empty_query_lists_everything_in_priority_order() {
        let p = sample();
        assert_eq!(p.hits.len(), 5);
        assert_eq!(p.current().unwrap().label, "projects");
    }

    #[test]
    fn typing_filters_and_ranks() {
        let mut p = sample();
        for c in "dow".chars() {
            p.insert(c);
        }
        assert_eq!(p.hits.len(), 1);
        assert_eq!(p.current().unwrap().label, "Downloads");
        assert_eq!(p.hits[0].label_pos, vec![0, 1, 2]);
        p.clear();
        for c in "doc".chars() {
            p.insert(c);
        }
        assert_eq!(p.current().unwrap().label, "Documents");
        p.clear();
        for c in "vel".chars() {
            p.insert(c);
        }
        assert_eq!(p.current().unwrap().label, "vela");
    }

    #[test]
    fn a_typed_path_is_offered_first_and_tilde_is_expanded() {
        let mut p = sample();
        for c in "~/src".chars() {
            p.insert(c);
        }
        let cur = p.current().unwrap();
        assert_eq!(cur.kind, PaletteKind::Typed);
        assert_eq!(cur.path, PathBuf::from("/home/user/src"));
        p.clear();
        assert!(
            p.items.iter().all(|i| i.kind != PaletteKind::Typed),
            "no leftover typed item"
        );
    }

    #[test]
    fn selection_stays_in_range() {
        let mut p = sample();
        p.move_by(-3);
        assert_eq!(p.selected, 0);
        p.move_by(100);
        assert_eq!(p.selected, 4);
        for c in "zzzz".chars() {
            p.insert(c);
        }
        assert!(p.current().is_none());
        p.move_by(1); // no panic on an empty result
    }
}
