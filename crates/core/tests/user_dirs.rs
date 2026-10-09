//! The user's standard folders as the sidebar gets them: read from `user-dirs.dirs` in
//! the sandbox's own config folder, under the names of the system (here, Italian), and
//! reported by the places worker only when they exist.

use std::time::Duration;

use crossbeam_channel::unbounded;
use rada_core::events::CoreEvent;
use rada_core::places::{PlacesStore, standard_places};
use rada_core::platform::PlaceKind;
use rada_core::testutil::*;

const ITALIAN: &str = r#"XDG_DESKTOP_DIR="$HOME/Scrivania"
XDG_DOWNLOAD_DIR="$HOME/Scaricati"
XDG_DOCUMENTS_DIR="$HOME/Documenti"
XDG_MUSIC_DIR="$HOME/Musica"
XDG_PICTURES_DIR="$HOME/Immagini"
XDG_VIDEOS_DIR="$HOME/Video"
"#;

#[test]
fn the_platform_reads_the_sandbox_user_dirs_file() {
    let sb = Sandbox::new();
    std::fs::create_dir_all(&sb.dirs.config).unwrap();
    std::fs::write(sb.dirs.config.join("user-dirs.dirs"), ITALIAN).unwrap();
    let u = sb.platform().user_dirs();
    assert_eq!(u.downloads, Some(sb.dirs.home.join("Scaricati")));
    assert_eq!(u.documents, Some(sb.dirs.home.join("Documenti")));
    assert_eq!(u.pictures, Some(sb.dirs.home.join("Immagini")));
}

#[test]
fn without_the_file_the_english_names_are_used() {
    let sb = Sandbox::new();
    let u = sb.platform().user_dirs();
    assert_eq!(u.downloads, Some(sb.dirs.home.join("Downloads")));
    assert_eq!(u.videos, Some(sb.dirs.home.join("Videos")));
}

#[test]
fn the_worker_reports_only_the_places_that_exist_under_their_real_names() {
    let sb = Sandbox::new();
    std::fs::create_dir_all(&sb.dirs.config).unwrap();
    std::fs::write(sb.dirs.config.join("user-dirs.dirs"), ITALIAN).unwrap();
    let home = &sb.dirs.home;
    // Only some of them exist; "Downloads" exists too but is not what this system uses.
    for d in ["Scaricati", "Documenti", "Downloads"] {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    let p = sb.platform();
    let standard = standard_places(home, &p.user_dirs(), &p.dirs().home_trash().join("files"));
    let (tx, rx) = unbounded();
    let store = PlacesStore::spawn(sb.dirs.rada_state(), standard, tx);
    store.load();
    let lists = loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("places") {
            CoreEvent::Paths(l) => break l,
            _ => continue,
        }
    };
    let shown: Vec<(PlaceKind, String)> = lists.places.iter().map(|p| (p.kind, p.name())).collect();
    assert_eq!(
        shown,
        [
            (PlaceKind::Home, "Home".to_string()),
            (PlaceKind::Documents, "Documenti".to_string()),
            (PlaceKind::Downloads, "Scaricati".to_string()),
        ]
    );
}
