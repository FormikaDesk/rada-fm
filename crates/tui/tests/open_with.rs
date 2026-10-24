//! "Open with…": the programs that can open the file, the system's choice first and those used
//! lately after it, filtered by typing, "Other program…" at the end with the free field; and
//! "always use this for this kind of file", which lives in rada's own state. The programs are
//! `.desktop` files these tests write in the sandbox's data folder; nothing is ever started
//! (the sandbox's opener writes down what it was asked).
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::*;
use crossterm::event::KeyCode;
use rada_core::platform::openwith::Prefs;
use rada_tui::app::{Modal, OpenRow};
use rada_tui::hits::Target;

struct World {
    h: H,
    dir: PathBuf,
    state: PathBuf,
    /// The system's own default-applications file, and what it said at the start.
    mimeapps: (PathBuf, String),
}

/// A folder with `a.txt`, `b.txt` and `data.bin`; three programs for text (Vim the system's
/// choice, in a terminal), one for pictures.
fn world() -> World {
    let (sb, dir) = sandbox_with_files();
    std::fs::write(dir.join("data.bin"), [0u8, 1, 2, 0, 255]).unwrap();
    let state = sb.dirs.rada_state();
    let apps = sb.dirs.data.join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    let mime = sb.dirs.data.join("mime");
    std::fs::create_dir_all(&mime).unwrap();
    std::fs::write(
        mime.join("globs2"),
        "50:text/plain:*.txt\n50:image/png:*.png\n",
    )
    .unwrap();
    std::fs::create_dir_all(&sb.dirs.config).unwrap();
    let mimeapps = sb.dirs.config.join("mimeapps.list");
    let said = "[Default Applications]\ntext/plain=vim.desktop;\n".to_string();
    std::fs::write(&mimeapps, &said).unwrap();
    // The program is a file that exists (the line is checked), and is never run.
    let prog = sb.root.join("never-run");
    std::fs::write(&prog, "#!/bin/sh\nexit 1\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for (id, name, extra) in [
        ("kate.desktop", "Kate", ""),
        ("vim.desktop", "Vim", "Terminal=true\n"),
        ("gedit.desktop", "Text Editor", ""),
    ] {
        std::fs::write(
            apps.join(id),
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec={} %f\nMimeType=text/plain;\n{extra}",
                prog.display()
            ),
        )
        .unwrap();
    }
    std::fs::write(
        apps.join("viewer.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Viewer\nExec={} %f\nMimeType=image/png;\n",
            prog.display()
        ),
    )
    .unwrap();
    World {
        h: H::new(sb, dir.clone(), 140, 36),
        dir,
        state,
        mimeapps: (mimeapps, said),
    }
}

fn names(h: &H) -> Vec<String> {
    let Some(Modal::OpenWith(v)) = &h.app.windows.modal else {
        panic!("no Open with window");
    };
    v.rows()
        .iter()
        .map(|r| match r {
            OpenRow::App(a) => a.name.clone(),
            OpenRow::Other => "Other program…".into(),
        })
        .collect()
}

fn open_window(h: &mut H, on: &str) {
    for _ in 0..10 {
        if h.cursor_name() == on {
            break;
        }
        h.keys("j");
    }
    assert_eq!(h.cursor_name(), on);
    h.press("alt+o");
    h.wait(
        "the programs",
        |a| matches!(&a.windows.modal, Some(Modal::OpenWith(v)) if !v.loading),
    );
}

/// Wait until the sandbox's opener has been asked `n` times (the launch is on a thread).
fn wait_opened(h: &mut H, n: usize) {
    let start = Instant::now();
    while h.opened().len() < n {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "nothing was opened: {:?}",
            h.opened()
        );
        h.pump(10);
    }
}

fn prefs_when(world_state: &Path, what: &str, mut ok: impl FnMut(&Prefs) -> bool) -> Prefs {
    let start = Instant::now();
    loop {
        let p = Prefs::load(world_state);
        if ok(&p) {
            return p;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{what}: {p:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_window_lists_the_programs_the_default_first_and_other_program_last() {
    let mut w = world();
    open_window(&mut w.h, "a.txt");
    assert_eq!(
        names(&w.h),
        ["Vim", "Kate", "Text Editor", "Other program…"]
    );
    let s = w.h.screen();
    assert!(s.contains("Open a.txt with"), "{s}");
    let vim = s.lines().find(|l| l.contains("Vim")).unwrap();
    assert!(
        vim.contains("default"),
        "the system's choice says so: {vim}"
    );
    assert!(
        vim.contains("in a terminal") || vim.contains("default"),
        "{vim}"
    );
    assert!(s.contains("Other program…"), "{s}");
    assert!(s.contains("Always use for .txt files"), "{s}");
    // Another kind of file: its own programs.
    w.h.key(KeyCode::Esc);
    std::fs::write(w.dir.join("pic.png"), "\u{89}PNG").unwrap();
    w.h.app.refresh();
    w.h.wait("pic", |a| {
        a.listing.all().iter().any(|e| e.display == "pic.png")
    });
    open_window(&mut w.h, "pic.png");
    assert_eq!(names(&w.h), ["Viewer", "Other program…"]);
}

#[test]
fn what_is_typed_filters_by_the_words_of_the_name() {
    let mut w = world();
    open_window(&mut w.h, "a.txt");
    w.h.keys("ka");
    assert_eq!(names(&w.h), ["Kate", "Other program…"]);
    w.h.press("ctrl+u");
    w.h.keys("text ed");
    assert_eq!(names(&w.h), ["Text Editor", "Other program…"]);
    w.h.keys("zzz");
    assert_eq!(names(&w.h), ["Other program…"]);
    assert!(w.h.screen().contains("No program matches"));
    w.h.key(KeyCode::Backspace);
    w.h.key(KeyCode::Backspace);
    w.h.key(KeyCode::Backspace);
    assert_eq!(names(&w.h), ["Text Editor", "Other program…"]);
}

#[test]
fn enter_starts_the_highlighted_program_and_it_is_remembered_as_used() {
    let mut w = world();
    open_window(&mut w.h, "a.txt");
    w.h.keys("kate");
    w.h.key(KeyCode::Enter);
    assert!(w.h.app.windows.modal.is_none());
    let file = w.dir.join("a.txt");
    wait_opened(&mut w.h, 1);
    assert_eq!(
        w.h.opened(),
        [format!("open_with_app kate.desktop {}", file.display())]
    );
    let p = prefs_when(&w.state, "kate remembered", |p| {
        p.recent == ["kate.desktop"]
    });
    assert!(p.always.is_empty(), "no 'always' unless asked");
    // Next time Kate comes right after the system's default.
    open_window(&mut w.h, "a.txt");
    assert_eq!(
        names(&w.h),
        ["Vim", "Kate", "Text Editor", "Other program…"]
    );
    // ... and a program used since is ahead of it.
    w.h.keys("text ed");
    w.h.key(KeyCode::Enter);
    prefs_when(&w.state, "editor remembered", |p| {
        p.recent.first().map(String::as_str) == Some("gedit.desktop")
    });
    open_window(&mut w.h, "a.txt");
    assert_eq!(
        names(&w.h),
        ["Vim", "Text Editor", "Kate", "Other program…"]
    );
}

#[test]
fn always_use_this_is_kept_in_rada_and_replaces_the_default_for_enter() {
    let mut w = world();
    open_window(&mut w.h, "a.txt");
    w.h.keys("kate");
    w.h.key(KeyCode::Tab);
    assert!(
        w.h.screen().contains("☑ Always use for .txt files"),
        "{}",
        w.h.screen()
    );
    w.h.key(KeyCode::Enter);
    let p = prefs_when(&w.state, "always saved", |p| !p.always.is_empty());
    assert_eq!(
        p.always.get("text/plain").map(String::as_str),
        Some("kate.desktop")
    );
    // The system's own lists are as they were.
    assert_eq!(
        std::fs::read_to_string(&w.mimeapps.0).unwrap(),
        w.mimeapps.1,
        "the system's own choice is not touched"
    );
    // Enter on a text file now opens it with Kate, not with the system's choice.
    let before = w.h.opened().len();
    w.h.key(KeyCode::Enter);
    let file = w.dir.join("a.txt");
    wait_opened(&mut w.h, before + 1);
    assert_eq!(
        w.h.opened().last().unwrap(),
        &format!("open_with_app kate.desktop {}", file.display())
    );
    // The window shows it first, as "always"; unticking it and opening goes back to the system.
    open_window(&mut w.h, "a.txt");
    assert_eq!(names(&w.h)[0], "Kate");
    let s = w.h.screen();
    assert!(
        s.lines()
            .find(|l| l.contains("Kate"))
            .unwrap()
            .contains("always"),
        "{s}"
    );
    assert!(s.contains("☑"), "the box reflects it: {s}");
    w.h.key(KeyCode::Tab);
    assert!(w.h.screen().contains("☐ Always use for .txt files"));
    w.h.key(KeyCode::Enter);
    prefs_when(&w.state, "always removed", |p| p.always.is_empty());
    let n = w.h.opened().len();
    // (b.txt is the same kind of file.)
    w.h.keys("j");
    assert_eq!(w.h.cursor_name(), "b.txt");
    w.h.key(KeyCode::Enter);
    wait_opened(&mut w.h, n + 1);
    assert!(
        w.h.opened().last().unwrap().starts_with("open "),
        "{:?}",
        w.h.opened()
    );
}

#[test]
fn other_program_is_the_free_field_of_before_with_what_was_typed() {
    let mut w = world();
    open_window(&mut w.h, "a.txt");
    w.h.keys("vlc --fullscreen");
    assert_eq!(names(&w.h), ["Other program…"]);
    w.h.key(KeyCode::Enter);
    let Some(Modal::Input(iv)) = &w.h.app.windows.modal else {
        panic!("the field: {}", w.h.screen());
    };
    assert_eq!(iv.text, "vlc --fullscreen");
    assert!(w.h.screen().contains("Program, with options"));
    w.h.key(KeyCode::Enter);
    let file = w.dir.join("a.txt");
    wait_opened(&mut w.h, 1);
    assert_eq!(
        w.h.opened(),
        [format!("open_with vlc --fullscreen {}", file.display())]
    );
    // From the list by the arrow keys, with nothing typed.
    let mut w2 = world();
    open_window(&mut w2.h, "a.txt");
    for _ in 0..3 {
        w2.h.key(KeyCode::Down);
    }
    w2.h.key(KeyCode::Down);
    w2.h.key(KeyCode::Enter);
    assert!(matches!(w2.h.app.windows.modal, Some(Modal::Input(_))));
}

#[test]
fn a_kind_nobody_opens_still_offers_the_free_field() {
    let mut w = world();
    open_window(&mut w.h, "data.bin");
    assert_eq!(names(&w.h), ["Other program…"]);
    let s = w.h.screen();
    assert!(s.contains("No installed program says it opens"), "{s}");
    // A folder is not opened with a program.
    w.h.key(KeyCode::Esc);
    for _ in 0..10 {
        if w.h.cursor_name() == "sub" {
            break;
        }
        w.h.keys("k");
    }
    assert_eq!(w.h.cursor_name(), "sub");
    w.h.press("alt+o");
    assert!(w.h.app.windows.modal.is_none());
}

#[test]
fn a_click_on_a_program_starts_it() {
    let mut w = world();
    open_window(&mut w.h, "a.txt");
    w.h.click(&Target::OpenWithRow(2));
    let file = w.dir.join("a.txt");
    wait_opened(&mut w.h, 1);
    assert_eq!(
        w.h.opened(),
        [format!("open_with_app gedit.desktop {}", file.display())]
    );
    // The box can be clicked too.
    open_window(&mut w.h, "a.txt");
    w.h.click(&Target::OpenWithAlways);
    assert!(w.h.screen().contains("☑"));
}
