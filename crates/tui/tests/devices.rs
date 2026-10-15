//! Phones, cameras and sticks in the sidebar: the ones that are open are listed under their own
//! names, the ones that are plugged in and not mounted offer to be mounted (Enter, a click, the
//! menu), a phone or a stick can be ejected, and a mount that cannot be done says what to do.
//! The system is a stand-in: what is asked of it is written down, nothing real is opened.
#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::*;
use crossterm::event::KeyCode;
use rada_core::fs::LocalFs;
use rada_core::i18n::{Key, Lang, with_lang};
use rada_core::platform::{Unmounted, Volume, VolumeKind};
use rada_core::testplatform::{CallLog, DevicesSpec, WithDevices};
use rada_core::testutil::*;
use rada_tui::app::Modal;
use rada_tui::hits::Target;

fn device(mount: &Path, name: &str, uri: &str) -> Volume {
    Volume {
        mount_point: mount.to_path_buf(),
        label: Some(name.into()),
        fs_type: "mtp".into(),
        device: uri.into(),
        kind: VolumeKind::Device,
        drive_letter: None,
        total: None,
        available: None,
        read_only: false,
        responsive: true,
    }
}

fn unmounted(id: &str, name: &str, kind: VolumeKind) -> Unmounted {
    Unmounted {
        id: id.into(),
        name: name.into(),
        kind,
        size: None,
    }
}

struct Scene {
    h: H,
    log: CallLog,
    phone: PathBuf,
}

/// One phone that is open, a camera and a stick that are plugged in and not mounted.
fn scene(mount_error: Option<String>) -> Scene {
    let sb = Sandbox::new();
    let gvfs = sb.mkdir("gvfs");
    let phone = sb.mkdir("gvfs/mtp:host=Pixel_6_1A2B3C4D");
    sb.mkdir("gvfs/mtp:host=Pixel_6_1A2B3C4D/Internal storage/DCIM");
    sb.write("work/a.txt", "hello");
    let start = sb.path("work");
    let log = CallLog::default();
    let platform = Arc::new(WithDevices::new(
        sb.platform(),
        DevicesSpec {
            root: gvfs,
            volumes: vec![device(&phone, "Pixel 6", "mtp://Pixel_6_1A2B3C4D/")],
            unmounted: vec![
                unmounted("gphoto2://Canon/", "Canon EOS", VolumeKind::Device),
                unmounted("/dev/sdb1", "BLUE STICK", VolumeKind::Removable),
            ],
            mount_error,
        },
        log.clone(),
    ));
    let mut h = H::with_system(sb, start, 150, 34, (Arc::new(LocalFs), platform), |_| {});
    h.wait("the devices", |a| {
        !a.volumes.is_empty() && !a.unmounted.is_empty()
    });
    Scene { h, log, phone }
}

fn calls(log: &CallLog) -> Vec<String> {
    log.lock().unwrap().clone()
}

#[test]
fn an_open_phone_and_the_devices_that_are_plugged_in_are_in_the_sidebar() {
    let mut s = scene(None);
    let screen = s.h.screen();
    for needle in [
        "DEVICES",
        "Pixel 6",
        "Canon EOS",
        // The name is what is cut short, not the words after it.
        "BLUE STI…",
        "not mounted",
    ] {
        assert!(screen.contains(needle), "{needle}:\n{screen}");
    }
    let names: Vec<String> = s.h.app.side_items().into_iter().map(|i| i.name).collect();
    assert!(names.contains(&"Pixel 6".to_string()), "{names:?}");
    assert!(names.contains(&"Canon EOS".to_string()), "{names:?}");
    assert!(names.contains(&"BLUE STICK".to_string()), "{names:?}");
    // The note after the name is in the language of the interface.
    let note_of = |lang| {
        with_lang(lang, || {
            s.h.app
                .side_items()
                .iter()
                .find(|i| i.name == "Canon EOS")
                .and_then(|i| i.note())
        })
    };
    assert_eq!(note_of(Lang::En), Some("not mounted"));
    assert_eq!(note_of(Lang::It), Some("non montato"));
}

#[test]
fn an_open_phone_opens_like_a_folder_and_nothing_inside_is_counted() {
    let mut s = scene(None);
    let phone = s.phone.clone();
    s.h.click(&Target::Place(phone.clone()));
    s.h.wait("the phone", move |a| a.cwd == phone && !a.is_loading());
    assert_eq!(s.h.names(), ["Internal storage"]);
    let storage = s.phone.join("Internal storage");
    s.h.app.open_dir(storage.clone());
    s.h.wait("the storage", move |a| a.cwd == storage && !a.is_loading());
    assert_eq!(s.h.names(), ["DCIM"]);
    s.h.pump(300);
    let screen = s.h.screen();
    assert!(
        !screen.contains("···"),
        "the size of a folder on a phone is never being counted:\n{screen}"
    );
    assert!(
        calls(&s.log).is_empty(),
        "opening asks nothing of the system"
    );
}

#[test]
fn a_click_on_a_device_that_is_not_mounted_mounts_it_and_it_becomes_a_device() {
    let mut s = scene(None);
    s.h.click(&Target::Place(PathBuf::from("gphoto2://Canon/")));
    s.h.wait("the camera is open", |a| {
        a.volumes.iter().any(|v| v.device == "gphoto2://Canon/")
    });
    assert_eq!(calls(&s.log), ["mount gphoto2://Canon/"]);
    s.h.wait("the entry is no longer 'not mounted'", |a| {
        a.pending_devices()
            .iter()
            .all(|u| u.id != "gphoto2://Canon/")
    });
    let screen = s.h.screen();
    assert!(screen.contains("Canon EOS"), "{screen}");
    assert!(
        s.h.app
            .side_items()
            .iter()
            .filter(|i| i.name == "Canon EOS")
            .all(|i| i.note().is_none()),
        "it is open now:\n{screen}"
    );
    // The card says so.
    let said: Vec<String> =
        s.h.app
            .shown_notices()
            .iter()
            .map(|n| n.text.clone())
            .collect();
    assert!(
        said.iter().any(|t| t.contains("Canon EOS is mounted")),
        "{said:?}"
    );
}

#[test]
fn enter_in_the_sidebar_and_the_menu_mount_a_stick_too() {
    let mut s = scene(None);
    // The menu of a device that is not mounted has one entry, and it is "Mount".
    s.h.right_click(&Target::Place(PathBuf::from("/dev/sdb1")));
    let screen = s.h.screen();
    assert!(screen.contains("Mount"), "{screen}");
    assert!(!screen.contains("Pin to navigation"), "{screen}");
    s.h.key(KeyCode::Enter);
    s.h.wait("the stick is mounted", |a| {
        a.volumes.iter().any(|v| v.device == "/dev/sdb1")
    });
    assert_eq!(calls(&s.log), ["mount /dev/sdb1"]);

    // And with the keyboard: Tab to the sidebar, walk to the camera, Enter.
    let mut s = scene(None);
    s.h.press("tab");
    let items = s.h.app.side_items();
    let at = items
        .iter()
        .position(|i| i.name.starts_with("Canon EOS"))
        .expect("the camera is listed");
    for _ in 0..at {
        s.h.key(KeyCode::Down);
    }
    s.h.key(KeyCode::Enter);
    s.h.wait("the camera is open", |a| {
        a.volumes.iter().any(|v| v.device == "gphoto2://Canon/")
    });
    assert_eq!(calls(&s.log), ["mount gphoto2://Canon/"]);
}

#[test]
fn a_mount_that_cannot_be_done_says_what_to_install_and_the_program_goes_on() {
    let why = rada_core::i18n::t(Key::DevNoMtp).to_string();
    let mut s = scene(Some(why.clone()));
    s.h.click(&Target::Place(PathBuf::from("gphoto2://Canon/")));
    s.h.wait("the answer", |a| {
        a.shown_notices()
            .iter()
            .any(|n| n.text.contains("gvfs-mtp"))
    });
    let said: Vec<String> =
        s.h.app
            .shown_notices()
            .iter()
            .map(|n| n.text.clone())
            .collect();
    assert!(
        said.iter()
            .any(|t| t.starts_with("Canon EOS: ") && t.contains("pacman -S gvfs-mtp")),
        "{said:?}"
    );
    // Still listed as not mounted, and it can be tried again (once the tool is installed).
    let screen = s.h.screen();
    assert!(
        screen.contains("Canon EOS") && screen.contains("not mounted"),
        "{screen}"
    );
    s.h.app.clear_notices();
    s.h.click(&Target::Place(PathBuf::from("gphoto2://Canon/")));
    let log = s.log.clone();
    s.h.wait("a second try", move |a| {
        !a.shown_notices().is_empty() && calls(&log).len() >= 2
    });
    assert_eq!(calls(&s.log).len(), 2);
}

#[test]
fn an_open_phone_is_ejected_after_a_question_and_a_device_not_mounted_cannot_be() {
    let mut s = scene(None);
    s.h.right_click(&Target::Place(s.phone.clone()));
    assert!(s.h.screen().contains("Eject"));
    s.h.key(KeyCode::Esc);
    s.h.app.start_eject(&s.phone.clone());
    assert!(
        matches!(s.h.app.modal, Some(Modal::Eject(_))),
        "{}",
        s.h.screen()
    );
    assert!(
        calls(&s.log).is_empty(),
        "nothing happens before the answer"
    );
    s.h.key(KeyCode::Enter);
    s.h.wait("done", |a| a.modal.is_none());
    assert_eq!(
        calls(&s.log),
        [format!("eject {}", s.phone.display())],
        "the phone is let go"
    );

    // A device that is only plugged in has nothing to eject: the key says so.
    s.h.press("tab");
    let items = s.h.app.side_items();
    let at = items
        .iter()
        .position(|i| i.name.starts_with("BLUE STICK"))
        .unwrap();
    for _ in 0..at {
        s.h.key(KeyCode::Down);
    }
    s.h.press("delete");
    assert!(s.h.app.modal.is_none());
    let said: Vec<String> =
        s.h.app
            .shown_notices()
            .iter()
            .map(|n| n.text.clone())
            .collect();
    assert!(
        said.iter().any(|t| t.contains("cannot be ejected")),
        "{said:?}"
    );
}

#[test]
fn pasting_onto_a_phone_shows_what_is_different_there_in_the_plan_before_anything_is_done() {
    let mut s = scene(None);
    s.h.wait("the file", |a| a.listing.all().len() == 1);
    s.h.press("ctrl+c");
    let storage = s.phone.join("Internal storage");
    let dest = storage.clone();
    s.h.app.open_dir(storage.clone());
    s.h.wait("the storage", move |a| a.cwd == dest && !a.is_loading());
    s.h.press("ctrl+v");
    plan_modal(&mut s.h);
    let screen = s.h.screen();
    for words in ["permissions", "not guaranteed", "closed"] {
        assert!(screen.contains(words), "{words}:\n{screen}");
    }
    // Nothing has been written yet, and Esc leaves it so.
    assert!(!storage.join("a.txt").exists());
    s.h.key(KeyCode::Esc);
    assert!(!storage.join("a.txt").exists());
    // Run it again and accept: the file is there.
    s.h.press("ctrl+v");
    plan_modal(&mut s.h);
    run_plan_and_wait(&mut s.h);
    assert_eq!(
        std::fs::read_to_string(storage.join("a.txt")).unwrap(),
        "hello"
    );
}
