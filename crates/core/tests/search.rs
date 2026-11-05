//! Search by name under a folder: what it finds, what it leaves alone and where it stops.

use std::path::{Path, PathBuf};

use rada_core::fs::LocalFs;
use rada_core::search::{self, MAX_MATCHES};
use rada_core::testutil::*;

fn find(sb: &Sandbox, root: &str, query: &str) -> Result<search::Found, String> {
    let platform = sb.platform();
    search::entries(&LocalFs, platform.as_ref(), &sb.path(root), query, &|| {
        false
    })
}

fn paths(found: &search::Found) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = found.entries.iter().map(|e| e.path.clone()).collect();
    v.sort();
    v
}

#[test]
fn it_finds_by_every_word_in_folders_at_any_depth_whatever_the_case() {
    let sb = Sandbox::new();
    sb.write("src/Main.rs", "m");
    sb.write("src/deep/er/main_test.rs", "t");
    sb.write("src/readme.md", "r");
    sb.write("other/main.c", "c");

    let found = find(&sb, "src", "MAIN rs").unwrap();

    assert_eq!(
        paths(&found),
        [sb.path("src/Main.rs"), sb.path("src/deep/er/main_test.rs")]
    );
    assert!(!found.truncated);
}

#[test]
fn a_folder_matches_too_and_is_searched_inside() {
    let sb = Sandbox::new();
    sb.write("a/photos/x/photo1.jpg", "p");

    let found = find(&sb, "a", "photo").unwrap();

    assert_eq!(
        paths(&found),
        [sb.path("a/photos"), sb.path("a/photos/x/photo1.jpg")]
    );
}

#[test]
fn two_files_with_one_name_are_two_rows() {
    let sb = Sandbox::new();
    sb.write("p/a/index.js", "1");
    sb.write("p/b/index.js", "2");

    let found = find(&sb, "p", "index").unwrap();

    let names: Vec<_> = found.entries.iter().map(|e| e.name.clone()).collect();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
    assert!(found.entries.iter().all(|e| e.display == "index.js"));
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_is_listed_but_not_entered() {
    let sb = Sandbox::new();
    sb.write("elsewhere/secret-target.txt", "s");
    sb.mkdir("root");
    sb.symlink(sb.path("elsewhere"), "root/secret-link");

    let found = find(&sb, "root", "secret").unwrap();

    assert_eq!(paths(&found), [sb.path("root/secret-link")]);
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_read_is_skipped_and_the_rest_is_found() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.write("r/open/note.txt", "n");
    sb.write("r/shut/note.txt", "n");
    chmod(&sb.path("r/shut"), 0o000);

    let found = find(&sb, "r", "note").unwrap();

    chmod(&sb.path("r/shut"), 0o755);
    assert_eq!(paths(&found), [sb.path("r/open/note.txt")]);
}

#[test]
fn a_file_is_not_a_place_to_search() {
    let sb = Sandbox::new();
    sb.write("f.txt", "x");
    assert!(find(&sb, "f.txt", "f").is_err());
    assert!(find(&sb, "missing", "f").is_err());
}

#[test]
fn it_stops_at_its_limit_and_says_so() {
    let sb = Sandbox::new();
    for i in 0..(MAX_MATCHES + 5) {
        sb.write(format!("many/hit-{i}.txt"), "");
    }

    let found = find(&sb, "many", "hit").unwrap();

    assert_eq!(found.entries.len(), MAX_MATCHES);
    assert!(found.truncated);
}

#[test]
fn a_request_that_was_replaced_stops_the_walk() {
    let sb = Sandbox::new();
    sb.write("w/a/hit.txt", "");
    let platform = sb.platform();

    let found =
        search::entries(&LocalFs, platform.as_ref(), &sb.path("w"), "hit", &|| true).unwrap();

    assert!(found.entries.is_empty());
}

#[test]
fn the_view_path_of_a_search_is_not_a_place_on_the_disk() {
    let p = search::view_path(Path::new("/home/u"), "a b").unwrap();
    assert!(!p.exists() && !p.is_absolute());
}
