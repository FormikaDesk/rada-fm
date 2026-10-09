//! PDF previews: the first page as a picture through poppler, in the worker, with limits.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, unbounded};

use rada_core::events::{CoreEvent, PreviewEvent};
use rada_core::fs::LocalFs;
use rada_core::preview::pdf::{self, Tools};
use rada_core::preview::{ImageLimits, ImagePreview, ImageState, Limits, Preview};
use rada_core::testutil::*;
use rada_core::workers::PreviewWorker;

const LOCKED: &[u8] = include_bytes!("fixtures/locked.pdf");

/// A small, valid PDF: `pages` pages of Helvetica text, with a title and an author.
fn make_pdf(title: &str, author: &str, pages: usize, text: &str) -> Vec<u8> {
    let mut objs: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i * 2)).collect();
    objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objs.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    ));
    for i in 0..pages {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents {} 0 R /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>",
            4 + i * 2
        ));
        let s = format!("BT /F1 24 Tf 20 100 Td ({text} {}) Tj ET", i + 1);
        objs.push(format!("<< /Length {} >>\nstream\n{s}\nendstream", s.len()));
    }
    objs.push(format!("<< /Title ({title}) /Author ({author}) >>"));
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let x = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offs {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{x}\n%%EOF\n",
            objs.len() + 1,
            objs.len()
        )
        .bytes(),
    );
    out
}

fn have_poppler() -> bool {
    let t = Tools::installed();
    if t.pdftoppm.is_some() && t.pdfinfo.is_some() && t.pdftotext.is_some() {
        true
    } else {
        eprintln!("poppler is not installed: test skipped");
        false
    }
}

fn worker() -> (PreviewWorker, Receiver<CoreEvent>) {
    let (tx, rx) = unbounded();
    (PreviewWorker::spawn(Arc::new(LocalFs), tx), rx)
}

fn next_for(rx: &Receiver<CoreEvent>, generation: u64, secs: u64) -> PreviewEvent {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(CoreEvent::Preview(p)) if p.generation == generation => return p,
            Ok(_) => {}
            Err(_) => panic!("no preview event for generation {generation}"),
        }
    }
}

fn settled(rx: &Receiver<CoreEvent>, generation: u64) -> ImagePreview {
    loop {
        match next_for(rx, generation, 30).preview {
            Preview::Image(i) if matches!(i.state, ImageState::Loading) => continue,
            Preview::Image(i) => return i,
            other => panic!("expected a PDF preview, got {other:?}"),
        }
    }
}

fn limits(sb: &Sandbox) -> Limits {
    Limits {
        image: ImageLimits {
            pdf_cache: Some(sb.path("cache")),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn cached(sb: &Sandbox) -> Vec<PathBuf> {
    std::fs::read_dir(sb.path("cache"))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// A folder with the named programs: symlinks to the real tools, or `script` bodies.
fn tools_dir(sb: &Sandbox, name: &str, links: &[&str], scripts: &[(&str, &str)]) -> OsString {
    let d = sb.mkdir(name);
    for l in links {
        let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(l))
            .find(|p| p.is_file())
            .unwrap();
        std::os::unix::fs::symlink(real, d.join(l)).unwrap();
    }
    for (n, body) in scripts {
        let p = d.join(n);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    d.into_os_string()
}

#[test]
fn a_pdf_shows_its_first_page_with_pages_title_and_author() {
    if !have_poppler() {
        return;
    }
    let sb = Sandbox::new();
    let p = sb.path("report.pdf");
    std::fs::write(&p, make_pdf("Annual report", "A. Writer", 3, "Hello page")).unwrap();
    let (w, rx) = worker();
    w.request(p.clone(), 1, limits(&sb));

    // The header arrives at once, before poppler has done anything.
    let t0 = Instant::now();
    let Preview::Image(first) = next_for(&rx, 1, 5).preview else {
        panic!()
    };
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "{:?}",
        t0.elapsed()
    );
    assert_eq!(first.info.format, "PDF");
    assert!(
        matches!(first.state, ImageState::Loading),
        "{:?}",
        first.state
    );

    let done = settled(&rx, 1);
    let ImageState::Ready(px) = done.state else {
        panic!("{:?} {:?}", done.state, done.note)
    };
    assert!(px.width() > 100 && px.height() > 100);
    assert!(px.width().max(px.height()) <= 1600);
    let doc = done.info.doc.expect("facts");
    assert_eq!(doc.pages, Some(3));
    assert_eq!(doc.title.as_deref(), Some("Annual report"));
    assert_eq!(doc.author.as_deref(), Some("A. Writer"));
    assert_eq!(done.info.size, std::fs::metadata(&p).unwrap().len());
    assert!(done.info.modified.is_some());
    // The page does hold something: not a blank sheet.
    let rgb = px.to_rgb8();
    let first_px = *rgb.get_pixel(0, 0);
    assert!(rgb.pixels().any(|q| *q != first_px), "the page is blank");
}

#[test]
fn the_rendered_page_is_cached_and_reused() {
    if !have_poppler() {
        return;
    }
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("T", "A", 1, "Cache me")).unwrap();
    let (w, rx) = worker();
    w.request(p.clone(), 1, limits(&sb));
    assert!(matches!(settled(&rx, 1).state, ImageState::Ready(_)));
    let files = cached(&sb);
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].extension().is_some_and(|e| e == "png"));

    // Again: served from the cache, not drawn again.
    std::thread::sleep(Duration::from_millis(30));
    w.request(p.clone(), 2, limits(&sb));
    assert!(matches!(settled(&rx, 2).state, ImageState::Ready(_)));
    assert_eq!(cached(&sb), files);

    // Changing the file makes a new picture (the old one stays until pruned).
    std::fs::write(&p, make_pdf("T", "A", 1, "Changed now!")).unwrap();
    w.request(p.clone(), 3, limits(&sb));
    assert!(matches!(settled(&rx, 3).state, ImageState::Ready(_)));
    assert_eq!(cached(&sb).len(), 2);
}

#[test]
fn a_password_protected_pdf_says_so() {
    if !have_poppler() {
        return;
    }
    let sb = Sandbox::new();
    let p = sb.path("locked.pdf");
    std::fs::write(&p, LOCKED).unwrap();
    let (w, rx) = worker();
    w.request(p, 1, limits(&sb));
    let done = settled(&rx, 1);
    let ImageState::Failed(m) = done.state else {
        panic!("{:?}", done.state)
    };
    assert!(m.contains("password"), "{m}");
    assert!(done.text.is_empty(), "no peeking at the words either");
    assert!(cached(&sb).is_empty());
}

#[test]
fn a_damaged_pdf_gives_a_message_not_a_hang_or_a_blank() {
    let sb = Sandbox::new();
    let p = sb.path("broken.pdf");
    let mut junk = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 99 0 R >>\n".to_vec();
    junk.extend((0..4000u32).map(|i| (i * 7 % 251) as u8));
    std::fs::write(&p, junk).unwrap();
    let (w, rx) = worker();
    let t0 = Instant::now();
    w.request(p, 1, limits(&sb));
    let done = settled(&rx, 1);
    assert!(t0.elapsed() < Duration::from_secs(10));
    let ImageState::Failed(m) = done.state else {
        panic!("{:?}", done.state)
    };
    assert!(!m.is_empty());
    assert!(cached(&sb).is_empty(), "no half-made picture is kept");
}

#[test]
fn a_huge_pdf_is_refused_before_poppler_is_asked() {
    let sb = Sandbox::new();
    let p = sb.path("huge.pdf");
    let mut b = make_pdf("T", "A", 1, "x");
    b.resize(300_000, b' ');
    std::fs::write(&p, b).unwrap();
    let mut l = limits(&sb);
    l.image.pdf_max_file_bytes = 100_000;
    let (w, rx) = worker();
    w.request(p, 1, l);
    let Preview::Image(i) = next_for(&rx, 1, 5).preview else {
        panic!()
    };
    let ImageState::TooLarge(m) = i.state else {
        panic!("{:?}", i.state)
    };
    assert!(m.contains("pdf_max_file_mb"), "{m}");
    assert!(
        rx.recv_timeout(Duration::from_millis(400)).is_err(),
        "nothing may follow"
    );
    assert!(cached(&sb).is_empty());
}

#[test]
fn without_poppler_the_user_is_told_how_to_get_it() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("T", "A", 1, "x")).unwrap();
    let mut l = limits(&sb);
    l.image.pdf_tools_path = Some(sb.mkdir("empty").into_os_string());
    let (w, rx) = worker();
    w.request(p, 1, l);
    let i = settled(&rx, 1);
    assert!(matches!(i.state, ImageState::Failed(_)), "{:?}", i.state);
    let note = i.note.expect("a hint");
    assert!(note.contains("poppler"), "{note}");
    assert_eq!(i.info.format, "PDF");
    assert!(i.info.size > 0, "the facts from the file itself remain");
}

#[test]
fn without_pdftoppm_the_text_of_the_first_page_stands_in() {
    if !have_poppler() {
        return;
    }
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("T", "A", 2, "Findable words")).unwrap();
    let mut l = limits(&sb);
    l.image.pdf_tools_path = Some(tools_dir(&sb, "partial", &["pdftotext", "pdfinfo"], &[]));
    let (w, rx) = worker();
    w.request(p, 1, l);
    let i = settled(&rx, 1);
    assert!(matches!(i.state, ImageState::Failed(_)), "{:?}", i.state);
    assert!(
        i.text.iter().any(|l| l.contains("Findable words 1")),
        "{:?}",
        i.text
    );
    assert!(i.note.is_some_and(|n| n.contains("poppler")));
    assert_eq!(i.info.doc.and_then(|d| d.pages), Some(2));
}

#[test]
fn a_tool_that_takes_too_long_is_stopped() {
    let sb = Sandbox::new();
    let p = sb.path("slow.pdf");
    std::fs::write(&p, make_pdf("T", "A", 1, "x")).unwrap();
    let mut l = limits(&sb);
    l.image.pdf_timeout = Duration::from_millis(600);
    l.image.pdf_tools_path = Some(tools_dir(
        &sb,
        "slow",
        &[],
        &[
            ("pdfinfo", "printf 'Pages: 1\\n'"),
            ("pdftoppm", "exec sleep 30"),
        ],
    ));
    let (w, rx) = worker();
    let t0 = Instant::now();
    w.request(p, 1, l);
    // The header is not held up by the slow tool.
    let _ = next_for(&rx, 1, 5);
    assert!(t0.elapsed() < Duration::from_millis(500));
    let i = settled(&rx, 1);
    let ImageState::Failed(m) = i.state else {
        panic!("{:?}", i.state)
    };
    assert!(m.contains("took more than"), "{m}");
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    assert!(cached(&sb).is_empty());
}

#[test]
fn moving_on_stops_the_work_and_the_next_file_is_not_kept_waiting() {
    let sb = Sandbox::new();
    let slow = sb.path("slow.pdf");
    std::fs::write(&slow, make_pdf("T", "A", 1, "x")).unwrap();
    let img = sb.path("next.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([10, 200, 10, 255]))
        .save(&img)
        .unwrap();
    let mut l = limits(&sb);
    l.image.pdf_timeout = Duration::from_secs(60);
    l.image.pdf_tools_path = Some(tools_dir(
        &sb,
        "slow",
        &[],
        &[
            ("pdfinfo", "printf 'Pages: 1\\n'"),
            ("pdftoppm", "exec sleep 60"),
        ],
    ));
    let (w, rx) = worker();
    w.request(slow, 1, l.clone());
    let _ = next_for(&rx, 1, 5);
    std::thread::sleep(Duration::from_millis(300)); // the tool is running now
    let t0 = Instant::now();
    w.request(img, 2, l);
    let done = settled(&rx, 2);
    assert!(matches!(done.state, ImageState::Ready(_)));
    assert!(
        t0.elapsed() < Duration::from_secs(3),
        "the picture waited {:?} behind the PDF",
        t0.elapsed()
    );
}

#[test]
fn the_cache_is_pruned_by_age_and_size_and_only_our_files_are_touched() {
    let sb = Sandbox::new();
    let dir = sb.mkdir("cache");
    let mk = |name: &str, len: usize, age_days: u64| {
        let f = dir.join(name);
        std::fs::write(&f, vec![1u8; len]).unwrap();
        let t = SystemTime::now() - Duration::from_secs(age_days * 86_400);
        std::fs::File::options()
            .append(true)
            .open(&f)
            .unwrap()
            .set_modified(t)
            .unwrap();
        f
    };
    let old = mk("pdf-0000000000000001.png", 100, 40);
    let mid = mk("pdf-0000000000000002.png", 1000, 3);
    let new = mk("pdf-0000000000000003.png", 1000, 1);
    let stranger = mk("notes.txt", 5000, 400);
    let stale_part = mk("pdf-0000000000000004.part.png", 10, 1);

    let removed = pdf::prune_cache(&dir, 1500, Duration::from_secs(30 * 86_400));
    assert!(!old.exists(), "older than the age limit");
    assert!(
        !mid.exists(),
        "oldest of what is left, to get under the size limit"
    );
    assert!(new.exists());
    assert!(stranger.exists(), "files that are not ours are left alone");
    assert!(!stale_part.exists(), "leftovers of a render that died");
    assert_eq!(removed, 3);
}

#[test]
fn parses_the_facts_poppler_prints() {
    let sample = "Title:           My Doc: a story\nAuthor:          Jane\nCreator:         x\nPages:           12\nEncrypted:       no\n";
    let f = pdf::parse_info_for_tests(sample);
    assert_eq!(f.pages, Some(12));
    assert_eq!(f.title.as_deref(), Some("My Doc: a story"));
    assert_eq!(f.author.as_deref(), Some("Jane"));
    assert_eq!(pdf::parse_info_for_tests("Pages: x\n"), Default::default());
}
