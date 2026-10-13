//! The real renderers of macOS and Windows, on the machine that has them (CI runs these on
//! both). Anything that needs a window server, which a CI runner does not have, is marked
//! `#[ignore = "…"]` with the reason.

#![cfg(any(target_os = "macos", windows))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};

use rada_core::events::{CoreEvent, PreviewEvent};
use rada_core::fs::LocalFs;
use rada_core::preview::{ImageLimits, ImagePreview, ImageState, Limits, Preview};
use rada_core::testutil::*;
use rada_core::workers::PreviewWorker;

fn worker() -> (PreviewWorker, Receiver<CoreEvent>) {
    let (tx, rx) = unbounded();
    (PreviewWorker::spawn(Arc::new(LocalFs), tx), rx)
}

fn next_for(rx: &Receiver<CoreEvent>, generation: u64) -> PreviewEvent {
    let end = Instant::now() + Duration::from_secs(60);
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
        match next_for(rx, generation).preview {
            Preview::Image(i) if matches!(i.state, ImageState::Loading) => continue,
            Preview::Image(i) => return i,
            other => panic!("expected an image preview, got {other:?}"),
        }
    }
}

fn limits(sb: &Sandbox) -> Limits {
    Limits {
        image: ImageLimits {
            pdf_cache: Some(sb.path("cache")),
            // Whatever poppler the machine has, this is about the system's own renderer.
            pdf_tools_path: Some(sb.mkdir("no-tools").into_os_string()),
            pdf_timeout: Duration::from_secs(30),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The picture is a page: not blank, and in the proportions of the page (300×200).
fn assert_a_page(i: &ImagePreview) {
    let ImageState::Ready(img) = &i.state else {
        panic!("{:?} {:?}", i.state, i.note)
    };
    let (w, h) = (img.width() as f32, img.height() as f32);
    assert!(w > h, "{w}×{h}");
    assert!((w / h - 1.5).abs() < 0.05, "{w}×{h}");
    let rgb = img.to_rgb8();
    let inked = rgb
        .pixels()
        .filter(|p| p.0.iter().any(|c| *c < 200))
        .count();
    assert!(inked > 20, "the page is blank ({inked} dark pixels)");
}

#[cfg(target_os = "macos")]
#[test]
fn macos_draws_the_first_page_of_a_pdf_with_sips() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, sample_pdf("Annual report", "Ada", 2, "Findable words")).unwrap();
    let (w, rx) = worker();
    w.request(p, 1, limits(&sb));
    let i = settled(&rx, 1);
    assert_a_page(&i);
    let d = i.info.doc.expect("facts from the file");
    assert_eq!(
        (d.pages, d.title.as_deref()),
        (Some(2), Some("Annual report"))
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "Quick Look thumbnails need a window server, which the CI runners do not have"]
fn macos_draws_a_thumbnail_of_a_rich_text_file_with_quick_look() {
    let sb = Sandbox::new();
    let p = sb.path("letter.rtf");
    std::fs::write(&p, r"{\rtf1\ansi Hello from Quick Look \par}").unwrap();
    let (w, rx) = worker();
    w.request(p, 1, limits(&sb));
    let i = settled(&rx, 1);
    assert_eq!(i.info.format, "Rich text document");
    assert!(matches!(i.state, ImageState::Ready(_)), "{:?}", i.state);
}

#[cfg(windows)]
#[test]
fn windows_draws_the_first_page_of_a_pdf_with_its_own_renderer() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, sample_pdf("Annual report", "Ada", 2, "Findable words")).unwrap();
    let (w, rx) = worker();
    w.request(p, 1, limits(&sb));
    let i = settled(&rx, 1);
    assert_a_page(&i);
    let d = i.info.doc.expect("facts from the file");
    assert_eq!(
        (d.pages, d.title.as_deref()),
        (Some(2), Some("Annual report"))
    );
}

#[cfg(windows)]
#[test]
fn windows_says_when_a_pdf_needs_a_password() {
    let sb = Sandbox::new();
    let p = sb.path("locked.pdf");
    std::fs::write(&p, include_bytes!("fixtures/locked.pdf")).unwrap();
    let (w, rx) = worker();
    w.request(p, 1, limits(&sb));
    let i = settled(&rx, 1);
    let ImageState::Failed(m) = i.state else {
        panic!("{:?}", i.state)
    };
    assert!(m.contains("password"), "{m}");
}

#[cfg(windows)]
#[test]
fn windows_asks_the_shell_for_a_thumbnail_and_a_type_it_cannot_draw_fails_cleanly() {
    let sb = Sandbox::new();
    // No handler on a CI runner draws a Word file: the answer is a plain failure, not a hang
    // or a crash; with Office installed it is a picture.
    let p = sb.path("report.docx");
    let mut b = b"PK\x03\x04".to_vec();
    b.extend(vec![0u8; 400]);
    std::fs::write(&p, b).unwrap();
    let (w, rx) = worker();
    w.request(p, 1, limits(&sb));
    let i = settled(&rx, 1);
    assert_eq!(i.info.format, "Word document");
    match i.state {
        ImageState::Ready(_) => {}
        ImageState::Failed(m) => assert!(!m.is_empty()),
        other => panic!("{other:?}"),
    }
}
