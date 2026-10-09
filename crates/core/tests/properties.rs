//! Property tests: whatever tree is thrown at it — symlinks (broken ones too), hard links,
//! sparse files, odd names, read-only files and folders — a copy is faithful and
//! copy-then-undo and move-then-undo bring the world back to exactly where it started.
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::os::unix::fs::{FileExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use rada_core::ops::*;
use rada_core::testutil::*;

/// Names that have broken file managers: spaces, dots, dashes, quotes, Unicode, a newline,
/// a name near the length limit.
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
        Just("emoji🙂".to_string()),
        Just("UPPER".to_string()),
        Just("two\nlines".to_string()),
        Just(format!("long_{}", "n".repeat(200))),
        "[a-z]{1,6}".prop_map(|s| s),
    ]
}

#[derive(Clone, Debug)]
enum Kind {
    Dir {
        read_only: bool,
    },
    File {
        data: Vec<u8>,
        mode: u32,
    },
    /// A second name for an earlier file (index among the files created so far).
    Hard(usize),
    Symlink(String),
    /// A hole of `holes` MiB with one byte of data after it.
    Sparse {
        holes: u8,
    },
}

#[derive(Clone, Debug)]
struct Node {
    /// Which folder (index among the folders so far) it goes in.
    parent: usize,
    name: String,
    kind: Kind,
}

fn node() -> impl Strategy<Value = Node> {
    (
        0usize..6,
        name(),
        prop_oneof![
            3 => any::<bool>().prop_map(|read_only| Kind::Dir { read_only }),
            4 => (proptest::collection::vec(any::<u8>(), 0..200), prop_oneof![Just(0o644u32), Just(0o600), Just(0o755), Just(0o444)])
                .prop_map(|(data, mode)| Kind::File { data, mode }),
            2 => (0usize..8).prop_map(Kind::Hard),
            2 => name().prop_map(Kind::Symlink),
            1 => (1u8..4).prop_map(|holes| Kind::Sparse { holes }),
        ],
    )
        .prop_map(|(parent, name, kind)| Node { parent, name, kind })
}

fn tree() -> impl Strategy<Value = Vec<Node>> {
    proptest::collection::vec(node(), 1..14)
}

/// Build `nodes` under `root` (which must exist); names that clash are skipped. Folders marked
/// read-only are made so at the very end, when everything is in them.
fn build(root: &Path, nodes: &[Node]) {
    let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut read_only: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for n in nodes {
        let parent = dirs[n.parent % dirs.len()].clone();
        let path = parent.join(&n.name);
        if std::fs::symlink_metadata(&path).is_ok() {
            continue;
        }
        match &n.kind {
            Kind::Dir { read_only: ro } => {
                std::fs::create_dir(&path).unwrap();
                if *ro {
                    read_only.push(path.clone());
                }
                dirs.push(path);
            }
            Kind::File { data, mode } => {
                std::fs::write(&path, data).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(*mode)).unwrap();
                files.push(path);
            }
            Kind::Hard(i) => {
                if files.is_empty() {
                    continue;
                }
                let target = files[i % files.len()].clone();
                std::fs::hard_link(&target, &path).unwrap();
                files.push(path);
            }
            Kind::Symlink(t) => {
                std::os::unix::fs::symlink(t, &path).unwrap();
            }
            Kind::Sparse { holes } => {
                let f = std::fs::File::create(&path).unwrap();
                f.set_len(*holes as u64 * (1 << 20) + 1).unwrap();
                f.write_all_at(b"Z", *holes as u64 * (1 << 20)).unwrap();
                files.push(path);
            }
        }
    }
    for d in read_only.iter().rev() {
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
}

/// Give write permission back to every folder below `root`, so that a test's temporary
/// directory can be deleted.
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

/// Everything a faithful copy must keep, relative to `root`: kinds, modes, contents, link
/// targets, file times, and which names are hard links of each other (as groups).
fn deep(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    let mut inodes: BTreeMap<(u64, u64), usize> = BTreeMap::new();
    fn walk(
        root: &Path,
        dir: &Path,
        out: &mut BTreeMap<PathBuf, String>,
        inodes: &mut BTreeMap<(u64, u64), usize>,
    ) {
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
                format!("dir {mode:o}")
            } else {
                let next = inodes.len();
                let group = *inodes.entry((m.dev(), m.ino())).or_insert(next);
                let content = std::fs::read(&p).unwrap();
                format!(
                    "file {mode:o} len={} sum={:x} group={} mtime={}.{:09}",
                    content.len(),
                    content
                        .iter()
                        .fold(0u64, |a, b| a.wrapping_mul(131).wrapping_add(*b as u64)),
                    group,
                    m.mtime(),
                    m.mtime_nsec()
                )
            };
            out.insert(rel, desc);
            if m.is_dir() {
                walk(root, &p, out, inodes);
            }
        }
    }
    walk(root, root, &mut out, &mut inodes);
    out
}

struct Unlock(PathBuf);
impl Drop for Unlock {
    fn drop(&mut self) {
        unlock(&self.0);
    }
}

fn cfg() -> ProptestConfig {
    ProptestConfig {
        // 40 cases keep the suite quick; PROPTEST_CASES=1000 for a long hunt.
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(40),
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(cfg())]

    #[test]
    fn a_copy_is_faithful_and_copy_then_undo_restores_the_world(nodes in tree()) {
        let sb = Sandbox::new();
        let e = sb.engine();
        let j = sb.journal();
        sb.mkdir("src/t");
        let _unlock = (Unlock(sb.path("src")), Unlock(sb.path("dest")));
        build(&sb.path("src/t"), &nodes);
        let before = deep(&sb.path("src"));
        let dest = sb.mkdir("dest");
        let plan = e.plan_transfer(&scan(&e, &[sb.path("src/t")]), &dest, &TransferOptions::copy(ConflictPolicy::Skip));
        prop_assert!(plan.is_executable(), "{:?}", plan.warnings);
        let out = run_journaled(&e, &j, &plan);
        prop_assert!(out.report.failed.is_empty(), "{:?}", out.report.failed);

        // The copy is the same tree: kinds, modes, contents, links, times, hard-link groups.
        let copy = deep(&dest);
        let original = deep(&sb.path("src"));
        let relocated: BTreeMap<PathBuf, String> = copy
            .into_iter()
            .filter_map(|(k, v)| k.strip_prefix("t").ok().map(|r| (PathBuf::from("t").join(r), v)))
            .collect();
        // Group numbers depend on the order of the walk, which is the same in both trees.
        prop_assert_eq!(relocated, original);

        // And undo puts everything back.
        let (up, rep) = undo(&e, &j, &out.id);
        prop_assert!(up.blocked.is_empty() && rep.failed.is_empty(), "{:?} {:?}", up.blocked, rep.failed);
        prop_assert!(snapshot(&dest).is_empty(), "left behind: {:?}", snapshot(&dest).keys().collect::<Vec<_>>());
        prop_assert_eq!(deep(&sb.path("src")), before);
    }

    #[test]
    fn move_then_undo_restores_the_world_on_one_filesystem(nodes in tree()) {
        let sb = Sandbox::new();
        let e = sb.engine();
        let j = sb.journal();
        sb.mkdir("src/t");
        let _unlock = (Unlock(sb.path("src")), Unlock(sb.path("dest")));
        build(&sb.path("src/t"), &nodes);
        let before = deep(&sb.path("src"));
        let dest = sb.mkdir("dest");
        let plan = e.plan_transfer(&scan(&e, &[sb.path("src/t")]), &dest, &TransferOptions::mv(ConflictPolicy::Skip));
        let out = run_journaled(&e, &j, &plan);
        prop_assert!(out.report.failed.is_empty(), "{:?}", out.report.failed);
        prop_assert!(!sb.path("src/t").exists());
        let (up, rep) = undo(&e, &j, &out.id);
        prop_assert!(up.blocked.is_empty() && rep.failed.is_empty(), "{:?} {:?}", up.blocked, rep.failed);
        prop_assert!(snapshot(&dest).is_empty());
        prop_assert_eq!(deep(&sb.path("src")), before);
    }

    #[test]
    fn move_then_undo_restores_the_world_across_filesystems(nodes in tree()) {
        // Taking things out of a read-only folder is refused by the system (as `mv` is), so
        // the trees moved across filesystems have none.
        let nodes: Vec<Node> = nodes
            .into_iter()
            .map(|mut n| {
                if let Kind::Dir { read_only } = &mut n.kind {
                    *read_only = false;
                }
                n
            })
            .collect();
        let sb = Sandbox::new();
        let Some(other) = other_filesystem_dir(&sb) else { return Ok(()); };
        let e = sb.engine();
        let j = sb.journal();
        sb.mkdir("src/t");
        let _unlock = (Unlock(sb.path("src")), Unlock(sb.path("dest")));
        build(&sb.path("src/t"), &nodes);
        let before = deep(&sb.path("src"));
        let plan = e.plan_transfer(&scan(&e, &[sb.path("src/t")]), other.path(), &TransferOptions::mv(ConflictPolicy::Skip));
        let out = run_journaled(&e, &j, &plan);
        prop_assert!(out.report.failed.is_empty(), "{:?}", out.report.failed);
        prop_assert!(!sb.path("src/t").exists());
        let (up, rep) = undo(&e, &j, &out.id);
        prop_assert!(up.blocked.is_empty() && rep.failed.is_empty(), "{:?} {:?}", up.blocked, rep.failed);
        prop_assert!(snapshot(other.path()).is_empty(), "left behind: {:?}", snapshot(other.path()).keys().collect::<Vec<_>>());
        prop_assert_eq!(deep(&sb.path("src")), before);
    }
}
