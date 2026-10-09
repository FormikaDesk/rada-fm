//! Archives in the interface: opened like folders, previewed, read-only, extracted and made.

mod common;

use std::time::{Duration, Instant};

use common::*;
use crossterm::event::KeyCode;
use rada_core::archive::testkit::{Member, write_tar, write_zip};
use rada_core::archive::{ArchiveLimits, Compression};
use rada_core::preview::Preview;
use rada_tui::app::{InputKind, Modal};

fn members() -> Vec<Member> {
    vec![
        Member::dir("docs"),
        Member::file("docs/readme.txt", "hello from inside\n"),
        Member::file("docs/sub/deep.txt", "deep"),
        Member::file("top.txt", "top"),
    ]
}

fn in_archive(h: &mut H, tail: &str) {
    let t = tail.to_string();
    h.wait(&format!("inside {tail}"), move |a| {
        a.archive.is_some() && !a.is_loading() && a.cwd.to_string_lossy().ends_with(&t)
    });
}

fn at(h: &mut H, tail: &str) {
    let t = tail.to_string();
    h.wait(&format!("in {tail}"), move |a| {
        a.archive.is_none() && !a.is_loading() && a.cwd.to_string_lossy().ends_with(&t)
    });
}

fn put_cursor(h: &mut H, name: &str) {
    h.key(KeyCode::Home);
    for _ in 0..h.app.visible.len() {
        if h.cursor_name() == name {
            return;
        }
        h.key(KeyCode::Down);
    }
    assert_eq!(h.cursor_name(), name, "{}", h.screen());
}

#[test]
fn enter_opens_an_archive_as_a_folder_and_the_breadcrumb_says_so() {
    let (sb, dir) = sandbox_with_files();
    write_zip(&dir.join("photos.zip"), &members());
    let mut h = H::new(sb, dir.clone(), 130, 30);
    put_cursor(&mut h, "photos.zip");
    h.key(KeyCode::Enter);
    in_archive(&mut h, "photos.zip");
    assert_eq!(h.names(), ["docs", "top.txt"], "{}", h.screen());
    // Into a folder of it.
    h.key(KeyCode::Enter);
    in_archive(&mut h, "photos.zip/docs");
    assert_eq!(h.names(), ["sub", "readme.txt"]);
    let screen = h.screen();
    assert!(screen.contains("photos.zip › docs"), "{screen}");
    // Up twice: back in the real folder with the cursor on the archive.
    h.key(KeyCode::Backspace);
    in_archive(&mut h, "photos.zip");
    h.key(KeyCode::Backspace);
    at(&mut h, "proj.v1");
    assert_eq!(h.cursor_name(), "photos.zip");
    // The archive's folders were never recorded as places to come back to.
    assert!(
        h.app.paths.recents.iter().all(|p| !p.to_string_lossy().contains("photos.zip")),
        "{:?}",
        h.app.paths.recents
    );
}

#[test]
fn a_file_whose_content_is_an_archive_is_recognised_by_its_preview() {
    let (sb, dir) = sandbox_with_files();
    write_tar(&dir.join("mystery.dat"), &members(), Compression::Xz);
    write_zip(&dir.join("letter.docx"), &[Member::file("word/document.xml", "<x/>")]);
    let mut h = H::new(sb, dir, 130, 30);
    put_cursor(&mut h, "mystery.dat");
    h.wait("the archive preview", |a| matches!(a.preview.content, Some(Preview::Archive(_))));
    let screen = h.screen();
    assert!(screen.contains("tar archive, xz") && screen.contains("3 files"), "{screen}");
    h.key(KeyCode::Enter);
    in_archive(&mut h, "mystery.dat");
    // A document that is a ZIP inside is not turned into a folder by Enter.
    h.key(KeyCode::Backspace);
    at(&mut h, "proj.v1");
    put_cursor(&mut h, "letter.docx");
    h.wait("its preview", |a| a.preview.content.is_some());
    h.key(KeyCode::Enter);
    h.pump(150);
    assert!(h.app.archive.is_none(), "opened with its program, not as a folder");
}

#[test]
fn members_are_previewed_inside_the_archive() {
    let (sb, dir) = sandbox_with_files();
    write_zip(&dir.join("a.zip"), &members());
    let mut h = H::new(sb, dir, 130, 30);
    put_cursor(&mut h, "a.zip");
    h.key(KeyCode::Enter);
    in_archive(&mut h, "a.zip");
    put_cursor(&mut h, "top.txt");
    h.wait("text of a member", |a| matches!(&a.preview.content, Some(Preview::Text(t)) if t.lines == ["top"]));
}

#[test]
fn an_archive_is_read_only_in_every_way_and_says_how_to_get_things_out() {
    let (sb, dir) = sandbox_with_files();
    write_zip(&dir.join("a.zip"), &members());
    let mut h = H::new(sb, dir, 130, 30);
    put_cursor(&mut h, "a.zip");
    h.key(KeyCode::Enter);
    in_archive(&mut h, "a.zip");
    for k in ["d", "D", "r", "R", "n", "x"] {
        h.keys(k);
        assert!(h.app.modal.is_none(), "{k}: no window may open\n{}", h.screen());
        let t = h.app.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
        assert!(t.contains("read-only"), "{k}: {t}");
    }
    // Pasting into it is refused too, even with something copied.
    h.keys("y");
    h.keys("p");
    assert!(h.app.modal.is_none());
    // Enter on a file explains instead of trying to open it.
    put_cursor(&mut h, "top.txt");
    h.key(KeyCode::Enter);
    assert!(h.app.toast.as_ref().unwrap().text.contains("inside an archive"));
}

#[test]
fn copying_members_out_is_a_partial_extraction_with_a_plan_and_undo() {
    let (sb, dir) = sandbox_with_files();
    write_zip(&dir.join("a.zip"), &members());
    let mut h = H::new(sb, dir.clone(), 130, 36);
    put_cursor(&mut h, "a.zip");
    h.key(KeyCode::Enter);
    in_archive(&mut h, "a.zip");
    put_cursor(&mut h, "top.txt");
    h.press("ctrl+c");
    h.key(KeyCode::Backspace);
    at(&mut h, "proj.v1");
    put_cursor(&mut h, "sub");
    h.key(KeyCode::Enter);
    at(&mut h, "sub");
    h.press("ctrl+v");
    plan_modal(&mut h);
    let screen = h.screen();
    assert!(screen.contains("Extract") && screen.contains("top.txt"), "{screen}");
    run_plan_and_wait(&mut h);
    assert_eq!(std::fs::read_to_string(dir.join("sub/top.txt")).unwrap(), "top");
    h.keys("u");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(!dir.join("sub/top.txt").exists());
}

#[test]
fn extract_here_names_the_folder_after_the_archive_and_not_after_the_path() {
    // The folder above has a dot in its name: the regression from another file manager.
    let (sb, dir) = sandbox_with_files();
    write_tar(
        &dir.join("pack.tar.gz"),
        &[Member::file("one.txt", "1"), Member::file("two.txt", "2")],
        Compression::Gzip,
    );
    let mut h = H::new(sb, dir.clone(), 130, 36);
    put_cursor(&mut h, "pack.tar.gz");
    h.keys("E"); // into a folder: the name is offered
    assert!(matches!(&h.app.modal, Some(Modal::Input(iv)) if matches!(iv.kind, InputKind::ExtractTo { .. }) && iv.text == "pack"));
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert_eq!(std::fs::read_to_string(dir.join("pack/one.txt")).unwrap(), "1");
    assert!(!dir.join("one.txt").exists());
    // Extract here puts the contents right in the folder.
    put_cursor(&mut h, "pack.tar.gz");
    h.keys("e");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert_eq!(std::fs::read_to_string(dir.join("two.txt")).unwrap(), "2");
}

#[test]
fn a_possible_bomb_needs_yes_typed_and_nothing_else_will_do() {
    let (sb, dir) = sandbox_with_files();
    write_tar(&dir.join("big.tar.gz"), &[Member::file("zeros", vec![0u8; 3 << 20])], Compression::Gzip);
    let limits = ArchiveLimits {
        max_total_bytes: 1 << 20,
        max_ratio: 50,
        ratio_floor_bytes: 1 << 10,
        max_entries: 1000,
    };
    let mut h = H::with_archive_limits(sb, dir.clone(), 130, 36, limits);
    put_cursor(&mut h, "big.tar.gz");
    h.keys("e");
    plan_modal(&mut h);
    let screen = h.screen();
    assert!(screen.contains("bomb"), "{screen}");
    h.key(KeyCode::Enter); // not enough
    assert!(h.modal_is_plan() && h.app.running.is_none());
    assert!(!dir.join("zeros").exists());
    h.keys("yes");
    run_plan_and_wait(&mut h);
    assert_eq!(std::fs::metadata(dir.join("zeros")).unwrap().len(), 3 << 20);
}

#[test]
fn compress_asks_for_a_name_and_a_format_and_makes_the_archive() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 130, 36);
    put_cursor(&mut h, "a.txt");
    h.key(KeyCode::Char(' '));
    h.key(KeyCode::Char(' ')); // a.txt and b.txt
    h.keys("z");
    match &h.app.modal {
        Some(Modal::Input(iv)) => assert_eq!(iv.text, "proj.v1.zip"),
        _ => panic!("{}", h.screen()),
    }
    h.key(KeyCode::Tab);
    match &h.app.modal {
        Some(Modal::Input(iv)) => assert_eq!(iv.text, "proj.v1.tar.gz"),
        _ => panic!(),
    }
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    let screen = h.screen();
    assert!(screen.contains("Compress") && screen.contains("proj.v1.tar.gz"), "{screen}");
    run_plan_and_wait(&mut h);
    assert!(dir.join("proj.v1.tar.gz").is_file());
    h.keys("u");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(!dir.join("proj.v1.tar.gz").exists(), "undo removes the archive");
}

#[test]
fn a_broken_or_fake_archive_says_what_is_wrong() {
    let (sb, dir) = sandbox_with_files();
    std::fs::write(dir.join("fake.zip"), "this is not a zip").unwrap();
    let mut h = H::new(sb, dir, 130, 30);
    put_cursor(&mut h, "fake.zip");
    h.key(KeyCode::Enter);
    h.wait("the message", |a| a.toast.is_some() && !a.is_loading());
    let t = h.app.toast.as_ref().unwrap().text.clone();
    assert!(t.contains("not an archive"), "{t}");
    assert!(h.app.archive.is_none());
    h.keys("e");
    h.wait("the message", |a| a.modal.is_some() || a.toast.is_some());
    let s = h.screen();
    assert!(s.contains("not an archive"), "{s}");
}

#[test]
fn having_a_big_archive_under_the_cursor_does_not_slow_navigation() {
    let (sb, dir) = sandbox_with_files();
    // A tar.gz with many members: counting them takes a while in the preview worker.
    let many: Vec<Member> = (0..30_000)
        .map(|i| Member::file(&format!("d{}/f{i:05}", i % 50), "x".repeat(300)))
        .collect();
    write_tar(&dir.join("0-huge.tar.gz"), &many, Compression::Gzip);
    let mut h = H::new(sb, dir, 130, 30);
    // Keys and drawing, with the archive selected and its preview being worked out.
    h.app.tick();
    let mut worst = Duration::ZERO;
    let end = Instant::now() + Duration::from_millis(700);
    while Instant::now() < end {
        let t = Instant::now();
        h.key(KeyCode::Down);
        let _ = h.screen();
        h.key(KeyCode::Up);
        let _ = h.screen();
        worst = worst.max(t.elapsed());
        // Deliver whatever the workers have said, as the real loop does.
        while let Ok(ev) = h.app.events().try_recv() {
            h.app.on_core_event(ev);
        }
    }
    assert!(worst < Duration::from_millis(100), "a keypress took {worst:?}");
}
