//! What a Mac user expects of a file manager, driven on a stand-in for macOS that does
//! nothing real (see `rada_core::testplatform`): packages that open like files, the Finder's
//! names, iCloud Drive, a folder the system has not allowed yet, ejecting a disk and Quick Look.

mod common;

use std::sync::Arc;

use common::*;
use crossterm::event::KeyCode;
use rada_core::fs::LocalFs;
use rada_core::platform::{AccessDenied, PlaceKind, Volume, VolumeKind};
use rada_core::testplatform::{MacLike, MacLikeSpec};
use rada_core::testutil::*;
use rada_tui::app::{EjectStage, Modal};
use rada_tui::hits::Target;
use rada_tui::keymap::Action;

fn mac(sb: &Sandbox, spec: MacLikeSpec) -> (Arc<MacLike>, System) {
    let p = Arc::new(MacLike::new(sb.platform(), spec));
    (p.clone(), (Arc::new(LocalFs), p))
}

fn usb(sb: &Sandbox) -> Volume {
    Volume {
        mount_point: sb.path("Volumes/Backup"),
        label: Some("Backup".into()),
        fs_type: "exfat".into(),
        device: "disk4s1".into(),
        kind: VolumeKind::Removable,
        drive_letter: None,
        total: Some(1_000_000_000),
        available: Some(400_000_000),
        read_only: false,
        responsive: true,
    }
}

fn denial(folder: std::path::PathBuf, name: &str) -> AccessDenied {
    AccessDenied {
        who: "Ghostty".into(),
        folder,
        folder_name: name.into(),
        settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders",
        full_disk_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
    }
}

// ---------------------------------------------------------------------------- packages

fn with_packages() -> (H, Arc<MacLike>) {
    let sb = Sandbox::new();
    sb.write("d/Notes.txt", "n");
    sb.write("d/Mail.app/Contents/Info.plist", "plist");
    sb.write("d/Report.pages/Index/Document.iwa", "x");
    sb.write("d/plain/inner.txt", "i");
    let dir = sb.path("d");
    let (p, sys) = mac(
        &sb,
        MacLikeSpec {
            package_suffixes: vec![".app", ".pages"],
            quick_look: true,
            ..Default::default()
        },
    );
    (H::with_system(sb, dir, 140, 36, sys, |_| {}), p)
}

#[test]
fn a_package_is_listed_with_the_files_and_called_what_it_is() {
    let (mut h, _) = with_packages();
    // Folders first; the packages go among the files, by name.
    assert_eq!(
        h.names(),
        ["plain", "Mail.app", "Notes.txt", "Report.pages"]
    );
    let s = h.screen();
    assert!(s.contains("Application"), "{s}");
    assert!(s.contains("Pages document"), "{s}");
}

#[test]
fn enter_opens_a_package_with_its_program_instead_of_going_inside() {
    let (mut h, p) = with_packages();
    h.keys("j"); // Mail.app
    assert_eq!(h.cursor_name(), "Mail.app");
    h.key(KeyCode::Enter);
    h.pump(300);
    assert!(
        h.app.cwd.ends_with("d"),
        "still in the folder: {:?}",
        h.app.cwd
    );
    let calls = p.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("open ") && c.ends_with("Mail.app")),
        "{calls:?}"
    );
}

#[test]
fn show_package_contents_goes_inside_from_the_menu_and_the_key() {
    let (mut h, _) = with_packages();
    h.keys("j");
    let (x, y) = h.where_is(&Target::Row(1));
    h.mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    let s = h.screen();
    assert!(s.contains("Show package contents"), "{s}");
    assert!(s.contains("Quick Look"), "{s}");
    h.key(KeyCode::Esc);
    h.press("alt+down");
    h.wait("inside the package", |a| {
        a.cwd.ends_with("Mail.app") && !a.is_loading()
    });
    assert_eq!(h.names(), ["Contents"]);
}

#[test]
fn an_ordinary_folder_has_no_show_package_entry() {
    let (mut h, _) = with_packages();
    let (x, y) = h.where_is(&Target::Row(0));
    h.mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
        x,
        y,
        crossterm::event::KeyModifiers::NONE,
    );
    assert!(!h.screen().contains("Show package contents"));
}

// ---------------------------------------------------------------------------- names

#[test]
fn the_sidebar_and_the_breadcrumbs_use_the_names_the_finder_shows() {
    let sb = Sandbox::new();
    let desktop = sb.dirs.home.join("Desktop");
    std::fs::create_dir_all(sb.dirs.home.join("Desktop/progetti")).unwrap();
    std::fs::write(sb.dirs.home.join("Desktop/progetti/x.txt"), "x").unwrap();
    std::fs::create_dir_all(sb.dirs.home.join("Documents")).unwrap();
    let (_, sys) = mac(
        &sb,
        MacLikeSpec {
            names: vec![
                (desktop.clone(), "Scrivania".into()),
                (sb.dirs.home.join("Documents"), "Documenti".into()),
            ],
            ..Default::default()
        },
    );
    let start = desktop.join("progetti");
    let mut h = H::with_system(sb, start, 150, 30, sys, |_| {});
    h.wait("the names", |a| {
        a.paths.shown_name(&a.home().join("Desktop")).is_some()
    });
    let s = h.screen();
    assert!(s.contains("Scrivania"), "the sidebar: {s}");
    assert!(s.contains("Documenti"), "the sidebar: {s}");
    assert!(
        !s.contains(" Desktop "),
        "the name on disk is not shown: {s}"
    );
    // The path bar says Scrivania too…
    let bar = s.lines().nth(1).unwrap().to_string();
    assert!(
        bar.contains("Scrivania") && bar.contains("progetti"),
        "{bar}"
    );
    // …but the real path is still what is copied and what Properties locates.
    h.keys("Y");
    let copied = h.app.copied_text.clone().unwrap();
    assert!(
        copied.replace('\\', "/").contains("Desktop/progetti"),
        "{copied}"
    );
    h.press("alt+enter");
    let s = h.screen();
    assert!(
        s.contains("Desktop"),
        "Properties shows the real location: {s}"
    );
}

#[test]
fn icloud_drive_is_in_the_sidebar_when_it_is_there() {
    let sb = Sandbox::new();
    let cloud = sb
        .dirs
        .home
        .join("Library/Mobile Documents/com~apple~CloudDocs");
    std::fs::create_dir_all(&cloud).unwrap();
    sb.mkdir("d");
    let dir = sb.path("d");
    let (_, sys) = mac(
        &sb,
        MacLikeSpec {
            extra_places: vec![(PlaceKind::ICloud, cloud.clone())],
            ..Default::default()
        },
    );
    let mut h = H::with_system(sb, dir, 150, 30, sys, |_| {});
    h.wait("places", |a| {
        a.paths.places.iter().any(|p| p.kind == PlaceKind::ICloud)
    });
    assert!(h.screen().contains("iCloud Drive"));
    h.click(&Target::Place(cloud.clone()));
    h.wait("iCloud", |a| a.cwd == cloud && !a.is_loading());
}

// ------------------------------------------------------------------ the system says no

#[test]
fn disallowed_folder_explains_how_to_allow() {
    let sb = Sandbox::new();
    std::fs::create_dir_all(sb.dirs.home.join("Documents/work")).unwrap();
    sb.mkdir("d");
    let docs = sb.dirs.home.join("Documents");
    let dir = sb.path("d");
    let (p, _) = mac(
        &sb,
        MacLikeSpec {
            denied: vec![denial(docs.clone(), "Documents")],
            ..Default::default()
        },
    );
    // The folder refuses with "Operation not permitted", as macOS does until it is allowed.
    let fs = FaultFs::new();
    fs.fail(Op::ReadDir, docs.clone(), 1);
    let mut h = H::with_system(sb, dir, 150, 36, (fs.clone(), p.clone()), |_| {});
    h.app.open_dir(docs.clone());
    h.wait("the explanation", |a| {
        matches!(a.modal, Some(Modal::Access(_)))
    });
    let s = h.screen();
    assert!(
        s.contains("macOS has not given Ghostty access to Documents"),
        "{s}"
    );
    assert!(s.contains("Privacy & Security"), "{s}");
    assert!(s.contains("Full Disk Access"), "{s}");
    assert!(s.contains("Open Files & Folders settings"), "{s}");

    // The button opens the page of the settings.
    h.click(&Target::Key(KeyCode::Enter));
    h.pump(300);
    let calls = p.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("open_url x-apple.systempreferences:")
                && c.contains("Privacy_FilesAndFolders")),
        "{calls:?}"
    );
    assert!(h.app.modal.is_none());

    // The sidebar carries the same words for that place.
    h.app.side_focus = true;
    let items = h.app.side_items();
    let i = items.iter().position(|i| i.path == docs).unwrap();
    assert!(items[i].locked);
    h.app.side_cursor = i;
    h.app.side_mode = rada_tui::sidebar::Mode::Rail;
    assert_eq!(
        h.app.side_description().unwrap(),
        "macOS has not given Ghostty access to Documents"
    );
    h.app.side_mode = rada_tui::sidebar::Mode::Full;

    // Full Disk Access is the other button.
    h.app.open_dir(docs.clone());
    h.wait("again", |a| matches!(a.modal, Some(Modal::Access(_))));
    h.keys("f");
    h.pump(300);
    assert!(
        p.calls().iter().any(|c| c.contains("Privacy_AllFiles")),
        "{:?}",
        p.calls()
    );

    // Once the person has allowed it, the folder opens and the lock goes away.
    fs.clear();
    h.app.open_dir(docs.clone());
    h.wait("opened", |a| a.cwd == docs && !a.is_loading());
    assert!(h.app.denied.is_empty());
}

#[test]
fn an_ordinary_refusal_is_still_just_an_error() {
    let sb = Sandbox::new();
    std::fs::create_dir_all(sb.dirs.home.join("Secret")).unwrap();
    sb.mkdir("d");
    let secret = sb.dirs.home.join("Secret");
    let dir = sb.path("d");
    let (p, _) = mac(&sb, MacLikeSpec::default());
    let fs = FaultFs::new();
    fs.fail(Op::ReadDir, secret.clone(), 13); // EACCES
    let mut h = H::with_system(sb, dir, 150, 30, (fs, p), |_| {});
    h.app.open_dir(secret);
    h.pump(400);
    assert!(h.app.modal.is_none());
    // An error as before (its wording is the system's own), no window about consent.
    assert!(h.app.current_notice().is_some(), "{}", h.screen());
}

// ---------------------------------------------------------------------------- eject

fn with_usb(error: Option<&str>) -> (H, Arc<MacLike>, Volume) {
    let sb = Sandbox::new();
    sb.mkdir("d");
    sb.mkdir("Volumes/Backup");
    let dir = sb.path("d");
    let v = usb(&sb);
    let (p, sys) = mac(
        &sb,
        MacLikeSpec {
            volumes: vec![v.clone()],
            eject_error: error.map(str::to_string),
            ..Default::default()
        },
    );
    let mut h = H::with_system(sb, dir, 150, 30, sys, |_| {});
    h.wait("the disks", |a| !a.volumes.is_empty());
    (h, p, v)
}

#[test]
fn a_removable_disk_can_be_ejected_from_its_menu_with_a_confirmation() {
    let (mut h, p, v) = with_usb(None);
    assert!(h.screen().contains("Backup"));
    h.right_click(&Target::Place(v.mount_point.clone()));
    let s = h.screen();
    assert!(s.contains("Eject"), "{s}");
    // Choose Eject: the question comes first and nothing has happened yet.
    h.key(KeyCode::Down);
    h.key(KeyCode::Down);
    h.key(KeyCode::Enter);
    assert!(
        matches!(h.app.modal, Some(Modal::Eject(_))),
        "{}",
        h.screen()
    );
    assert!(h.screen().contains("Eject") && h.screen().contains("Backup"));
    assert!(p.calls().iter().all(|c| !c.starts_with("eject")));
    h.key(KeyCode::Enter);
    h.wait("ejected", |a| a.modal.is_none());
    assert_eq!(
        p.calls().iter().filter(|c| c.starts_with("eject")).count(),
        1,
        "{:?}",
        p.calls()
    );
    assert!(h.screen().contains("was ejected"), "{}", h.screen());
}

#[test]
fn a_busy_disk_says_so_and_can_be_tried_again() {
    let (mut h, p, v) = with_usb(Some(
        "the disk is busy: Finder (process 412) is still using it. Close it and try again.",
    ));
    h.app.start_eject(&v.mount_point);
    h.key(KeyCode::Enter);
    h.wait(
        "the failure",
        |a| matches!(&a.modal, Some(Modal::Eject(e)) if matches!(e.stage, EjectStage::Failed(_))),
    );
    let s = h.screen();
    assert!(s.contains("It was not ejected"), "{s}");
    assert!(s.contains("Finder (process 412)"), "{s}");
    assert!(s.contains("Try again"), "{s}");
    h.key(KeyCode::Enter);
    h.wait(
        "second try",
        |a| matches!(&a.modal, Some(Modal::Eject(e)) if matches!(e.stage, EjectStage::Failed(_))),
    );
    assert_eq!(
        p.calls().iter().filter(|c| c.starts_with("eject")).count(),
        2
    );
    h.key(KeyCode::Esc);
    assert!(h.app.modal.is_none());
}

#[test]
fn delete_in_sidebar_ejects_a_disk() {
    let (mut h, p, v) = with_usb(None);
    // Looking inside the disk: ejecting it moves out first.
    h.app.open_dir(v.mount_point.clone());
    h.wait("in the disk", |a| a.cwd == v.mount_point && !a.is_loading());
    h.press("tab");
    let items = h.app.side_items();
    h.app.side_cursor = items.iter().position(|i| i.path == v.mount_point).unwrap();
    h.press("delete");
    assert!(matches!(h.app.modal, Some(Modal::Eject(_))));
    h.key(KeyCode::Enter);
    h.wait("done", |a| a.modal.is_none());
    h.wait("out of the disk", |a| {
        !a.cwd.starts_with(&v.mount_point) && !a.is_loading()
    });
    assert!(p.calls().iter().any(|c| c.starts_with("eject")));
}

#[test]
fn a_disk_that_cannot_be_ejected_has_no_eject_entry() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    let dir = sb.path("d");
    let (_, sys) = mac(&sb, MacLikeSpec::default());
    let mut h = H::with_system(sb, dir, 150, 30, sys, |_| {});
    h.wait("disks", |a| !a.volumes.is_empty());
    let first = h.app.volumes[0].mount_point.clone();
    h.right_click(&Target::Place(first));
    assert!(!h.screen().contains("Eject"));
}

// ------------------------------------------------------------------------- Quick Look

#[test]
fn quick_look_is_in_the_menu_the_details_pane_and_on_a_key() {
    let (mut h, p) = with_packages();
    h.keys("jj"); // Notes.txt
    h.wait("preview", |a| a.preview.name == "Notes.txt");
    let s = h.screen();
    assert!(
        s.contains("Quick Look"),
        "the details pane has the button: {s}"
    );
    h.click(&Target::Act(Action::QuickLook));
    h.pump(300);
    h.press("alt+y");
    h.pump(300);
    let n = p
        .calls()
        .iter()
        .filter(|c| c.starts_with("quick_look ") && c.ends_with("Notes.txt"))
        .count();
    assert_eq!(n, 2, "{:?}", p.calls());
    // Space still selects.
    h.keys(" ");
    assert_eq!(h.marked_names(), ["Notes.txt"]);
}

#[test]
fn without_quick_look_the_buttons_are_not_there() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::new(sb, dir, 140, 36);
    h.keys("jj");
    assert!(!h.screen().contains("Quick Look"));
    h.press("alt+y");
    assert!(h.screen().contains("Quick Look is a macOS feature"));
}

#[test]
fn the_full_screen_preview_offers_quick_look_on_q() {
    let (mut h, p) = with_packages();
    h.keys("jj");
    h.wait("preview", |a| a.preview.name == "Notes.txt");
    h.click(&Target::FullPreview);
    assert!(matches!(h.app.modal, Some(Modal::FullPreview)));
    assert!(h.screen().contains("Q  Quick Look") || h.screen().contains("Quick Look"));
    h.keys("Q");
    h.pump(300);
    assert!(p.calls().iter().any(|c| c.starts_with("quick_look ")));
    assert!(
        matches!(h.app.modal, Some(Modal::FullPreview)),
        "it stays open"
    );
}

// ------------------------------------------------------------------------------ Cmd keys

fn with_cmd_keys() -> H {
    let (sb, dir) = sandbox_with_files();
    H::with_config(sb, dir, 140, 30, |c| {
        c.keymap = rada_tui::keymap::Keymap::new_for(
            rada_tui::keymap::Preset::VimClassic,
            &Default::default(),
            true,
        );
    })
}

fn cmd(h: &mut H, code: KeyCode, extra: crossterm::event::KeyModifiers) {
    h.app.on_key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::SUPER | extra,
    ));
}

#[test]
fn cmd_c_cmd_v_and_cmd_backspace_work_like_in_the_finder() {
    use crossterm::event::KeyModifiers as M;
    let mut h = with_cmd_keys();
    h.keys("j"); // a.txt
    cmd(&mut h, KeyCode::Char('c'), M::NONE);
    assert!(h.app.clipboard.is_some(), "Cmd+C copies");
    cmd(&mut h, KeyCode::Backspace, M::NONE);
    plan_modal(&mut h); // a plan to trash it, as with Del
}

#[test]
fn cmd_shift_dot_shows_the_hidden_files_and_cmd_i_the_properties() {
    use crossterm::event::KeyModifiers as M;
    let mut h = with_cmd_keys();
    assert!(!h.names().contains(&".hidden".to_string()));
    cmd(&mut h, KeyCode::Char('.'), M::SHIFT);
    assert!(
        h.names().contains(&".hidden".to_string()),
        "{:?}",
        h.names()
    );
    cmd(&mut h, KeyCode::Char('i'), M::NONE);
    assert!(matches!(h.app.modal, Some(Modal::Properties(_))));
}

#[test]
fn cmd_up_goes_to_the_parent_and_cmd_brackets_go_back_and_forward() {
    use crossterm::event::KeyModifiers as M;
    let mut h = with_cmd_keys();
    let start = h.app.cwd.clone();
    cmd(&mut h, KeyCode::Up, M::NONE);
    h.wait("parent", |a| a.cwd != start && !a.is_loading());
    cmd(&mut h, KeyCode::Char('['), M::NONE);
    h.wait("back", |a| a.cwd == start && !a.is_loading());
    cmd(&mut h, KeyCode::Char(']'), M::NONE);
    h.wait("forward", |a| a.cwd != start && !a.is_loading());
}

#[test]
fn without_the_mac_table_cmd_does_nothing() {
    let (sb, dir) = sandbox_with_files();
    let mut h = H::with_config(sb, dir, 140, 30, |c| {
        c.keymap = rada_tui::keymap::Keymap::new_for(
            rada_tui::keymap::Preset::VimClassic,
            &Default::default(),
            false,
        );
    });
    h.keys("j");
    cmd(
        &mut h,
        KeyCode::Char('c'),
        crossterm::event::KeyModifiers::NONE,
    );
    assert!(h.app.clipboard.is_none());
}

// ---------------------------------------------------------------------------- the Finder's names

#[test]
fn localized_folder_hides_its_suffix() {
    let sb = Sandbox::new();
    sb.write("d/Chrome Apps.localized/.localized", "");
    sb.write("d/Chrome Apps.localized/App.txt", "a");
    sb.write("d/Notes.localized", "a file called like that");
    let dir = sb.path("d");
    let (_, sys) = mac(&sb, MacLikeSpec::default());
    let mut h = H::with_system(sb, dir, 140, 36, sys, |_| {});
    let s = h.screen();
    let rows: Vec<&str> = s.lines().collect();
    assert!(
        rows.iter()
            .any(|l| l.contains("▸ Chrome Apps ") || l.contains("Chrome Apps  ")),
        "{s}"
    );
    assert!(
        !rows.iter().any(|l| l.contains("Chrome Apps.localized")),
        "the folder's suffix is not shown:\n{s}"
    );
    // A file keeps its name.
    assert!(rows.iter().any(|l| l.contains("Notes.localized")), "{s}");
    // The real name is in Properties.
    h.keys("g");
    h.press("alt+enter");
    let p = h.screen();
    assert!(p.contains("Chrome Apps.localized"), "{p}");
}

#[test]
fn folder_preview_follows_hidden_files() {
    let sb = Sandbox::new();
    sb.write("d/Apps/.DS_Store", "x");
    sb.write("d/Apps/.localized", "");
    sb.write("d/Apps/Mail.txt", "m");
    let dir = sb.path("d");
    let (_, sys) = mac(&sb, MacLikeSpec::default());
    let mut h = H::with_system(sb, dir, 170, 36, sys, |_| {});
    h.app.details = Some(true);
    let in_preview = |a: &rada_tui::app::App| match &a.preview.content {
        Some(rada_core::preview::Preview::Dir(d)) => {
            Some(d.entries.iter().map(|e| e.0.clone()).collect::<Vec<_>>())
        }
        _ => None,
    };
    h.wait("the folder preview", |a| in_preview(a).is_some());
    assert_eq!(in_preview(&h.app).unwrap(), ["Mail.txt"]);
    assert!(!h.screen().contains(".DS_Store"));
    // Hidden files on (the "." key): the preview follows the list.
    h.press(".");
    h.wait("the preview with hidden files", |a| {
        in_preview(a).is_some_and(|v| v.len() == 3)
    });
    assert!(h.screen().contains(".DS_Store"));
}
