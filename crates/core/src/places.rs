//! Recent folders, bookmarks and standard places, kept in the state folder.
//!
//! Reading and writing happen on a worker thread; the interface only sends requests and
//! receives [`PathLists`] events.

use std::collections::HashMap;
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
            PlaceKind::Trash => crate::i18n::Key::PlaceTrash.tr().into(),
            PlaceKind::ICloud => "iCloud Drive".into(),
            _ => self
                .path
                .file_name()
                .map(crate::display::name)
                .unwrap_or_default(),
        }
    }
}

/// What the places worker asks of the system once: the names it shows for folders and
/// drives, and whether it has a list of recent files.
pub type AskSystem = dyn FnOnce() -> (Vec<(PathBuf, String)>, bool) + Send;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathLists {
    /// Most recent first.
    pub recents: Vec<PathBuf>,
    pub bookmarks: Vec<PathBuf>,
    /// Standard places that exist: Home, Documents, Downloads…, Trash.
    pub places: Vec<Place>,
    /// The names the system's file manager shows for some folders and drives, where they are
    /// not the names on disk ("Scrivania" for `Desktop`). Arrives a moment after the places.
    pub names: HashMap<PathBuf, String>,
    /// The system has a list of the files used lately that can be read: the sidebar shows
    /// "Recent". Arrives with the names, a moment after the places.
    pub recent_files: bool,
}

impl PathLists {
    /// The name to show for a folder: the system's, or `None` when it is the name on disk.
    pub fn shown_name(&self, path: &Path) -> Option<&str> {
        self.names.get(path).map(String::as_str)
    }
}

/// `standard` with the places a system has beyond the usual ones (iCloud Drive), put before
/// the trash.
pub fn with_extra_places(mut standard: Vec<Place>, extra: Vec<(PlaceKind, PathBuf)>) -> Vec<Place> {
    let at = standard
        .iter()
        .position(|p| p.kind == PlaceKind::Trash)
        .unwrap_or(standard.len());
    let mut put = 0;
    for (kind, path) in extra {
        // A place a system names twice is listed once.
        if standard.iter().any(|p| same_folder(&p.path, &path)) {
            continue;
        }
        standard.insert(at + put, Place { kind, path });
        put += 1;
    }
    standard
}

/// Whether two paths name the same folder as far as their spelling goes: the same
/// components, and on a system that ignores case (macOS, Windows) the same letters whatever
/// their case.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    same_folder_with(a, b, cfg!(any(windows, target_os = "macos")))
}

/// [`same_folder`], with the case rule given.
pub fn same_folder_with(a: &Path, b: &Path, ignore_case: bool) -> bool {
    let mut x = a.components();
    let mut y = b.components();
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) => {
                let (p, q) = (p.as_os_str(), q.as_os_str());
                let same = if ignore_case {
                    p.to_string_lossy().to_lowercase() == q.to_string_lossy().to_lowercase()
                } else {
                    p == q
                };
                if !same {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// The standard places a system defines, whether or not they exist yet: Home first, then
/// the user folders in display order, then the trash.
pub fn standard_places(home: &Path, user: &UserDirs, trash: &Path) -> Vec<Place> {
    let mut v = vec![Place {
        kind: PlaceKind::Home,
        path: home.to_path_buf(),
    }];
    // Two user folders that are the same place (a system that points Videos at Movies and
    // Movies again, a custom setup) are one entry, the first.
    for (kind, p) in user.entries() {
        if !v.iter().any(|x| same_folder(&x.path, p)) {
            v.push(Place {
                kind,
                path: p.to_path_buf(),
            });
        }
    }
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
        PlacesStore::spawn_with_names(state_dir, standard, Box::new(|| (Vec::new(), false)), out)
    }

    /// [`spawn`](Self::spawn), and `names` is asked once, after the first report, for the
    /// names the system shows for folders and drives and for whether it has a list of recent
    /// files; they follow in a second report.
    pub fn spawn_with_names(
        state_dir: PathBuf,
        standard: Vec<Place>,
        names: Box<AskSystem>,
        out: Sender<CoreEvent>,
    ) -> PlacesStore {
        let (tx, rx) = mpsc::channel::<Req>();
        std::thread::Builder::new()
            .name("rada-places".into())
            .spawn(move || {
                let recent_file = state_dir.join("recent.json");
                let mark_file = state_dir.join("bookmarks.json");
                let mut recents = read(&recent_file);
                let mut bookmarks = read(&mark_file);
                let mut names = Some(names);
                let mut known: HashMap<PathBuf, String> = HashMap::new();
                let mut has_recents = false;
                let send = |r: &Vec<PathBuf>,
                            b: &Vec<PathBuf>,
                            n: &HashMap<PathBuf, String>,
                            recent: bool| {
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
                        names: n.clone(),
                        recent_files: recent,
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
                            write(&recent_file, &recents);
                        }
                        Req::ToggleBookmark(p) => {
                            if let Some(i) = bookmarks.iter().position(|x| *x == p) {
                                bookmarks.remove(i);
                            } else {
                                bookmarks.push(p);
                            }
                            write(&mark_file, &bookmarks);
                        }
                    }
                    if !send(&recents, &bookmarks, &known, has_recents) {
                        return;
                    }
                    // The names come second: asking the system may take a moment.
                    if let Some(ask) = names.take() {
                        let (found, recent) = ask();
                        known = found.into_iter().collect();
                        has_recents = recent;
                        if (!known.is_empty() || has_recents)
                            && !send(&recents, &bookmarks, &known, has_recents)
                        {
                            return;
                        }
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

fn write(file: &Path, list: &[PathBuf]) {
    let data: Vec<P> = list.iter().cloned().map(P).collect();
    let Ok(json) = serde_json::to_vec(&data) else {
        return;
    };
    if let Err(e) = crate::statefile::write_atomic(file, &json) {
        tracing::debug!("cannot save {}: {e}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::UserDirs;

    fn names(v: &[Place]) -> Vec<(PlaceKind, String)> {
        v.iter()
            .map(|p| (p.kind, p.path.display().to_string()))
            .collect()
    }

    #[test]
    fn a_user_folder_that_is_listed_twice_is_one_place() {
        let home = Path::new("/Users/me");
        // Videos and Movies: on a Mac both are ~/Movies.
        let mut u = UserDirs::conventional(home, "Movies");
        u.music = Some(home.join("Movies"));
        let v = standard_places(home, &u, &home.join(".Trash"));
        let movies: Vec<_> = v.iter().filter(|p| p.path == home.join("Movies")).collect();
        assert_eq!(movies.len(), 1, "{:?}", names(&v));
        assert_eq!(v.first().map(|p| p.kind), Some(PlaceKind::Home));
        assert_eq!(v.last().map(|p| p.kind), Some(PlaceKind::Trash));
    }

    #[test]
    fn the_folders_a_system_adds_are_not_repeated_either() {
        let home = Path::new("/Users/me");
        let std = standard_places(
            home,
            &UserDirs::conventional(home, "Movies"),
            &home.join(".Trash"),
        );
        let n = std.len();
        let same = vec![(PlaceKind::ICloud, home.join("Movies"))];
        assert_eq!(with_extra_places(std.clone(), same).len(), n);
        let new = vec![(PlaceKind::ICloud, home.join("iCloud"))];
        let with = with_extra_places(std, new);
        assert_eq!(with.len(), n + 1);
        assert_eq!(
            with[with.len() - 2].kind,
            PlaceKind::ICloud,
            "before the trash"
        );
    }

    #[test]
    fn spelling_decides_and_a_system_that_ignores_case_ignores_it_here_too() {
        let (a, b) = (Path::new("/Users/me/Movies"), Path::new("/users/ME/movies"));
        assert!(same_folder_with(a, b, true));
        assert!(!same_folder_with(a, b, false));
        assert!(same_folder_with(a, Path::new("/Users/me/Movies/"), false));
        assert!(!same_folder_with(a, Path::new("/Users/me/Movies/x"), true));
        assert!(!same_folder_with(a, Path::new("/Users/me"), true));
    }
}
