//! The names the Finder shows for the folders every Mac has ("Scrivania" for `Desktop`) with an
//! Italian interface, on a stand-in for macOS: in the sidebar, the list, the path bar and the
//! tabs; and the real name everywhere else. The language is a process-wide setting, so the two
//! halves run one after the other in one test.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use rada_core::fs::LocalFs;
use rada_core::i18n::{Lang, set_lang};
use rada_core::testplatform::{MacLike, MacLikeSpec};
use rada_core::testutil::*;
use rada_tui::hits::Target;

/// A home with the folders of a Mac, each with its `.localized` marker, and a folder made by
/// hand that has the name of one of them but no marker.
fn mac_home(sb: &Sandbox) {
    for f in [
        "Desktop",
        "Documents",
        "Downloads",
        "Music",
        "Movies",
        "Library",
    ] {
        let d = sb.dirs.home.join(f);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".localized"), b"").unwrap();
    }
    std::fs::create_dir_all(sb.dirs.home.join("Desktop/progetti")).unwrap();
    std::fs::write(sb.dirs.home.join("Desktop/progetti/x.txt"), "x").unwrap();
    std::fs::create_dir_all(sb.dirs.home.join("Pictures")).unwrap();
}

fn go(h: &mut H, to: &Path) {
    h.click(&Target::Place(to.to_path_buf()));
    let to = to.to_path_buf();
    h.wait("the folder", move |a| a.cwd == to && !a.is_loading());
}

fn start(sb: Sandbox, at: &Path, tables: &Path) -> H {
    let p = Arc::new(MacLike::new(
        sb.platform(),
        MacLikeSpec {
            folder_tables: Some(tables.to_path_buf()),
            ..Default::default()
        },
    ));
    H::with_system(
        sb,
        at.to_path_buf(),
        150,
        34,
        (Arc::new(LocalFs), p),
        |_| {},
    )
}

#[test]
fn the_folders_of_a_mac_are_named_in_the_language_of_the_interface() {
    // ------------------------------------------------------------------- Italian
    set_lang(Lang::It);
    let sb = Sandbox::new();
    mac_home(&sb);
    let tables = sb.path("system-tables");
    std::fs::create_dir_all(&tables).unwrap();
    let home = sb.dirs.home.clone();
    let desktop_projects = home.join("Desktop/progetti");
    let mut h = start(sb, &desktop_projects, &tables);
    h.wait("the names", |a| {
        a.places
            .paths
            .shown_name(&a.home().join("Desktop"))
            .is_some()
    });
    let s = h.screen();
    // The sidebar, with the nine names of the Finder that exist here.
    for shown in ["Scrivania", "Documenti", "Download", "Musica"] {
        assert!(s.contains(shown), "the sidebar has {shown}: {s}");
    }
    assert!(
        !s.contains(" Desktop "),
        "no name on disk in the sidebar: {s}"
    );
    // `Pictures` here has no marker, like a folder made by hand: it keeps its name.
    assert!(s.contains("Pictures") && !s.contains("Immagini"), "{s}");
    // The path bar and the tab.
    let bar = s
        .lines()
        .find(|l| l.contains("progetti") && l.contains('›'));
    assert!(
        bar.is_some_and(|b| b.contains("Scrivania")),
        "the path bar: {s}"
    );
    go(&mut h, &home.join("Desktop"));
    assert_eq!(h.app.tab_title(0), "Scrivania");
    // The list of the home folder.
    go(&mut h, &home);
    let names = h.names();
    for shown in ["Scrivania", "Documenti", "Filmati", "Libreria", "Musica"] {
        assert!(names.contains(&shown.to_string()), "{shown} in {names:?}");
    }
    for real in ["Desktop", "Documents", "Movies", "Music"] {
        assert!(
            !names.contains(&real.to_string()),
            "{real} hidden in {names:?}"
        );
    }
    // The real name is what a rename starts from, what is copied, and what the filter finds.
    h.keys("Y");
    h.press("f2");
    let copied = h.app.copied_text.clone().unwrap_or_default();
    assert!(
        copied.contains(&home.to_string_lossy().to_string()),
        "{copied}"
    );
    let s = h.screen();
    assert!(
        s.contains("Desktop") || s.contains("Documents") || s.contains("Downloads"),
        "{s}"
    );
    h.press("esc");
    h.press("ctrl+f");
    h.keys("desktop");
    let names = h.names();
    assert_eq!(names, ["Scrivania"], "the filter takes the real name too");
    h.press("esc");

    // ------------------------------------------------------------------- English
    set_lang(Lang::En);
    let sb = Sandbox::new();
    mac_home(&sb);
    let tables = sb.path("system-tables");
    std::fs::create_dir_all(&tables).unwrap();
    let home = sb.dirs.home.clone();
    let mut h = start(sb, &home, &tables);
    h.wait("the places", |a| a.places.paths.places.len() > 3);
    let names = h.names();
    assert!(names.contains(&"Desktop".to_string()), "{names:?}");
    assert!(!names.contains(&"Scrivania".to_string()), "{names:?}");
    assert!(!h.screen().contains("Scrivania"));
    set_lang(Lang::default());
}
