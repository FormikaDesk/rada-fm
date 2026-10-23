//! The "Recent" entry of the sidebar: a read-only list of the files used lately, newest first,
//! from the system's own list (here the sandbox's `recently-used.xbel`, and a stand-in for
//! Spotlight). No list, or one that fails, means no entry and no error.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use common::*;
use rada_core::fs::LocalFs;
use rada_core::testplatform::{MacLike, MacLikeSpec};
use rada_core::testutil::*;
use rada_tui::hits::Target;

fn at(day: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_760_000_000 + day * 86_400)
}

fn go_recent(h: &mut H) {
    h.click(&Target::Place(rada_core::recents::view_path()));
    h.wait("the list", |a| a.recents_view() && !a.is_loading());
}

fn mac_with(spec: MacLikeSpec, sb: Sandbox, start: PathBuf) -> (H, Arc<MacLike>) {
    let p = Arc::new(MacLike::new(sb.platform(), spec));
    let h = H::with_system(sb, start, 150, 34, (Arc::new(LocalFs), p.clone()), |_| {});
    (h, p)
}

#[test]
fn recents_newest_first_read_only() {
    let sb = Sandbox::new();
    let a = sb.write("docs/old.txt", "o");
    let b = sb.write("work/new.txt", "n");
    let c = sb.write("work/mid.txt", "m");
    let dir = sb.path("docs");
    let (a2, b2, c2) = (a.clone(), b.clone(), c.clone());
    let (mut h, mac) = mac_with(
        MacLikeSpec {
            recents: Some(vec![
                (a2, at(1)),
                (b2, at(9)),
                (c2, at(5)),
                (sb.path("docs/gone.txt"), at(20)),
            ]),
            ..Default::default()
        },
        sb,
        dir,
    );
    h.wait("the entry", |a| a.paths.recent_files);
    let s = h.screen();
    assert!(s.contains("Recent"), "the sidebar has the entry: {s}");
    go_recent(&mut h);
    // The newest first, the missing file absent, the column says "Last used".
    assert_eq!(h.names(), ["new.txt", "mid.txt", "old.txt"]);
    let s = h.screen();
    assert!(s.contains("Last used"), "{s}");
    assert_eq!(h.app.tab_title(0), "Recent");
    // Nothing is made in it.
    h.keys("N");
    h.press("ctrl+shift+n");
    assert!(h.app.recents_view(), "still the list");
    // Enter opens the file.
    h.press("enter");
    h.pump(50);
    assert!(
        mac.calls().iter().any(|o| o.ends_with("new.txt")),
        "{:?}",
        mac.calls()
    );
    // "Show in folder" goes to the folder, the cursor on the file.
    h.press("down");
    h.press("alt+f");
    let work = b.parent().unwrap().to_path_buf();
    h.wait("the folder", move |a| a.cwd == work && !a.is_loading());
    assert_eq!(h.cursor_name(), "mid.txt");
    // Leaving the list gives the folders their own order back.
    assert_eq!(h.app.sort.key, rada_core::model::SortKey::Name);
}

#[test]
fn without_a_source_there_is_no_entry_and_no_error() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let dir = sb.path("d");
    let (mut h, _) = mac_with(MacLikeSpec::default(), sb, dir);
    h.pump(200);
    assert!(!h.app.paths.recent_files);
    assert!(!h.screen().contains("Recent"));
}

#[cfg(target_os = "linux")]
#[test]
fn the_linux_list_is_the_recently_used_file_of_the_data_folder() {
    use std::path::Path;
    let sb = Sandbox::new();
    let a = sb.write("docs/a.txt", "a");
    let b = sb.write("docs/b.txt", "b");
    let dir = sb.path("docs");
    let line = |p: &Path, when: &str| {
        format!(
            "<bookmark href=\"file://{}\" added=\"{when}\" modified=\"{when}\" visited=\"{when}\"/>\n",
            p.display()
        )
    };
    std::fs::create_dir_all(&sb.dirs.data).unwrap();
    std::fs::write(
        sb.dirs.data.join("recently-used.xbel"),
        format!(
            "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\">\n{}{}</xbel>\n",
            line(&a, "2026-10-01T10:00:00Z"),
            line(&b, "2026-10-02T10:00:00Z")
        ),
    )
    .unwrap();
    let mut h = H::new(sb, dir, 150, 34);
    h.wait("the entry", |a| a.paths.recent_files);
    go_recent(&mut h);
    assert_eq!(h.names(), ["b.txt", "a.txt"]);
}

#[cfg(target_os = "linux")]
#[test]
fn a_missing_recently_used_file_leaves_the_sidebar_as_it_was() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let dir = sb.path("d");
    let mut h = H::new(sb, dir, 150, 34);
    h.pump(200);
    assert!(!h.app.paths.recent_files);
    assert!(!h.screen().contains("Recent"));
}
