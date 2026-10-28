//! The names Windows keeps for devices (CON, NUL, COM1, LPT9 ...) in every form it treats as
//! the device: any case, with an extension, with trailing dots or spaces. Planned here with
//! the rules of Windows on whatever system runs the tests, for a rename, a new folder, a new
//! file and the members and the destination of an extraction.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use rada_core::archive::Compression;
use rada_core::archive::testkit::{Member, write_tar, write_zip};
use rada_core::ops::*;
use rada_core::platform::Platform;
use rada_core::platform::windows::WindowsPlatform;
use rada_core::testplatform::WithPathRules;
use rada_core::testutil::*;

const RESERVED: &[&str] = &[
    "CON",
    "con",
    "Con",
    "CON.txt",
    "con.tar.gz",
    "NUL.",
    "nul..",
    "NUL.txt.",
    "COM1",
    "com1.log",
    "LPT9",
    "lpt9.txt",
    "PRN",
    "aux.x",
    "COM9",
    "lpt1",
    // The device name stands before the dot, trailing spaces or not.
    "CON .txt",
    "nul  .log",
    // Superscript digits name devices too.
    "COM\u{b9}",
    "com\u{b2}.txt",
    "LPT\u{b3}",
];

const FINE: &[&str] = &[
    "console.txt",
    "CONSOLE",
    "com10",
    "COM1x",
    "lpt",
    "xcon",
    "a.con",
    "nul-1",
    "auxiliary.txt",
    "COM",
    "LPT10.txt",
];

/// The engine of the sandbox, with the rules for names of Windows.
fn windows_engine(sb: &Sandbox) -> Engine {
    let rules = WindowsPlatform::new(sb.platform().dirs().clone()).path_rules();
    Engine::local(Arc::new(WithPathRules::new(sb.platform(), rules)))
}

fn refused(plan: &Plan) -> bool {
    plan.warnings
        .iter()
        .any(|w| w.kind == WarningKind::InvalidName)
}

#[test]
fn a_rename_to_a_device_name_is_refused_in_every_form() {
    let sb = Sandbox::new();
    let e = windows_engine(&sb);
    sb.write("a.txt", "x");
    for name in RESERVED {
        let plan = e.plan_rename(&sb.path("a.txt"), OsStr::new(name), ConflictPolicy::Skip);
        assert!(refused(&plan) && plan.steps.is_empty(), "{name:?}");
    }
    for name in FINE {
        let plan = e.plan_rename(&sb.path("a.txt"), OsStr::new(name), ConflictPolicy::Skip);
        assert!(!refused(&plan), "{name:?}: {:?}", plan.warnings);
    }
}

#[test]
fn a_new_folder_or_file_with_a_device_name_is_refused_in_every_form() {
    let sb = Sandbox::new();
    let e = windows_engine(&sb);
    for name in RESERVED {
        assert!(
            refused(&e.plan_mkdir(&sb.path("."), OsStr::new(name))),
            "dir {name:?}"
        );
        assert!(
            refused(&e.plan_mkfile(&sb.path("."), OsStr::new(name))),
            "file {name:?}"
        );
    }
    for name in FINE {
        assert!(
            !refused(&e.plan_mkdir(&sb.path("."), OsStr::new(name))),
            "dir {name:?}"
        );
    }
}

#[test]
fn members_of_an_archive_with_a_device_name_are_not_extracted_in_any_form() {
    for zip in [false, true] {
        for name in RESERVED {
            let sb = Sandbox::new();
            let e = windows_engine(&sb);
            let out = sb.mkdir("out");
            let a = sb.path(if zip { "a.zip" } else { "a.tar" });
            // The name as a file, as a folder with a file in it, and one that is fine.
            let members = [
                Member::file(name, "x"),
                Member::file(&format!("dir/{name}"), "x"),
                Member::file(&format!("{name}/inside"), "x"),
                Member::file("fine.txt", "ok"),
            ];
            if zip {
                write_zip(&a, &members);
            } else {
                write_tar(&a, &members, Compression::None);
            }
            let plan = plan_of(&e, &extract(&a, &out, ExtractInto::Here)).unwrap();
            assert!(refused(&plan), "{name:?} zip={zip}");
            let extracted: Vec<_> = plan
                .steps
                .iter()
                .filter_map(|s| match s {
                    Step::ExtractFile { dst, .. } => dst.file_name().map(|n| n.to_owned()),
                    _ => None,
                })
                .collect();
            assert_eq!(extracted, [OsStr::new("fine.txt")], "{name:?} zip={zip}");
        }
    }
}

#[test]
fn the_folder_an_archive_extracts_into_is_not_a_device_name() {
    let sb = Sandbox::new();
    let e = windows_engine(&sb);
    let out = sb.mkdir("out");
    for stem in ["CON", "nul", "com1", "LPT9", "aux"] {
        let a = sb.path(format!("{stem}.zip"));
        write_zip(&a, &[Member::file("a", "1"), Member::file("b", "2")]);
        // Two files at the top: with `Auto` they go into a folder named after the archive.
        let plan = plan_of(&e, &extract(&a, &out, ExtractInto::Auto));
        assert!(
            plan.is_err(),
            "{stem}: a folder named after the archive is checked"
        );
    }
    let a = sb.path("fine.zip");
    write_zip(&a, &[Member::file("a", "1"), Member::file("b", "2")]);
    let into = ExtractInto::Folder { name: "NUL".into() };
    let plan = plan_of(&e, &extract(&a, &out, into));
    assert!(
        plan.is_err(),
        "a folder name chosen by the user is checked as well"
    );
}

fn extract(archive: &Path, dest: &Path, into: ExtractInto) -> OpRequest {
    OpRequest::Extract {
        archive: archive.to_path_buf(),
        destination: dest.to_path_buf(),
        into,
        only: Vec::new(),
        conflict: ConflictPolicy::Skip,
    }
}

fn plan_of(e: &Engine, req: &OpRequest) -> rada_core::Result<Plan> {
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
