//! Windows has links to files and links to folders. A link copied or moved from another link
//! is of the same kind, also when its target does not exist (a guess from the target would
//! make every such link a link to a file).
#![cfg(windows)]

use std::os::windows::fs::{FileTypeExt, symlink_dir, symlink_file};
use std::path::Path;

use rada_core::fs::{FsEngine, LocalFs};
use rada_core::ops::*;
use rada_core::testutil::*;

fn is_dir_link(p: &Path) -> bool {
    let t = std::fs::symlink_metadata(p).unwrap().file_type();
    assert!(t.is_symlink(), "{} is not a link", p.display());
    t.is_symlink_dir()
}

/// Making links takes a right (developer mode, or an administrator); a runner without it
/// has nothing to test.
fn dir_link_to_nowhere(sb: &Sandbox, name: &str) -> Option<std::path::PathBuf> {
    let link = sb.path(name);
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    match symlink_dir("nowhere", &link) {
        Ok(()) => Some(link),
        Err(e) => {
            eprintln!("skipped: cannot make links here: {e}");
            None
        }
    }
}

#[test]
fn a_copy_of_a_folder_link_with_no_target_is_a_folder_link() {
    let sb = Sandbox::new();
    let Some(origin) = dir_link_to_nowhere(&sb, "src/dirlink") else {
        return;
    };
    let copy = sb.path("dest/copy");
    std::fs::create_dir_all(sb.path("dest")).unwrap();
    LocalFs
        .create_symlink(Path::new("nowhere"), &copy, Some(&origin))
        .unwrap();
    assert!(is_dir_link(&copy));
}

#[test]
fn a_copy_of_a_file_link_with_no_target_is_a_file_link() {
    let sb = Sandbox::new();
    if dir_link_to_nowhere(&sb, "src/probe").is_none() {
        return;
    }
    let origin = sb.path("src/filelink");
    symlink_file("nowhere", &origin).unwrap();
    let copy = sb.path("src/copy");
    LocalFs
        .create_symlink(Path::new("nowhere"), &copy, Some(&origin))
        .unwrap();
    assert!(!is_dir_link(&copy));
}

#[test]
fn without_an_origin_the_target_decides_and_a_missing_one_is_a_file() {
    let sb = Sandbox::new();
    if dir_link_to_nowhere(&sb, "src/probe").is_none() {
        return;
    }
    sb.mkdir("src/real");
    LocalFs
        .create_symlink(Path::new("real"), &sb.path("src/to_real"), None)
        .unwrap();
    LocalFs
        .create_symlink(Path::new("nowhere"), &sb.path("src/to_nowhere"), None)
        .unwrap();
    assert!(is_dir_link(&sb.path("src/to_real")));
    assert!(!is_dir_link(&sb.path("src/to_nowhere")));
}

#[test]
fn copying_and_moving_a_folder_link_with_no_target_keeps_its_kind() {
    let sb = Sandbox::new();
    let Some(origin) = dir_link_to_nowhere(&sb, "src/dirlink") else {
        return;
    };
    let e = sb.engine();
    let dest = sb.mkdir("dest");
    let (_, copied) = do_copy(
        &e,
        std::slice::from_ref(&origin),
        &dest,
        ConflictPolicy::Skip,
    );
    assert!(copied.failed.is_empty(), "{:?}", copied.failed);
    assert!(is_dir_link(&dest.join("dirlink")));

    let other = sb.mkdir("other");
    let (_, moved) = do_move(&e, &[dest.join("dirlink")], &other, ConflictPolicy::Skip);
    assert!(moved.failed.is_empty(), "{:?}", moved.failed);
    assert!(is_dir_link(&other.join("dirlink")));
}
