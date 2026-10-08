//! Recent folders, bookmarks and standard places, kept in the state folder.
//!
//! Reading and writing happen on a worker thread; the interface only sends requests and
//! receives [`PathLists`] events.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};

use crate::events::CoreEvent;
use crate::pathcodec;

const MAX_RECENT: usize = 60;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathLists {
    /// Most recent first.
    pub recents: Vec<PathBuf>,
    pub bookmarks: Vec<PathBuf>,
    /// Existing standard folders: home, Documents, Downloads...
    pub places: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct P(#[serde(with = "pathcodec::path")] PathBuf);

enum Req {
    Load,
    Visit(PathBuf),
    ToggleBookmark(PathBuf),
}

#[derive(Clone)]
pub struct PlacesStore {
    tx: mpsc::Sender<Req>,
}

impl PlacesStore {
    /// `state_dir` is `$XDG_STATE_HOME/rada`; `home` is the user's home folder.
    pub fn spawn(state_dir: PathBuf, home: PathBuf, out: Sender<CoreEvent>) -> PlacesStore {
        let (tx, rx) = mpsc::channel::<Req>();
        std::thread::Builder::new()
            .name("rada-places".into())
            .spawn(move || {
                let recent_file = state_dir.join("recent.json");
                let mark_file = state_dir.join("bookmarks.json");
                let mut recents = read(&recent_file);
                let mut bookmarks = read(&mark_file);
                let places = standard_places(&home);
                let send = |r: &Vec<PathBuf>, b: &Vec<PathBuf>| {
                    out.send(CoreEvent::Paths(PathLists {
                        recents: r.clone(),
                        bookmarks: b.clone(),
                        places: places.clone(),
                    }))
                    .is_ok()
                };
                while let Ok(req) = rx.recv() {
                    match req {
                        Req::Load => {}
                        Req::Visit(p) => {
                            recents.retain(|x| *x != p);
                            recents.insert(0, p);
                            recents.truncate(MAX_RECENT);
                            write(&state_dir, &recent_file, &recents);
                        }
                        Req::ToggleBookmark(p) => {
                            if let Some(i) = bookmarks.iter().position(|x| *x == p) {
                                bookmarks.remove(i);
                            } else {
                                bookmarks.push(p);
                            }
                            write(&state_dir, &mark_file, &bookmarks);
                        }
                    }
                    if !send(&recents, &bookmarks) {
                        return;
                    }
                }
            })
            .expect("spawn places worker");
        PlacesStore { tx }
    }

    pub fn load(&self) {
        let _ = self.tx.send(Req::Load);
    }

    pub fn visit(&self, dir: PathBuf) {
        let _ = self.tx.send(Req::Visit(dir));
    }

    pub fn toggle_bookmark(&self, dir: PathBuf) {
        let _ = self.tx.send(Req::ToggleBookmark(dir));
    }
}

fn read(file: &Path) -> Vec<PathBuf> {
    std::fs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<P>>(&b).ok())
        .map(|v| v.into_iter().map(|p| p.0).collect())
        .unwrap_or_default()
}

fn write(dir: &Path, file: &Path, list: &[PathBuf]) {
    let data: Vec<P> = list.iter().cloned().map(P).collect();
    let Ok(json) = serde_json::to_vec(&data) else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = file.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, file);
    }
}

fn standard_places(home: &Path) -> Vec<PathBuf> {
    let mut v = vec![home.to_path_buf()];
    for name in [
        "Desktop",
        "Documents",
        "Downloads",
        "Music",
        "Pictures",
        "Videos",
        "Projects",
        "Code",
    ] {
        let p = home.join(name);
        if p.is_dir() {
            v.push(p);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use crossbeam_channel::unbounded;
    use std::time::Duration;

    fn next(rx: &crossbeam_channel::Receiver<CoreEvent>) -> PathLists {
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).expect("event") {
                CoreEvent::Paths(p) => return p,
                _ => continue,
            }
        }
    }

    #[test]
    fn recents_and_bookmarks_persist_and_places_are_listed() {
        let sb = Sandbox::new();
        std::fs::create_dir_all(sb.dirs.home.join("Documents")).unwrap();
        let (tx, rx) = unbounded();
        let dir = sb.dirs.rada_state();
        let s = PlacesStore::spawn(dir.clone(), sb.dirs.home.clone(), tx);
        s.load();
        let p = next(&rx);
        assert!(p.recents.is_empty() && p.bookmarks.is_empty());
        assert_eq!(
            p.places,
            vec![sb.dirs.home.clone(), sb.dirs.home.join("Documents")]
        );

        s.visit(PathBuf::from("/a"));
        s.visit(PathBuf::from("/b"));
        s.visit(PathBuf::from("/a"));
        assert_eq!(next(&rx).recents.len(), 1);
        assert_eq!(next(&rx).recents.len(), 2);
        let p = next(&rx);
        assert_eq!(
            p.recents,
            vec![PathBuf::from("/a"), PathBuf::from("/b")],
            "newest first, no duplicates"
        );
        s.toggle_bookmark(PathBuf::from("/b"));
        assert_eq!(next(&rx).bookmarks, vec![PathBuf::from("/b")]);

        // A new process sees the same data.
        let (tx2, rx2) = unbounded();
        let s2 = PlacesStore::spawn(dir, sb.dirs.home.clone(), tx2);
        s2.load();
        let p = next(&rx2);
        assert_eq!(p.recents.len(), 2);
        assert_eq!(p.bookmarks, vec![PathBuf::from("/b")]);
        s2.toggle_bookmark(PathBuf::from("/b"));
        assert!(next(&rx2).bookmarks.is_empty());
    }
}
