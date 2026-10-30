//! Text that does not come from rada (the label of a disk, the answer of a program, a name
//! typed by someone else) is shown with its escapes escaped. The harness fails any frame that
//! has a control character in a cell (`assert_no_control`), so these tests only have to bring
//! such text on screen; a few of them also look at what it was turned into.
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::*;
use rada_core::events::{CoreEvent, JobEvent};
use rada_core::fs::LocalFs;
use rada_core::journal::{EntryStatus, JournalEntry, UndoState};
use rada_core::ops::{OpKind, Recovered, RunStatus};
use rada_core::platform::{Unmounted, Volume, VolumeKind};
use rada_core::testplatform::{CallLog, DevicesSpec, WithDevices};
use rada_core::testutil::*;
use rada_core::update::{Notice, UpdateEvent};
use rada_tui::app::Modal;
use rada_tui::hits::Target;

/// An escape, a colour, a write to the clipboard (OSC 52), a link (OSC 8), a line break, a
/// carriage return, a C1 control, DEL and a right-to-left override.
const NASTY: &str = "a\x1b[31mred\x1b]52;c;ZXZpbA==\x07\x1b]8;;http://example.invalid\x1b\\link\nline\rback\u{85}\u{7f}\u{202e}rtl";

fn volume(mount: &Path, label: &str) -> Volume {
    Volume {
        mount_point: mount.to_path_buf(),
        label: Some(label.into()),
        fs_type: "ext4".into(),
        device: "/dev/sdz1".into(),
        kind: VolumeKind::Removable,
        drive_letter: None,
        total: Some(1 << 30),
        available: Some(1 << 29),
        read_only: false,
        responsive: true,
    }
}

/// A sidebar with a disk and a phone called `label`, and a stick that is not mounted, called
/// `label` too; mounting anything answers `mount_error`.
fn scene(label: &str, mount_error: Option<String>) -> H {
    let sb = Sandbox::new();
    let gvfs = sb.mkdir("gvfs");
    let disk = sb.mkdir("disk");
    sb.write("work/a.txt", "hello");
    let start = sb.path("work");
    let platform = Arc::new(WithDevices::new(
        sb.platform(),
        DevicesSpec {
            root: gvfs,
            volumes: vec![volume(&disk, label)],
            unmounted: vec![Unmounted {
                id: "/dev/sdb1".into(),
                name: label.into(),
                kind: VolumeKind::Removable,
                size: None,
            }],
            mount_error,
        },
        CallLog::default(),
    ));
    let mut h = H::with_system(sb, start, 150, 34, (Arc::new(LocalFs), platform), |_| {});
    h.wait("the devices", |a| {
        !a.devices.volumes.is_empty() && !a.devices.unmounted.is_empty()
    });
    h
}

fn said(h: &H) -> Vec<String> {
    h.app
        .shown_notices()
        .iter()
        .map(|n| n.text.clone())
        .collect()
}

#[test]
fn the_label_of_a_disk_and_the_name_of_a_device_are_shown_escaped() {
    let mut h = scene(NASTY, None);
    let screen = h.screen();
    // The sidebar is narrow: the start and the end of the label are what is left.
    assert!(screen.contains("a\\e[31mr"), "{screen}");
    assert!(screen.contains("u{202e}rtl"), "{screen}");
}

#[test]
fn the_answer_of_a_program_that_could_not_mount_is_shown_escaped() {
    let mut h = scene("Canon", Some(NASTY.into()));
    h.click(&Target::Place(PathBuf::from("/dev/sdb1")));
    h.wait("the answer", |a| {
        a.shown_notices().iter().any(|n| n.text.contains("link"))
    });
    for text in said(&h) {
        assert!(
            !text.chars().any(rada_core::display::is_dangerous),
            "{text:?}"
        );
    }
    let screen = h.screen();
    assert!(screen.contains("\\e[31mred"), "{screen}");
}

/// Whatever is on the screen, an escape that was not escaped would have left its tail
/// (`[31m`, `]52;`, `]8;;`) without the `\e` before it, because the terminal library drops the
/// escape itself.
fn assert_escaped(screen: &str) {
    for tail in ["[31m", "]52;c;", "]8;;"] {
        for (at, _) in screen.match_indices(tail) {
            assert!(
                screen[..at].ends_with("\\e"),
                "{tail} without its escape in:\n{screen}"
            );
        }
    }
}

#[test]
fn an_eject_that_failed_says_so_in_escaped_words() {
    let mut h = scene("disk", None);
    let mount = h.app.devices.volumes[0].mount_point.clone();
    h.app.on_core_event(CoreEvent::Ejected {
        mount,
        name: NASTY.into(),
        result: Err(NASTY.into()),
    });
    assert!(matches!(h.app.windows.modal, Some(Modal::Eject(_))));
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

#[test]
fn what_a_note_a_font_and_an_update_say_is_shown_escaped() {
    let mut h = scene("disk", None);
    h.app.on_core_event(CoreEvent::Note {
        text: NASTY.into(),
        error: true,
    });
    h.app
        .on_core_event(CoreEvent::FontInstalled(Err(NASTY.into())));
    h.app
        .on_core_event(CoreEvent::Update(UpdateEvent::CheckFailed(NASTY.into())));
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

#[test]
fn release_notes_and_the_reason_an_update_cannot_run_are_shown_escaped() {
    let mut h = scene("disk", None);
    h.app
        .on_core_event(CoreEvent::Update(UpdateEvent::Available {
            notice: Notice {
                version: "0.7.0".into(),
                tag: "v0.7.0".into(),
                prerelease: false,
                notes: format!("### Fixed\n- {NASTY}"),
            },
            how: Err(NASTY.into()),
        }));
    h.press("alt+u");
    assert!(matches!(h.app.windows.modal, Some(Modal::Update(_))));
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

#[test]
fn what_the_journal_says_of_an_operation_is_shown_escaped() {
    let mut h = scene("disk", None);
    h.keys("U");
    h.wait(
        "the history",
        |a| matches!(&a.windows.modal, Some(Modal::History(v)) if !v.loading),
    );
    let entry = JournalEntry {
        id: "1".into(),
        time: 0,
        kind: OpKind::Copy,
        title: NASTY.into(),
        reversible: false,
        totals: Default::default(),
        status: EntryStatus::Finished(RunStatus::Completed),
        done: 1,
        failed: 0,
        bytes: 0,
        undo_steps: 0,
        undo_state: UndoState::NotUndone,
        unrecorded_from: None,
        request: None,
        redoable: false,
        damaged: None,
    };
    h.app.on_core_event(CoreEvent::Job(JobEvent::History {
        job: 0,
        entries: vec![entry],
    }));
    assert!(matches!(h.app.windows.modal, Some(Modal::History(_))));
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

#[test]
fn what_a_recovery_could_not_do_is_shown_escaped() {
    let mut h = scene("disk", None);
    h.app.on_core_event(CoreEvent::Job(JobEvent::Recovered {
        items: vec![Recovered {
            id: "1".into(),
            title: NASTY.into(),
            removed_temps: Vec::new(),
            adopted: Vec::new(),
            notes: vec![NASTY.into()],
        }],
    }));
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

/// A file name can hold anything but a slash and a zero byte.
const NAME: &str =
    "a\x1b[31mred\x1b]52;c;ZXZpbA==\x07\x1b]8;;x\x1b\\link\nline\rback\u{85}\u{7f}\u{202e}rtl";

fn with_hostile_names() -> (H, PathBuf) {
    let sb = Sandbox::new();
    sb.write(format!("work/{NAME}"), "hello");
    let dir = sb.mkdir(format!("work/{NAME} dir"));
    sb.write(format!("work/{NAME} dir/inside.txt"), "x");
    let start = sb.path("work");
    (H::new(sb, start, 150, 34), dir)
}

#[test]
fn a_file_name_in_a_confirmation_a_prompt_and_the_properties_is_shown_escaped() {
    let (mut h, _) = with_hostile_names();
    h.cursor_to(&rada_core::display::name(std::ffi::OsStr::new(NAME)));
    for keys in ["D", "r", "i"] {
        h.keys(keys);
        let screen = h.screen();
        assert!(screen.contains("\\e[31m"), "{keys}:\n{screen}");
        assert_escaped(&screen);
        h.key(crossterm::event::KeyCode::Esc);
    }
}

#[test]
fn a_folder_name_in_a_tab_the_path_bar_and_a_notice_is_shown_escaped() {
    let (mut h, dir) = with_hostile_names();
    h.app.open_dir(dir.clone());
    h.wait("the folder", move |a| {
        a.tab.cwd == dir && !a.tab.is_loading()
    });
    h.app.new_tab();
    h.app.toast(
        rada_tui::app::ToastKind::Warn,
        format!("cannot read {}", rada_core::display::path(&h.app.tab.cwd)),
        5,
    );
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

#[test]
fn what_is_typed_in_the_address_bar_and_the_palette_is_shown_escaped() {
    for open in ["ctrl+l", "ctrl+p"] {
        let mut h = scene("disk", None);
        h.press(open);
        for c in NASTY.chars() {
            h.key(crossterm::event::KeyCode::Char(c));
        }
        h.key(crossterm::event::KeyCode::Left);
        let screen = h.screen();
        assert!(screen.contains("\\e[31m"), "{open}:\n{screen}");
        assert_escaped(&screen);
    }
}

/// Letters, no slash and no parenthesis (it goes in a PDF string).
const META: &str = "a\x1b[31mred\x1b]52;c;ZXZpbA==\x07\x1b]8;;x\x1blink\u{85}\u{202e}rtl";

fn preview_of(name: &str, bytes: Vec<u8>, w: u16) -> H {
    let sb = Sandbox::new();
    sb.write(format!("work/{name}"), bytes);
    let start = sb.path("work");
    let mut h = H::new(sb, start, w, 40);
    h.wait("the preview", |a| a.preview.name == name);
    h.pump(300);
    h
}

#[test]
fn the_interpreter_an_elf_names_is_shown_escaped() {
    let mut elf = vec![0u8; 400];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[16] = 3;
    elf[18] = 62;
    elf[32..40].copy_from_slice(&64u64.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());
    elf[64..68].copy_from_slice(&3u32.to_le_bytes());
    elf[72..80].copy_from_slice(&200u64.to_le_bytes());
    elf[96..104].copy_from_slice(&(META.len() as u64 + 1).to_le_bytes());
    elf[200..200 + META.len()].copy_from_slice(META.as_bytes());
    let mut h = preview_of("prog", elf, 150);
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

/// A stand-in for the programs that read a PDF (`pdfinfo`, `pdftoppm`, `pdftotext`), whose
/// answers hold an escape, a colour and a link.
fn hostile_pdf_tools(sb: &Sandbox) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt;
    let bin = sb.mkdir("tools");
    let nasty =
        "a\\033[31mred\\033]52;c;ZXZpbA==\\007\\033]8;;x\\033\\\\link\\302\\205\\342\\200\\256rtl";
    for (name, body) in [
        (
            "pdfinfo",
            format!("printf 'Title: {nasty}\\nAuthor: {nasty}\\nPages: 1\\n'"),
        ),
        ("pdftoppm", format!("printf '{nasty}\\n' >&2; exit 1")),
        ("pdftotext", format!("printf '{nasty}\\n'")),
    ] {
        let exe = bin.join(name);
        std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin.into_os_string()
}

#[test]
fn what_the_pdf_programs_print_is_shown_escaped() {
    let sb = Sandbox::new();
    sb.write("work/doc.pdf", sample_pdf("t", "a", 1, "page"));
    let tools = hostile_pdf_tools(&sb);
    let cache = sb.path("cache");
    let start = sb.path("work");
    let mut h = H::with_config(sb, start, 150, 40, move |c| {
        c.limits.image.pdf_tools_path = Some(tools);
        c.limits.image.pdf_cache = Some(cache);
    });
    h.wait("the preview", |a| a.preview.name == "doc.pdf");
    h.pump(800);
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
}

#[cfg(target_os = "linux")]
#[test]
fn the_name_of_a_tag_somebody_gave_a_file_is_shown_escaped() {
    use rada_core::platform::Tag;
    let sb = Sandbox::new();
    let file = sb.write("work/a.txt", "hello");
    if rada_core::platform::tags::xattr::set(&file, "user.rada-probe", b"1").is_err() {
        eprintln!("this filesystem keeps no user attributes: test skipped");
        return;
    }
    let platform = sb.platform();
    let tags = platform.tags().expect("tags on this platform");
    tags.write(&file, &[Tag::named(META)]).unwrap();
    let start = sb.path("work");
    let mut h = H::new(sb, start, 150, 34);
    h.wait("the tag", |a| {
        a.tab.listing.all().iter().any(|e| !e.tags.is_empty())
    });
    h.pump(300);
    let screen = h.screen();
    assert!(screen.contains("\\e[31m"), "{screen}");
    assert_escaped(&screen);
    // And the view of the files with that tag, which is a tab of its own.
    h.app.open_dir(rada_core::tagsearch::view_path(META));
    h.pump(500);
    h.app.new_tab();
    assert_escaped(&h.screen());
}
