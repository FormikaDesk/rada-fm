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
        !a.devices.volumes.is_empty() && !a.devices.unmounted.is_empty()
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
        // The name is what is cut short, not the words after it.
        "Cano",
        "BLUE",
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
    s.h.wait("the phone", move |a| a.tab.cwd == phone && !a.is_loading());
    assert_eq!(s.h.names(), ["Internal storage"]);
    let storage = s.phone.join("Internal storage");
    s.h.app.open_dir(storage.clone());
    s.h.wait("the storage", move |a| {
        a.tab.cwd == storage && !a.is_loading()
    });
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
fn click_mounts_an_unmounted_device() {
    let mut s = scene(None);
    s.h.click(&Target::Place(PathBuf::from("gphoto2://Canon/")));
    s.h.wait("the camera is open", |a| {
        a.devices
            .volumes
            .iter()
            .any(|v| v.device == "gphoto2://Canon/")
    });
    assert_eq!(calls(&s.log), ["mount gphoto2://Canon/"]);
    s.h.wait("the entry is no longer 'not mounted'", |a| {
        a.devices
            .pending()
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
        a.devices.volumes.iter().any(|v| v.device == "/dev/sdb1")
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
        a.devices
            .volumes
            .iter()
            .any(|v| v.device == "gphoto2://Canon/")
    });
    assert_eq!(calls(&s.log), ["mount gphoto2://Canon/"]);
}

#[test]
fn failed_mount_says_what_to_install() {
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
fn eject_asks_first_and_needs_a_mount() {
    let mut s = scene(None);
    s.h.right_click(&Target::Place(s.phone.clone()));
    assert!(s.h.screen().contains("Eject"));
    s.h.key(KeyCode::Esc);
    s.h.app.start_eject(&s.phone.clone());
    assert!(
        matches!(s.h.app.windows.modal, Some(Modal::Eject(_))),
        "{}",
        s.h.screen()
    );
    assert!(
        calls(&s.log).is_empty(),
        "nothing happens before the answer"
    );
    s.h.key(KeyCode::Enter);
    s.h.wait("done", |a| a.windows.modal.is_none());
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
    assert!(s.h.app.windows.modal.is_none());
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
fn paste_to_phone_plan_shows_differences() {
    let mut s = scene(None);
    s.h.wait("the file", |a| a.tab.listing.all().len() == 1);
    s.h.press("ctrl+c");
    let storage = s.phone.join("Internal storage");
    let dest = storage.clone();
    s.h.app.open_dir(storage.clone());
    s.h.wait("the storage", move |a| a.tab.cwd == dest && !a.is_loading());
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

// ------------------------------------------------ the real platform, with stand-in programs

#[cfg(target_os = "linux")]
mod linux_pipeline {
    //! The whole way from the programs' words to the sidebar: the Linux platform, its lister,
    //! the worker thread, the event and the screen. Only `gio`, `lsblk` and `udisksctl` are
    //! stand-ins, and they print what a real machine printed (the serial number is made up).
    use super::*;
    use rada_core::platform::Platform;
    use rada_core::platform::linux::LinuxPlatform;
    use rada_core::platform::linux::devices::Tools;

    const GIO_UNMOUNTED: &str = "\
Drive(0): KBG40ZNS256G BG4A KIOXIA
  Type: GProxyDrive (GProxyVolumeMonitorUDisks2)
  ids:
   unix-device: '/dev/nvme0n1'
  is_removable=0
Volume(0): motorola edge 40
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  ids:
   unix-device: '/dev/bus/usb/003/030'
  activation_root=mtp://motorola_motorola_edge_40_ZY22FAKE0001/
  themed icons:  [phone]
  can_mount=1
  can_eject=0
  should_automount=1
";

    const GIO_OPEN: &str = "\
Volume(0): motorola edge 40
  Type: GProxyVolume (GProxyVolumeMonitorMTP)
  activation_root=mtp://motorola_motorola_edge_40_ZY22FAKE0001/
  can_mount=1
  Mount(0): motorola edge 40 -> mtp://motorola_motorola_edge_40_ZY22FAKE0001/
    Type: GDaemonMount
    can_unmount=1
";

    fn tool(dir: &Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let exe = dir.join(name);
        std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn run(listing: &str, gvfs_entry: bool) -> (H, PathBuf, PathBuf) {
        let sb = Sandbox::new();
        let bin = sb.mkdir("tools");
        let calls = sb.root.join("calls.log");
        let list = sb.root.join("gio.txt");
        std::fs::write(&list, listing).unwrap();
        tool(
            &bin,
            "gio",
            &format!(
                "echo \"gio $*\" >> '{}'\nif [ \"$1 $2\" = \"mount -li\" ]; then cat '{}'; fi",
                calls.display(),
                list.display()
            ),
        );
        tool(&bin, "lsblk", "echo '{\"blockdevices\": []}'");
        let gvfs = sb.mkdir("gvfs");
        let phone = gvfs.join("mtp:host=motorola_motorola_edge_40_ZY22FAKE0001");
        if gvfs_entry {
            std::fs::create_dir_all(phone.join("Internal shared storage")).unwrap();
        }
        sb.mkdir("work");
        let start = sb.path("work");
        let platform: Arc<dyn Platform> = Arc::new(
            LinuxPlatform::new(sb.dirs.clone())
                .with_gvfs_dir(Some(gvfs))
                .with_tools(Tools::with_path(bin.into_os_string())),
        );
        let h = H::with_system(sb, start, 150, 34, (Arc::new(LocalFs), platform), |_| {});
        (h, phone, calls)
    }

    #[test]
    fn closed_phone_is_listed_and_enter_mounts() {
        let (mut h, _, calls) = run(GIO_UNMOUNTED, false);
        h.wait("the phone is listed", |a| !a.devices.unmounted.is_empty());
        let screen = h.screen();
        assert!(screen.contains("moto"), "{screen}");
        assert!(screen.contains("not mounted"), "{screen}");
        let id = "mtp://motorola_motorola_edge_40_ZY22FAKE0001/";
        h.click(&Target::Place(PathBuf::from(id)));
        let log = calls.clone();
        h.wait("gio was asked to mount it", move |_| {
            std::fs::read_to_string(&log)
                .unwrap_or_default()
                .contains(&format!("gio mount {id}"))
        });
    }

    #[test]
    fn open_phone_is_listed_by_name() {
        let (mut h, phone, _) = run(GIO_OPEN, true);
        h.wait("the phone is listed", |a| {
            a.devices.volumes.iter().any(|v| {
                v.kind == VolumeKind::Device && v.label.as_deref() == Some("motorola edge 40")
            })
        });
        let screen = h.screen();
        assert!(screen.contains("motorola edge 40"), "{screen}");
        assert!(
            !screen.contains("not mounted"),
            "it is open, not waiting to be mounted:\n{screen}"
        );
        assert!(h.app.devices.pending().is_empty());
        // And it opens.
        h.click(&Target::Place(phone.clone()));
        h.wait("the phone", move |a| a.tab.cwd == phone && !a.is_loading());
        assert_eq!(h.names(), ["Internal shared storage"]);
    }
}

#[test]
fn phone_path_bar_names_the_device() {
    let mut s = scene(None);
    let storage = s.phone.join("Internal storage");
    s.h.app.open_dir(storage.clone());
    let dest = storage.clone();
    s.h.wait("the storage", move |a| a.tab.cwd == dest && !a.is_loading());
    let screen = s.h.screen();
    let bar = screen
        .lines()
        .find(|l| l.contains("Pixel 6"))
        .unwrap_or_else(|| panic!("the device is named in the bar:\n{screen}"));
    assert!(bar.contains("Pixel 6 › Internal storage"), "{bar}");
    assert!(
        !screen
            .lines()
            .take(8)
            .any(|l| l.contains("gvfs") || l.contains("mtp:host")),
        "the virtual filesystem's folders are not in the bar:\n{screen}"
    );
    // The first crumb is the device itself: a click goes to it.
    s.h.click(&Target::Crumb(s.phone.clone()));
    let phone = s.phone.clone();
    s.h.wait("the device", move |a| a.tab.cwd == phone && !a.is_loading());
    // The bar of the folder holding the phones is the plain one.
    s.h.app.open_dir(s.phone.parent().unwrap().to_path_buf());
    let root = s.phone.parent().unwrap().to_path_buf();
    s.h.wait("the folder of the devices", move |a| {
        a.tab.cwd == root && !a.is_loading()
    });
    assert!(s.h.app.device_head().is_none());
    // The details of an item still give the real folder.
    s.h.app.open_dir(storage.clone());
    let dest = storage.clone();
    s.h.wait("the storage again", move |a| {
        a.tab.cwd == dest && !a.is_loading()
    });
    s.h.keys("j");
    s.h.wait("the details", |a| !a.preview.name.is_empty());
    let screen = s.h.screen();
    let location = screen
        .lines()
        .find(|l| l.contains("Location"))
        .unwrap_or_else(|| panic!("the details have a Location row:\n{screen}"));
    // The row is the real folder (cut at the width of the pane), not the device's name.
    assert!(
        location.contains(&storage.parent().unwrap().to_string_lossy()[..12]),
        "{location}"
    );
}
