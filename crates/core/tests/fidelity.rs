//! What a copy keeps besides the bytes: permissions and times, extended attributes and ACLs,
//! hard links between the copied files, sparse files. And that undo brings everything back.
#![cfg(target_os = "linux")]

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rada_core::fs::{CopyControl, CopyMethod, CopyRequest, FsEngine, LocalFs};
use rada_core::ops::*;
use rada_core::platform::{
    AttrOutcome, DestSupport, Dirs, Fidelity, FileAttributes, Opener, PathRules, Platform,
    Preserve, SourceAttrs, TrashBackend, UserDirs, VolumeLister,
};
use rada_core::testutil::*;

// ------------------------------------------------------------------------------ helpers

fn set_xattr(p: &Path, name: &str, value: &[u8]) -> bool {
    let c = CString::new(p.as_os_str().as_bytes()).unwrap();
    let n = CString::new(name).unwrap();
    // SAFETY: valid C strings and value buffer.
    unsafe {
        libc::lsetxattr(
            c.as_ptr(),
            n.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        ) == 0
    }
}

fn get_xattr(p: &Path, name: &str) -> Option<Vec<u8>> {
    let c = CString::new(p.as_os_str().as_bytes()).unwrap();
    let n = CString::new(name).unwrap();
    let mut buf = vec![0u8; 4096];
    // SAFETY: valid C strings; `buf` is valid for its length.
    let r = unsafe { libc::lgetxattr(c.as_ptr(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    (r >= 0).then(|| {
        buf.truncate(r as usize);
        buf
    })
}

/// A valid POSIX ACL (version 2): owner rw-, one named user rw-, group r--, mask rw-, other r--.
fn acl_for(uid: u32) -> Vec<u8> {
    let mut v = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (0x01u16, 6u16, u32::MAX),
        (0x02, 6, uid),
        (0x04, 4, u32::MAX),
        (0x10, 6, u32::MAX),
        (0x20, 4, u32::MAX),
    ] {
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&perm.to_le_bytes());
        v.extend_from_slice(&id.to_le_bytes());
    }
    v
}

fn ino(p: &Path) -> (u64, u64) {
    let m = std::fs::symlink_metadata(p).unwrap();
    (m.dev(), m.ino())
}

fn nlink(p: &Path) -> u64 {
    std::fs::symlink_metadata(p).unwrap().nlink()
}

fn stored(p: &Path) -> u64 {
    std::fs::symlink_metadata(p).unwrap().blocks() * 512
}

fn copy_tree(sb: &Sandbox, e: &Engine, src: &Path, dest: &Path) -> (Plan, ExecReport) {
    let _ = sb;
    do_copy(e, &[src.to_path_buf()], dest, ConflictPolicy::Skip)
}

/// A platform that answers the fidelity questions as told (and delegates everything else).
struct Told {
    inner: Arc<dyn Platform>,
    support: DestSupport,
    can_own: bool,
    calls: Mutex<Vec<PathBuf>>,
}

impl Fidelity for Told {
    fn implemented(&self) -> bool {
        true
    }
    fn destination_support(&self, _dir: &Path) -> DestSupport {
        self.support
    }
    fn source_attributes(&self, path: &Path) -> SourceAttrs {
        self.inner.fidelity().source_attributes(path)
    }
    fn can_set_owner(&self, _u: u32, _g: u32) -> bool {
        self.can_own
    }
    fn copy_attributes(
        &self,
        s: &Path,
        d: &Path,
        w: Preserve,
        o: Option<(u32, u32)>,
    ) -> AttrOutcome {
        self.calls.lock().unwrap().push(d.to_path_buf());
        self.inner
            .fidelity()
            .copy_attributes(s, d, w, if self.can_own { o } else { None })
    }
}

struct TellingPlatform(Arc<dyn Platform>, Arc<Told>);

impl Platform for TellingPlatform {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn dirs(&self) -> &Dirs {
        self.0.dirs()
    }
    fn trash(&self) -> &dyn TrashBackend {
        self.0.trash()
    }
    fn volumes(&self) -> &dyn VolumeLister {
        self.0.volumes()
    }
    fn attributes(&self) -> &dyn FileAttributes {
        self.0.attributes()
    }
    fn opener(&self) -> &dyn Opener {
        self.0.opener()
    }
    fn path_rules(&self) -> PathRules {
        self.0.path_rules()
    }
    fn user_dirs(&self) -> UserDirs {
        self.0.user_dirs()
    }
    fn fidelity(&self) -> &dyn Fidelity {
        &*self.1
    }
}

fn told_engine(sb: &Sandbox, support: DestSupport, can_own: bool) -> (Engine, Arc<Told>) {
    let told = Arc::new(Told {
        inner: sb.platform(),
        support,
        can_own,
        calls: Mutex::new(Vec::new()),
    });
    let p: Arc<dyn Platform> = Arc::new(TellingPlatform(sb.platform(), told.clone()));
    (Engine::local(p), told)
}

fn warning(plan: &Plan, kind: WarningKind) -> Option<&Warning> {
    plan.warnings.iter().find(|w| w.kind == kind)
}

// ------------------------------------------------------------------------------ permissions, times

#[test]
fn a_copy_keeps_permissions_including_special_bits_and_both_times() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("src/tool", "#!/bin/sh\n");
    chmod(&f, 0o4755);
    let when = std::time::UNIX_EPOCH + std::time::Duration::new(1_500_000_000, 123_000_000);
    let h = std::fs::File::options().write(true).open(&f).unwrap();
    h.set_times(
        std::fs::FileTimes::new()
            .set_modified(when)
            .set_accessed(when),
    )
    .unwrap();
    drop(h);
    let dest = sb.mkdir("dest");
    let (_plan, rep) = copy_tree(&sb, &e, &sb.path("src"), &dest);
    assert!(rep.failed.is_empty(), "{rep:?}");
    let copy = dest.join("src/tool");
    let m = std::fs::metadata(&copy).unwrap();
    assert_eq!(m.mode() & 0o7777, 0o4755, "setuid and the rest survive");
    assert_eq!(m.modified().unwrap(), when);
    assert_eq!(m.accessed().unwrap(), when);
}

#[test]
fn a_read_only_file_still_gets_its_extended_attributes() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("src/ro.txt", "data");
    if !set_xattr(&f, "user.rada.note", b"kept") {
        return; // this filesystem has no user attributes
    }
    chmod(&f, 0o444);
    let dest = sb.mkdir("dest");
    let (_p, rep) = copy_tree(&sb, &e, &sb.path("src"), &dest);
    assert!(
        rep.failed.is_empty() && rep.not_preserved.is_empty(),
        "{rep:?}"
    );
    let copy = dest.join("src/ro.txt");
    assert_eq!(std::fs::metadata(&copy).unwrap().mode() & 0o777, 0o444);
    assert_eq!(
        get_xattr(&copy, "user.rada.note").as_deref(),
        Some(&b"kept"[..])
    );
}

// ------------------------------------------------------------------------------ xattr and ACL

#[test]
fn extended_attributes_of_files_and_folders_are_copied() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("src/sub/doc.txt", "x");
    let dir = sb.path("src/sub");
    if !set_xattr(&f, "user.rada.tag", b"blue") {
        return;
    }
    assert!(set_xattr(&dir, "user.rada.folder", b"yes"));
    assert!(set_xattr(&sb.path("src"), "user.rada.root", b"top"));
    let dest = sb.mkdir("dest");
    let (plan, rep) = copy_tree(&sb, &e, &sb.path("src"), &dest);
    assert!(
        rep.failed.is_empty() && rep.not_preserved.is_empty(),
        "{rep:?}"
    );
    assert_eq!(plan.preserved.xattr_items, 3, "{:?}", plan.preserved);
    assert_eq!(
        get_xattr(&dest.join("src/sub/doc.txt"), "user.rada.tag").as_deref(),
        Some(&b"blue"[..])
    );
    assert_eq!(
        get_xattr(&dest.join("src/sub"), "user.rada.folder").as_deref(),
        Some(&b"yes"[..])
    );
    assert_eq!(
        get_xattr(&dest.join("src"), "user.rada.root").as_deref(),
        Some(&b"top"[..])
    );
}

#[test]
fn posix_acls_are_copied_byte_for_byte() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("src/shared.txt", "x");
    let uid = unsafe { libc::geteuid() };
    if !set_xattr(&f, "system.posix_acl_access", &acl_for(uid)) {
        return; // no ACL support here
    }
    let want = get_xattr(&f, "system.posix_acl_access").unwrap();
    let dest = sb.mkdir("dest");
    let (plan, rep) = copy_tree(&sb, &e, &sb.path("src"), &dest);
    assert!(
        rep.failed.is_empty() && rep.not_preserved.is_empty(),
        "{rep:?}"
    );
    assert_eq!(plan.preserved.acl_items, 1);
    assert_eq!(
        get_xattr(&dest.join("src/shared.txt"), "system.posix_acl_access"),
        Some(want)
    );
}

#[test]
fn the_plan_says_before_confirming_what_the_destination_cannot_hold() {
    let sb = Sandbox::new();
    let f = sb.write("src/a.txt", "x");
    let g = sb.write("src/b.txt", "y");
    sb.write("src/plain.txt", "z");
    if !set_xattr(&f, "user.rada.tag", b"v") {
        return;
    }
    let uid = unsafe { libc::geteuid() };
    let acl_ok = set_xattr(&g, "system.posix_acl_access", &acl_for(uid));
    let (e, _t) = told_engine(
        &sb,
        DestSupport {
            xattrs: false,
            acl: false,
        },
        true,
    );
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let w = warning(&plan, WarningKind::XattrsLost).expect("a warning about extended attributes");
    assert_eq!(w.severity, Severity::Warning);
    assert!(
        w.count >= 1 && w.examples.iter().any(|p| p.ends_with("a.txt")),
        "{w:?}"
    );
    if acl_ok {
        let w = warning(&plan, WarningKind::AclLost).expect("a warning about ACLs");
        assert!(w.examples.iter().any(|p| p.ends_with("b.txt")), "{w:?}");
    }
    // And the plan is still executable: a lossy copy is the user's choice, never a blocker.
    assert!(plan.is_executable());
}

#[test]
fn a_destination_that_supports_everything_gets_no_attribute_warning() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("src/a.txt", "x");
    if !set_xattr(&f, "user.rada.tag", b"v") {
        return;
    }
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(
        warning(&plan, WarningKind::XattrsLost).is_none()
            && warning(&plan, WarningKind::AclLost).is_none(),
        "{:?}",
        plan.warnings
    );
}

#[test]
fn owner_that_cannot_be_kept_is_a_note_in_the_plan_never_an_error() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "x");
    let (e, told) = told_engine(
        &sb,
        DestSupport {
            xattrs: true,
            acl: true,
        },
        false,
    );
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let w = warning(&plan, WarningKind::OwnerNotKept).expect("owner note");
    assert_eq!(w.severity, Severity::Info);
    assert!(plan.is_executable());
    let rep = run(&e, &plan);
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert!(
        !told.calls.lock().unwrap().is_empty(),
        "attributes were still carried over"
    );
}

#[test]
fn a_platform_without_attribute_support_says_so_once() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "x");
    let e = sb.engine();
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert!(
        warning(&plan, WarningKind::AttributesNotKept).is_none(),
        "Linux does it"
    );
    assert!(
        !rada_core::platform::NotYet.implemented(),
        "the stubs say they do nothing"
    );
}

// ------------------------------------------------------------------------------ hard links

fn linked_tree(sb: &Sandbox) {
    let x = sb.write("a/x", "same inode");
    std::fs::hard_link(&x, sb.path("a/y")).unwrap();
    std::fs::create_dir_all(sb.path("a/deep")).unwrap();
    std::fs::hard_link(&x, sb.path("a/deep/w")).unwrap();
    sb.write("a/z", "separate");
}

#[test]
fn files_sharing_an_inode_are_recreated_as_hard_links() {
    let sb = Sandbox::new();
    let e = sb.engine();
    linked_tree(&sb);
    let dest = sb.mkdir("dest");
    let (plan, rep) = copy_tree(&sb, &e, &sb.path("a"), &dest);
    assert!(rep.failed.is_empty(), "{rep:?}");
    let (x, y, w, z) = (
        dest.join("a/x"),
        dest.join("a/y"),
        dest.join("a/deep/w"),
        dest.join("a/z"),
    );
    assert_eq!(ino(&x), ino(&y));
    assert_eq!(ino(&x), ino(&w));
    assert_ne!(ino(&x), ino(&z));
    assert_eq!(nlink(&x), 3);
    assert_eq!(plan.preserved.hard_links, 2);
    let w = warning(&plan, WarningKind::HardLinksKept)
        .expect("the plan says how many links stay links");
    assert_eq!(w.count, 2);
    assert!(
        plan.steps
            .iter()
            .filter(|s| matches!(s, Step::HardLink { .. }))
            .count()
            == 2
    );
    // The sources are untouched and still linked to each other, not to the copy.
    assert_eq!(nlink(&sb.path("a/x")), 3);
    assert_ne!(ino(&sb.path("a/x")), ino(&x));
}

#[test]
fn links_to_files_outside_the_selection_make_independent_copies() {
    let sb = Sandbox::new();
    let e = sb.engine();
    linked_tree(&sb);
    let dest = sb.mkdir("dest");
    // Only one name of the three is selected.
    let (plan, rep) = do_copy(&e, &[sb.path("a/x")], &dest, ConflictPolicy::Skip);
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert_eq!(nlink(&dest.join("x")), 1);
    let w = warning(&plan, WarningKind::HardLinks).expect("said in the plan");
    assert!(w.message.contains("outside the selection"), "{}", w.message);
    assert!(warning(&plan, WarningKind::HardLinksKept).is_none());
}

#[test]
fn undoing_a_copy_with_hard_links_removes_every_name_and_nothing_else() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let j = sb.journal();
    linked_tree(&sb);
    let before = snapshot(&sb.path("a"));
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("a")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(out.report.failed.is_empty(), "{:?}", out.report);
    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert!(
        snapshot(&dest).is_empty(),
        "everything the copy made is gone"
    );
    assert_eq!(snapshot(&sb.path("a")), before);
    assert_eq!(nlink(&sb.path("a/x")), 3, "the sources keep their links");
}

#[test]
fn a_move_across_filesystems_keeps_the_links_and_its_undo_restores_them() {
    let sb = Sandbox::new();
    let Some(other) = other_filesystem_dir(&sb) else {
        return; // one filesystem only: nothing to cross
    };
    let e = sb.engine();
    let j = sb.journal();
    linked_tree(&sb);
    let before = snapshot(&sb.path("a"));
    let dest = other.path().to_path_buf();
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("a")]),
        &dest,
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(out.report.failed.is_empty(), "{:?}", out.report);
    assert!(!sb.path("a").exists(), "moved away");
    let (x, y, w) = (dest.join("a/x"), dest.join("a/y"), dest.join("a/deep/w"));
    assert_eq!(ino(&x), ino(&y));
    assert_eq!(ino(&x), ino(&w));

    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert_eq!(
        snapshot(&sb.path("a")),
        before,
        "back where and what it was"
    );
    let (x, y, w) = (sb.path("a/x"), sb.path("a/y"), sb.path("a/deep/w"));
    assert_eq!(ino(&x), ino(&y), "still hard links of each other");
    assert_eq!(ino(&x), ino(&w));
    assert_eq!(nlink(&x), 3);
    assert!(
        snapshot(&dest).is_empty(),
        "nothing left at the destination: {:?}",
        snapshot(&dest).keys().collect::<Vec<_>>()
    );
}

#[test]
fn a_destination_that_cannot_link_gets_separate_copies_and_a_note() {
    let sb = Sandbox::new();
    let x = sb.write("a/x", "same");
    std::fs::hard_link(&x, sb.path("a/y")).unwrap();
    let ffs = FaultFs::new();
    ffs.fail(Op::HardLink, "y", libc::EPERM);
    let e = sb.engine_with(ffs);
    let dest = sb.mkdir("dest");
    let (_plan, rep) = do_copy(&e, &[sb.path("a")], &dest, ConflictPolicy::Skip);
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert_eq!(std::fs::read(dest.join("a/y")).unwrap(), b"same");
    assert_ne!(ino(&dest.join("a/x")), ino(&dest.join("a/y")));
    assert!(
        rep.not_preserved
            .iter()
            .any(|(_, what)| what.contains("hard link")),
        "{:?}",
        rep.not_preserved
    );
}

// ------------------------------------------------------------------------------ sparse files

const MIB: u64 = 1 << 20;

fn sparse_file(path: &Path, size: u64, data: &[(u64, u64)]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = std::fs::File::create(path).unwrap();
    f.set_len(size).unwrap();
    for (off, len) in data {
        f.write_all_at(&vec![0xAB; *len as usize], *off).unwrap();
    }
}

fn holes_supported(dir: &Path) -> bool {
    let p = dir.join(".probe-holes");
    sparse_file(&p, 8 * MIB, &[(0, 4096)]);
    let ok = stored(&p) < 8 * MIB / 2;
    let _ = std::fs::remove_file(&p);
    ok
}

struct Count(u64);
impl CopyControl for Count {
    fn advance(&mut self, n: u64) -> bool {
        self.0 += n;
        true
    }
}

#[test]
fn a_sparse_file_keeps_its_holes_and_progress_counts_the_real_bytes() {
    let sb = Sandbox::new();
    if !holes_supported(&sb.root) {
        return;
    }
    let src = sb.path("big.img");
    sparse_file(
        &src,
        64 * MIB,
        &[(0, MIB), (32 * MIB, MIB), (64 * MIB - 4096, 4096)],
    );
    let dst = sb.path("copy.img");
    let mut ctl = Count(0);
    let out = LocalFs
        .copy_file(
            &CopyRequest {
                src: src.clone(),
                dst: dst.clone(),
                mode: None,
                mtime: None,
                atime: None,
                verify: false,
                sync: false,
            },
            &mut ctl,
        )
        .unwrap();
    assert_eq!(out.method, CopyMethod::Sparse);
    let real = 2 * MIB + 4096;
    assert_eq!(out.bytes, real, "only the data is counted");
    assert_eq!(ctl.0, real);
    assert_eq!(std::fs::metadata(&dst).unwrap().len(), 64 * MIB);
    assert!(
        stored(&dst) < 8 * MIB,
        "the copy is sparse too: {} bytes stored",
        stored(&dst)
    );
    assert_eq!(
        std::fs::read(&src).unwrap(),
        std::fs::read(&dst).unwrap(),
        "same content, holes read as zeros"
    );
}

#[test]
fn a_file_that_is_all_hole_and_a_verified_sparse_copy_both_work() {
    let sb = Sandbox::new();
    if !holes_supported(&sb.root) {
        return;
    }
    let hole = sb.path("hole.img");
    sparse_file(&hole, 16 * MIB, &[]);
    let h2 = sb.path("hole2.img");
    let out = LocalFs
        .copy_file(
            &CopyRequest {
                src: hole.clone(),
                dst: h2.clone(),
                mode: None,
                mtime: None,
                atime: None,
                verify: false,
                sync: false,
            },
            &mut Count(0),
        )
        .unwrap();
    assert_eq!((out.method, out.bytes), (CopyMethod::Sparse, 0));
    assert_eq!(std::fs::metadata(&h2).unwrap().len(), 16 * MIB);

    let src = sb.path("v.img");
    sparse_file(&src, 16 * MIB, &[(MIB, 8192)]);
    let dst = sb.path("v-copy.img");
    LocalFs
        .copy_file(
            &CopyRequest {
                src: src.clone(),
                dst: dst.clone(),
                mode: None,
                mtime: None,
                atime: None,
                verify: true,
                sync: true,
            },
            &mut Count(0),
        )
        .unwrap();
    assert_eq!(std::fs::read(&src).unwrap(), std::fs::read(&dst).unwrap());
    assert!(stored(&dst) < 4 * MIB);
}

#[test]
fn the_plan_counts_real_bytes_for_sparse_files_and_undo_removes_the_copy() {
    let sb = Sandbox::new();
    if !holes_supported(&sb.root) {
        return;
    }
    let e = sb.engine();
    let j = sb.journal();
    sparse_file(&sb.path("src/disk.img"), 100 * MIB, &[(0, MIB)]);
    sb.write("src/small.txt", "hi");
    let dest = sb.mkdir("dest");
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &dest,
        &TransferOptions::copy(ConflictPolicy::Skip),
    );
    assert_eq!(plan.preserved.sparse_files, 1);
    assert!(warning(&plan, WarningKind::SparseFiles).is_some());
    assert!(
        plan.total_bytes() < 5 * MIB,
        "progress and free space count {} bytes, not 100 MiB",
        plan.total_bytes()
    );
    assert!(
        plan.totals.bytes >= 100 * MIB,
        "the totals shown to the user stay logical"
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(out.report.failed.is_empty(), "{:?}", out.report);
    assert!(stored(&dest.join("src/disk.img")) < 8 * MIB);
    let (_up, rep) = undo(&e, &j, &out.id);
    assert!(rep.failed.is_empty(), "{rep:?}");
    assert!(snapshot(&dest).is_empty());
}

#[test]
fn a_sparse_file_moved_across_filesystems_stays_sparse_and_comes_back_sparse() {
    let sb = Sandbox::new();
    let Some(other) = other_filesystem_dir(&sb) else {
        return;
    };
    if !holes_supported(&sb.root) || !holes_supported(other.path()) {
        return;
    }
    let e = sb.engine();
    let j = sb.journal();
    sparse_file(&sb.path("m/disk.img"), 64 * MIB, &[(MIB, MIB)]);
    let before_len = 64 * MIB;
    let plan = e.plan_transfer(
        &scan(&e, &[sb.path("m")]),
        other.path(),
        &TransferOptions::mv(ConflictPolicy::Skip),
    );
    let out = run_journaled(&e, &j, &plan);
    assert!(out.report.failed.is_empty(), "{:?}", out.report);
    let moved = other.path().join("m/disk.img");
    assert_eq!(std::fs::metadata(&moved).unwrap().len(), before_len);
    assert!(stored(&moved) < 8 * MIB);
    let (_up, rep) = undo(&e, &j, &out.id);
    assert!(rep.failed.is_empty(), "{rep:?}");
    let back = sb.path("m/disk.img");
    assert_eq!(std::fs::metadata(&back).unwrap().len(), before_len);
    assert!(stored(&back) < 8 * MIB, "sparse again after the undo");
}

#[test]
fn modes_are_not_disturbed_by_the_read_write_fallback() {
    // Sanity for the fallback path: a file with a tight mode is still copied and closed.
    let sb = Sandbox::new();
    let e = sb.engine();
    let f = sb.write("src/t.txt", "abc");
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
    let dest = sb.mkdir("dest");
    let (_p, rep) = copy_tree(&sb, &e, &sb.path("src"), &dest);
    assert!(rep.failed.is_empty());
    assert_eq!(
        std::fs::metadata(dest.join("src/t.txt")).unwrap().mode() & 0o777,
        0o640
    );
}
