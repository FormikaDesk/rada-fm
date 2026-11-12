//! Search by name under a folder: what it finds, the three ways of asking, what it leaves
//! alone, where it stops and what it says then.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rada_core::archive::ArchiveKind;
use rada_core::fs::{FsEngine, LocalFs};
use rada_core::ops::*;
use rada_core::search::{self, SearchLimits, SearchSource};
use rada_core::testutil::*;
use rada_core::views::{self, Collected, Limit, ViewCx, ViewEnd, ViewSink, ViewSource};

/// What a search handed over, and how it ended.
struct Found {
    end: Result<ViewEnd, String>,
    paths: Vec<PathBuf>,
    batches: usize,
    problems: Vec<PathBuf>,
    visited: usize,
}

/// A search with everything that can be chosen about it.
struct Ask<'a> {
    fs: &'a dyn FsEngine,
    hidden: bool,
    limits: SearchLimits,
    cancel: &'a dyn Fn() -> bool,
}

impl Default for Ask<'_> {
    fn default() -> Self {
        Ask {
            fs: &LocalFs,
            hidden: false,
            limits: SearchLimits::default(),
            cancel: &|| false,
        }
    }
}

fn find_with(sb: &Sandbox, root: &str, query: &str, ask: Ask<'_>) -> Found {
    let platform = sb.platform();
    let cx = ViewCx {
        fs: ask.fs,
        platform: platform.as_ref(),
        show_hidden: ask.hidden,
    };
    let source = SearchSource {
        root: sb.path(root),
        query: query.into(),
        limits: ask.limits,
    };
    let mut sink = Collected::new(ask.cancel);
    let end = source.run(&cx, &mut sink);
    let mut paths: Vec<PathBuf> = sink.entries.iter().map(|e| e.path.clone()).collect();
    paths.sort();
    Found {
        end,
        paths,
        batches: sink.batches,
        problems: sink.problems.into_iter().map(|p| p.path).collect(),
        visited: sink.progress.visited,
    }
}

fn find(sb: &Sandbox, root: &str, query: &str) -> Found {
    find_with(sb, root, query, Ask::default())
}

/// A search that ran to its end without meeting a bound.
fn complete(found: &Found) -> bool {
    found.end == Ok(ViewEnd::default())
}

fn limits_of(found: &Found) -> Vec<Limit> {
    found.end.clone().unwrap().limits
}

#[test]
fn it_finds_by_every_word_in_folders_at_any_depth_whatever_the_case() {
    let sb = Sandbox::new();
    sb.write("src/Main.rs", "m");
    sb.write("src/deep/er/main_test.rs", "t");
    sb.write("src/readme.md", "r");
    sb.write("other/main.c", "c");

    let found = find(&sb, "src", "RS main");

    assert_eq!(
        found.paths,
        [sb.path("src/Main.rs"), sb.path("src/deep/er/main_test.rs")]
    );
    assert!(complete(&found));
    assert!(found.problems.is_empty());
}

#[test]
fn a_pattern_with_a_star_fits_the_whole_name() {
    let sb = Sandbox::new();
    sb.write("d/a.pdf", "");
    sb.write("d/sub/B.PDF", "");
    sb.write("d/sub/c.pdf.bak", "");
    sb.write("d/pdf-notes.txt", "");

    let found = find(&sb, "d", "*.pdf");

    assert_eq!(found.paths, [sb.path("d/a.pdf"), sb.path("d/sub/B.PDF")]);
}

#[test]
fn a_text_between_slashes_is_a_regular_expression() {
    let sb = Sandbox::new();
    sb.write("d/img_001.jpg", "");
    sb.write("d/sub/IMG_17.jpg", "");
    sb.write("d/sub/img_x.jpg", "");

    let found = find(&sb, "d", r"/^img_\d+\.jpg$/");

    assert_eq!(
        found.paths,
        [sb.path("d/img_001.jpg"), sb.path("d/sub/IMG_17.jpg")]
    );
    let bad = find(&sb, "d", "/(unclosed/");
    assert!(bad.end.is_err() && bad.paths.is_empty());
}

#[test]
fn a_folder_matches_too_and_is_searched_inside() {
    let sb = Sandbox::new();
    sb.write("a/photos/x/photo1.jpg", "p");

    let found = find(&sb, "a", "photo");

    assert_eq!(
        found.paths,
        [sb.path("a/photos"), sb.path("a/photos/x/photo1.jpg")]
    );
}

#[test]
fn two_files_with_one_name_are_two_rows() {
    let sb = Sandbox::new();
    sb.write("p/a/index.js", "1");
    sb.write("p/b/index.js", "2");
    let platform = sb.platform();
    let cx = ViewCx {
        fs: &LocalFs,
        platform: platform.as_ref(),
        show_hidden: false,
    };
    let source = views::source(&rada_core::ViewKind::Search {
        root: sb.path("p"),
        query: "index".into(),
    });
    let never = || false;
    let mut sink = Collected::new(&never);

    source.run(&cx, &mut sink).unwrap();

    let names: Vec<_> = sink.entries.iter().map(|e| e.name.clone()).collect();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
    assert!(sink.entries.iter().all(|e| e.display == "index.js"));
}

#[test]
fn hidden_items_are_neither_listed_nor_entered_unless_they_are_shown() {
    let sb = Sandbox::new();
    sb.write("h/seen/note.txt", "");
    sb.write("h/.cache/note.txt", "");
    sb.write("h/.note.txt", "");
    sb.hide("h/.cache");
    sb.hide("h/.note.txt");

    let without = find(&sb, "h", "note");
    let with = find_with(
        &sb,
        "h",
        "note",
        Ask {
            hidden: true,
            ..Ask::default()
        },
    );

    assert_eq!(without.paths, [sb.path("h/seen/note.txt")]);
    assert_eq!(with.paths.len(), 3, "{:?}", with.paths);
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_is_listed_but_not_entered() {
    let sb = Sandbox::new();
    sb.write("elsewhere/secret-target.txt", "s");
    sb.mkdir("root");
    sb.symlink(sb.path("elsewhere"), "root/secret-link");

    let found = find(&sb, "root", "secret");

    assert_eq!(found.paths, [sb.path("root/secret-link")]);
}

#[cfg(unix)]
#[test]
fn a_link_that_makes_a_loop_does_not_keep_the_search_going() {
    let sb = Sandbox::new();
    sb.write("loop/a/b/file.txt", "");
    sb.symlink(sb.path("loop"), "loop/a/b/up");

    let found = find(&sb, "loop", "file up");
    let all = find(&sb, "loop", "*");

    assert!(found.paths.is_empty());
    assert!(complete(&all));
    assert_eq!(
        all.paths.len(),
        4,
        "a, b, file.txt and the link: {:?}",
        all.paths
    );
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_read_is_told_and_the_rest_is_found() {
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.write("r/open/note.txt", "n");
    sb.write("r/shut/note.txt", "n");
    chmod(&sb.path("r/shut"), 0o000);

    let found = find(&sb, "r", "note");

    chmod(&sb.path("r/shut"), 0o755);
    assert_eq!(found.paths, [sb.path("r/open/note.txt")]);
    assert_eq!(found.problems, [sb.path("r/shut")]);
    assert!(complete(&found), "it is not a bound, and not an error");
}

#[test]
fn an_archive_is_a_file_and_is_not_looked_into() {
    let sb = Sandbox::new();
    sb.write("z/inside/needle.txt", "n");
    let e = sb.engine();
    let req = OpRequest::Compress {
        sources: vec![sb.path("z/inside")],
        archive: sb.path("z/box.zip"),
        format: ArchiveKind::Zip,
        conflict: ConflictPolicy::Skip,
    };
    let cancel = Cancel::new();
    let ctl = ScanControl {
        cancel: cancel.flag(),
        progress: &mut |_| {},
    };
    let plan = e.plan_request(&req, None, ctl).unwrap().plan;
    assert!(run(&e, &plan).failed.is_empty());
    std::fs::remove_dir_all(sb.path("z/inside")).unwrap();

    assert!(find(&sb, "z", "needle").paths.is_empty());
    assert_eq!(find(&sb, "z", "box").paths, [sb.path("z/box.zip")]);
}

#[test]
fn a_listing_that_does_not_say_the_filesystem_does_not_stop_the_descent() {
    // As on Windows, where only the folder itself can be asked.
    let sb = Sandbox::new();
    sb.write("w/a/b/hit.txt", "");
    let ffs = FaultFs::new();
    ffs.hide_identity();

    let found = find_with(
        &sb,
        "w",
        "hit",
        Ask {
            fs: ffs.as_ref(),
            ..Ask::default()
        },
    );

    assert_eq!(found.paths, [sb.path("w/a/b/hit.txt")]);
}

#[test]
fn a_file_is_not_a_place_to_search() {
    let sb = Sandbox::new();
    sb.write("f.txt", "x");
    assert!(find(&sb, "f.txt", "f").end.is_err());
    assert!(find(&sb, "missing", "f").end.is_err());
}

fn many(sb: &Sandbox, dir: &str, n: usize) {
    for i in 0..n {
        sb.write(format!("{dir}/hit-{i:04}.txt"), "");
    }
}

#[test]
fn it_stops_at_the_most_matches_and_says_so() {
    let sb = Sandbox::new();
    many(&sb, "many", 30);
    let limits = SearchLimits {
        results: 10,
        ..SearchLimits::default()
    };

    let found = find_with(
        &sb,
        "many",
        "hit",
        Ask {
            limits,
            ..Ask::default()
        },
    );

    assert_eq!(found.paths.len(), 10);
    assert_eq!(limits_of(&found), [Limit::Results(10)]);
}

#[test]
fn it_stops_at_the_most_entries_looked_at_and_says_so() {
    let sb = Sandbox::new();
    many(&sb, "many", 600);
    let limits = SearchLimits {
        visited: 256,
        ..SearchLimits::default()
    };

    let found = find_with(
        &sb,
        "many",
        "hit",
        Ask {
            limits,
            ..Ask::default()
        },
    );

    assert_eq!(limits_of(&found), [Limit::Visited(256)]);
    assert_eq!(found.visited, 256);
    assert!(found.paths.len() < 600 && !found.paths.is_empty());
}

#[test]
fn it_stops_when_its_time_is_over_and_says_so() {
    let sb = Sandbox::new();
    many(&sb, "many", 600);
    let limits = SearchLimits {
        time: Duration::ZERO,
        ..SearchLimits::default()
    };

    let found = find_with(
        &sb,
        "many",
        "hit",
        Ask {
            limits,
            ..Ask::default()
        },
    );

    assert_eq!(limits_of(&found), [Limit::Time(Duration::ZERO)]);
    assert!(found.paths.len() < 600);
}

#[test]
fn folders_nested_too_deep_are_not_entered_and_it_says_so() {
    let sb = Sandbox::new();
    sb.write("d/one/hit.txt", "");
    sb.write("d/one/two/hit.txt", "");
    sb.write("d/one/two/three/hit.txt", "");
    let limits = SearchLimits {
        depth: 2,
        ..SearchLimits::default()
    };

    let found = find_with(
        &sb,
        "d",
        "hit",
        Ask {
            limits,
            ..Ask::default()
        },
    );

    assert_eq!(
        found.paths,
        [sb.path("d/one/hit.txt"), sb.path("d/one/two/hit.txt")]
    );
    assert_eq!(limits_of(&found), [Limit::Depth(2)]);
}

#[test]
fn asked_to_stop_it_keeps_what_it_found() {
    let sb = Sandbox::new();
    many(&sb, "many", 600);

    let found = find_with(
        &sb,
        "many",
        "hit",
        Ask {
            cancel: &|| true,
            ..Ask::default()
        },
    );

    let end = found.end.unwrap();
    assert!(end.cancelled && end.limits.is_empty());
    assert!(!found.paths.is_empty() && found.paths.len() < 600);
}

#[test]
fn matches_arrive_in_batches_and_none_is_lost() {
    let sb = Sandbox::new();
    many(&sb, "many", 450);

    let found = find(&sb, "many", "hit");

    assert_eq!(found.paths.len(), 450);
    assert!(found.batches >= 3, "{} batches", found.batches);
    assert!(complete(&found));
}

/// A sink that counts what it is told, to see that progress is said while the search runs.
struct Counting {
    progress: usize,
    last_visited: usize,
}

impl ViewSink for Counting {
    fn batch(&mut self, _entries: Vec<rada_core::model::Entry>) {}
    fn problem(&mut self, _problem: views::Problem) {}
    fn progress(&mut self, progress: views::ViewProgress) {
        assert!(progress.visited >= self.last_visited);
        self.last_visited = progress.visited;
        self.progress += 1;
    }
    fn cancelled(&self) -> bool {
        false
    }
}

#[test]
fn it_says_how_far_it_is_while_it_runs() {
    let sb = Sandbox::new();
    many(&sb, "many", 600);
    let platform = sb.platform();
    let cx = ViewCx {
        fs: &LocalFs,
        platform: platform.as_ref(),
        show_hidden: false,
    };
    let source = SearchSource {
        root: sb.path("many"),
        query: "nothing-called-so".into(),
        limits: SearchLimits::default(),
    };
    let mut sink = Counting {
        progress: 0,
        last_visited: 0,
    };

    source.run(&cx, &mut sink).unwrap();

    assert!(sink.progress >= 3, "told {} times", sink.progress);
    assert_eq!(sink.last_visited, 600);
}

#[test]
fn the_view_path_of_a_search_is_not_a_place_on_the_disk() {
    let p = search::view_path(Path::new("/home/u"), "a b").unwrap();
    assert!(!p.exists() && !p.is_absolute());
}

#[cfg(unix)]
#[test]
fn the_walk_for_a_tag_tells_the_folders_it_could_not_read_too() {
    use rada_core::tagsearch::{self, ScanLimits, TagQuery};
    if is_root() {
        return;
    }
    let sb = Sandbox::new();
    sb.write("t/open/a.txt", "");
    sb.write("t/shut/b.txt", "");
    let platform = sb.platform();
    let Some(store) = platform.tags() else {
        return;
    };
    chmod(&sb.path("t/shut"), 0o000);
    let query = TagQuery::new("Red");
    let walk = || {
        tagsearch::scan(
            store,
            &sb.path("t"),
            &query,
            &|| false,
            ScanLimits::default(),
        )
    };

    let (found, problems) = tagsearch::with_problems(walk);
    // Nobody asking: the same walk, and nothing is kept for the next one who asks.
    walk();
    let (_, none) = tagsearch::with_problems(|| ());

    chmod(&sb.path("t/shut"), 0o755);
    assert!(found.is_empty());
    let paths: Vec<_> = problems.into_iter().map(|p| p.path).collect();
    assert_eq!(paths, [sb.path("t/shut")]);
    assert!(none.is_empty());
}
