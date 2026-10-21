//! The folders of the NAV tree of the panels view: the subfolders of one folder, with the number
//! of items each holds and whether any of them is a folder, listed on a thread of their own.

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::unbounded;
use rada_core::events::CoreEvent;
use rada_core::fs::LocalFs;
use rada_core::testutil::*;
use rada_core::workers::{TreeKid, tree_dirs};

fn kids_of(dir: &std::path::Path, hidden: bool) -> Vec<TreeKid> {
    let (tx, rx) = unbounded();
    tree_dirs(Arc::new(LocalFs), dir.to_path_buf(), hidden, tx);
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(CoreEvent::Tree {
                dir: d,
                hidden: h,
                kids,
            }) => {
                assert_eq!(d, dir);
                assert_eq!(h, hidden);
                return kids;
            }
            Ok(_) => {}
            Err(_) => panic!("no answer about {}", dir.display()),
        }
    }
}

#[test]
fn the_subfolders_come_with_their_item_counts_and_whether_they_have_folders() {
    let sb = Sandbox::new();
    sb.write("home/docs/a.txt", "a");
    sb.write("home/docs/b.txt", "b");
    sb.write("home/projects/demo/main.rs", "fn main() {}");
    sb.write("home/projects/notes.md", "n");
    sb.mkdir("home/empty");
    sb.write("home/loose.txt", "not a folder");
    sb.write("home/.cache/x", "hidden");
    let home = sb.path("home");

    let kids = kids_of(&home, false);
    let names: Vec<&str> = kids.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(
        names,
        ["docs", "empty", "projects"],
        "folders only, no hidden ones, sorted"
    );
    let by = |n: &str| kids.iter().find(|k| k.name == n).unwrap();
    assert_eq!((by("docs").items, by("docs").has_dirs), (2, false));
    assert_eq!((by("empty").items, by("empty").has_dirs), (0, false));
    assert_eq!((by("projects").items, by("projects").has_dirs), (2, true));
    assert_eq!(by("projects").path, home.join("projects"));

    // Asked for, the hidden folders are in the list too.
    let all = kids_of(&home, true);
    assert!(all.iter().any(|k| k.name == ".cache" && k.items == 1));
}

#[test]
fn a_folder_that_cannot_be_read_gives_an_empty_list() {
    let sb = Sandbox::new();
    let nothing = sb.path("not/there");
    assert!(kids_of(&nothing, false).is_empty());
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_is_a_leaf_and_never_followed() {
    let sb = Sandbox::new();
    sb.mkdir("home/real/inner");
    std::os::unix::fs::symlink(sb.path("home/real"), sb.path("home/link")).unwrap();
    let kids = kids_of(&sb.path("home"), false);
    let names: Vec<&str> = kids.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, ["real"], "the link is not a folder of the tree");
}
