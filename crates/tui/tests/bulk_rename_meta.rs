//! The bulk rename window with a pattern that names files by what they say about themselves.
//! The preview is made in the background: it says how far the reading of the files is, marks
//! the items that lack the field with the reason, and Enter starts no plan until they are
//! taken out (`Ctrl+X`) or the pattern is changed.

mod common;

use std::time::{Duration, SystemTime};

use common::*;
use crossterm::event::KeyCode;
use rada_core::i18n::{Lang, with_lang};
use rada_core::testmedia::*;
use rada_core::testutil::{Sandbox, set_mtime};
use rada_tui::Theme;
use rada_tui::app::{InputKind, Modal};

/// 2030-01-15 12:00 UTC: the same day in every time zone.
fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_894_708_800)
}

/// `~/photos` with two photos, a picture without EXIF and a song, all of them marked.
fn scene() -> (H, std::path::PathBuf) {
    let sb = Sandbox::new();
    let dir = sb.dirs.home.join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let files: [(&str, Vec<u8>); 4] = [
        ("IMG_0001.jpg", jpeg(&[exif_segment()])),
        ("IMG_0002.jpg", jpeg(&[exif_segment()])),
        ("scan.jpg", jpeg(&[segment(0xE0, b"JFIF\0\x01\x01")])),
        (
            "song.mp3",
            mp3(&[("TPE1", "Lucio Dalla"), ("TIT2", "Caruso")]),
        ),
    ];
    for (n, bytes) in files {
        let p = dir.join(n);
        std::fs::write(&p, bytes).unwrap();
        set_mtime(&p, fixed_now());
    }
    let mut h = H::with_config(sb, dir.clone(), 100, 30, |c| c.theme = Theme::rada());
    h.wait("the places", |a| !a.places.paths.places.is_empty());
    h.wait("the volumes", |a| a.devices.volumes_seen);
    h.app.hooks.clock = Some(fixed_now());
    h.app.devices.volumes.clear();
    h.press("ctrl+a");
    (h, dir)
}

/// Open the bulk rename and type `pattern` in place of the one offered.
fn type_pattern(h: &mut H, pattern: &str) {
    h.keys("R");
    h.press("ctrl+u");
    h.keys(pattern);
}

fn wait_preview(h: &mut H) {
    h.wait("the preview of the names", |a| match &a.windows.modal {
        Some(Modal::Input(iv)) => match &iv.kind {
            InputKind::BulkRename { preview, .. } => preview.ready_for(&iv.text),
            _ => false,
        },
        _ => false,
    });
}

fn items(h: &H) -> usize {
    match &h.app.windows.modal {
        Some(Modal::Input(iv)) => match &iv.kind {
            InputKind::BulkRename { items, .. } => items.len(),
            _ => 0,
        },
        _ => 0,
    }
}

fn shot(h: &mut H) -> String {
    h.screen()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn the_window_says_it_is_reading_the_files_before_the_names_come() {
    let (mut h, _) = scene();
    type_pattern(&mut h, "{exif.date}_{n:2}.{ext}");
    // No event of the worker has been handled yet.
    assert!(
        h.screen().contains("Reading metadata… 0/4"),
        "{}",
        h.screen()
    );
    wait_preview(&mut h);
    assert!(!h.screen().contains("Reading metadata"));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAGS section, which the Linux snapshot has"
)]
fn items_without_the_field_are_marked_with_the_reason() {
    let (mut h, _) = scene();
    type_pattern(&mut h, "{exif.date}_{n:2}.{ext}");
    wait_preview(&mut h);
    insta::assert_snapshot!("bulk_rename_missing_100x30", shot(&mut h));
}

#[test]
#[cfg_attr(
    windows,
    ignore = "the navigation pane of a system with no tags has no TAGS section, which the Linux snapshot has"
)]
fn the_names_of_a_pattern_with_a_fallback() {
    let (mut h, _) = scene();
    type_pattern(&mut h, "{exif.date|mtime:%Y-%m-%d}_{n:2}.{ext}");
    wait_preview(&mut h);
    insta::assert_snapshot!("bulk_rename_fallback_100x30", shot(&mut h));
}

#[test]
fn enter_starts_no_plan_until_the_marked_items_are_taken_out() {
    let (mut h, dir) = scene();
    type_pattern(&mut h, "{exif.date}_{n:2}.{ext}");
    wait_preview(&mut h);
    h.key(KeyCode::Enter);
    h.pump(200);
    assert!(!h.modal_is_plan(), "{}", h.screen());
    assert!(h.screen().contains("Not renamable: 2"), "{}", h.screen());
    h.press("ctrl+x");
    assert_eq!(items(&h), 2);
    wait_preview(&mut h);
    assert!(!h.screen().contains("Not renamable"), "{}", h.screen());
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    for name in [
        "2024-05-01_01.jpg",
        "2024-05-01_02.jpg",
        "scan.jpg",
        "song.mp3",
    ] {
        assert!(dir.join(name).exists(), "{name}");
    }
}

#[test]
fn a_changed_pattern_lets_the_plan_start() {
    let (mut h, dir) = scene();
    type_pattern(&mut h, "{exif.date}_{n:2}.{ext}");
    wait_preview(&mut h);
    for _ in 0.."{exif.date}_{n:2}.{ext}".len() {
        h.key(KeyCode::Backspace);
    }
    h.keys("{n:2}_{name}.{ext}");
    wait_preview(&mut h);
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("04_song.mp3").exists());
}

#[test]
fn the_window_speaks_italian() {
    with_lang(Lang::It, || {
        let (mut h, _) = scene();
        type_pattern(&mut h, "{audio.title}.{ext}");
        assert!(h.screen().contains("Lettura dei metadati… 0/4"));
        wait_preview(&mut h);
        let s = h.screen();
        assert!(s.contains("Non rinominabili: 3"), "{s}");
        assert!(s.contains("nessun valore per {audio.title}"), "{s}");
        assert!(s.contains("Caruso.mp3"), "{s}");
    });
}
