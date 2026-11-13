//! What files say about themselves: the EXIF of a photo, the tags of a song, the title of a
//! PDF, the length of a video. Whatever the file, reading gives fields or nothing, never an
//! error, and never takes long.
//!
//! The files are made here, byte by byte. `ffprobe` is never the real one: it is this test
//! binary, started again to run the one test `stand_in`, which answers as `ffprobe` would.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use rada_core::events::CoreEvent;
use rada_core::meta::{self, Ffprobe, Gps, Meta, MetaEvent, MetaLimits, MetaTools, MetaWorker};

use rada_core::testmedia::*;

struct Dir(tempfile::TempDir);

impl Dir {
    fn new() -> Dir {
        Dir(tempfile::tempdir().unwrap())
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

fn read(p: &Path) -> Meta {
    meta::read(p, &MetaTools::none(), &MetaLimits::default(), &|| false)
}

// ---------------------------------------------------------------------------------------------
// Photos
// ---------------------------------------------------------------------------------------------

#[test]
fn a_photo_gives_its_date_time_camera_orientation_and_position() {
    let d = Dir::new();
    let p = d.file("IMG_0001.JPG", &jpeg(&[exif_segment()]));
    let e = read(&p).exif;
    assert_eq!(e.date, Some(jiff::civil::date(2024, 5, 1)));
    assert_eq!(e.time, Some(jiff::civil::time(14, 30, 5, 0)));
    assert_eq!(e.camera.as_deref(), Some("Canon EOS R6"));
    assert_eq!(e.orientation, Some(6));
    let Gps { lat, lon } = e.gps.unwrap();
    assert!((lat - 41.902_777).abs() < 1e-4, "{lat}");
    assert!((lon - 12.483_888).abs() < 1e-4, "{lon}");
    assert_eq!(e.place, None, "coordinates alone give no place");
}

#[test]
fn the_place_is_the_one_written_in_the_photo() {
    let d = Dir::new();
    let xmp = xmp_segment(r#"photoshop:City="Roma" photoshop:Country="Italia""#);
    let p = d.file("a.jpg", &jpeg(&[exif_segment(), xmp]));
    assert_eq!(read(&p).exif.place.as_deref(), Some("Roma, Italia"));
}

#[test]
fn text_in_a_photo_is_cleaned_like_any_outside_text() {
    let d = Dir::new();
    let xmp = xmp_segment("photoshop:City=\"Ro\u{1b}[2Jma\u{202e}\"");
    let p = d.file("a.jpg", &jpeg(&[xmp]));
    let place = read(&p).exif.place.unwrap();
    assert!(
        !place.chars().any(rada_core::display::is_dangerous),
        "{place:?}"
    );
    assert_eq!(place, "Ro\\e[2Jma\\u{202e}");
}

#[test]
fn a_photo_without_exif_a_damaged_one_and_one_that_is_not_a_photo_give_nothing() {
    let d = Dir::new();
    let bare = d.file("bare.jpg", &jpeg(&[segment(0xE0, b"JFIF\0\x01\x01")]));
    let mut cut = jpeg(&[exif_segment()]);
    cut.truncate(40);
    let cut = d.file("cut.jpg", &cut);
    let mut scrambled = jpeg(&[exif_segment()]);
    for b in scrambled.iter_mut().skip(20) {
        *b = b.wrapping_mul(31).wrapping_add(7);
    }
    let scrambled = d.file("scrambled.jpg", &scrambled);
    let text = d.file("text.jpg", b"I am not a picture");
    for p in [bare, cut, scrambled, text] {
        assert!(read(&p).is_empty(), "{}", p.display());
    }
}

#[test]
fn only_the_start_of_a_photo_is_read() {
    let d = Dir::new();
    // The EXIF comes after a big segment of something else: past what is read, not found.
    let filler = segment(0xE2, &vec![0u8; 60_000]);
    let p = d.file("late.jpg", &jpeg(&[filler, exif_segment()]));
    let limits = MetaLimits {
        prefix: 4096,
        ..MetaLimits::default()
    };
    assert!(meta::read(&p, &MetaTools::none(), &limits, &|| false).is_empty());
    assert!(
        read(&p).exif.camera.is_some(),
        "found when the start read is big enough"
    );
}

#[test]
fn a_file_over_the_limit_is_not_read() {
    let d = Dir::new();
    let p = d.file("big.jpg", &jpeg(&[exif_segment()]));
    let limits = MetaLimits {
        max_bytes: 100,
        ..MetaLimits::default()
    };
    assert!(meta::read(&p, &MetaTools::none(), &limits, &|| false).is_empty());
}

/// A PDF bigger than the PDF limit, made sparse: nothing is read and it answers at once.
#[cfg(unix)]
#[test]
fn a_huge_pdf_is_skipped_without_reading_it() {
    let d = Dir::new();
    let p = d.file("huge.pdf", &pdf("<< /Title (Big) >>"));
    let f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.set_len(200 << 20).unwrap();
    let started = Instant::now();
    assert!(read(&p).is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn a_pipe_with_the_name_of_a_song_is_not_opened() {
    let d = Dir::new();
    let p = d.0.path().join("song.mp3");
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: `c` is a NUL-terminated path in a folder of the test.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let started = Instant::now();
    assert!(read(&p).is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
}

// ---------------------------------------------------------------------------------------------
// Songs and documents
// ---------------------------------------------------------------------------------------------

#[test]
fn an_mp3_gives_its_artist_title_album_track_and_year() {
    let d = Dir::new();
    let p = d.file(
        "01.mp3",
        &mp3(&[
            ("TPE1", "Lucio Dalla"),
            ("TIT2", "Caruso"),
            ("TALB", "DallAmeriCaruso"),
            ("TRCK", "3/12"),
            ("TDRC", "1986"),
        ]),
    );
    let a = read(&p).audio;
    assert_eq!(a.artist.as_deref(), Some("Lucio Dalla"));
    assert_eq!(a.title.as_deref(), Some("Caruso"));
    assert_eq!(a.album.as_deref(), Some("DallAmeriCaruso"));
    assert_eq!(a.track, Some(3));
    assert_eq!(a.year, Some(1986));
}

#[test]
fn a_flac_gives_its_vorbis_comments() {
    let d = Dir::new();
    let p = d.file(
        "a.flac",
        &flac(&[
            "ARTIST=Mina",
            "TITLE=Se telefonando",
            "ALBUM=Studio Uno 66",
            "TRACKNUMBER=7",
            "DATE=1966-04-01",
        ]),
    );
    let a = read(&p).audio;
    assert_eq!(a.artist.as_deref(), Some("Mina"));
    assert_eq!(a.title.as_deref(), Some("Se telefonando"));
    assert_eq!(a.album.as_deref(), Some("Studio Uno 66"));
    assert_eq!(a.track, Some(7));
    assert_eq!(a.year, Some(1966));
}

#[test]
fn a_song_without_tags_or_a_damaged_one_gives_nothing() {
    let d = Dir::new();
    let bare = d.file("bare.mp3", &mpeg_frames());
    let mut cut = mp3(&[("TPE1", "Someone")]);
    cut.truncate(14);
    let cut = d.file("cut.mp3", &cut);
    let junk = d.file("junk.flac", b"fLaC\x00\x00\x00\x22 not really");
    for p in [bare, cut, junk] {
        assert!(read(&p).audio.artist.is_none(), "{}", p.display());
    }
}

#[test]
fn a_pdf_gives_its_title_author_and_pages() {
    let d = Dir::new();
    let p = d.file(
        "doc.pdf",
        &pdf("<< /Title (Relazione 2024) /Author (Ada Rossi) >>"),
    );
    let m = read(&p).pdf;
    assert_eq!(m.title.as_deref(), Some("Relazione 2024"));
    assert_eq!(m.author.as_deref(), Some("Ada Rossi"));
    assert_eq!(m.pages, Some(2));
}

#[test]
fn a_damaged_pdf_or_an_empty_file_gives_nothing() {
    let d = Dir::new();
    let mut cut = pdf("<< /Title (X) >>");
    cut.truncate(60);
    for p in [
        d.file("cut.pdf", &cut),
        d.file("empty.pdf", b""),
        d.file("junk.pdf", b"%PDF-1.7 junk"),
    ] {
        assert!(read(&p).pdf.title.is_none(), "{}", p.display());
    }
}

#[test]
fn a_file_of_a_kind_that_is_not_known_gives_nothing() {
    let d = Dir::new();
    let p = d.file("notes.txt", &mp3(&[("TPE1", "hidden in a text file")]));
    assert!(read(&p).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Videos, through a stand-in ffprobe
// ---------------------------------------------------------------------------------------------

/// The stand-in playing `role`, which writes the arguments it was given to `record`. After
/// `--` the test harness takes everything as a name, even what starts with a dash.
fn probe(role: &str, record: &Path) -> MetaTools {
    let lead = ["stand_in", "--exact", "--ignored", "--nocapture", "--"]
        .iter()
        .map(OsString::from)
        .chain([
            OsString::from(format!("role:{role}")),
            record.as_os_str().to_owned(),
        ])
        .collect();
    MetaTools::with_ffprobe(Ffprobe {
        program: std::env::current_exe().unwrap(),
        lead,
    })
}

#[test]
fn a_video_gives_what_ffprobe_says_and_ffprobe_is_given_the_file_as_a_name_only() {
    let d = Dir::new();
    let record = d.0.path().join("args");
    let p = d.file("-clip:1.mp4", b"not really a video");
    let v = meta::read(
        &p,
        &probe("ffprobe", &record),
        &MetaLimits::default(),
        &|| false,
    )
    .video;
    assert_eq!(v.duration, Some(Duration::from_millis(62_500)));
    assert_eq!((v.width, v.height), (Some(1920), Some(1080)));
    let args = std::fs::read_to_string(&record).unwrap();
    let want = format!(
        "-v\nerror\n-print_format\njson\n-show_entries\nformat=duration:stream=codec_type,width,height,duration\nfile:{}\n",
        p.display()
    );
    assert_eq!(args, want);
}

#[test]
fn without_ffprobe_or_when_it_fails_a_video_gives_nothing() {
    let d = Dir::new();
    let record = d.0.path().join("args");
    let p = d.file("clip.mkv", b"x");
    assert!(read(&p).is_empty(), "no ffprobe");
    let failing = probe("fail", &record);
    assert!(meta::read(&p, &failing, &MetaLimits::default(), &|| false).is_empty());
    let nonsense = probe("nonsense", &record);
    assert!(meta::read(&p, &nonsense, &MetaLimits::default(), &|| false).is_empty());
}

#[test]
fn an_ffprobe_that_does_not_answer_is_stopped_at_the_timeout() {
    let d = Dir::new();
    let record = d.0.path().join("args");
    let p = d.file("clip.mov", b"x");
    let limits = MetaLimits {
        timeout: Duration::from_millis(500),
        ..MetaLimits::default()
    };
    let started = Instant::now();
    assert!(meta::read(&p, &probe("slow", &record), &limits, &|| false).is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
}

// ---------------------------------------------------------------------------------------------
// The worker
// ---------------------------------------------------------------------------------------------

fn next_meta(rx: &Receiver<CoreEvent>, secs: u64) -> Option<MetaEvent> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(CoreEvent::Meta(m)) => return Some(m),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

#[test]
fn the_worker_answers_file_by_file_and_says_how_far_it_is() {
    let d = Dir::new();
    let a = d.file("a.jpg", &jpeg(&[exif_segment()]));
    let b = d.file("b.txt", b"nothing");
    let c = d.file("c.pdf", &pdf("<< /Title (C) >>"));
    let (tx, rx) = unbounded();
    let w = MetaWorker::spawn(MetaTools::none(), MetaLimits::default(), tx);
    w.newest(1);
    w.request(1, vec![a.clone(), b.clone(), c.clone()]);
    let got: Vec<MetaEvent> = (0..3).map(|_| next_meta(&rx, 10).unwrap()).collect();
    assert_eq!(
        got.iter().map(|e| e.path.clone()).collect::<Vec<_>>(),
        [a, b, c]
    );
    assert_eq!(
        got.iter().map(|e| (e.done, e.total)).collect::<Vec<_>>(),
        [(1, 3), (2, 3), (3, 3)]
    );
    assert!(got[0].meta.exif.camera.is_some());
    assert!(got[1].meta.is_empty());
    assert_eq!(got[2].meta.pdf.title.as_deref(), Some("C"));
}

#[test]
fn the_worker_keeps_an_answer_while_the_file_stays_as_it_was() {
    let d = Dir::new();
    let p = d.file("doc.pdf", &pdf("<< /Title (First) >>"));
    let (tx, rx) = unbounded();
    let w = MetaWorker::spawn(MetaTools::none(), MetaLimits::default(), tx);
    w.newest(1);
    w.request(1, vec![p.clone()]);
    let first = next_meta(&rx, 10).unwrap();
    w.request(1, vec![p.clone()]);
    let again = next_meta(&rx, 10).unwrap();
    assert!(Arc::ptr_eq(&first.meta, &again.meta), "from the cache");

    std::fs::write(&p, pdf("<< /Title (Second, longer) >>")).unwrap();
    w.request(1, vec![p.clone()]);
    let changed = next_meta(&rx, 10).unwrap();
    assert_eq!(changed.meta.pdf.title.as_deref(), Some("Second, longer"));
}

#[test]
fn a_newer_request_stops_the_one_being_read() {
    let d = Dir::new();
    let record = d.0.path().join("args");
    let slow = d.file("slow.mp4", b"x");
    let after = d.file("after.mp4", b"x");
    let photo = d.file("p.jpg", &jpeg(&[exif_segment()]));
    let (tx, rx) = unbounded();
    let w = MetaWorker::spawn(probe("slow", &record), MetaLimits::default(), tx);
    w.newest(1);
    w.request(1, vec![slow, after]);
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    w.newest(2);
    w.request(2, vec![photo.clone()]);
    let ev = next_meta(&rx, 10).unwrap();
    assert_eq!(
        (ev.epoch, ev.path),
        (2, photo),
        "nothing of the first request is told"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

// ---------------------------------------------------------------------------------------------
// The stand-in
// ---------------------------------------------------------------------------------------------

/// Not a test: the `ffprobe` the tests above run. Without a part to play it does nothing.
#[test]
#[ignore = "the stand-in program of the other tests"]
fn stand_in() {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|a| a.starts_with("role:")) else {
        return;
    };
    let role = args[at].trim_start_matches("role:");
    let record = &args[at + 1];
    let rest: String = args[at + 2..].iter().map(|a| format!("{a}\n")).collect();
    std::fs::write(record, rest).unwrap();
    let code = match role {
        "ffprobe" => {
            println!(
                r#"{{"programs":[],"streams":[{{"codec_type":"audio"}},{{"codec_type":"video","width":1920,"height":1080}}],"format":{{"duration":"62.500000"}}}}"#
            );
            0
        }
        "fail" => {
            eprintln!("clip.mkv: Invalid data found when processing input");
            1
        }
        "nonsense" => {
            println!("{{ this is not json");
            0
        }
        "slow" => {
            std::thread::sleep(Duration::from_secs(60));
            0
        }
        other => panic!("no part called {other}"),
    };
    std::process::exit(code);
}
