//! The details pane says what the file under the cursor says about itself: when and with what
//! a photo was taken, who sings a song. It comes from a worker, so the pane is drawn first
//! without it and never waits.

mod common;

use std::time::{Duration, SystemTime};

use common::*;
use rada_core::i18n::{Key, Lang, with_lang};
use rada_core::testmedia::*;
use rada_core::testutil::{Sandbox, set_mtime};
use rada_tui::Theme;

/// 2030-01-01 12:00 UTC.
fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_499_200)
}

/// `~/photos` with a photo, a song and a note, the details pane open, the cursor on `name`
/// and what it says about itself arrived.
fn scene(name: &str) -> H {
    let sb = Sandbox::new();
    let dir = sb.dirs.home.join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let xmp = xmp_segment(r#"photoshop:City="Roma" photoshop:Country="Italia""#);
    let files: [(&str, Vec<u8>); 3] = [
        ("IMG_0001.jpg", jpeg(&[exif_segment(), xmp])),
        (
            "caruso.mp3",
            mp3(&[("TPE1", "Lucio Dalla"), ("TIT2", "Caruso"), ("TRCK", "3")]),
        ),
        ("notes.txt", b"remember the milk\n".to_vec()),
    ];
    for (n, bytes) in files {
        let p = dir.join(n);
        std::fs::write(&p, bytes).unwrap();
        set_mtime(&p, fixed_now() - Duration::from_secs(86_400));
    }
    let mut h = H::with_config(sb, dir, 110, 40, |c| c.theme = Theme::rada());
    h.wait("the places", |a| !a.places.paths.places.is_empty());
    h.wait("the volumes", |a| a.devices.volumes_seen);
    h.app.hooks.clock = Some(fixed_now());
    h.app.settings.tz = jiff::tz::TimeZone::UTC;
    h.app.settings.details = Some(true);
    h.app.devices.volumes.clear();
    h.cursor_to(name);
    h.wait("the preview", |a| a.preview.name == name);
    // The pane asks when it is drawn.
    h.wait("the metadata", |a| {
        a.tab.current().is_some_and(|e| a.meta_of(e).is_some())
    });
    h
}

fn shot(h: &mut H) -> String {
    h.screen()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The metadata section at the bottom of the details pane, as drawn. The rows above it differ
/// from system to system (permissions or attributes, the time a file was made).
fn section(screen: &str) -> String {
    let pane = |l: &str| l.split('│').nth(4).unwrap_or("").trim_end().to_string();
    let rows: Vec<String> = screen.lines().map(pane).collect();
    let from = rows.iter().position(|r| r == " Metadata").unwrap();
    rows[from..]
        .iter()
        .take_while(|r| !r.trim().is_empty())
        .map(|r| format!("{r}\n"))
        .collect()
}

#[test]
fn a_photo_shows_when_where_and_with_what_it_was_taken() {
    let mut h = scene("IMG_0001.jpg");
    let screen = shot(&mut h);
    assert!(screen.contains("Metadata"), "{screen}");
    assert!(screen.contains("2024-05-01 14:30:05"), "{screen}");
    assert!(screen.contains("Canon EOS R6"), "{screen}");
    assert!(screen.contains("Roma, Italia"), "{screen}");
    insta::assert_snapshot!("file_meta_photo_110x40", section(&screen));
}

#[test]
fn a_song_shows_its_artist_title_and_track() {
    let mut h = scene("caruso.mp3");
    let screen = shot(&mut h);
    for want in ["Lucio Dalla", "Caruso", "Track"] {
        assert!(screen.contains(want), "{want}: {screen}");
    }
}

#[test]
fn a_file_with_nothing_to_say_has_no_metadata_section() {
    let mut h = scene("IMG_0001.jpg");
    h.cursor_to("notes.txt");
    h.wait("the preview", |a| a.preview.name == "notes.txt");
    let screen = shot(&mut h);
    assert!(!screen.contains(Key::PropMetaHeading.tr()), "{screen}");
}

#[test]
fn the_section_speaks_italian() {
    let mut h = scene("IMG_0001.jpg");
    let screen = with_lang(Lang::It, || shot(&mut h));
    for want in ["Metadati", "Scattata", "Fotocamera", "Luogo"] {
        assert!(screen.contains(want), "{want}: {screen}");
    }
}
