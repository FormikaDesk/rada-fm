//! Creating archives, and the round trip: compress, extract, get the same tree back.
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use proptest::prelude::*;
use rada_core::archive::ArchiveKind;
use rada_core::archive::testkit::have_tool;
use rada_core::ops::*;
use rada_core::testutil::*;

fn plan_of(e: &Engine, req: &OpRequest) -> Plan {
    let cancel = Cancel::new();
    e.plan_request(
        req,
        None,
        ScanControl {
            cancel: cancel.flag(),
            progress: &mut |_| {},
        },
    )
    .unwrap_or_else(|err| panic!("{req:?}: {err}"))
    .plan
}

fn compress_req(sources: &[PathBuf], archive: &Path, format: ArchiveKind) -> OpRequest {
    OpRequest::Compress {
        sources: sources.to_vec(),
        archive: archive.to_path_buf(),
        format,
        conflict: ConflictPolicy::Skip,
    }
}

fn extract_req(archive: &Path, dest: &Path) -> OpRequest {
    OpRequest::Extract {
        archive: archive.to_path_buf(),
        destination: dest.to_path_buf(),
        into: ExtractInto::Auto,
        only: Vec::new(),
        conflict: ConflictPolicy::Skip,
    }
}

fn has(p: &Plan, k: WarningKind) -> bool {
    p.warnings.iter().any(|w| w.kind == k)
}

/// kinds, permissions, contents, link targets and times (to the second) below `root`.
fn deep(root: &Path) -> BTreeMap<PathBuf, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, String>) {
        let mut items: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            let m = std::fs::symlink_metadata(&p).unwrap();
            let mode = m.mode() & 0o7777;
            let desc = if m.file_type().is_symlink() {
                format!("link -> {}", std::fs::read_link(&p).unwrap().display())
            } else if m.is_dir() {
                format!("dir {mode:o} mtime={}", m.mtime())
            } else {
                let c = std::fs::read(&p).unwrap();
                format!(
                    "file {mode:o} len={} data={:x?} mtime={}",
                    c.len(),
                    &c[..c.len().min(64)],
                    m.mtime()
                )
            };
            out.insert(rel, desc);
            if m.is_dir() && !m.file_type().is_symlink() {
                walk(root, &p, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn unlock(root: &Path) {
    if let Ok(rd) = std::fs::read_dir(root) {
        let _ = std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o755));
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && !p.is_symlink() {
                unlock(&p);
            }
        }
    }
}

struct Unlock(PathBuf);
impl Drop for Unlock {
    fn drop(&mut self) {
        unlock(&self.0);
    }
}

fn fixture(sb: &Sandbox) -> PathBuf {
    let t = sb.mkdir("src/proj");
    std::fs::create_dir_all(t.join("sub/deep")).unwrap();
    std::fs::write(t.join("a.txt"), "alpha\n".repeat(300)).unwrap();
    std::fs::write(
        t.join("sub/b.bin"),
        (0..5000u32).map(|i| (i * 7) as u8).collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(t.join("sub/deep/empty"), "").unwrap();
    std::fs::set_permissions(t.join("a.txt"), std::fs::Permissions::from_mode(0o640)).unwrap();
    std::os::unix::fs::symlink("a.txt", t.join("link")).unwrap();
    std::os::unix::fs::symlink("missing", t.join("broken")).unwrap();
    t
}

#[test]
fn every_kind_makes_an_archive_other_programs_can_read() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let src = fixture(&sb);
    for kind in ArchiveKind::ALL {
        let a = sb.path(format!("out{}", kind.extension()));
        let plan = plan_of(&e, &compress_req(std::slice::from_ref(&src), &a, kind));
        assert!(plan.is_executable(), "{kind:?}: {:?}", plan.warnings);
        assert!(plan.estimated_bytes.is_some());
        let rep = run(&e, &plan);
        assert!(rep.failed.is_empty(), "{kind:?}: {:?}", rep.failed);
        assert!(a.is_file());
        // bsdtar reads zip, tar.gz, tar.xz and tar.zst: an independent check.
        if have_tool("bsdtar") {
            let out = Command::new("bsdtar").arg("-tf").arg(&a).output().unwrap();
            assert!(
                out.status.success(),
                "{kind:?}: bsdtar: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let list = String::from_utf8_lossy(&out.stdout);
            assert!(list.contains("proj/sub/b.bin"), "{kind:?}: {list}");
        }
        match kind {
            ArchiveKind::Zip if have_tool("unzip") => {
                let st = Command::new("unzip").arg("-tq").arg(&a).output().unwrap();
                assert!(
                    st.status.success(),
                    "unzip -t: {}",
                    String::from_utf8_lossy(&st.stdout)
                );
            }
            ArchiveKind::TarZst if have_tool("zstd") => {
                let st = Command::new("zstd").arg("-tq").arg(&a).output().unwrap();
                assert!(
                    st.status.success(),
                    "zstd -t: {}",
                    String::from_utf8_lossy(&st.stderr)
                );
            }
            ArchiveKind::TarXz if have_tool("xz") => {
                let st = Command::new("xz").arg("-tq").arg(&a).output().unwrap();
                assert!(
                    st.status.success(),
                    "xz -t: {}",
                    String::from_utf8_lossy(&st.stderr)
                );
            }
            ArchiveKind::TarGz if have_tool("gzip") => {
                let st = Command::new("gzip").arg("-tq").arg(&a).output().unwrap();
                assert!(st.status.success());
            }
            _ => {}
        }
    }
}

#[test]
fn a_program_made_archive_of_the_same_tree_reads_back_identically() {
    // The other direction: archives made by system tools, extracted by rada.
    if !have_tool("bsdtar") {
        return;
    }
    let sb = Sandbox::new();
    let e = sb.engine();
    let src = fixture(&sb);
    for (ext, flag) in [
        ("tar.gz", "--gzip"),
        ("tar.xz", "--xz"),
        ("tar.zst", "--zstd"),
        ("tar.bz2", "--bzip2"),
    ] {
        let a = sb.path(format!("sys.{ext}"));
        let st = Command::new("bsdtar")
            .args(["-cf"])
            .arg(&a)
            .arg(flag)
            .arg("-C")
            .arg(src.parent().unwrap())
            .arg("proj")
            .status()
            .unwrap();
        assert!(st.success());
        let out = sb.mkdir(format!("sysout-{ext}"));
        let plan = plan_of(&e, &extract_req(&a, &out));
        let rep = run(&e, &plan);
        assert!(rep.failed.is_empty(), "{ext}: {:?}", rep.failed);
        let want = deep(&sb.path("src"));
        let got = deep(&out);
        assert_eq!(got, want, "{ext}");
    }
}

#[test]
fn compress_then_extract_gives_the_same_tree_in_every_format() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let src = fixture(&sb);
    let want = deep(&sb.path("src"));
    for kind in ArchiveKind::ALL {
        let a = sb.path(format!("rt{}", kind.extension()));
        run(
            &e,
            &plan_of(&e, &compress_req(std::slice::from_ref(&src), &a, kind)),
        );
        let out = sb.mkdir(format!("rt-out-{}", kind.label()));
        let rep = run(&e, &plan_of(&e, &extract_req(&a, &out)));
        assert!(rep.failed.is_empty(), "{kind:?}: {:?}", rep.failed);
        assert_eq!(deep(&out), want, "{kind:?}");
    }
}

#[test]
fn zip_says_it_stores_links_and_tar_does_not_need_to() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let src = fixture(&sb);
    let z = plan_of(
        &e,
        &compress_req(
            std::slice::from_ref(&src),
            &sb.path("a.zip"),
            ArchiveKind::Zip,
        ),
    );
    assert!(has(&z, WarningKind::ZipLinks));
    let t = plan_of(
        &e,
        &compress_req(&[src], &sb.path("a.tar.gz"), ArchiveKind::TarGz),
    );
    assert!(!has(&t, WarningKind::ZipLinks));
}

#[test]
fn an_existing_archive_is_a_conflict_with_the_usual_choices() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    let src = fixture(&sb);
    let a = sb.path("same.tar.gz");
    std::fs::write(&a, "old archive").unwrap();
    let mut req = compress_req(&[src], &a, ArchiveKind::TarGz);
    // skip: nothing to do, and it says why
    let plan = plan_of(&e, &req);
    assert!(!plan.is_executable());
    // keep both
    let OpRequest::Compress { conflict, .. } = &mut req else {
        unreachable!()
    };
    *conflict = ConflictPolicy::KeepBoth;
    let plan = plan_of(&e, &req);
    assert_eq!(
        plan.destination.as_deref(),
        Some(sb.path("same (1).tar.gz").as_path())
    );
    // overwrite: the old one goes to the trash, undo brings it back
    let OpRequest::Compress { conflict, .. } = &mut req else {
        unreachable!()
    };
    *conflict = ConflictPolicy::Overwrite;
    let plan = plan_of(&e, &req);
    let out = run_journaled(&e, &j, &plan);
    assert!(out.report.failed.is_empty(), "{:?}", out.report.failed);
    assert_ne!(std::fs::read(&a).unwrap(), b"old archive");
    undo(&e, &j, &out.id);
    assert_eq!(std::fs::read(&a).unwrap(), b"old archive");
}

#[test]
fn the_archive_is_never_part_of_itself_and_undo_removes_it() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    let src = fixture(&sb);
    // Made inside the folder it archives.
    let a = src.join("inside.zip");
    let plan = plan_of(
        &e,
        &compress_req(std::slice::from_ref(&src), &a, ArchiveKind::Zip),
    );
    let o = run_journaled(&e, &j, &plan);
    assert!(o.report.failed.is_empty(), "{:?}", o.report.failed);
    let out = sb.mkdir("o");
    run(&e, &plan_of(&e, &extract_req(&a, &out)));
    assert!(!out.join("proj/inside.zip").exists());
    assert!(out.join("proj/a.txt").exists());
    // No temporary file is left next to it.
    let leftovers: Vec<_> = std::fs::read_dir(&src)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".rada-part"))
        .collect();
    assert!(leftovers.is_empty());
    let (up, rep) = undo(&e, &j, &o.id);
    assert!(up.blocked.is_empty() && rep.failed.is_empty());
    assert!(!a.exists());
}

#[test]
fn cancelling_a_compression_removes_the_half_written_archive() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    let big = sb.mkdir("big");
    // Enough data that the copy loop reports progress several times.
    let data: Vec<u8> = (0..(24u32 << 20))
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    std::fs::write(big.join("blob"), &data).unwrap();
    let a = sb.path("big.tar.gz");
    let plan = plan_of(&e, &compress_req(&[big], &a, ArchiveKind::TarGz));
    struct Stopper(Cancel);
    impl ExecHandler for Stopper {
        fn progress(&mut self, p: &Progress) {
            if p.bytes_done > 4 << 20 {
                self.0.cancel();
            }
        }
    }
    let cancel = Cancel::new();
    let o = e
        .run_operation(&j, &plan, &mut Stopper(cancel.clone()), &cancel)
        .unwrap();
    assert!(o.report.cancelled);
    assert!(!a.exists(), "no archive under its final name");
    let leftovers: Vec<_> = std::fs::read_dir(sb.path(""))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".rada-part"))
        .collect();
    assert!(leftovers.is_empty(), "no temporary file either");
}

#[test]
fn progress_is_counted_in_real_bytes_and_the_archive_can_be_cancelled_at_any_time() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let src = fixture(&sb);
    let a = sb.path("p.tar.zst");
    let plan = plan_of(&e, &compress_req(&[src], &a, ArchiveKind::TarZst));
    let want = plan.total_bytes();
    assert!(want > 5000);
    let mut h = Scripted::new(ErrorChoice::Skip);
    let rep = e.execute(&plan, &mut h, &Cancel::new());
    assert_eq!(rep.bytes, want);
    assert_eq!(h.progress.last().unwrap().bytes_done, want);
}

// --------------------------------------------------------------------------- properties

fn name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_string()),
        Just("b c".to_string()),
        Just(".hidden".to_string()),
        Just("ünï-cödé".to_string()),
        Just("日本語".to_string()),
        Just("x.tar.gz".to_string()),
        Just("-dash".to_string()),
        Just("it's".to_string()),
        Just("UPPER".to_string()),
        Just("two\nlines".to_string()),
        Just(format!("long_{}", "n".repeat(120))),
        "[a-z]{1,6}".prop_map(|s| s),
    ]
}

#[derive(Clone, Debug)]
enum Kind {
    Dir { read_only: bool },
    File { data: Vec<u8>, mode: u32 },
    Symlink(String),
}

#[derive(Clone, Debug)]
struct Node {
    parent: usize,
    name: String,
    kind: Kind,
}

fn tree() -> impl Strategy<Value = Vec<Node>> {
    let node = (
        0usize..6,
        name(),
        prop_oneof![
            3 => any::<bool>().prop_map(|read_only| Kind::Dir { read_only }),
            5 => (proptest::collection::vec(any::<u8>(), 0..300), prop_oneof![Just(0o644u32), Just(0o600), Just(0o755), Just(0o444), Just(0o4755)])
                .prop_map(|(data, mode)| Kind::File { data, mode }),
            2 => name().prop_map(Kind::Symlink),
        ],
    )
        .prop_map(|(parent, name, kind)| Node { parent, name, kind });
    proptest::collection::vec(node, 1..14)
}

fn build(root: &Path, nodes: &[Node]) {
    let mut dirs = vec![root.to_path_buf()];
    let mut ro = Vec::new();
    for n in nodes {
        let path = dirs[n.parent % dirs.len()].join(&n.name);
        if std::fs::symlink_metadata(&path).is_ok() {
            continue;
        }
        match &n.kind {
            Kind::Dir { read_only } => {
                std::fs::create_dir(&path).unwrap();
                if *read_only {
                    ro.push(path.clone());
                }
                dirs.push(path);
            }
            Kind::File { data, mode } => {
                std::fs::write(&path, data).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(*mode)).unwrap();
            }
            Kind::Symlink(t) => std::os::unix::fs::symlink(t, &path).unwrap(),
        }
    }
    for d in ro.iter().rev() {
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
}

fn cfg() -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(24),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(cfg())]

    /// compress → extract returns the same tree (contents, permissions — setuid aside, which
    /// extraction drops on purpose —, symlinks and times) for random trees, in every format.
    #[test]
    fn compress_then_extract_is_the_identity(nodes in tree()) {
        let sb = Sandbox::new();
        let e = sb.engine();
        sb.mkdir("src/t");
        let _u = (Unlock(sb.path("src")), Unlock(sb.path("out")));
        build(&sb.path("src/t"), &nodes);
        // Setuid and setgid bits are removed when extracting; the original is compared without.
        fn strip(m: BTreeMap<PathBuf, String>) -> BTreeMap<PathBuf, String> {
            m.into_iter()
                .map(|(k, v)| (k, v.replace("file 4755", "file 755")))
                .collect()
        }
        let want = strip(deep(&sb.path("src")));
        for kind in ArchiveKind::ALL {
            let a = sb.path(format!("rt{}", kind.extension()));
            let plan = plan_of(&e, &compress_req(&[sb.path("src/t")], &a, kind));
            prop_assert!(plan.is_executable(), "{kind:?} {:?}", plan.warnings);
            let rep = run(&e, &plan);
            prop_assert!(rep.failed.is_empty(), "{kind:?} {:?}", rep.failed);
            let out = sb.mkdir(format!("out/{}", kind.label()));
            let plan = plan_of(&e, &extract_req(&a, &out));
            prop_assert!(plan.is_executable(), "{kind:?} {:?}", plan.warnings);
            let rep = run(&e, &plan);
            prop_assert!(rep.failed.is_empty(), "{kind:?} {:?}", rep.failed);
            prop_assert_eq!(strip(deep(&out)), want.clone(), "{:?}", kind);
        }
    }
}
