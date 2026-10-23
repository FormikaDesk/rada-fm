//! Previews without poppler, through the system's own tools: `sips` and Quick Look on a Mac,
//! tried here on any Unix system with small programs of our own standing in for them (the
//! real ones are not on a Linux machine, and no test may depend on a window server).
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use image::{ImageFormat, Rgba, RgbaImage};

use rada_core::events::{CoreEvent, PreviewEvent};
use rada_core::fs::LocalFs;
use rada_core::preview::pdf;
use rada_core::preview::system::SystemRenderer;
use rada_core::preview::{ImageLimits, ImagePreview, ImageState, Limits, Preview};
use rada_core::testutil::*;
use rada_core::workers::PreviewWorker;

const LOCKED: &[u8] = include_bytes!("fixtures/locked.pdf");

fn make_pdf(title: &str, author: &str, pages: usize) -> Vec<u8> {
    let mut objs: Vec<String> = Vec::new();
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i * 2)).collect();
    objs.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    objs.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    ));
    for i in 0..pages {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents {} 0 R >>",
            4 + i * 2
        ));
        let s = "BT ET";
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
            other => panic!("expected an image preview, got {other:?}"),
        }
    }
}

/// A folder holding small programs named like the system's tools.
fn tools(sb: &Sandbox, name: &str, scripts: &[(&str, String)]) -> OsString {
    let d = sb.mkdir(name);
    for (n, body) in scripts {
        let p = d.join(n);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    d.into_os_string()
}

fn picture(sb: &Sandbox) -> PathBuf {
    let p = sb.path("fake-page.png");
    RgbaImage::from_fn(40, 60, |x, y| Rgba([(x * 6) as u8, (y * 4) as u8, 90, 255]))
        .save_with_format(&p, ImageFormat::Png)
        .unwrap();
    p
}

fn mac_limits(sb: &Sandbox, path: OsString) -> Limits {
    Limits {
        image: ImageLimits {
            pdf_cache: Some(sb.path("cache")),
            pdf_tools_path: Some(path),
            system_renderer: Some(SystemRenderer::Mac),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// `sips -s format png -Z <n> <in> --out <out>`: copies the prepared picture to `<out>` and
/// counts its calls.
fn good_sips(sb: &Sandbox) -> String {
    format!(
        "echo x >> '{}'\nfor last; do :; done\ncp '{}' \"$last\"",
        sb.path("sips-calls").display(),
        picture(sb).display()
    )
}

fn calls(sb: &Sandbox, name: &str) -> usize {
    std::fs::read_to_string(sb.path(name))
        .map(|t| t.lines().count())
        .unwrap_or(0)
}

#[test]
fn a_pdf_is_drawn_by_sips_with_its_facts_read_from_the_file() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("Annual report", "Ada", 3)).unwrap();
    let t = tools(&sb, "bin", &[("sips", good_sips(&sb))]);
    let (w, rx) = worker();
    w.request(p.clone(), 1, mac_limits(&sb, t.clone()));
    let i = settled(&rx, 1);
    assert!(
        matches!(i.state, ImageState::Ready(_)),
        "{:?} {:?}",
        i.state,
        i.note
    );
    assert_eq!(i.info.format, "PDF");
    let d = i.info.doc.expect("facts");
    assert_eq!(d.pages, Some(3));
    assert_eq!(d.title.as_deref(), Some("Annual report"));
    assert_eq!(d.author.as_deref(), Some("Ada"));
    assert_eq!(calls(&sb, "sips-calls"), 1);

    // The second time it comes from the cache: the tool is not run again.
    w.request(p, 2, mac_limits(&sb, t));
    let i = settled(&rx, 2);
    assert!(matches!(i.state, ImageState::Ready(_)));
    assert_eq!(calls(&sb, "sips-calls"), 1, "served from the cache");
    assert_eq!(std::fs::read_dir(sb.path("cache")).unwrap().count(), 1);
}

#[test]
fn sips_gets_the_page_size_and_the_paths_it_needs() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("T", "A", 1)).unwrap();
    let log = sb.path("args");
    let body = format!(
        "echo \"$@\" > '{}'\nfor last; do :; done\ncp '{}' \"$last\"",
        log.display(),
        picture(&sb).display()
    );
    let t = tools(&sb, "bin", &[("sips", body)]);
    let (w, rx) = worker();
    w.request(p.clone(), 1, mac_limits(&sb, t));
    settled(&rx, 1);
    let args = std::fs::read_to_string(log).unwrap();
    assert!(args.starts_with("-s format png -Z 1600 "), "{args}");
    assert!(args.contains(&p.display().to_string()), "{args}");
    assert!(args.trim_end().contains("--out"), "{args}");
}

#[test]
fn a_password_protected_pdf_says_so_and_sips_is_never_run() {
    let sb = Sandbox::new();
    let p = sb.path("locked.pdf");
    std::fs::write(&p, LOCKED).unwrap();
    let t = tools(&sb, "bin", &[("sips", good_sips(&sb))]);
    let (w, rx) = worker();
    w.request(p, 1, mac_limits(&sb, t));
    let i = settled(&rx, 1);
    let ImageState::Failed(m) = i.state else {
        panic!("{:?}", i.state)
    };
    assert!(m.contains("password"), "{m}");
    assert_eq!(calls(&sb, "sips-calls"), 0);
}

#[test]
fn a_sips_that_fails_or_is_too_slow_is_reported_not_waited_for() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("T", "A", 1)).unwrap();

    let t = tools(
        &sb,
        "bad",
        &[(
            "sips",
            "echo 'Error 4: no valid image data' >&2\nexit 1".into(),
        )],
    );
    let (w, rx) = worker();
    w.request(p.clone(), 1, mac_limits(&sb, t));
    let ImageState::Failed(m) = settled(&rx, 1).state else {
        panic!()
    };
    assert!(m.contains("no valid image data"), "{m}");

    let t = tools(&sb, "slow", &[("sips", "sleep 20".into())]);
    let mut l = mac_limits(&sb, t);
    l.image.pdf_timeout = Duration::from_millis(300);
    let started = Instant::now();
    w.request(p.clone(), 2, l);
    let ImageState::Failed(m) = settled(&rx, 2).state else {
        panic!()
    };
    assert!(m.contains("took more than"), "{m}");
    assert!(started.elapsed() < Duration::from_secs(10));
    // Nothing half-made is left in the cache.
    let left: Vec<_> = std::fs::read_dir(sb.path("cache"))
        .map(|rd| rd.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn poppler_wins_when_it_is_there() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("T", "A", 1)).unwrap();
    // A pdftoppm that writes the file it is told to, and a sips that must not be called.
    let body = format!(
        "for last; do :; done\ncp '{}' \"$last.png\"",
        picture(&sb).display()
    );
    let t = tools(&sb, "bin", &[("pdftoppm", body), ("sips", good_sips(&sb))]);
    let (w, rx) = worker();
    w.request(p, 1, mac_limits(&sb, t));
    let i = settled(&rx, 1);
    assert!(matches!(i.state, ImageState::Ready(_)), "{:?}", i.state);
    assert_eq!(calls(&sb, "sips-calls"), 0);
}

// ------------------------------------------------------------------------ thumbnails

/// `qlmanage -t -s <n> -o <dir> <file>`: writes `<dir>/<file name>.png`.
fn good_qlmanage(sb: &Sandbox) -> String {
    format!(
        "echo x >> '{}'\ndir=''; file=''\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in -o) dir=\"$2\"; shift;; -t) ;; -s) shift;; *) file=\"$1\";; esac\n  shift\ndone\ncp '{}' \"$dir/$(basename \"$file\").png\"",
        sb.path("ql-calls").display(),
        picture(sb).display()
    )
}

fn zip_like(sb: &Sandbox, name: &str) -> PathBuf {
    // Starts like a zip, as every Office file does.
    let p = sb.path(name);
    let mut b = b"PK\x03\x04".to_vec();
    b.extend(vec![0u8; 200]);
    std::fs::write(&p, b).unwrap();
    p
}

#[test]
fn an_office_document_gets_the_thumbnail_quick_look_makes() {
    let sb = Sandbox::new();
    let p = zip_like(&sb, "Budget.XLSX");
    let t = tools(&sb, "bin", &[("qlmanage", good_qlmanage(&sb))]);
    let (w, rx) = worker();
    w.request(p, 1, mac_limits(&sb, t));
    let i = settled(&rx, 1);
    assert!(matches!(i.state, ImageState::Ready(_)), "{:?}", i.state);
    // It says what the file is, not that it is a zip.
    assert_eq!(i.info.format, "Excel spreadsheet");
    assert_eq!(calls(&sb, "ql-calls"), 1);
}

#[test]
fn pages_numbers_keynote_video_and_rtf_go_the_same_way() {
    let sb = Sandbox::new();
    let t = tools(&sb, "bin", &[("qlmanage", good_qlmanage(&sb))]);
    let (w, rx) = worker();
    for (n, (name, kind)) in [
        ("letter.pages", "Pages document"),
        ("sheet.numbers", "Numbers spreadsheet"),
        ("talk.key", "Keynote presentation"),
        ("clip.mov", "Video"),
        ("note.rtf", "Rich text document"),
    ]
    .into_iter()
    .enumerate()
    {
        let p = zip_like(&sb, name);
        w.request(p, n as u64 + 1, mac_limits(&sb, t.clone()));
        let i = settled(&rx, n as u64 + 1);
        assert!(
            matches!(i.state, ImageState::Ready(_)),
            "{name}: {:?}",
            i.state
        );
        assert_eq!(i.info.format, kind, "{name}");
    }
    assert_eq!(calls(&sb, "ql-calls"), 5);
}

#[test]
fn quick_look_with_nothing_to_show_is_said_plainly() {
    let sb = Sandbox::new();
    let p = zip_like(&sb, "odd.docx");
    // It exits 0 and writes nothing, as the real one does for a type it does not know.
    let t = tools(
        &sb,
        "bin",
        &[("qlmanage", "echo 'No thumbnail created'".into())],
    );
    let (w, rx) = worker();
    w.request(p, 1, mac_limits(&sb, t));
    let ImageState::Failed(m) = settled(&rx, 1).state else {
        panic!()
    };
    assert!(m.contains("no preview"), "{m}");
}

#[test]
fn office_file_without_thumbnail_is_a_card() {
    let sb = Sandbox::new();
    let p = sb.path("a.docx");
    // A real, tiny zip.
    {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        z.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        z.write_all(b"<w/>").unwrap();
        z.finish().unwrap();
    }
    let (w, rx) = worker();
    let mut l = mac_limits(&sb, sb.mkdir("none").into_os_string());
    l.image.system_renderer = None;
    w.request(p, 1, l);
    match next_for(&rx, 1, 10).preview {
        Preview::Archive(_) => {}
        other => panic!("expected the archive summary, got {other:?}"),
    }
}

#[test]
fn a_picture_a_text_file_and_a_pdf_never_go_to_the_thumbnailer() {
    let sb = Sandbox::new();
    let t = tools(
        &sb,
        "bin",
        &[("qlmanage", good_qlmanage(&sb)), ("sips", good_sips(&sb))],
    );
    let (w, rx) = worker();
    let png = picture(&sb);
    w.request(png, 1, mac_limits(&sb, t.clone()));
    let i = settled(&rx, 1);
    assert_eq!(i.info.format, "PNG");
    let txt = sb.path("a.txt");
    std::fs::write(&txt, "hello").unwrap();
    w.request(txt, 2, mac_limits(&sb, t));
    assert!(matches!(next_for(&rx, 2, 10).preview, Preview::Text(_)));
    assert_eq!(calls(&sb, "ql-calls"), 0);
}

// ---------------------------------------------------------------------- facts and hints

#[test]
fn the_facts_of_a_pdf_are_read_without_any_tool() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("Title", "Author", 7)).unwrap();
    let f = pdf::facts_in_process(&p).unwrap();
    assert_eq!(
        (f.pages, f.title.as_deref(), f.author.as_deref()),
        (Some(7), Some("Title"), Some("Author"))
    );

    let locked = sb.path("l.pdf");
    std::fs::write(&locked, LOCKED).unwrap();
    assert_eq!(
        pdf::facts_in_process(&locked),
        Err(pdf::PdfProblem::Password)
    );

    let junk = sb.path("j.pdf");
    std::fs::write(&junk, b"%PDF-1.4 this is not really one").unwrap();
    assert!(matches!(
        pdf::facts_in_process(&junk),
        Err(pdf::PdfProblem::Damaged(_))
    ));
}

#[test]
fn the_install_hint_names_the_command_of_the_distribution() {
    let hint = |os: &str| pdf::install_hint_for(Some(os));
    assert!(hint("ID=arch\n").contains("sudo pacman -S poppler"));
    assert!(hint("ID=manjaro\nID_LIKE=arch\n").contains("pacman"));
    assert!(hint("ID=ubuntu\nID_LIKE=debian\n").contains("sudo apt install poppler-utils"));
    assert!(hint("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n").contains("apt install"));
    assert!(hint("ID=fedora\n").contains("sudo dnf install poppler-utils"));
    assert!(hint("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n").contains("zypper"));
    assert!(hint("ID=alpine\n").contains("apk add poppler-utils"));
    assert!(hint("ID=void\n").contains("xbps-install"));
    assert!(hint("ID=nixos\n").contains("nix-env"));
    // An unknown system, or none, gets the general sentence with the usual commands.
    assert_eq!(hint("ID=plan9\n"), pdf::INSTALL_HINT);
    assert_eq!(pdf::install_hint_for(None), pdf::INSTALL_HINT);
}

#[test]
fn without_poppler_or_a_system_renderer_the_hint_is_for_this_distribution() {
    let sb = Sandbox::new();
    let p = sb.path("a.pdf");
    std::fs::write(&p, make_pdf("T", "A", 1)).unwrap();
    let mut l = mac_limits(&sb, sb.mkdir("empty").into_os_string());
    l.image.system_renderer = None;
    let (w, rx) = worker();
    w.request(p, 1, l);
    let i = settled(&rx, 1);
    assert!(matches!(i.state, ImageState::Failed(_)));
    let note = i.note.expect("a hint");
    assert!(note.contains("install poppler"), "{note}");
    assert_eq!(i.info.format, "PDF");
}
