//! Phones and cameras: what the plan says before anything is done there, and what the
//! operations do when the "device" is a folder standing in for one (the folder of the desktop's
//! virtual filesystem is a folder of the sandbox; nothing real is opened).
#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;

use rada_core::events::CoreEvent;
use rada_core::fs::LocalFs;
use rada_core::model;
use rada_core::ops::*;
use rada_core::platform::{Platform, Unmounted, VolumeKind};
use rada_core::preview::{Limits, Preview};
use rada_core::testplatform::{DevicesSpec, WithDevices};
use rada_core::testutil::*;
use rada_core::workers::PreviewWorker;

/// An engine on a platform where `gvfs/` is the folder of the devices, and one phone with one
/// storage in it. Returns the engine, the phone's storage folder and the folder of devices.
fn with_phone(sb: &Sandbox) -> (Engine, PathBuf, PathBuf) {
    let root = sb.mkdir("gvfs");
    let storage = sb.mkdir("gvfs/mtp:host=Pixel_6_1A2B3C4D/Internal storage");
    let platform: Arc<dyn Platform> = Arc::new(WithDevices::new(
        sb.platform(),
        DevicesSpec {
            root: root.clone(),
            ..Default::default()
        },
        Default::default(),
    ));
    (Engine::local(platform), storage, root)
}

fn kinds(plan: &Plan) -> Vec<WarningKind> {
    plan.warnings.iter().map(|w| w.kind).collect()
}

fn severity(plan: &Plan, kind: WarningKind) -> Option<Severity> {
    plan.warnings
        .iter()
        .find(|w| w.kind == kind)
        .map(|w| w.severity)
}

#[test]
fn a_copy_to_a_phone_says_what_is_different_there_before_it_starts() {
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    sb.write("src/a.txt", "hello");
    sb.symlink("a.txt", "src/link");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &storage,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    let k = kinds(&plan);
    for kind in [
        WarningKind::DeviceWrite,
        WarningKind::DeviceAttrs,
        WarningKind::DeviceUndo,
        WarningKind::DeviceLink,
    ] {
        assert!(k.contains(&kind), "{kind:?} in {k:?}");
    }
    assert_eq!(
        severity(&plan, WarningKind::DeviceUndo),
        Some(Severity::Warning),
        "the promise that is not made is a warning, not a footnote"
    );
    assert_eq!(
        severity(&plan, WarningKind::DeviceLink),
        Some(Severity::Warning)
    );
    let undo = plan
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::DeviceUndo)
        .unwrap();
    assert!(undo.message.contains("not guaranteed"), "{}", undo.message);
    // The link is not a step at all, and nothing asks the phone for permissions or owners.
    assert!(
        !plan
            .steps
            .iter()
            .any(|s| matches!(s, Step::CopySymlink { .. } | Step::HardLink { .. }))
    );
    for s in &plan.steps {
        match s {
            Step::CopyFile {
                mode,
                uid,
                gid,
                atime,
                stored,
                ..
            } => {
                assert_eq!(
                    (*mode, *uid, *gid, *atime, *stored),
                    (None, None, None, None, None)
                );
            }
            Step::MakeDir { mode, .. } | Step::FinishDir { mode, .. } => assert_eq!(*mode, None),
            _ => {}
        }
        if let Step::FinishDir { src, uid, gid, .. } = s {
            assert_eq!((src, uid, gid), (&None, &None, &None));
        }
    }
    // The same copy between ordinary folders says none of this.
    let plain = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &sb.mkdir("plain"),
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(
        !kinds(&plain)
            .iter()
            .any(|k| format!("{k:?}").starts_with("Device")),
        "{:?}",
        kinds(&plain)
    );
}

#[test]
fn files_that_are_hard_links_become_separate_files_on_a_phone() {
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    sb.write("src/one", "same bytes");
    std::fs::hard_link(sb.path("src/one"), sb.path("src/two")).unwrap();
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &storage,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(kinds(&plan).contains(&WarningKind::DeviceHardLink));
    let copies = plan
        .steps
        .iter()
        .filter(|s| matches!(s, Step::CopyFile { .. }))
        .count();
    assert_eq!(copies, 2, "one copy per name");
    let rep = run(&e, &plan);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert_eq!(
        std::fs::read_to_string(storage.join("src/two")).unwrap(),
        "same bytes"
    );
}

#[test]
fn a_tree_is_copied_to_a_phone_under_its_own_names_with_no_temporary_file_and_no_rename() {
    let sb = Sandbox::new();
    let (_, storage, root) = with_phone(&sb);
    sb.write("src/a.txt", "hello");
    sb.write("src/deep/b.bin", vec![9u8; 300_000]);
    chmod(&sb.path("src/a.txt"), 0o444);
    // A file system that would refuse a rename: on a phone none is attempted.
    let fs = FaultFs::new();
    fs.fail(Op::Rename, "", libc::EIO);
    let platform: Arc<dyn Platform> = Arc::new(WithDevices::new(
        sb.platform(),
        DevicesSpec {
            root,
            ..Default::default()
        },
        Default::default(),
    ));
    let e = Engine::new(fs.clone(), platform);
    let (plan, rep) = do_copy(&e, &[sb.path("src")], &storage, ConflictPolicy::Skip);
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(
        !fs.hits
            .lock()
            .unwrap()
            .iter()
            .any(|(op, _)| *op == Op::Rename),
        "no rename on a phone"
    );
    assert_eq!(
        std::fs::read(storage.join("src/deep/b.bin")).unwrap(),
        vec![9u8; 300_000]
    );
    assert_eq!(
        std::fs::read_to_string(storage.join("src/a.txt")).unwrap(),
        "hello"
    );
    let leftovers: Vec<_> = snapshot(&storage)
        .keys()
        .filter(|p| p.to_string_lossy().contains(".rada-part"))
        .cloned()
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn what_a_phone_sends_back_is_given_the_usual_permissions_not_the_phones() {
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    // On the "phone": files whose permissions mean nothing.
    std::fs::create_dir_all(storage.join("DCIM")).unwrap();
    std::fs::write(storage.join("DCIM/photo.jpg"), "jpeg").unwrap();
    chmod(&storage.join("DCIM/photo.jpg"), 0o000 | 0o400);
    chmod(&storage.join("DCIM"), 0o700);
    let dest = sb.mkdir("pc");
    let (plan, rep) = do_copy(&e, &[storage.join("DCIM")], &dest, ConflictPolicy::Skip);
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(dest.join("DCIM/photo.jpg")), 0o644);
    assert_eq!(mode(dest.join("DCIM")), 0o755);
    assert!(kinds(&plan).contains(&WarningKind::DeviceAttrs));
    assert!(
        !kinds(&plan).contains(&WarningKind::DeviceUndo),
        "a copy from a phone is undone by removing the copy: that is sure"
    );
}

#[test]
fn a_move_from_a_phone_asks_for_the_undo_it_cannot_promise() {
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    std::fs::write(storage.join("a.txt"), "a").unwrap();
    let dest = sb.mkdir("pc");
    let plan = e.plan_transfer(
        &scan(&e, &[storage.join("a.txt")]),
        &dest,
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    assert!(kinds(&plan).contains(&WarningKind::DeviceUndo));
    assert!(kinds(&plan).contains(&WarningKind::CrossDevice));
}

#[test]
fn a_copy_the_phone_did_not_keep_whole_is_a_failure_and_a_move_keeps_its_source() {
    let sb = Sandbox::new();
    let (_, storage, root) = with_phone(&sb);
    sb.write("src/big.bin", vec![1u8; 200_000]);
    let fs = FaultFs::new();
    let platform: Arc<dyn Platform> = Arc::new(WithDevices::new(
        sb.platform(),
        DevicesSpec {
            root,
            ..Default::default()
        },
        Default::default(),
    ));
    let e = Engine::new(fs.clone(), platform);
    // The device loses the end of the file as it is closed.
    let kept = storage.join("big.bin");
    fs.on_bytes(
        "big.bin",
        200_000,
        ByteAction::Run(Arc::new(move || {
            let f = std::fs::OpenOptions::new().write(true).open(&kept).unwrap();
            f.set_len(150_000).unwrap();
        })),
    );
    let (plan, rep) = do_move(
        &e,
        &[sb.path("src/big.bin")],
        &storage,
        ConflictPolicy::Skip,
    );
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.status());
    assert!(
        rep.failed[0].error.contains("kept 150000 of 200000"),
        "{}",
        rep.failed[0].error
    );
    assert!(
        sb.path("src/big.bin").exists(),
        "the move did not take the source away"
    );
    assert!(
        !storage.join("big.bin").exists(),
        "and what the phone half-kept is not left there"
    );
}

#[test]
fn a_copy_that_the_read_back_shows_to_differ_is_a_failure_too() {
    let sb = Sandbox::new();
    let (_, storage, root) = with_phone(&sb);
    sb.write("src/pic.raw", vec![5u8; 100_000]);
    let fs = FaultFs::new();
    let platform: Arc<dyn Platform> = Arc::new(WithDevices::new(
        sb.platform(),
        DevicesSpec {
            root,
            ..Default::default()
        },
        Default::default(),
    ));
    let e = Engine::new(fs.clone(), platform);
    let dst = storage.join("pic.raw");
    // Same length, one byte different: the size check passes, the read-back must not.
    fs.on_bytes(
        "pic.raw",
        100_000,
        ByteAction::Run(Arc::new(move || {
            use std::io::{Seek, Write};
            let mut f = std::fs::OpenOptions::new().write(true).open(&dst).unwrap();
            f.seek(std::io::SeekFrom::Start(10)).unwrap();
            f.write_all(&[6]).unwrap();
        })),
    );
    let (_, rep) = do_move(
        &e,
        &[sb.path("src/pic.raw")],
        &storage,
        ConflictPolicy::Skip,
    );
    assert_eq!(rep.failed.len(), 1);
    assert!(
        rep.failed[0].error.contains("differs"),
        "{}",
        rep.failed[0].error
    );
    assert!(sb.path("src/pic.raw").exists());
    assert!(!storage.join("pic.raw").exists());
}

#[test]
fn a_copy_to_a_phone_and_its_undo_leave_both_sides_as_they_were() {
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    sb.write("src/a.txt", "hello");
    let j = sb.journal();
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src/a.txt")]),
        &storage,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(storage.join("a.txt").exists());
    let id = out.id;
    let (_, rep) = undo(&e, &j, &id);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(!storage.join("a.txt").exists());
    assert!(sb.path("src/a.txt").exists());
}

#[test]
fn a_move_inside_one_storage_is_a_rename_with_a_warning_and_between_two_phones_a_copy() {
    let sb = Sandbox::new();
    let (e, storage, root) = with_phone(&sb);
    std::fs::create_dir_all(storage.join("DCIM")).unwrap();
    std::fs::write(storage.join("DCIM/a.jpg"), "a").unwrap();
    let pics = storage.join("Pictures");
    std::fs::create_dir_all(&pics).unwrap();
    let plan = e.plan_transfer(
        &scan(&e, &[storage.join("DCIM/a.jpg")]),
        &pics,
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    assert!(
        plan.steps.iter().any(|s| matches!(s, Step::Rename { .. })),
        "{:?}",
        plan.steps
    );
    assert!(kinds(&plan).contains(&WarningKind::DeviceRename));
    assert!(!kinds(&plan).contains(&WarningKind::CrossDevice));

    // Another phone, or another storage of this one: to the system one filesystem, to a rename a
    // place it cannot reach.
    let other_phone = root.join("mtp:host=Other_1/Internal storage");
    std::fs::create_dir_all(&other_phone).unwrap();
    let other_storage = root.join("mtp:host=Pixel_6_1A2B3C4D/SD card");
    std::fs::create_dir_all(&other_storage).unwrap();
    for dest in [&other_phone, &other_storage] {
        let plan = e.plan_transfer(
            &scan(&e, &[storage.join("DCIM/a.jpg")]),
            dest,
            &TransferOptions::mv(ConflictPolicy::Skip),
        );
        assert!(
            plan.steps.iter().all(|s| !matches!(s, Step::Rename { .. })),
            "{:?}",
            plan.steps
        );
        assert!(
            plan.steps.iter().any(|s| matches!(
                s,
                Step::CopyFile {
                    remove_source: true,
                    verify: true,
                    ..
                }
            )),
            "copied, verified, then removed"
        );
        assert!(kinds(&plan).contains(&WarningKind::CrossDevice));
    }
}

#[test]
fn renaming_trashing_and_archiving_on_a_phone_have_their_own_words() {
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    std::fs::write(storage.join("a.txt"), "a").unwrap();
    let plan = e.plan_rename(&storage.join("a.txt"), "b.txt".as_ref());
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(
        severity(&plan, WarningKind::DeviceRename),
        Some(Severity::Info)
    );

    let plan = e.plan_trash(&scan(&e, &[storage.join("a.txt")]));
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    assert_eq!(
        severity(&plan, WarningKind::DeviceNoTrash),
        Some(Severity::Warning)
    );
    assert_eq!(
        severity(&plan, WarningKind::DeviceUndo),
        Some(Severity::Warning)
    );
    let text = &plan
        .warnings
        .iter()
        .find(|w| w.kind == WarningKind::DeviceNoTrash)
        .unwrap()
        .message;
    assert!(
        text.contains("no trash") && text.contains("copied to your trash"),
        "{text}"
    );

    // Nothing of this on an ordinary folder.
    sb.write("plain/a.txt", "a");
    let plan = e.plan_trash(&scan(&e, &[sb.path("plain/a.txt")]));
    assert!(
        kinds(&plan)
            .iter()
            .all(|k| !format!("{k:?}").starts_with("Device"))
    );

    // An archive cannot be written onto the phone; one made here can be copied there.
    sb.write("pack/x.txt", "x");
    let plan = e.plan_compress(
        &scan(&e, &[sb.path("pack")]),
        &storage.join("pack.zip"),
        rada_core::archive::ArchiveKind::Zip,
        ConflictPolicy::Skip,
    );
    assert!(!plan.is_executable());
    assert_eq!(
        severity(&plan, WarningKind::DeviceArchive),
        Some(Severity::Blocking)
    );
    let plan = e.plan_compress(
        &scan(&e, &[sb.path("pack")]),
        &sb.path("pack.zip"),
        rada_core::archive::ArchiveKind::Zip,
        ConflictPolicy::Skip,
    );
    assert!(plan.is_executable(), "{:?}", plan.warnings);
}

#[test]
fn the_words_of_the_warnings_exist_in_both_languages() {
    use rada_core::i18n::{Lang, with_lang};
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    sb.write("src/a.txt", "a");
    let plan_in = |lang| {
        with_lang(lang, || {
            e.plan_transfer(
                &scan(&e, &[sb.path("src/a.txt")]),
                &storage,
                &TransferOptions::copy(ConflictPolicy::Skip),
            )
        })
    };
    let (en, it) = (plan_in(Lang::En), plan_in(Lang::It));
    let msg = |p: &Plan, k| {
        p.warnings
            .iter()
            .find(|w| w.kind == k)
            .map(|w| w.message.clone())
            .unwrap()
    };
    for kind in [
        WarningKind::DeviceWrite,
        WarningKind::DeviceAttrs,
        WarningKind::DeviceUndo,
    ] {
        assert_ne!(msg(&en, kind), msg(&it, kind), "{kind:?}");
    }
    assert!(msg(&it, WarningKind::DeviceUndo).contains("non è garantito"));
}

// ------------------------------------------------------- not counted, not read, not tagged

#[test]
fn an_entry_on_a_phone_is_not_asked_for_tags_and_a_file_there_is_not_read_for_a_preview() {
    let sb = Sandbox::new();
    let (e, storage, _) = with_phone(&sb);
    let probe = sb.write("probe", "x");
    if rada_core::platform::tags::xattr::set(&probe, "user.rada-probe", b"1").is_err() {
        eprintln!("this filesystem keeps no user attributes: tags not checked");
    } else {
        let on_phone = storage.join("a.txt");
        std::fs::write(&on_phone, "hello").unwrap();
        let here = sb.write("here.txt", "hello");
        let tag = rada_core::platform::Tag::named("Work");
        let store = e.platform.tags().expect("tags");
        store.write(&on_phone, std::slice::from_ref(&tag)).unwrap();
        store.write(&here, std::slice::from_ref(&tag)).unwrap();
        let tags = |p: &PathBuf| {
            model::read_entry(&LocalFs, e.platform.as_ref(), p)
                .unwrap()
                .tags
        };
        assert_eq!(tags(&here).len(), 1, "an ordinary file shows its tag");
        assert!(tags(&on_phone).is_empty(), "a file on a phone is not asked");
    }

    // The preview of a file on the phone is a sentence; of a file here, its contents.
    let text = storage.join("notes.txt");
    std::fs::write(&text, "the contents").unwrap();
    let local = sb.write("notes.txt", "the contents");
    let (tx, rx) = crossbeam_channel::unbounded();
    let worker = PreviewWorker::spawn_with(Arc::new(LocalFs), Some(e.platform.clone()), tx);
    let preview_of = |path: &PathBuf, generation| {
        worker.request(path.clone(), generation, Limits::default());
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(CoreEvent::Preview(p)) if p.generation == generation => return p.preview,
                Ok(_) => {}
                Err(_) => panic!("no preview came"),
            }
        }
    };
    match preview_of(&text, 1) {
        Preview::Special(s) => assert!(s.contains("not read"), "{s}"),
        other => panic!("a file on a phone must not be read: {other:?}"),
    }
    match preview_of(&local, 2) {
        Preview::Text(t) => assert_eq!(t.lines, ["the contents"]),
        other => panic!("{other:?}"),
    }
    // A folder of the phone is still listed.
    match preview_of(&storage, 3) {
        Preview::Dir(d) => assert!(d.entries.iter().any(|(n, _)| n == "notes.txt")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_stand_in_platform_mounts_a_device_and_says_so() {
    // The lister the interface tests use: what is asked of it is written down.
    let sb = Sandbox::new();
    let log: rada_core::testplatform::CallLog = Default::default();
    let p = WithDevices::new(
        sb.platform(),
        DevicesSpec {
            root: sb.mkdir("gvfs"),
            unmounted: vec![Unmounted {
                id: "mtp://Pixel_6/".into(),
                name: "Pixel 6".into(),
                kind: VolumeKind::Device,
                size: None,
            }],
            ..Default::default()
        },
        log.clone(),
    );
    let one = p.volumes().unmounted();
    assert_eq!(one.len(), 1);
    p.volumes().mount(&one[0]).unwrap();
    assert!(p.volumes().unmounted().is_empty());
    let listed = p.volumes().list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, VolumeKind::Device);
    assert_eq!(log.lock().unwrap().as_slice(), ["mount mtp://Pixel_6/"]);
}
