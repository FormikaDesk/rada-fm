//! The interface, driven headlessly: real workers, real (sandboxed) filesystem, a virtual
//! terminal. Keys go in, the screen and the disk are inspected.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rada_core::testutil::*;
use rada_tui::app::Modal;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn lists_sorted_hides_dotfiles_and_previews_the_selection() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 120, 30);
    assert_eq!(
        h.names(),
        ["sub", "a.txt", "b.txt"],
        "folders first, hidden files not shown"
    );
    h.keys(".");
    assert_eq!(h.names(), ["sub", ".hidden", "a.txt", "b.txt"]);
    h.keys(".");

    h.keys("jj"); // b.txt
    h.wait("preview of b.txt", |a| a.preview.name == "b.txt");
    let s = h.screen();
    assert!(s.contains("second line"), "{s}");
    assert_eq!(h.app.visible.len(), 3, "{s}");
}

#[test]
fn sorting_changes_the_screen_immediately_without_any_io() {
    let (sb, dir) = sandbox_with_files();
    sb.write("proj.v1/zzz.bin", vec![0u8; 5000]);
    let mut h = H::new(sb, dir, 120, 30);
    assert_eq!(h.names(), ["sub", "a.txt", "b.txt", "zzz.bin"]);
    h.keys("s"); // by size, no worker involved: the new order is there right now
    assert_eq!(h.names(), ["sub", "a.txt", "b.txt", "zzz.bin"]);
    h.keys("S");
    assert_eq!(
        h.names()[1],
        "zzz.bin",
        "reverse by size puts the biggest file first: {:?}",
        h.names()
    );
    assert!(h.screen().contains("size ↓"));
}

#[test]
fn navigating_into_a_folder_and_back_remembers_the_cursor() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.key(KeyCode::Enter); // into sub
    h.wait("sub loaded", |a| a.cwd.ends_with("sub") && !a.is_loading());
    assert_eq!(h.names(), ["inner.txt"]);
    h.keys("h");
    h.wait("back", |a| a.cwd == dir && !a.is_loading());
    assert_eq!(
        h.app.current().unwrap().display,
        "sub",
        "cursor returns to the folder we came from"
    );
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "uses POSIX permissions or file names that Windows rejects; Windows support is in development"
)]
fn entering_an_unreadable_folder_shows_the_real_reason_and_stays_put() {
    if is_root() {
        return;
    }
    let (sb, dir) = sandbox_with_files();
    sb.mkdir("proj.v1/locked");
    chmod(&sb.path("proj.v1/locked"), 0o000);
    let mut h = H::new(sb, dir.clone(), 120, 30);
    assert_eq!(h.names()[0], "locked");
    h.key(KeyCode::Enter);
    h.wait("an error toast", |a| a.toast.is_some());
    chmod(&dir.join("locked"), 0o755);
    let s = h.screen();
    // The message is wrapped in a box, so look for its pieces rather than one line.
    let flat = s
        .replace(['│', '╭', '╮', '╰', '╯', '─'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert!(
        flat.contains("permission denied") && flat.contains("locked"),
        "{s}"
    );
    assert_eq!(h.app.cwd, dir);
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "live-update timing is validated on Linux only; Windows and macOS file watching is in development"
)]
fn a_file_created_from_outside_appears_without_pressing_anything() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    std::thread::sleep(Duration::from_millis(250));
    std::fs::write(dir.join("ext_created.txt"), "hi").unwrap();
    h.wait("live update", |a| {
        (0..a.visible.len()).any(|i| {
            a.entry_at(i)
                .is_some_and(|e| e.display == "ext_created.txt")
        })
    });
}

#[test]
fn copy_shows_a_plan_then_runs_then_u_undoes_it() {
    let (sb, dir) = sandbox_with_files();
    let dest = sb.mkdir("elsewhere.d");
    let mut h = H::new(sb, dir.clone(), 130, 36);
    h.keys("j"); // a.txt
    h.keys("y");
    h.app.open_dir(dest.clone());
    h.wait("destination", |a| a.cwd == dest && !a.is_loading());
    h.keys("p");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(s.contains("Copy 1 item"), "{s}");
    // The totals stack a number over its label.
    let lines: Vec<&str> = s.lines().collect();
    let i = lines
        .iter()
        .position(|l| l.contains(" file "))
        .unwrap_or_else(|| panic!("no 'file' label: {s}"));
    assert!(
        lines[i - 1].trim_start_matches([' ', '│']).starts_with('1'),
        "{s}"
    );
    assert!(s.contains("What will happen"), "{s}");
    assert!(
        !dest.join("a.txt").exists(),
        "nothing happens before the plan is confirmed"
    );

    run_plan_and_wait(&mut h);
    assert_eq!(std::fs::read_to_string(dest.join("a.txt")).unwrap(), "ay");
    h.wait("toast", |a| a.toast.is_some());
    let toast = h.app.toast.as_ref().unwrap().text.clone();
    assert_eq!(toast, "Copied 1 file — press u to undo");
    assert!(h.screen().contains("Copied 1 file"));

    h.keys("u");
    plan_modal(&mut h);
    assert!(h.screen().contains("Undo: Copy"));
    run_plan_and_wait(&mut h);
    assert!(!dest.join("a.txt").exists(), "undo removed the copy");
    assert!(dir.join("a.txt").exists(), "the original is untouched");

    h.keys("u");
    h.wait("nothing to undo", |a| {
        a.toast
            .as_ref()
            .is_some_and(|t| t.text.contains("nothing to undo"))
    });
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "uses the system Trash, implemented for Linux only; Windows and macOS are in development"
)]
fn trash_then_undo_restores_the_file() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.keys("jd"); // a.txt
    plan_modal(&mut h);
    assert!(h.screen().contains("Move 1 item to the trash"));
    run_plan_and_wait(&mut h);
    assert!(!dir.join("a.txt").exists());
    h.keys("u");
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "ay");
}

#[test]
fn permanent_delete_needs_the_word_yes() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.keys("jD");
    plan_modal(&mut h);
    let s = h.screen();
    assert!(s.contains("cannot be undone") && s.contains("yes"), "{s}");
    h.key(KeyCode::Enter);
    assert!(dir.join("a.txt").exists(), "Enter alone does nothing");
    h.keys("ye");
    h.key(KeyCode::Enter);
    assert!(
        dir.join("a.txt").exists(),
        "an incomplete confirmation does nothing"
    );
    h.keys("s");
    run_plan_and_wait(&mut h);
    assert!(!dir.join("a.txt").exists());
    h.keys("u");
    h.wait("nothing to undo", |a| {
        a.toast
            .as_ref()
            .is_some_and(|t| t.text.contains("nothing to undo"))
    });
}

#[test]
fn rename_asks_for_a_plan_and_bulk_rename_previews_names() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.keys("jr");
    for _ in 0..5 {
        h.key(KeyCode::Backspace);
    }
    h.keys("renamed.v2.txt");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("renamed.v2.txt").exists() && !dir.join("a.txt").exists());

    h.app
        .on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)); // mark all
    h.keys("R");
    let s = h.screen();
    assert!(
        s.contains("Bulk rename") && s.contains("→"),
        "live preview of the new names: {s}"
    );
}

#[test]
#[cfg_attr(
    not(unix),
    ignore = "uses POSIX permissions or file names that Windows rejects; Windows support is in development"
)]
fn the_screen_never_contains_control_characters_from_hostile_names_at_any_size() {
    let sb = Sandbox::new();
    let evil = [
        "line\nbreak.txt",
        "esc\u{1b}[2Jclear.txt",
        "osc\u{1b}]0;pwned\u{7}.txt",
        "bidi\u{202e}gpj.exe",
        "tab\there",
        "emoji 👨‍👩‍👧‍👦 日本語 العربية",
        "x".repeat(250).as_str().to_owned().leak(),
    ];
    for n in evil {
        sb.write(Path::new("evil").join(n), "content \u{1b}[31mred\nnext");
    }
    #[cfg(unix)]
    {
        std::fs::write(
            sb.path("evil").join(os_from_bytes(b"bad\xff\xfebytes")),
            "x",
        )
        .unwrap();
        sb.symlink("nowhere\nthere", "evil/dangling");
    }
    let dir = sb.path("evil");
    let mut h = H::new(sb, dir, 100, 30);
    h.keys("j");
    h.wait("preview", |a| a.preview.content.is_some());
    for (w, hh) in [
        (100, 30),
        (60, 20),
        (40, 12),
        (24, 7),
        (200, 60),
        (91, 8),
        (20, 6),
    ] {
        h.term = Terminal::new(TestBackend::new(w, hh)).unwrap();
        let s = h.screen();
        let bad: Vec<char> = s.chars().filter(|c| c.is_control() && *c != '\n').collect();
        assert!(
            bad.is_empty(),
            "{w}x{hh}: control characters on screen: {bad:?}"
        );
        assert!(!s.contains('\u{202e}'), "bidi override must be escaped");
    }
}

#[test]
fn history_lists_operations_and_a_second_operation_is_refused_while_busy() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir.clone(), 120, 30);
    h.keys("n");
    h.keys("fresh");
    h.key(KeyCode::Enter);
    plan_modal(&mut h);
    run_plan_and_wait(&mut h);
    assert!(dir.join("fresh").is_dir());
    h.keys("U");
    h.wait(
        "history",
        |a| matches!(&a.modal, Some(Modal::History(hv)) if !hv.loading),
    );
    let s = h.screen();
    assert!(
        s.contains("History") && s.contains("Create folder fresh") && s.contains("can undo"),
        "{s}"
    );
}

// ------------------------------------------------------------------------ images and binary cards

fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 200, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A PNG with a valid signature and IHDR (correct CRC) claiming w x h, and a stub IDAT.
fn fake_huge_png(w: u32, h: u32) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(&ihdr);
    out.extend_from_slice(&crc32(&ihdr).to_be_bytes());
    let idat = [b'I', b'D', b'A', b'T', 0x78, 0x9c, 0x01, 0x00];
    out.extend_from_slice(&4u32.to_be_bytes());
    out.extend_from_slice(&idat);
    out.extend_from_slice(&crc32(&idat).to_be_bytes());
    out
}

fn is_block(c: char) -> bool {
    matches!(c, '▀' | '▄' | '█')
}

#[test]
fn an_image_is_drawn_with_its_facts_underneath() {
    let sb = Sandbox::new();
    sb.write("pics/a_first.txt", "x");
    sb.write("pics/photo.png", png_bytes(160, 100));
    let dir = sb.path("pics");
    let mut h = H::new(sb, dir, 130, 36);
    h.keys("j"); // photo.png
    h.wait("the picture", |a| {
        a.preview.name == "photo.png"
            && matches!(
                a.preview.image.as_ref().map(|i| &i.status),
                Some(rada_tui::app::ImageStatus::Shown)
            )
    });
    let mut s = h.screen();
    for _ in 0..100 {
        if s.chars().filter(|c| is_block(*c)).count() > 50 {
            break;
        }
        h.pump(50);
        s = h.screen();
    }
    assert!(
        s.chars().filter(|c| is_block(*c)).count() > 50,
        "the picture must be on screen:\n{s}"
    );
    assert!(s.contains("PNG · 160 × 100 px"), "{s}");
    assert!(s.contains("modified 20"), "{s}");
    assert!(
        !s.contains("half blocks") && !s.contains("kitty graphics"),
        "the protocol name is not shown in the preview: {s}"
    );
    assert_eq!(
        h.app.image_ui.as_ref().unwrap().protocol_name(),
        "half blocks"
    );
}

/// Waits for the picture of `photo.png` (made of blocks) to be on screen.
fn show_photo(h: &mut H) -> String {
    h.keys("j"); // photo.png
    h.wait("the picture", |a| {
        a.preview.name == "photo.png"
            && matches!(
                a.preview.image.as_ref().map(|i| &i.status),
                Some(rada_tui::app::ImageStatus::Shown)
            )
    });
    let mut s = h.screen();
    for _ in 0..100 {
        if s.chars().filter(|c| is_block(*c)).count() > 50 {
            break;
        }
        h.pump(50);
        s = h.screen();
    }
    s
}

#[test]
fn the_image_facts_follow_the_picture_directly() {
    let sb = Sandbox::new();
    sb.write("pics/a_first.txt", "x");
    sb.write("pics/photo.png", png_bytes(160, 100));
    let dir = sb.path("pics");
    let mut h = H::new(sb, dir, 150, 36);
    let s = show_photo(&mut h);
    let rows: Vec<&str> = s.lines().collect();
    let last_picture_row = rows
        .iter()
        .rposition(|l| l.chars().filter(|c| is_block(*c)).count() > 5)
        .expect("a picture");
    let facts_row = rows
        .iter()
        .position(|l| l.contains("PNG ·"))
        .expect("the facts");
    assert_eq!(
        facts_row,
        last_picture_row + 1,
        "no empty row between the picture and its facts:\n{s}"
    );
}

#[test]
fn the_image_facts_wrap_in_a_narrow_preview_instead_of_being_cut() {
    let sb = Sandbox::new();
    sb.write("pics/a_first.txt", "x");
    sb.write("pics/photo.png", png_bytes(1600, 1066));
    let dir = sb.path("pics");
    let mut h = H::new(sb, dir, 100, 36);
    let s = show_photo(&mut h);
    assert!(s.contains("1600 × 1066 px"), "{s}");
    for l in s
        .lines()
        .filter(|l| l.contains("px") || l.contains("modified"))
    {
        assert!(!l.contains('…'), "cut instead of wrapped: {l}");
    }
    assert!(s.contains("modified 20"), "{s}");
}

#[test]
fn a_huge_image_shows_a_clear_message_instead_of_a_picture() {
    let sb = Sandbox::new();
    // Header claims 12000x12000; no pixel data follows (it would be 700 MB).
    let png = fake_huge_png(12000, 12000);
    sb.write("big/huge.png", png);
    let dir = sb.path("big");
    let mut h = H::new(sb, dir, 130, 36);
    h.wait("the message", |a| {
        matches!(
            a.preview.image.as_ref().map(|i| &i.status),
            Some(rada_tui::app::ImageStatus::TooLarge(_))
        )
    });
    let s = h.screen();
    assert!(s.contains("image too large to preview"), "{s}");
    assert!(s.contains("144"), "{s}");
    assert!(
        s.contains("12000 × 12000 px"),
        "the facts are still shown: {s}"
    );
    assert_eq!(s.chars().filter(|c| is_block(*c)).count(), 0);
}

#[test]
fn with_images_off_only_the_facts_are_shown() {
    let sb = Sandbox::new();
    sb.write("pics/photo.png", png_bytes(40, 30));
    let dir = sb.path("pics");
    let mut h = H::with_images(sb, dir, 130, 36, None, Default::default());
    h.wait("facts", |a| a.preview.image.is_some());
    let s = h.screen();
    assert!(
        s.contains("PNG · 40 × 30 px") && s.contains("image rendering is off"),
        "{s}"
    );
}

#[test]
fn binary_files_show_a_card_and_the_hex_dump_only_on_request() {
    let sb = Sandbox::new();
    let mut elf = vec![0u8; 4096];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[16] = 3;
    elf[18] = 62;
    elf[200] = 0xff;
    sb.write("bin/tool", elf);
    chmod(&sb.path("bin/tool"), 0o755);
    let dir = sb.path("bin");
    let mut h = H::new(sb, dir, 130, 36);
    h.wait("card", |a| {
        matches!(
            a.preview.content,
            Some(rada_core::preview::Preview::Binary(_))
        )
    });
    let s = h.screen();
    assert!(s.contains("ELF"), "{s}");
    assert!(
        s.contains("x86-64") && s.contains("64-bit") && s.contains("little-endian"),
        "{s}"
    );
    assert!(
        s.contains("Size") && s.contains("Modified") && s.contains("Permissions"),
        "{s}"
    );
    #[cfg(unix)]
    assert!(s.contains("-rwxr-xr-x"), "{s}");
    assert!(!s.contains("00000000  7f 45"), "no hex by default: {s}");
    assert!(s.contains("show the hex dump"));

    h.keys("H");
    let s = h.screen();
    assert!(s.contains("00000000  7f 45 4c 46"), "hex on request: {s}");
    h.keys("H");
    assert!(
        !h.screen().contains("00000000  7f 45"),
        "H again goes back to the card"
    );
}

#[test]
fn the_ui_thread_never_waits_for_an_image_to_decode() {
    let sb = Sandbox::new();
    for i in 0..6u8 {
        let img = image::RgbImage::from_fn(5000, 4000, |x, y| {
            image::Rgb([(x / 20) as u8, (y / 16) as u8, i * 40])
        });
        let p = sb.path(format!("many/img_{i}.png"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        img.save(&p).unwrap();
    }
    let dir = sb.path("many");
    let mut h = H::new(sb, dir, 130, 36);
    let mut worst = Duration::ZERO;
    for _ in 0..30 {
        let t = Instant::now();
        h.keys("j");
        let _ = h.screen(); // the frame that follows every key
        worst = worst.max(t.elapsed());
        h.keys("k");
        let _ = h.screen();
        std::thread::sleep(Duration::from_millis(5));
    }
    // Decoding a 20-megapixel PNG takes far longer than this, even in release builds.
    assert!(
        worst < Duration::from_millis(100),
        "a key + redraw took {worst:?} while images were decoding"
    );
}

/// A one-page PDF with a title and an author, written by hand.
fn tiny_pdf() -> Vec<u8> {
    let stream = "BT /F1 24 Tf 20 100 Td (Hello) Tj ET";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        "<< /Title (Quarterly numbers) /Author (Ada Lovelace) >>".to_string(),
    ];
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
            "trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{x}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    out
}

#[test]
fn a_pdf_shows_pages_title_and_author_under_its_first_page() {
    let tools = rada_core::preview::pdf::Tools::installed();
    if tools.pdftoppm.is_none() || tools.pdfinfo.is_none() {
        eprintln!("poppler is not installed: test skipped");
        return;
    }
    let sb = Sandbox::new();
    sb.write("docs/report.pdf", tiny_pdf());
    let dir = sb.path("docs");
    let mut h = H::new(sb, dir, 130, 36);
    h.wait("the page", |a| {
        matches!(
            a.preview.image.as_ref().map(|i| &i.status),
            Some(rada_tui::app::ImageStatus::Shown | rada_tui::app::ImageStatus::NoGraphics)
        )
    });
    let s = h.screen();
    assert!(s.contains("PDF · 1 page"), "{s}");
    assert!(s.contains("title: Quarterly numbers"), "{s}");
    assert!(s.contains("author: Ada Lovelace"), "{s}");
}
