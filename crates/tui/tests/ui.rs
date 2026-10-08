//! The interface, driven headlessly: real workers, real (sandboxed) filesystem, a virtual
//! terminal. Keys go in, the screen and the disk are inspected.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vela_core::fs::LocalFs;
use vela_core::model::SortSpec;
use vela_core::testutil::*;
use vela_tui::app::{App, Config, Modal};
use vela_tui::{IconSet, Services, Theme, ui};

struct H {
    app: App,
    term: Terminal<TestBackend>,
    _sb: Sandbox,
}

impl H {
    fn new(sb: Sandbox, start: PathBuf, w: u16, h: u16) -> H {
        H::with_images(
            sb,
            start,
            w,
            h,
            Some(vela_tui::ImageUi::halfblocks()),
            Default::default(),
        )
    }

    fn with_images(
        sb: Sandbox,
        start: PathBuf,
        w: u16,
        h: u16,
        images: Option<vela_tui::ImageUi>,
        limits: vela_core::preview::Limits,
    ) -> H {
        let svc = Services::start(Arc::new(LocalFs), sb.platform(), Some(sb.journal()));
        let cfg = Config {
            start_dir: start,
            icons: IconSet::Unicode,
            show_hidden: false,
            sort: SortSpec::default(),
            theme: Theme::vela(),
            image_mode: vela_tui::ImageMode::Halfblocks,
            limits,
            select: None,
            bookmarks: Vec::new(),
            demo: None,
        };
        let app = App::new(cfg, svc, images);
        let mut h = H {
            app,
            term: Terminal::new(TestBackend::new(w, h)).unwrap(),
            _sb: sb,
        };
        h.settle();
        h
    }

    /// Process worker events until `done` holds (or fail after a few seconds).
    fn wait(&mut self, what: &str, mut done: impl FnMut(&App) -> bool) {
        let rx = self.app.events();
        let resized = self.app.image_ui.as_ref().map(|u| u.results());
        let end = Instant::now() + Duration::from_secs(8);
        while !done(&self.app) {
            assert!(
                Instant::now() < end,
                "timed out waiting for: {what}\n{}",
                self.screen()
            );
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(20)) {
                self.app.on_core_event(ev);
            }
            // The picture is resized and encoded by its own thread once it has been drawn.
            if let Some(r) = &resized {
                while let Ok(done) = r.try_recv() {
                    self.app.on_image_resized(done);
                }
            }
            self.app.tick();
            let _ = self.screen(); // drawing is what asks for the resize
        }
    }

    /// Let workers and the encoder thread deliver for a moment, drawing as the real loop does.
    fn pump(&mut self, ms: u64) {
        let rx = self.app.events();
        let resized = self.app.image_ui.as_ref().map(|u| u.results());
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(10)) {
                self.app.on_core_event(ev);
            }
            if let Some(r) = &resized {
                while let Ok(done) = r.try_recv() {
                    self.app.on_image_resized(done);
                }
            }
            let _ = self.screen();
        }
    }

    fn settle(&mut self) {
        self.wait("initial load", |a| !a.is_loading());
    }

    fn key(&mut self, c: KeyCode) {
        self.app.on_key(KeyEvent::new(c, KeyModifiers::NONE));
    }

    fn keys(&mut self, s: &str) {
        for c in s.chars() {
            self.key(KeyCode::Char(c));
        }
    }

    fn screen(&mut self) -> String {
        self.term.draw(|f| ui::draw(f, &mut self.app)).unwrap();
        let buf = self.term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn names(&self) -> Vec<String> {
        (0..self.app.visible.len())
            .filter_map(|i| self.app.entry_at(i))
            .map(|e| e.display.clone())
            .collect()
    }

    fn modal_is_plan(&self) -> bool {
        matches!(self.app.modal, Some(Modal::Plan(_)))
    }
}

fn sandbox_with_files() -> (Sandbox, PathBuf) {
    let sb = Sandbox::new();
    sb.write("proj.v1/b.txt", "bee\nsecond line");
    sb.write("proj.v1/a.txt", "ay");
    sb.write("proj.v1/.hidden", "h");
    sb.write("proj.v1/sub/inner.txt", "inner");
    let dir = sb.path("proj.v1");
    (sb, dir)
}

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
    assert!(s.contains("3/3 items"), "position counter: {s}");
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

fn run_plan_and_wait(h: &mut H) {
    assert!(h.modal_is_plan(), "{}", h.screen());
    h.key(KeyCode::Enter);
    h.wait("operation to finish", |a| {
        a.running.is_none() && !matches!(a.modal, Some(Modal::Scanning { .. }))
    });
}

fn plan_modal(h: &mut H) {
    h.wait("a plan window", |a| matches!(a.modal, Some(Modal::Plan(_))));
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
    assert!(h.screen().contains("done"));

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
                Some(vela_tui::app::ImageStatus::Shown)
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
            Some(vela_tui::app::ImageStatus::TooLarge(_))
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
            Some(vela_core::preview::Preview::Binary(_))
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
