//! Documents drawn by the system's thumbnailer show up in the details pane like pictures.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;

use common::*;
use rada_core::preview::system::SystemRenderer;
use rada_core::testutil::*;
use rada_tui::app::ImageStatus;

#[test]
fn a_word_document_is_shown_as_the_picture_quick_look_makes() {
    let sb = Sandbox::new();
    sb.write("d/notes.txt", "plain");
    let doc = sb.write("d/report.docx", {
        let mut b = b"PK\x03\x04".to_vec();
        b.extend(vec![0u8; 300]);
        b
    });
    let _ = doc;
    // A picture for the stand-in Quick Look to hand back.
    let png = sb.path("page.png");
    image::RgbaImage::from_fn(30, 40, |x, y| {
        image::Rgba([(x * 8) as u8, (y * 6) as u8, 60, 255])
    })
    .save_with_format(&png, image::ImageFormat::Png)
    .unwrap();
    let bin = sb.mkdir("bin");
    let ql = bin.join("qlmanage");
    std::fs::write(
        &ql,
        format!(
            "#!/bin/sh\ndir=''; file=''\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in -o) dir=\"$2\"; shift;; -t) ;; -s) shift;; *) file=\"$1\";; esac\n  shift\ndone\ncp '{}' \"$dir/$(basename \"$file\").png\"\n",
            png.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&ql, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cache = sb.path("cache");
    let dir = sb.path("d");
    let mut h = H::with_config(sb, dir, 140, 36, move |c| {
        c.limits.image.system_renderer = Some(SystemRenderer::Mac);
        c.limits.image.pdf_tools_path = Some(bin.into_os_string());
        c.limits.image.pdf_cache = Some(cache);
    });
    assert_eq!(h.names(), ["notes.txt", "report.docx"]);
    h.keys("j");
    assert_eq!(h.cursor_name(), "report.docx");
    h.wait("the thumbnail", |a| {
        a.preview.name == "report.docx"
            && a.preview
                .image
                .as_ref()
                .is_some_and(|i| matches!(i.status, ImageStatus::Shown))
    });
    let s = h.screen();
    assert!(s.contains("Word document"), "{s}");
}

#[test]
fn without_a_thumbnailer_the_same_file_keeps_its_old_preview() {
    let sb = Sandbox::new();
    sb.write("d/report.docx", {
        let mut b = b"PK\x03\x04".to_vec();
        b.extend(vec![0u8; 300]);
        b
    });
    let dir = sb.path("d");
    let mut h = H::with_config(sb, dir, 140, 36, |c| {
        c.limits.image.system_renderer = None;
    });
    h.wait("a preview", |a| a.preview.name == "report.docx");
    assert!(h.app.preview.image.is_none());
}
