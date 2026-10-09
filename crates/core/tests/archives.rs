//! Archives: what a plan says, what an extraction writes, and what it must never write.

use std::path::{Path, PathBuf};

use rada_core::archive::testkit::{Member, have_tool, write_tar, write_zip};
use rada_core::archive::{ArchiveLimits, Compression};
use rada_core::ops::*;
use rada_core::testutil::*;

fn plan_req(e: &Engine, req: &OpRequest) -> Plan {
    plan_try(e, req).unwrap_or_else(|err| panic!("{req:?}: {err}"))
}

fn plan_try(e: &Engine, req: &OpRequest) -> rada_core::Result<Plan> {
    let cancel = Cancel::new();
    e.plan_request(
        req,
        None,
        ScanControl {
            cancel: cancel.flag(),
            progress: &mut |_| {},
        },
    )
    .map(|p| p.plan)
}

fn extract_req(archive: &Path, dest: &Path, into: ExtractInto) -> OpRequest {
    OpRequest::Extract {
        archive: archive.to_path_buf(),
        destination: dest.to_path_buf(),
        into,
        only: Vec::new(),
        conflict: ConflictPolicy::Skip,
    }
}

fn warning<'a>(p: &'a Plan, k: rada_core::ops::WarningKind) -> Option<&'a Warning> {
    p.warnings.iter().find(|w| w.kind == k)
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn sample_members() -> Vec<Member> {
    vec![
        Member::dir("docs"),
        Member::file("docs/a.txt", "alpha"),
        Member::file("docs/sub/b.txt", "beta").mode(0o600),
        Member::file("top.txt", "top"),
    ]
}

// ---------------------------------------------------------------------- formats

#[test]
fn every_format_extracts_the_same_tree() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let members = sample_members();
    let mut archives: Vec<(&str, PathBuf)> = Vec::new();
    let z = sb.path("in/a.zip");
    std::fs::create_dir_all(z.parent().unwrap()).unwrap();
    write_zip(&z, &members);
    archives.push(("zip", z));
    for (name, c) in [
        ("a.tar", Compression::None),
        ("a.tar.gz", Compression::Gzip),
        ("a.tar.bz2", Compression::Bzip2),
        ("a.tar.xz", Compression::Xz),
        ("a.tar.zst", Compression::Zstd),
    ] {
        let p = sb.path(format!("in/{name}"));
        write_tar(&p, &members, c);
        archives.push((name, p));
    }
    for (label, archive) in archives {
        let out = sb.mkdir(format!("out-{label}"));
        let plan = plan_req(&e, &extract_req(&archive, &out, ExtractInto::Here));
        assert!(plan.is_executable(), "{label}: {:?}", plan.warnings);
        let rep = run(&e, &plan);
        assert!(rep.failed.is_empty(), "{label}: {:?}", rep.failed);
        assert_eq!(read(&out.join("docs/a.txt")), "alpha", "{label}");
        assert_eq!(read(&out.join("docs/sub/b.txt")), "beta", "{label}");
        assert_eq!(read(&out.join("top.txt")), "top", "{label}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(out.join("docs/sub/b.txt")).unwrap();
            assert_eq!(m.permissions().mode() & 0o777, 0o600, "{label}");
        }
    }
}

#[test]
fn tar_xz_and_tar_bz2_are_not_silently_dropped() {
    // Regression from another file manager: these two formats failed without an error.
    let sb = Sandbox::new();
    let e = sb.engine();
    for (name, c) in [("x.tar.xz", Compression::Xz), ("x.tar.bz2", Compression::Bzip2)] {
        let a = sb.path(name);
        write_tar(&a, &[Member::file("f.txt", "content")], c);
        let out = sb.mkdir(format!("o-{name}"));
        let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
        let rep = run(&e, &plan);
        assert!(rep.failed.is_empty(), "{name}: {:?}", rep.failed);
        assert_eq!(read(&out.join("f.txt")), "content");
    }
}

#[test]
fn a_single_compressed_file_extracts_to_its_name_without_the_extension() {
    let sb = Sandbox::new();
    let e = sb.engine();
    for (name, c) in [
        ("notes.txt.gz", Compression::Gzip),
        ("notes.txt.xz", Compression::Xz),
        ("notes.txt.bz2", Compression::Bzip2),
        ("notes.txt.zst", Compression::Zstd),
    ] {
        let dir = sb.mkdir(format!("d-{name}"));
        let a = dir.join(name);
        std::fs::write(&a, rada_core::archive::testkit::compress(b"just text\n", c)).unwrap();
        let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Auto));
        assert!(plan.is_executable(), "{name}: {:?}", plan.warnings);
        assert!(run(&e, &plan).failed.is_empty());
        assert_eq!(read(&dir.join("notes.txt")), "just text\n", "{name}");
    }
}

#[test]
fn the_format_is_known_from_the_content_not_the_name() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let a = sb.path("looks-like-nothing.dat");
    write_zip(&a, &[Member::file("inside.txt", "x")]);
    let out = sb.mkdir("out");
    let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(read(&out.join("inside.txt")), "x");
    // And a name that says zip over bytes that are not an archive is refused with words.
    let fake = sb.write("fake.zip", "this is not a zip");
    let err = plan_try(&e, &extract_req(&fake, &out, ExtractInto::Here)).unwrap_err();
    assert!(err.to_string().contains("not an archive"), "{err}");
}

// ---------------------------------------------------------------------- folders

#[test]
fn the_default_folder_is_named_after_the_archive_name_only() {
    // Regression from another file manager: a dot in a folder above the archive put the
    // extraction in the wrong place.
    let sb = Sandbox::new();
    let e = sb.engine();
    let dir = sb.mkdir("v1.2/sub");
    let a = dir.join("x.tar.gz");
    write_tar(
        &a,
        &[Member::file("one.txt", "1"), Member::file("two.txt", "2")],
        Compression::Gzip,
    );
    let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Auto));
    assert_eq!(plan.destination.as_deref(), Some(dir.join("x").as_path()));
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(read(&dir.join("x/one.txt")), "1");
    assert!(!sb.path("v1.2/sub/one.txt").exists());
}

#[test]
fn a_single_folder_at_the_top_is_not_wrapped_in_a_second_one() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let dir = sb.mkdir("here");
    let a = dir.join("photos.zip");
    write_zip(&a, &[Member::file("photos/a.jpg", "a"), Member::file("photos/b.jpg", "b")]);
    let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Auto));
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(read(&dir.join("photos/a.jpg")), "a");
    assert!(!dir.join("photos/photos").exists());
}

#[test]
fn several_things_at_the_top_get_a_folder_and_a_named_folder_is_taken_literally() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let dir = sb.mkdir("here");
    let a = dir.join("pair.zip");
    write_zip(&a, &[Member::file("a", "1"), Member::file("b", "2")]);
    let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Auto));
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(read(&dir.join("pair/a")), "1");
    let plan = plan_req(
        &e,
        &extract_req(&a, &dir, ExtractInto::Folder { name: "mine".into() }),
    );
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(read(&dir.join("mine/b")), "2");
    // A name that is a path is refused.
    assert!(plan_try(&e, &extract_req(&a, &dir, ExtractInto::Folder { name: "a/b".into() })).is_err());
}

// ---------------------------------------------------------------------- safety

#[test]
fn zip_slip_names_are_blocked_with_their_reason() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let dir = sb.mkdir("work/out");
    let a = sb.path("work/evil.zip");
    write_zip(
        &a,
        &[
            Member::file("ok.txt", "fine"),
            Member::file("x", "evil").raw_name(b"../escaped.txt"),
            Member::file("x", "evil").raw_name(b"/tmp/rada-absolute-escape.txt"),
            Member::file("x", "evil").raw_name(b"sub/../../escaped2.txt"),
            Member::file("x", "evil").raw_name(b"..\\escaped3.txt"),
        ],
    );
    let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Here));
    let w = warning(&plan, WarningKind::UnsafePath).expect("unsafe paths are reported");
    assert_eq!(w.count, 4);
    assert!(plan.is_executable());
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(read(&dir.join("ok.txt")), "fine");
    for bad in ["work/escaped.txt", "work/escaped2.txt", "work/escaped3.txt"] {
        assert!(!sb.path(bad).exists(), "{bad} was written outside");
    }
    assert!(!Path::new("/tmp/rada-absolute-escape.txt").exists());
    // Nor did the cleaned versions land inside as stand-ins.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn tar_slip_names_are_blocked_too() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let dir = sb.mkdir("work/out");
    let a = sb.path("work/evil.tar");
    write_tar(
        &a,
        &[
            Member::file("../outside.txt", "evil"),
            Member::file("good.txt", "good"),
        ],
        Compression::None,
    );
    let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Here));
    assert_eq!(warning(&plan, WarningKind::UnsafePath).unwrap().count, 1);
    assert!(run(&e, &plan).failed.is_empty());
    assert!(!sb.path("work/outside.txt").exists());
    assert_eq!(read(&dir.join("good.txt")), "good");
}

#[cfg(unix)]
#[test]
fn nothing_is_written_through_a_link_the_archive_made() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let victim = sb.mkdir("victim");
    let dir = sb.mkdir("out");
    let a = sb.path("links.tar");
    write_tar(
        &a,
        &[
            Member::symlink("door", victim.to_str().unwrap()),
            Member::file("door/pwned.txt", "evil"),
            Member::symlink("rel", "../../../etc"),
            Member::file("fine.txt", "ok"),
        ],
        Compression::None,
    );
    let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Here));
    // Links pointing out are reported, created as links, never followed.
    assert_eq!(warning(&plan, WarningKind::LinkOutside).unwrap().count, 2);
    assert_eq!(warning(&plan, WarningKind::UnsafePath).unwrap().count, 1);
    assert!(run(&e, &plan).failed.is_empty());
    assert!(!victim.join("pwned.txt").exists());
    assert_eq!(std::fs::read_link(dir.join("door")).unwrap(), victim);
    assert_eq!(read(&dir.join("fine.txt")), "ok");
}

#[cfg(unix)]
#[test]
fn links_inside_the_archive_come_out_as_links() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let dir = sb.mkdir("out");
    let a = sb.path("ok.zip");
    write_zip(&a, &[Member::file("real.txt", "r"), Member::symlink("alias", "real.txt")]);
    let plan = plan_req(&e, &extract_req(&a, &dir, ExtractInto::Here));
    assert!(warning(&plan, WarningKind::LinkOutside).is_none());
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(std::fs::read_link(dir.join("alias")).unwrap(), Path::new("real.txt"));
}

#[test]
fn a_bomb_needs_confirmation_and_the_limits_are_configurable() {
    let sb = Sandbox::new();
    let e = sb.engine().with_archive_limits(ArchiveLimits {
        max_total_bytes: 1 << 20,
        max_ratio: 50,
        ratio_floor_bytes: 1 << 10,
        max_entries: 1000,
    });
    let a = sb.path("bomb.tar.gz");
    write_tar(
        &a,
        &[Member::file("zeros", vec![0u8; 3 << 20])],
        Compression::Gzip,
    );
    let out = sb.mkdir("out");
    let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    let w = warning(&plan, WarningKind::ArchiveBomb).expect("flagged");
    assert!(w.message.contains("type yes"), "{}", w.message);
    // A small, ordinary archive is not.
    let small = sb.path("small.tar.gz");
    write_tar(&small, &[Member::file("a", "hello")], Compression::Gzip);
    let plan = plan_req(&e, &extract_req(&small, &out, ExtractInto::Here));
    assert!(warning(&plan, WarningKind::ArchiveBomb).is_none());
}

#[test]
fn a_member_that_holds_more_than_it_declares_is_stopped() {
    // The header of a tar says 5 bytes; the stream cannot be made to write more.
    let sb = Sandbox::new();
    let e = sb.engine();
    let a = sb.path("lie.zip");
    // A stored zip whose central directory claims 4 bytes while the data is 4: honest, then
    // flip the declared size in the plan to simulate a lying list.
    write_zip(&a, &[Member::file("f", "abcd")]);
    let out = sb.mkdir("out");
    let mut plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    for s in &mut plan.steps {
        if let Step::ExtractFile { size, .. } = s {
            *size = 2;
        }
    }
    let rep = run(&e, &plan);
    assert_eq!(rep.failed.len(), 1, "{:?}", rep.failed);
    assert!(rep.failed[0].error.contains("more than"), "{}", rep.failed[0].error);
    assert!(!out.join("f").exists());
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0, "no temporary file left");
}

#[test]
fn password_protected_members_are_reported_not_extracted() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let a = sb.path("secret.zip");
    write_zip(&a, &[Member::file("s.txt", "x").encrypted()]);
    let out = sb.mkdir("out");
    let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    assert!(!plan.is_executable());
    let w = warning(&plan, WarningKind::Encrypted).unwrap();
    assert_eq!(w.severity, Severity::Blocking);
    assert!(w.message.contains("password"));
}

#[test]
fn old_code_page_names_are_decoded_or_kept_safe() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let out = sb.mkdir("out");
    let a = sb.path("old.zip");
    // "caffè.txt" in CP437 (0x8A), no UTF-8 flag; and one in UTF-8 without the flag.
    write_zip(
        &a,
        &[
            Member::file("x", "1").raw_name(b"caff\x8a.txt").no_utf8_flag(),
            Member::file("y", "2").raw_name("più.txt".as_bytes()).no_utf8_flag(),
        ],
    );
    let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    assert!(run(&e, &plan).failed.is_empty());
    assert_eq!(read(&out.join("caffè.txt")), "1");
    assert_eq!(read(&out.join("più.txt")), "2");
}

#[test]
fn damaged_archives_say_so() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let out = sb.mkdir("out");
    // A zip cut in the middle: its directory is gone.
    let mut bytes = rada_core::archive::testkit::zip_bytes(&sample_members());
    bytes.truncate(bytes.len() / 2);
    let z = sb.path("cut.zip");
    std::fs::write(&z, &bytes).unwrap();
    let err = plan_try(&e, &extract_req(&z, &out, ExtractInto::Here)).unwrap_err();
    assert!(err.to_string().contains("damaged"), "{err}");
    // A tar.gz cut in the middle: what could be read is offered, with a warning.
    let t = sb.path("cut.tar.gz");
    let members: Vec<Member> = (0..40)
        .map(|i| Member::file(&format!("f{i:02}"), format!("{}", i).repeat(2000)))
        .collect();
    write_tar(&t, &members, Compression::Gzip);
    let full = std::fs::read(&t).unwrap();
    std::fs::write(&t, &full[..full.len() / 2]).unwrap();
    let plan = plan_req(&e, &extract_req(&t, &out, ExtractInto::Here));
    assert!(warning(&plan, WarningKind::ArchiveDamaged).is_some(), "{:?}", plan.warnings);
    // And a plain tar cut in the middle of a file.
    let p = sb.path("cut.tar");
    write_tar(&p, &members, Compression::None);
    let full = std::fs::read(&p).unwrap();
    std::fs::write(&p, &full[..full.len() / 2]).unwrap();
    let plan = plan_req(&e, &extract_req(&p, &out, ExtractInto::Here));
    assert!(warning(&plan, WarningKind::ArchiveDamaged).is_some(), "{:?}", plan.warnings);
}

// ---------------------------------------------------------------------- conflicts, undo

#[test]
fn conflicts_follow_the_usual_policies_and_overwrite_goes_through_the_trash() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    let out = sb.mkdir("out");
    std::fs::write(out.join("top.txt"), "old").unwrap();
    let a = sb.path("a.zip");
    write_zip(&a, &[Member::file("top.txt", "new"), Member::file("fresh.txt", "f")]);
    // skip
    let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    assert!(warning(&plan, WarningKind::Conflict).is_some());
    run(&e, &plan);
    assert_eq!(read(&out.join("top.txt")), "old");
    assert_eq!(read(&out.join("fresh.txt")), "f");
    std::fs::remove_file(out.join("fresh.txt")).unwrap();
    // keep both
    let mut req = extract_req(&a, &out, ExtractInto::Here);
    if let OpRequest::Extract { conflict, .. } = &mut req {
        *conflict = ConflictPolicy::KeepBoth;
    }
    run(&e, &plan_req(&e, &req));
    assert_eq!(read(&out.join("top.txt")), "old");
    assert_eq!(read(&out.join("top (1).txt")), "new");
    // overwrite, undone
    if let OpRequest::Extract { conflict, .. } = &mut req {
        *conflict = ConflictPolicy::Overwrite;
    }
    let plan = plan_req(&e, &req);
    assert!(warning(&plan, WarningKind::Overwrite).is_some());
    let o = run_journaled(&e, &j, &plan);
    assert_eq!(read(&out.join("top.txt")), "new");
    undo(&e, &j, &o.id);
    assert_eq!(read(&out.join("top.txt")), "old", "the old file came back from the trash");
}

#[test]
fn undoing_an_extraction_removes_exactly_what_it_made() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    let out = sb.mkdir("out");
    std::fs::write(out.join("mine.txt"), "mine").unwrap();
    let a = sb.path("a.tar.gz");
    write_tar(&a, &sample_members(), Compression::Gzip);
    let before = snapshot(&out);
    let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    let o = run_journaled(&e, &j, &plan);
    assert!(o.report.failed.is_empty());
    assert!(out.join("docs/sub/b.txt").exists());
    // One file is changed afterwards: it is left alone and reported.
    std::fs::write(out.join("top.txt"), "I edited this").unwrap();
    let (up, rep) = undo(&e, &j, &o.id);
    assert_eq!(up.blocked.len(), 1, "{:?}", up.blocked);
    assert!(rep.failed.is_empty());
    assert_eq!(read(&out.join("top.txt")), "I edited this");
    assert_eq!(read(&out.join("mine.txt")), "mine");
    assert!(!out.join("docs").exists());
    let _ = before;
}

#[test]
fn nothing_can_be_written_into_or_changed_inside_an_archive() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let a = sb.path("ro.zip");
    write_zip(&a, &[Member::file("f.txt", "x")]);
    let inside = a.join("f.txt");
    let src = sb.write("loose.txt", "y");
    for req in [
        OpRequest::Trash { sources: vec![inside.clone()] },
        OpRequest::Delete { sources: vec![inside.clone()] },
        OpRequest::Rename { path: inside.clone(), new_name: "g".into() },
        OpRequest::MakeDir { parent: a.clone(), name: "d".into() },
        OpRequest::Move { sources: vec![inside.clone()], destination: sb.path(""), conflict: ConflictPolicy::Skip, verify: false },
        OpRequest::Copy { sources: vec![src.clone()], destination: a.join("sub"), conflict: ConflictPolicy::Skip, verify: false },
    ] {
        // `a` itself is a regular file: only paths *inside* are refused.
        let r = plan_try(&e, &req);
        match &req {
            OpRequest::MakeDir { .. } => {}
            _ => assert!(r.is_err(), "{req:?} should be refused"),
        }
    }
}

#[test]
fn copying_selected_members_out_is_a_partial_extraction() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let out = sb.mkdir("out");
    let a = sb.path("a.tar.xz");
    write_tar(&a, &sample_members(), Compression::Xz);
    let req = OpRequest::Copy {
        sources: vec![a.join("docs/sub"), a.join("top.txt")],
        destination: out.clone(),
        conflict: ConflictPolicy::Skip,
        verify: false,
    };
    let plan = plan_req(&e, &req);
    assert_eq!(plan.kind, OpKind::Extract);
    let rep = run(&e, &plan);
    assert!(rep.failed.is_empty(), "{:?}\n{:#?}", rep.failed, plan.steps);
    assert_eq!(read(&out.join("sub/b.txt")), "beta");
    assert_eq!(read(&out.join("top.txt")), "top");
    assert!(!out.join("docs").exists(), "only what was selected comes out");
}

// ---------------------------------------------------------------------- 7z and rar

#[test]
fn seven_zip_archives_extract_when_the_tool_to_make_them_exists() {
    if !have_tool("7z") {
        eprintln!("skipped: no 7z to make a fixture");
        return;
    }
    let sb = Sandbox::new();
    let e = sb.engine();
    let src = sb.mkdir("src");
    std::fs::create_dir_all(src.join("d")).unwrap();
    std::fs::write(src.join("a.txt"), "hello").unwrap();
    std::fs::write(src.join("d/b.txt"), "x".repeat(5000)).unwrap();
    std::fs::write(src.join("empty"), "").unwrap();
    let a = sb.path("t.7z");
    let st = std::process::Command::new("7z")
        .args(["a", "-bso0", "-bsp0"])
        .arg(&a)
        .arg(src.join("a.txt"))
        .arg(src.join("d"))
        .arg(src.join("empty"))
        .status()
        .unwrap();
    assert!(st.success());
    let out = sb.mkdir("out");
    let plan = plan_req(&e, &extract_req(&a, &out, ExtractInto::Here));
    assert!(plan.is_executable(), "{:?}", plan.warnings);
    let rep = run(&e, &plan);
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    assert_eq!(read(&out.join("a.txt")), "hello");
    assert_eq!(read(&out.join("d/b.txt")).len(), 5000);
    assert_eq!(std::fs::metadata(out.join("empty")).unwrap().len(), 0);
}

#[test]
fn a_missing_rar_tool_is_said_plainly() {
    // Whatever is installed here, the message for "no program" must tell what to do.
    assert!(rada_core::archive::external::MISSING.contains("7-Zip"));
    assert!(rada_core::archive::external::MISSING.contains("unrar"));
}
