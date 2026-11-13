//! A bulk rename that names files by what they say about themselves: the day a photo was
//! taken, the artist and title of a song, the title of a PDF, the length of a video. A file
//! without the field asked for, and without a field to fall back on, blocks the plan and says
//! why; the rules of every rename still hold (valid names, conflicts, the file checked again
//! just before it is renamed).
//!
//! The files are made here, byte by byte. `ffprobe` is never the real one: it is this test
//! binary, started again to run the one test `stand_in`, which answers as `ffprobe` would.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::unbounded;
use rada_core::events::CoreEvent;
use rada_core::meta::{Ffprobe, MetaLimits, MetaReader, MetaTools};
use rada_core::ops::*;
use rada_core::testmedia::*;
use rada_core::testutil::*;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn plan(e: &Engine, items: &[PathBuf], pattern: &str) -> Plan {
    e.plan_bulk_rename(items, &Pattern::parse(pattern).unwrap())
}

fn plan_request(e: &Engine, req: &OpRequest) -> Plan {
    let never = AtomicBool::new(false);
    let ctl = ScanControl {
        cancel: &never,
        progress: &mut |_| {},
    };
    e.plan_request(req, None, ctl).unwrap().plan
}

fn blocking(plan: &Plan) -> Vec<String> {
    plan.warnings
        .iter()
        .filter(|w| w.severity == Severity::Blocking)
        .map(|w| w.message.clone())
        .collect()
}

#[test]
fn photos_are_named_by_the_day_they_were_taken() {
    let sb = Sandbox::new();
    let photo = jpeg(&[exif_segment()]);
    let items = vec![
        sb.write("trip/IMG_0001.JPG", &photo),
        sb.write("trip/IMG_0002.JPG", &photo),
    ];
    let e = sb.engine();
    let p = plan(&e, &items, "{exif.date:%Y-%m-%d}_{exif.time}_{n:2}.{ext}");
    assert!(p.is_executable(), "{:?}", p.warnings);
    assert_eq!(run(&e, &p).status(), RunStatus::Completed);
    assert_eq!(
        names(&sb.path("trip")),
        ["2024-05-01_143005_01.JPG", "2024-05-01_143005_02.JPG"]
    );
}

#[test]
fn a_file_without_the_field_blocks_the_plan_and_says_why() {
    let sb = Sandbox::new();
    let items = vec![
        sb.write("d/a.jpg", jpeg(&[exif_segment()])),
        sb.write("d/b.jpg", jpeg(&[segment(0xE0, b"JFIF\0\x01\x01")])),
    ];
    let e = sb.engine();
    let p = plan(&e, &items, "{exif.date}_{n}.{ext}");
    assert!(!p.is_executable());
    let why = blocking(&p);
    assert_eq!(why.len(), 1, "{why:?}");
    assert!(why[0].contains("{exif.date}"), "{why:?}");
    let it = rada_core::i18n::with_lang(rada_core::i18n::Lang::It, || {
        blocking(&plan(&e, &items, "{exif.place}.{ext}"))
    });
    assert!(it[0].contains("nessun valore per {exif.place}"), "{it:?}");
    assert_eq!(names(&sb.path("d")), ["a.jpg", "b.jpg"], "nothing renamed");
}

#[test]
fn the_time_of_the_last_change_stands_in_for_a_missing_date() {
    let sb = Sandbox::new();
    let photo = sb.write("d/a.jpg", jpeg(&[exif_segment()]));
    let bare = sb.write("d/b.jpg", jpeg(&[segment(0xE0, b"JFIF\0\x01\x01")]));
    // Mid-June at noon: the same year and month in every time zone.
    let june = SystemTime::UNIX_EPOCH + Duration::from_secs(992_606_400);
    std::fs::File::options()
        .write(true)
        .open(&bare)
        .unwrap()
        .set_modified(june)
        .unwrap();
    let e = sb.engine();
    let p = plan(&e, &[photo, bare], "{exif.date|mtime:%Y-%m}_{name}.{ext}");
    assert!(p.is_executable(), "{:?}", p.warnings);
    run(&e, &p);
    assert_eq!(names(&sb.path("d")), ["2001-06_b.jpg", "2024-05_a.jpg"]);
}

#[test]
fn songs_are_named_by_track_artist_and_title_through_a_request_too() {
    let sb = Sandbox::new();
    let items = vec![
        sb.write(
            "m/01.mp3",
            mp3(&[
                ("TPE1", "Lucio Dalla"),
                ("TIT2", "Caruso"),
                ("TRCK", "3/12"),
                ("TDRC", "1986"),
            ]),
        ),
        sb.write(
            "m/x.flac",
            flac(&["ARTIST=AC/DC", "TITLE=T.N.T.", "TRACKNUMBER=7"]),
        ),
    ];
    let req = OpRequest::BulkRename {
        items,
        pattern: "{audio.track:02} - {audio.artist} - {audio.title}.{ext}".into(),
        counter_start: None,
        counter_step: None,
    };
    let e = sb.engine();
    let p = plan_request(&e, &req);
    assert!(p.is_executable(), "{:?}", p.warnings);
    run(&e, &p);
    // A slash from the file never makes a folder.
    assert_eq!(
        names(&sb.path("m")),
        ["03 - Lucio Dalla - Caruso.mp3", "07 - AC_DC - T.N.T..flac"]
    );
}

#[test]
fn documents_are_named_by_author_and_title() {
    let sb = Sandbox::new();
    let doc = sb.write(
        "p/scan0001.pdf",
        pdf("<< /Title (Relazione 2024) /Author (Ada Rossi) >>"),
    );
    let e = sb.engine();
    let p = plan(&e, &[doc], "{pdf.author} - {pdf.title}.{ext}");
    run(&e, &p);
    assert_eq!(names(&sb.path("p")), ["Ada Rossi - Relazione 2024.pdf"]);
}

#[test]
fn two_files_that_would_get_the_same_name_still_conflict() {
    let sb = Sandbox::new();
    let photo = jpeg(&[exif_segment()]);
    let items = vec![sb.write("d/a.jpg", &photo), sb.write("d/b.jpg", &photo)];
    let p = plan(&sb.engine(), &items, "{exif.camera}.{ext}");
    assert!(!p.is_executable());
    assert!(
        p.warnings.iter().any(|w| w.kind == WarningKind::Conflict),
        "{:?}",
        p.warnings
    );
}

#[test]
fn a_file_changed_after_the_plan_is_not_renamed() {
    let sb = Sandbox::new();
    let f = sb.write("d/a.jpg", jpeg(&[exif_segment()]));
    let e = sb.engine();
    let p = plan(&e, std::slice::from_ref(&f), "{exif.date}.{ext}");
    assert!(p.is_executable(), "{:?}", p.warnings);
    let mut longer = jpeg(&[exif_segment()]);
    longer.extend(b"written after the plan");
    std::fs::write(&f, longer).unwrap();
    let rep = run(&e, &p);
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert_eq!(names(&sb.path("d")), ["a.jpg"]);
}

#[test]
fn a_name_that_only_changes_case_is_renamed() {
    let sb = Sandbox::new();
    let f = sb.write("m/caruso.mp3", mp3(&[("TIT2", "Caruso")]));
    let e = sb.engine();
    let p = plan(&e, &[f], "{audio.title}.{ext}");
    assert!(p.is_executable(), "{:?}", p.warnings);
    assert_eq!(run(&e, &p).status(), RunStatus::Completed);
    assert_eq!(names(&sb.path("m")), ["Caruso.mp3"]);
}

// ---------------------------------------------------------------------------------------------
// Videos, through a stand-in ffprobe
// ---------------------------------------------------------------------------------------------

/// The stand-in playing `role`, which adds a line to `record` each time it is started.
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

fn runs(record: &Path) -> usize {
    std::fs::read_to_string(record).map_or(0, |s| s.lines().count())
}

#[test]
fn videos_are_named_by_their_length_and_need_ffprobe_for_it() {
    let sb = Sandbox::new();
    let clip = sb.write("v/clip.mp4", b"not really a video");
    let record = sb.path("record");
    let reader = MetaReader::new(probe("ffprobe", &record), MetaLimits::default());
    let without = plan(
        &sb.engine(),
        std::slice::from_ref(&clip),
        "{name}_{video.duration}.{ext}",
    );
    assert!(blocking(&without)[0].contains("{video.duration}"));
    let e = sb.engine().with_meta(reader);
    run(&e, &plan(&e, &[clip], "{name}_{video.duration}.{ext}"));
    assert_eq!(names(&sb.path("v")), ["clip_1m02s.mp4"]);
}

#[test]
fn the_preview_and_the_plan_read_each_file_once() {
    let sb = Sandbox::new();
    let items: Vec<PathBuf> = (1..=3)
        .map(|i| sb.write(format!("v/{i}.mkv"), b"x"))
        .collect();
    let record = sb.path("record");
    let reader = MetaReader::new(probe("ffprobe", &record), MetaLimits::default());
    let (tx, rx) = unbounded();
    let previewer = RenamePreviewer::spawn(reader.clone(), tx);
    let pattern = Pattern::parse("{video.duration}_{n}.{ext}").unwrap();
    let rules = sb.platform().path_rules();
    let epoch = previewer.ask(pattern.clone(), Arc::new(items.clone()), rules);
    let rows = ready(&rx, epoch, Duration::from_secs(30)).expect("the preview");
    let shown: Vec<String> = rows
        .iter()
        .map(|r| r.to.as_ref().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(shown, ["1m02s_1.mkv", "1m02s_2.mkv", "1m02s_3.mkv"]);
    let e = sb.engine().with_meta(reader);
    assert!(e.plan_bulk_rename(&items, &pattern).is_executable());
    assert_eq!(runs(&record), 3, "ffprobe once per file");
}

/// The rows of `epoch`, or `None` when they did not come within `wait`.
fn ready(
    rx: &crossbeam_channel::Receiver<CoreEvent>,
    epoch: u64,
    wait: Duration,
) -> Option<Vec<Preview>> {
    let until = Instant::now() + wait;
    while let Ok(ev) = rx.recv_deadline(until) {
        if let CoreEvent::RenamePreview(RenamePreview::Ready { epoch: e, rows }) = ev
            && e == epoch
        {
            return Some(rows);
        }
    }
    None
}

#[test]
fn the_preview_says_how_far_the_reading_is() {
    let sb = Sandbox::new();
    let items: Vec<PathBuf> = (1..=4)
        .map(|i| sb.write(format!("v/{i}.mp4"), b"x"))
        .collect();
    let reader = MetaReader::new(probe("late", &sb.path("record")), MetaLimits::default());
    let (tx, rx) = unbounded();
    let previewer = RenamePreviewer::spawn(reader, tx);
    let pattern = Pattern::parse("{video.duration}_{n}.{ext}").unwrap();
    let epoch = previewer.ask(pattern, Arc::new(items), sb.platform().path_rules());
    let mut seen = Vec::new();
    let until = Instant::now() + Duration::from_secs(30);
    while let Ok(ev) = rx.recv_deadline(until) {
        match ev {
            CoreEvent::RenamePreview(RenamePreview::Reading { done, total, .. }) => {
                seen.push((done, total))
            }
            CoreEvent::RenamePreview(RenamePreview::Ready { epoch: e, .. }) if e == epoch => break,
            _ => {}
        }
    }
    assert!(!seen.is_empty(), "no progress was said");
    assert!(
        seen.iter().all(|(d, t)| *t == 4 && *d >= 1 && *d <= 4),
        "{seen:?}"
    );
}

#[test]
fn a_newer_pattern_stops_the_reading_for_the_older_one() {
    let sb = Sandbox::new();
    let items: Vec<PathBuf> = (1..=3)
        .map(|i| sb.write(format!("v/{i}.mp4"), b"x"))
        .collect();
    let limits = MetaLimits {
        timeout: Duration::from_secs(60),
        ..MetaLimits::default()
    };
    let reader = MetaReader::new(probe("slow", &sb.path("record")), limits);
    let (tx, rx) = unbounded();
    let previewer = RenamePreviewer::spawn(reader, tx);
    let rules = sb.platform().path_rules();
    let items = Arc::new(items);
    let slow = Pattern::parse("{video.duration}.{ext}").unwrap();
    let first = previewer.ask(slow, items.clone(), rules.clone());
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    let second = previewer.ask(Pattern::parse("{n}.{ext}").unwrap(), items, rules);
    let rows = ready(&rx, second, Duration::from_secs(10)).expect("the newer preview");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(rows[2].to.as_ref().unwrap(), "3.mp4");
    assert!(ready(&rx, first, Duration::from_millis(200)).is_none());
}

#[test]
fn a_plan_stops_reading_when_it_is_cancelled() {
    let sb = Sandbox::new();
    let items: Vec<PathBuf> = (1..=3)
        .map(|i| sb.write(format!("v/{i}.mp4"), b"x"))
        .collect();
    let limits = MetaLimits {
        timeout: Duration::from_secs(60),
        ..MetaLimits::default()
    };
    let reader = MetaReader::new(probe("slow", &sb.path("record")), limits);
    let e = sb.engine().with_meta(reader);
    let cancel = Arc::new(AtomicBool::new(false));
    let later = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        later.store(true, Ordering::SeqCst);
    });
    let started = Instant::now();
    let mut said = 0;
    let ctl = ScanControl {
        cancel: &cancel,
        progress: &mut |_| said += 1,
    };
    let pattern = Pattern::parse("{video.duration}.{ext}").unwrap();
    e.plan_bulk_rename_with(&items, &pattern, ctl);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(said <= 1, "{said}");
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
    use std::io::Write;
    let mut log = std::fs::File::options()
        .create(true)
        .append(true)
        .open(record)
        .unwrap();
    writeln!(log, "{}", args[at + 2..].join(" ")).unwrap();
    let answer = r#"{"streams":[{"codec_type":"video","width":640,"height":360}],"format":{"duration":"62.5"}}"#;
    match role {
        "ffprobe" => println!("{answer}"),
        "late" => {
            std::thread::sleep(Duration::from_millis(150));
            println!("{answer}");
        }
        "slow" => std::thread::sleep(Duration::from_secs(60)),
        other => panic!("no part called {other}"),
    }
    std::process::exit(0);
}
