//! Looking into archives without extracting them: folders, previews of the archive and of its
//! members.

use std::path::Path;

use rada_core::archive::testkit::{Member, write_tar, write_zip};
use rada_core::archive::{self, Compression};
use rada_core::fs::LocalFs;
use rada_core::preview::{self, Limits, Preview};
use rada_core::testutil::*;

fn png_bytes() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(8, 6, |x, y| {
        image::Rgba([x as u8 * 30, y as u8 * 40, 7, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn members() -> Vec<Member> {
    vec![
        Member::dir("docs"),
        Member::file("docs/readme.txt", "hello from inside\nsecond line\n"),
        Member::file("docs/sub/deep.txt", "deep"),
        Member::file("pic.png", png_bytes()),
        Member::symlink("alias", "docs/readme.txt"),
        Member::file(
            "data.bin",
            (0..2000u32).map(|i| (i * 31) as u8).collect::<Vec<_>>(),
        ),
    ]
}

fn limits(sb: &Sandbox) -> Limits {
    let mut l = Limits::default();
    l.image.pdf_cache = Some(sb.root.join("cache"));
    l
}

#[test]
fn a_path_through_a_file_is_found_in_the_archive_and_an_ordinary_one_is_not() {
    let sb = Sandbox::new();
    let a = sb.path("a.zip");
    write_zip(&a, &members());
    let fs = LocalFs;
    let loc = archive::locate(&fs, &a.join("docs/sub")).unwrap();
    assert_eq!(loc.archive, a);
    assert_eq!(loc.inner, Path::new("docs/sub"));
    assert_eq!(archive::locate(&fs, &a).unwrap().inner, Path::new(""));
    assert!(archive::locate(&fs, &sb.path("")).is_none());
    assert!(archive::locate(&fs, &sb.path("nothing/here")).is_none());
    assert!(!archive::is_inside(&fs, &a));
    assert!(archive::is_inside(&fs, &a.join("docs")));
}

#[test]
fn folders_of_an_archive_list_like_folders() {
    let sb = Sandbox::new();
    let p = sb.platform();
    for (name, c) in [("a.tar.gz", Some(Compression::Gzip)), ("a.zip", None)] {
        let a = sb.path(name);
        match c {
            Some(c) => write_tar(&a, &members(), c),
            None => write_zip(&a, &members()),
        }
        let loc = archive::locate(&LocalFs, &a).unwrap();
        let (top, view) = archive::browse::read_dir(&*p, &loc, &|| false).unwrap();
        let names: Vec<String> = top.iter().map(|e| e.display.clone()).collect();
        assert_eq!(names.len(), 4, "{name}: {names:?}");
        assert!(names.contains(&"docs".to_string()) && names.contains(&"alias".to_string()));
        assert_eq!(view.files, 4, "{name}");
        assert!(
            view.format
                .contains(if name.ends_with(".zip") { "ZIP" } else { "tar" }),
            "{}",
            view.format
        );
        let docs = top.iter().find(|e| e.display == "docs").unwrap();
        assert!(docs.is_dir());
        assert_eq!(docs.path, a.join("docs"));
        assert!(docs.readonly);
        let link = top.iter().find(|e| e.display == "alias").unwrap();
        assert_eq!(
            link.link.as_ref().unwrap().target,
            Path::new("docs/readme.txt")
        );
        // Inside.
        let inner = archive::locate(&LocalFs, &a.join("docs")).unwrap();
        let (kids, _) = archive::browse::read_dir(&*p, &inner, &|| false).unwrap();
        let kn: Vec<String> = kids.iter().map(|e| e.display.clone()).collect();
        assert_eq!(kn, ["readme.txt", "sub"], "{name}"); // the interface sorts
        // A file is not a folder; a missing path says so.
        let bad = archive::locate(&LocalFs, &a.join("docs/readme.txt")).unwrap();
        assert!(archive::browse::read_dir(&*p, &bad, &|| false).is_err());
        let gone = archive::locate(&LocalFs, &a.join("nope")).unwrap();
        assert!(archive::browse::read_dir(&*p, &gone, &|| false).is_err());
    }
}

#[test]
fn an_unopened_archive_shows_its_counts_and_top_items() {
    let sb = Sandbox::new();
    let a = sb.path("whatever.dat"); // the name says nothing: the content does
    write_zip(&a, &members());
    match preview::generate(&LocalFs, &a, &limits(&sb)) {
        Preview::Archive(p) => {
            assert!(p.problem.is_none(), "{:?}", p.problem);
            assert_eq!(p.files, 4);
            assert!(p.bytes > 2000);
            assert!(p.complete);
            assert!(p.packed > 0);
            assert_eq!(
                p.first[0],
                ("docs".to_string(), true),
                "folders first: {:?}",
                p.first
            );
            assert!(p.first.iter().any(|(n, _)| n == "pic.png"));
        }
        other => panic!("{other:?}"),
    }
    // A damaged one says so in the preview instead of showing nothing.
    let cut = sb.path("cut.zip");
    let mut b = rada_core::archive::testkit::zip_bytes(&members());
    b.truncate(b.len() - 30);
    std::fs::write(&cut, b).unwrap();
    match preview::generate(&LocalFs, &cut, &limits(&sb)) {
        Preview::Archive(p) => assert!(
            p.problem.as_deref().unwrap_or("").contains("damaged"),
            "{:?}",
            p.problem
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_and_pictures_inside_an_archive_are_previewed() {
    let sb = Sandbox::new();
    let l = limits(&sb);
    for (name, c) in [("a.tar.xz", Some(Compression::Xz)), ("a.zip", None)] {
        let a = sb.path(name);
        match c {
            Some(c) => write_tar(&a, &members(), c),
            None => write_zip(&a, &members()),
        }
        match preview::generate(&LocalFs, &a.join("docs/readme.txt"), &l) {
            Preview::Text(t) => assert_eq!(t.lines, ["hello from inside", "second line"], "{name}"),
            other => panic!("{name}: {other:?}"),
        }
        match preview::generate(&LocalFs, &a.join("pic.png"), &l) {
            Preview::Image(i) => {
                assert_eq!(i.info.format, "PNG", "{name}");
                assert_eq!((i.info.width, i.info.height), (Some(8), Some(6)));
                let src = i.source.expect("decoded from the cache");
                assert!(src.starts_with(sb.root.join("cache")), "{src:?}");
            }
            other => panic!("{name}: {other:?}"),
        }
        assert!(
            matches!(
                preview::generate(&LocalFs, &a.join("data.bin"), &l),
                Preview::Binary(_)
            ),
            "{name}"
        );
        match preview::generate(&LocalFs, &a.join("docs"), &l) {
            Preview::Dir(d) => assert_eq!(
                d.entries,
                [("sub".to_string(), true), ("readme.txt".to_string(), false)]
            ),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            preview::generate(&LocalFs, &a.join("alias"), &l),
            Preview::Symlink { .. }
        ));
        assert!(
            matches!(
                preview::generate(&LocalFs, &a.join("nope"), &l),
                Preview::Error(_)
            ),
            "{name}"
        );
    }
}

#[test]
fn a_cancelled_preview_stops_reading() {
    let sb = Sandbox::new();
    let a = sb.path("big.tar.gz");
    let big: Vec<Member> = (0..2000)
        .map(|i| Member::file(&format!("f{i:04}"), "x".repeat(4000)))
        .collect();
    write_tar(&a, &big, Compression::Gzip);
    let started = std::time::Instant::now();
    let p = preview::generate_with(&LocalFs, &a, &limits(&sb), &|| true);
    assert!(matches!(p, Preview::Empty), "{p:?}");
    assert!(started.elapsed().as_secs() < 2);
}

#[test]
fn a_big_stream_gives_a_lower_bound_instead_of_blocking_the_preview() {
    let sb = Sandbox::new();
    let a = sb.path("big.tar.gz");
    let big: Vec<Member> = (0..3000)
        .map(|i| Member::file(&format!("f{i:04}"), "x".repeat(2000)))
        .collect();
    write_tar(&a, &big, Compression::Gzip);
    let mut l = limits(&sb);
    l.archive_seconds = 0.0; // no time at all
    match preview::generate(&LocalFs, &a, &l) {
        Preview::Archive(p) => {
            assert!(!p.complete, "{p:?}");
            assert!(p.note.is_some());
            assert!(p.files < 3000);
        }
        other => panic!("{other:?}"),
    }
}
