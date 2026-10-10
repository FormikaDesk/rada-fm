//! Navigation history, like a browser's: the folders visited in order, a position in
//! them, and what "back" and "forward" mean from there.

use std::path::{Path, PathBuf};

const MAX: usize = 200;

#[derive(Clone, Debug)]
pub struct NavHistory {
    entries: Vec<PathBuf>,
    pos: usize,
}

impl Default for NavHistory {
    fn default() -> Self {
        NavHistory::new(PathBuf::new())
    }
}

impl NavHistory {
    pub fn new(start: PathBuf) -> NavHistory {
        NavHistory {
            entries: vec![start],
            pos: 0,
        }
    }

    /// A folder reached by going somewhere (not by back or forward): whatever was
    /// "forward" of here is gone, as in a browser. Visiting the folder we are already in
    /// changes nothing.
    pub fn visit(&mut self, path: PathBuf) {
        if self.entries[self.pos] == path {
            return;
        }
        self.entries.truncate(self.pos + 1);
        self.entries.push(path);
        if self.entries.len() > MAX {
            self.entries.remove(0);
        }
        self.pos = self.entries.len() - 1;
    }

    pub fn back_target(&self) -> Option<&Path> {
        self.pos.checked_sub(1).map(|i| self.entries[i].as_path())
    }

    pub fn forward_target(&self) -> Option<&Path> {
        self.entries.get(self.pos + 1).map(PathBuf::as_path)
    }

    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.entries.len()
    }

    /// The move to the previous folder has happened.
    pub fn stepped_back(&mut self) {
        self.pos = self.pos.saturating_sub(1);
    }

    /// The move to the next folder has happened.
    pub fn stepped_forward(&mut self) {
        if self.can_forward() {
            self.pos += 1;
        }
    }
}

/// A back or forward move in flight: the folder it asked for, so that a different
/// navigation arriving first is not mistaken for it.
#[derive(Clone, Debug)]
pub enum NavMove {
    Back(PathBuf),
    Forward(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn a_fresh_history_goes_nowhere() {
        let h = NavHistory::new(p("/a"));
        assert!(!h.can_back() && !h.can_forward());
        assert_eq!(h.back_target(), None);
    }

    #[test]
    fn back_and_forward_walk_the_visited_folders() {
        let mut h = NavHistory::new(p("/a"));
        h.visit(p("/a/b"));
        h.visit(p("/a/b/c"));
        assert_eq!(h.back_target(), Some(Path::new("/a/b")));
        h.stepped_back();
        h.stepped_back();
        assert!(!h.can_back() && h.can_forward());
        assert_eq!(h.forward_target(), Some(Path::new("/a/b")));
        h.stepped_forward();
        assert_eq!(h.forward_target(), Some(Path::new("/a/b/c")));
    }

    #[test]
    fn going_somewhere_new_drops_the_forward_part() {
        let mut h = NavHistory::new(p("/a"));
        h.visit(p("/b"));
        h.visit(p("/c"));
        h.stepped_back();
        h.visit(p("/d"));
        assert!(!h.can_forward());
        assert_eq!(h.back_target(), Some(Path::new("/b")));
    }

    #[test]
    fn visiting_the_current_folder_again_adds_nothing() {
        let mut h = NavHistory::new(p("/a"));
        h.visit(p("/a"));
        assert!(!h.can_back());
    }

    #[test]
    fn the_history_is_bounded() {
        let mut h = NavHistory::new(p("/0"));
        for i in 1..=300 {
            h.visit(p(&format!("/{i}")));
        }
        assert_eq!(h.entries.len(), MAX);
        assert_eq!(h.back_target(), Some(Path::new("/299")));
    }
}
