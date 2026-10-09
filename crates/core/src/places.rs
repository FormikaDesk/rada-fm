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
use crate::platform::{PlaceKind, UserDirs};

const MAX_RECENT: usize = 60;

/// A standard place of the sidebar: where it is and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub kind: PlaceKind,
    pub path: PathBuf,
}

impl Place {
    /// The name shown for it: the real name of the folder (so "Scaricati" on an Italian
    /// system), "Home" and "Trash" for those two.
    pub fn name(&self) -> String {
        match self.kind {
            PlaceKind::Home => "Home".into(),
            PlaceKind::Trash => "Trash".into(),
            _ => self
                .path
                .file_name()
                .map(|n| crate::display::name(n))
                .unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathLists {
    /// Most recent first.
    pub recents: Vec<PathBuf>,
    pub bookmarks: Vec<PathBuf>,
    /// Standard places that exist: Home, Documents, Downloads…, Trash.
    pub places: Vec<Place>,
}

/// The standard places a system defines, whether or not they exist yet: Home first, then
/// the user folders in display order, then the trash.
pub fn standard_places(home: &Path, user: &UserDirs, trash: &Path) -> Vec<Place> {
    let mut v = vec![Place {
        kind: PlaceKind::Home,
        path: home.to_path_buf(),
    }];
    v.extend(user.entries().into_iter().map(|(kind, p)| Place {
        kind,
        path: p.to_path_buf(),
    }));
    v.push(Place {
        kind: PlaceKind::Trash,
        path: trash.to_path_buf(),
    });
    v
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
    /// `state_dir` is `$XDG_STATE_HOME/rada`; `standard` are the candidate standard places
    /// (see [`standard_places`]), of which the ones that exist are reported.
    pub fn spawn(state_dir: PathBuf, standard: Vec<Place>, out: Sender<CoreEvent>) -> PlacesStore {
        let (tx, rx) = mpsc::channel::<Req>();
        std::thread::Builder::new()
            .name("rada-places".into())
            .spawn(move || {
                let recent_file = state_dir.join("recent.json");
                let mark_file = state_dir.join("bookmarks.json");
                let mut recents = read(&recent_file);
                let mut bookmarks = read(&mark_file);
                let send = |r: &Vec<PathBuf>, b: &Vec<PathBuf>| {
                    // Checked on every report, here on the worker: a folder created or
                    // removed since the last one comes and goes.
                    let places = standard
                        .iter()
                        .filter(|p| p.path.is_dir())
                        .cloned()
                        .collect();
                    out.send(CoreEvent::Paths(PathLists {
                        recents: r.clone(),
                        bookmarks: b.clone(),
                        places,
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
