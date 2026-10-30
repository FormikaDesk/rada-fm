//! Turning a request into a [`Plan`]. Planning only reads; nothing is modified.

use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use super::engine::Engine;
use super::names;
use super::plan::*;
use super::scan::{LinkState, Scan, ScanNode};
use crate::display;
use crate::fs::{FileKind, FsMeta};
use crate::i18n::{self, Key, tf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferMode {
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug)]
pub struct TransferOptions {
    pub mode: TransferMode,
    pub policy: ConflictPolicy,
    /// Verify plain copies too (moves across devices are always verified).
    pub verify: bool,
}

impl TransferOptions {
    pub fn copy(policy: ConflictPolicy) -> Self {
        TransferOptions {
            mode: TransferMode::Copy,
            policy,
            verify: false,
        }
    }
    pub fn mv(policy: ConflictPolicy) -> Self {
        TransferOptions {
            mode: TransferMode::Move,
            policy,
            verify: false,
        }
    }
}

fn summary_of(r: &ScanNode, action: ItemAction) -> ItemSummary {
    let st = r.stats();
    ItemSummary {
        path: r.path.clone(),
        kind: r.meta.kind,
        action,
        files: st.files,
        dirs: st.dirs,
        symlinks: st.symlinks,
        bytes: st.bytes,
        target: None,
    }
}

fn items_label(n: u64) -> String {
    i18n::count(n, Key::ItemOne, Key::ItemMany)
}

fn rename_step(from: &Path, to: &Path) -> Step {
    Step::Rename {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
    }
}

fn not_found(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::NotFound
}

fn scan_problems(scan: &Scan, ws: &mut WarningSet) {
    for p in &scan.missing {
        ws.add(WarningKind::Missing, Severity::Warning, Some(p));
    }
    for (p, msg) in &scan.errors {
        ws.add_with(
            WarningKind::Unreadable,
            Severity::Warning,
            Some(p),
            Some(msg.clone()),
        );
    }
}

impl Engine {
    // ------------------------------------------------------------------ copy / move

    pub fn plan_transfer(&self, scan: &Scan, dest_dir: &Path, opts: &TransferOptions) -> Plan {
        let kind = match opts.mode {
            TransferMode::Copy => OpKind::Copy,
            TransferMode::Move => OpKind::Move,
        };
        let title = tf(
            Key::TitleTransfer,
            &[
                &kind.verb(),
                &items_label(scan.roots.len() as u64),
                &display::path(dest_dir),
            ],
        );
        let mut plan = Plan::empty(kind, title);
        plan.destination = Some(dest_dir.to_path_buf());
        plan.policy = opts.policy;

        let mut t = Transfer {
            engine: self,
            opts: *opts,
            dest_dev: None,
            dest_device: self.platform.on_device(dest_dir),
            steps: Vec::new(),
            ws: WarningSet::default(),
            totals: Totals {
                items: scan.roots.len() as u64,
                ..Default::default()
            },
            taken: HashSet::new(),
            case_insensitive: self.platform.path_rules().case_insensitive,
            cross_device: false,
            same_location: false,
            items: Vec::new(),
            merged_root: false,
            support: self.platform.fidelity().destination_support(dest_dir),
            links: BTreeMap::new(),
            preserved: Preserved::default(),
        };
        scan_problems(scan, &mut t.ws);
        if !self.platform.fidelity().implemented() {
            t.ws.add(WarningKind::AttributesNotKept, Severity::Info, None);
        }

        // The destination must be a writable directory.
        match self.fs.stat(dest_dir) {
            Ok(m) if m.is_dir() => {
                t.dest_dev = m.dev;
                if !self.fs.can_write(dest_dir) {
                    t.ws.add(WarningKind::NotWritable, Severity::Blocking, Some(dest_dir));
                }
            }
            Ok(_) => t.ws.add_with(
                WarningKind::NotWritable,
                Severity::Blocking,
                Some(dest_dir),
                Some("not a folder".into()),
            ),
            Err(e) => t.ws.add_with(
                WarningKind::NotWritable,
                Severity::Blocking,
                Some(dest_dir),
                Some(crate::Error::io("open", dest_dir, e).to_string()),
            ),
        }

        let dest_canon = std::fs::canonicalize(dest_dir).ok();
        let mut blocked = false;
        for root in &scan.roots {
            if root.meta.is_dir()
                && let (Some(dc), Some(parent)) = (&dest_canon, root.path.parent())
            {
                let src_canon = std::fs::canonicalize(parent)
                    .unwrap_or_else(|_| parent.to_path_buf())
                    .join(&root.name);
                if dc.starts_with(&src_canon) {
                    t.ws.add(
                        WarningKind::InsideItself,
                        Severity::Blocking,
                        Some(&root.path),
                    );
                    blocked = true;
                }
            }
        }
        if !blocked && !t.ws_has_blocking() {
            for root in &scan.roots {
                let start = t.steps.len();
                t.merged_root = false;
                t.visit(root, dest_dir);
                let item = t.summarise(root, start, dest_dir);
                t.items.push(item);
            }
        }

        // Free space: only data that is actually written needs room.
        let need: u64 = t.steps.iter().map(Step::bytes).sum();
        if need > 0
            && !t.ws_has_blocking()
            && let Ok(avail) = self.fs.available_space(dest_dir)
            && need > avail
        {
            t.ws.add_with(
                WarningKind::LowSpace,
                Severity::Warning,
                Some(dest_dir),
                Some(format!(
                    "need {}, {} free",
                    display::bytes(need),
                    display::bytes(avail)
                )),
            );
        }
        if t.cross_device {
            t.ws.add(WarningKind::CrossDevice, Severity::Info, None);
        }
        // Phones and cameras: what is different there, said before anything is done.
        let from_device = scan.roots.iter().any(|r| self.platform.on_device(&r.path));
        if t.dest_device {
            t.ws.add(WarningKind::DeviceWrite, Severity::Info, Some(dest_dir));
        }
        if t.dest_device || from_device {
            t.ws.add(WarningKind::DeviceAttrs, Severity::Info, None);
        }
        // A copy from a device is undone by removing the copies, here; whatever puts something
        // back onto a device is not.
        if t.dest_device || (from_device && opts.mode == TransferMode::Move) {
            t.ws.add(WarningKind::DeviceUndo, Severity::Warning, None);
        }
        // Files that also have names outside the selection: their copies stand alone.
        for g in t.links.values() {
            if g.nlink > g.found {
                t.ws.add(WarningKind::HardLinks, Severity::Info, Some(&g.src));
            }
        }

        plan.preserved = t.preserved;
        plan.steps = t.steps;
        plan.items = t.items;
        plan.totals = t.totals;
        plan.warnings = t.ws.finish();
        plan
    }

    // ------------------------------------------------------------------ trash / delete

    pub fn plan_trash(&self, scan: &Scan) -> Plan {
        let mut plan = Plan::empty(
            OpKind::Trash,
            tf(Key::TitleTrash, &[&items_label(scan.roots.len() as u64)]),
        );
        let mut ws = WarningSet::default();
        scan_problems(scan, &mut ws);
        let st = scan.stats();
        plan.totals = Totals {
            items: scan.roots.len() as u64,
            files: st.files,
            dirs: st.dirs,
            symlinks: st.symlinks,
            bytes: st.bytes,
        };
        for r in &scan.roots {
            if r.path.parent().is_none() {
                ws.add_with(
                    WarningKind::InvalidName,
                    Severity::Blocking,
                    Some(&r.path),
                    Some("cannot trash the root".into()),
                );
                continue;
            }
            plan.steps.push(Step::TrashItem {
                path: r.path.clone(),
            });
            plan.items.push(summary_of(r, ItemAction::Trash));
            if self.platform.on_device(&r.path) {
                ws.add(WarningKind::DeviceNoTrash, Severity::Warning, Some(&r.path));
                ws.add(WarningKind::DeviceUndo, Severity::Warning, Some(&r.path));
            }
        }
        plan.warnings = ws.finish();
        plan
    }

    pub fn plan_delete(&self, scan: &Scan) -> Plan {
        let mut plan = Plan::empty(
            OpKind::Delete,
            tf(Key::TitleDelete, &[&items_label(scan.roots.len() as u64)]),
        );
        plan.reversible = false;
        let mut ws = WarningSet::default();
        scan_problems(scan, &mut ws);
        ws.add(WarningKind::Irreversible, Severity::Warning, None);
        let st = scan.stats();
        plan.totals = Totals {
            items: scan.roots.len() as u64,
            files: st.files,
            dirs: st.dirs,
            symlinks: st.symlinks,
            bytes: st.bytes,
        };
        fn walk(n: &ScanNode, steps: &mut Vec<Step>, ws: &mut WarningSet) {
            match n.meta.kind {
                FileKind::Dir => {
                    if n.loop_skipped {
                        ws.add(WarningKind::MountLoop, Severity::Warning, Some(&n.path));
                        return;
                    }
                    n.warn_unreadable(ws);
                    for c in &n.children {
                        walk(c, steps, ws);
                    }
                    steps.push(Step::RemoveDir {
                        path: n.path.clone(),
                    });
                }
                _ => steps.push(Step::RemoveFile {
                    path: n.path.clone(),
                    expect: None,
                }),
            }
        }
        for r in &scan.roots {
            if r.path.parent().is_none() {
                ws.add_with(
                    WarningKind::InvalidName,
                    Severity::Blocking,
                    Some(&r.path),
                    Some("cannot delete the root".into()),
                );
                continue;
            }
            walk(r, &mut plan.steps, &mut ws);
            plan.items.push(summary_of(r, ItemAction::Delete));
        }
        plan.warnings = ws.finish();
        plan
    }

    // ------------------------------------------------------------------ rename / mkdir

    pub fn plan_rename(&self, from: &Path, new_name: &OsStr, policy: ConflictPolicy) -> Plan {
        let mut ws = WarningSet::default();
        let src = self.fs.lstat(from);
        let target = from.parent().unwrap_or(Path::new(".")).join(new_name);
        let (steps, to) = match &src {
            Err(_) => {
                ws.add(WarningKind::Missing, Severity::Blocking, Some(from));
                (Vec::new(), target)
            }
            Ok(meta) => self.rename_steps(from, meta, target, policy, &mut ws),
        };
        let name = to.file_name().unwrap_or(new_name);
        let title = tf(
            Key::TitleRename,
            &[&display::path(from), &display::name(name)],
        );
        let mut plan = Plan::empty(OpKind::Rename, title);
        plan.steps = steps;
        plan.policy = policy;
        plan.totals.items = 1;
        plan.items.push(ItemSummary {
            path: from.to_path_buf(),
            kind: src.map(|m| m.kind).unwrap_or(FileKind::File),
            action: ItemAction::Rename,
            files: 0,
            dirs: 0,
            symlinks: 0,
            bytes: 0,
            target: Some(to.clone()),
        });
        if self.platform.on_device(from) {
            ws.add(WarningKind::DeviceRename, Severity::Info, Some(from));
        }
        plan.renames.push((from.to_path_buf(), to));
        plan.warnings = ws.finish();
        plan
    }

    /// The steps that rename `from` to `to`, and the name it ends up with.
    fn rename_steps(
        &self,
        from: &Path,
        src: &FsMeta,
        to: PathBuf,
        policy: ConflictPolicy,
        ws: &mut WarningSet,
    ) -> (Vec<Step>, PathBuf) {
        if let Some(why) = self.bad_rename_name(from, &to) {
            ws.add_with(
                WarningKind::InvalidName,
                Severity::Blocking,
                Some(&to),
                Some(why),
            );
            return (Vec::new(), to);
        }
        match self.fs.lstat(&to) {
            Ok(existing) if self.is_same_file(src, &existing) => (vec![rename_step(from, &to)], to),
            Ok(existing) => self.rename_onto(from, src, &existing, to, policy, ws),
            Err(e) if not_found(&e) => {
                let parent = to.parent().unwrap_or(Path::new("."));
                if self.fs.can_write(parent) {
                    return (vec![rename_step(from, &to)], to);
                }
                ws.add(WarningKind::NotWritable, Severity::Blocking, Some(parent));
                (Vec::new(), to)
            }
            Err(e) => {
                let detail = Some(e.to_string());
                ws.add_with(
                    WarningKind::NotWritable,
                    Severity::Blocking,
                    Some(&to),
                    detail,
                );
                (Vec::new(), to)
            }
        }
    }

    fn bad_rename_name(&self, from: &Path, to: &Path) -> Option<String> {
        let name = to.file_name().unwrap_or_default();
        let unchanged = || (from.file_name() == Some(name)).then(|| "the name is unchanged".into());
        let rules = self.platform.path_rules();
        names::validate(&rules, name).err().or_else(unchanged)
    }

    /// On case-insensitive filesystems "a" -> "A" names the same file.
    fn is_same_file(&self, src: &FsMeta, existing: &FsMeta) -> bool {
        self.platform.path_rules().case_insensitive
            && existing.ino.is_some()
            && existing.ino == src.ino
            && existing.dev == src.dev
    }

    fn rename_onto(
        &self,
        from: &Path,
        src: &FsMeta,
        existing: &FsMeta,
        to: PathBuf,
        policy: ConflictPolicy,
        ws: &mut WarningSet,
    ) -> (Vec<Step>, PathBuf) {
        match policy {
            ConflictPolicy::Skip => {
                ws.add(WarningKind::Conflict, Severity::Warning, Some(&to));
                (Vec::new(), to)
            }
            ConflictPolicy::KeepBoth => {
                ws.add(WarningKind::Conflict, Severity::Warning, Some(&to));
                let parent = to.parent().unwrap_or(Path::new("."));
                let name = to.file_name().unwrap_or_default();
                let free = names::first_free(parent, name, |p| self.fs.lstat(p).is_ok());
                (vec![rename_step(from, &free)], free)
            }
            ConflictPolicy::Overwrite if self.can_replace(src, existing, &to) => {
                ws.add(WarningKind::Conflict, Severity::Warning, Some(&to));
                ws.add(WarningKind::Overwrite, Severity::Warning, Some(&to));
                let steps = vec![Step::TrashItem { path: to.clone() }, rename_step(from, &to)];
                (steps, to)
            }
            ConflictPolicy::Overwrite => {
                let why = i18n::t(Key::RenameCannotReplace).to_string();
                ws.add_with(
                    WarningKind::Conflict,
                    Severity::Blocking,
                    Some(&to),
                    Some(why),
                );
                (Vec::new(), to)
            }
        }
    }

    /// A file replaces a file, a folder replaces an empty folder; nothing else.
    fn can_replace(&self, src: &FsMeta, existing: &FsMeta, to: &Path) -> bool {
        match (src.is_dir(), existing.is_dir()) {
            (false, false) => true,
            (true, true) => self.fs.read_dir(to).is_ok_and(|l| l.items.is_empty()),
            _ => false,
        }
    }

    pub fn plan_mkdir(&self, parent: &Path, name: &OsStr) -> Plan {
        let path = parent.join(name);
        let mut plan = Plan::empty(
            OpKind::MakeDir,
            tf(Key::TitleMkdir, &[&display::name(name)]),
        );
        let mut ws = WarningSet::default();
        plan.destination = Some(parent.to_path_buf());
        plan.totals.items = 1;
        plan.totals.dirs = 1;
        if let Err(why) = names::validate(&self.platform.path_rules(), name) {
            ws.add_with(
                WarningKind::InvalidName,
                Severity::Blocking,
                Some(&path),
                Some(why),
            );
        } else if self.fs.lstat(&path).is_ok() {
            ws.add(WarningKind::Conflict, Severity::Blocking, Some(&path));
        } else if !self.fs.can_write(parent) {
            ws.add(WarningKind::NotWritable, Severity::Blocking, Some(parent));
        } else {
            plan.items.push(ItemSummary {
                path: path.clone(),
                kind: crate::fs::FileKind::Dir,
                action: ItemAction::Create,
                files: 0,
                dirs: 1,
                symlinks: 0,
                bytes: 0,
                target: None,
            });
            plan.steps.push(Step::MakeDir {
                path,
                mode: None,
                restore_mode: None,
            });
        }
        plan.warnings = ws.finish();
        plan
    }

    /// An empty file in `parent`, never over an existing name.
    pub fn plan_mkfile(&self, parent: &Path, name: &OsStr) -> Plan {
        let path = parent.join(name);
        let mut plan = Plan::empty(
            OpKind::MakeFile,
            tf(Key::TitleMkfile, &[&display::name(name)]),
        );
        let mut ws = WarningSet::default();
        plan.destination = Some(parent.to_path_buf());
        plan.totals.items = 1;
        plan.totals.files = 1;
        if let Err(why) = names::validate(&self.platform.path_rules(), name) {
            ws.add_with(
                WarningKind::InvalidName,
                Severity::Blocking,
                Some(&path),
                Some(why),
            );
        } else if self.fs.lstat(&path).is_ok() {
            ws.add(WarningKind::Conflict, Severity::Blocking, Some(&path));
        } else if !self.fs.can_write(parent) {
            ws.add(WarningKind::NotWritable, Severity::Blocking, Some(parent));
        } else {
            plan.items.push(ItemSummary {
                path: path.clone(),
                kind: crate::fs::FileKind::File,
                action: ItemAction::Create,
                files: 1,
                dirs: 0,
                symlinks: 0,
                bytes: 0,
                target: None,
            });
            plan.steps.push(Step::MakeFile { path, mode: None });
        }
        plan.warnings = ws.finish();
        plan
    }
}

// ---------------------------------------------------------------------- transfer builder

struct Transfer<'a> {
    engine: &'a Engine,
    opts: TransferOptions,
    dest_dev: Option<u64>,
    /// The destination is a phone or a camera.
    dest_device: bool,
    steps: Vec<Step>,
    ws: WarningSet,
    totals: Totals,
    taken: HashSet<OsString>,
    case_insensitive: bool,
    cross_device: bool,
    same_location: bool,
    items: Vec<ItemSummary>,
    /// The root being planned was merged into an existing folder.
    merged_root: bool,
    /// What the destination's filesystem accepts of extended attributes and ACLs.
    support: crate::platform::DestSupport,
    /// Files seen with several names, by (device, inode): the first one is copied, the others
    /// become hard links of that copy.
    links: BTreeMap<(u64, u64), LinkGroup>,
    preserved: Preserved,
}

/// The permissions a copy is given: the source's, except that a device keeps none (nothing is
/// asked of it) and says none that mean anything (what comes from it gets the usual ones).
fn file_mode(source: Option<u32>, to_device: bool, from_device: bool, usual: u32) -> Option<u32> {
    if to_device {
        None
    } else if from_device {
        Some(usual)
    } else {
        source
    }
}

struct LinkGroup {
    /// Index in `steps` of the step that copies the first name.
    step_index: usize,
    dst: PathBuf,
    src: PathBuf,
    /// Names the file has in total, and how many of them are in the selection.
    nlink: u64,
    found: u64,
}

impl Transfer<'_> {
    fn ws_has_blocking(&self) -> bool {
        self.ws.has_blocking()
    }

    /// The device and the storage of it that `p` is in (`<root>/mtp:host=…/Internal storage`):
    /// two paths with the same key are on one storage, where a move can be a rename.
    fn device_key(&self, p: &Path) -> Option<PathBuf> {
        let root = self.engine.platform.device_root()?;
        let mut parts = p.strip_prefix(root).ok()?.components();
        let device = parts.next()?;
        let key = root.join(device);
        Some(match parts.next() {
            Some(storage) => key.join(storage),
            None => key,
        })
    }

    fn on_device(&self, p: &Path) -> bool {
        self.engine.platform.on_device(p)
    }

    fn key(&self, p: &Path) -> OsString {
        if self.case_insensitive {
            OsString::from(p.as_os_str().to_string_lossy().to_lowercase())
        } else {
            p.as_os_str().to_os_string()
        }
    }

    fn exists(&self, p: &Path) -> bool {
        self.engine.fs.lstat(p).is_ok() || self.taken.contains(&self.key(p))
    }

    /// First free `name (n).ext` in `parent`.
    fn keep_both(&self, parent: &Path, name: &OsStr) -> PathBuf {
        names::first_free(parent, name, |p| self.exists(p))
    }

    /// Plan one source node into `parent`, checking what is already there.
    fn visit(&mut self, node: &ScanNode, parent: &Path) {
        let dst = parent.join(&node.name);

        if node.path == dst {
            // Pasting an item into the folder it already lives in.
            self.same_location = true;
            self.ws
                .add(WarningKind::SameLocation, Severity::Info, Some(&node.path));
            if self.opts.mode == TransferMode::Copy {
                let new = self.keep_both(parent, &node.name);
                self.proceed(node, new);
            }
            return;
        }

        let planned = self.taken.contains(&self.key(&dst));
        match self.engine.fs.lstat(&dst) {
            Err(e) if not_found(&e) && !planned => self.proceed(node, dst),
            Err(e) if not_found(&e) => self.resolve_conflict(node, parent, dst, false),
            Err(e) => {
                self.ws.add_with(
                    WarningKind::Unreadable,
                    Severity::Warning,
                    Some(&dst),
                    Some(crate::Error::io("inspect", &dst, e).to_string()),
                );
            }
            Ok(existing) => {
                if existing.is_dir() && node.meta.is_dir() && !planned {
                    self.merge(node, dst);
                } else {
                    self.resolve_conflict(node, parent, dst, true);
                }
            }
        }
    }

    /// What became of `root`, from the steps its planning appended (from `start`).
    fn summarise(&self, root: &ScanNode, start: usize, dest_dir: &Path) -> ItemSummary {
        let added = &self.steps[start..];
        let target = added
            .iter()
            .find_map(|s| s.destination().map(Path::to_path_buf));
        let renamed = added.iter().any(|s| matches!(s, Step::Rename { .. }));
        let (mut files, mut dirs, mut symlinks, mut bytes) = (0, 0, 0, 0);
        // A whole-tree rename, or an item left alone, is described by the source itself.
        if (renamed && !self.merged_root) || added.is_empty() {
            let st = root.stats();
            (files, dirs, symlinks, bytes) = (st.files, st.dirs, st.symlinks, st.bytes);
        } else {
            for s in added {
                match s {
                    Step::CopyFile { size, .. } => {
                        files += 1;
                        bytes += size;
                    }
                    Step::CopySymlink { .. } => symlinks += 1,
                    Step::MakeDir { .. } => dirs += 1,
                    _ => {}
                }
            }
        }
        let overwrote = matches!(added.first(), Some(Step::TrashItem { .. }));
        let natural = dest_dir.join(&root.name);
        let action = if added.is_empty() {
            ItemAction::Skip
        } else if self.merged_root {
            ItemAction::Merge
        } else if overwrote {
            ItemAction::Overwrite
        } else if target.as_ref().is_some_and(|t| *t != natural) {
            ItemAction::KeepBoth
        } else if self.opts.mode == TransferMode::Move {
            ItemAction::Move
        } else {
            ItemAction::Copy
        };
        ItemSummary {
            path: root.path.clone(),
            kind: root.meta.kind,
            action,
            files,
            dirs,
            symlinks,
            bytes,
            target: if added.is_empty() { None } else { target },
        }
    }

    fn merge(&mut self, node: &ScanNode, dst: PathBuf) {
        self.merged_root = true;
        self.ws.add(WarningKind::Merge, Severity::Info, Some(&dst));
        self.taken.insert(self.key(&dst));
        self.note_unreadable_dir(node);
        for c in &node.children {
            self.visit(c, &dst);
        }
        if self.opts.mode == TransferMode::Move {
            // Source folder is removed only if every child really left it.
            self.steps.push(Step::RemoveDir {
                path: node.path.clone(),
            });
        }
    }

    fn resolve_conflict(
        &mut self,
        node: &ScanNode,
        parent: &Path,
        dst: PathBuf,
        exists_on_disk: bool,
    ) {
        self.ws
            .add(WarningKind::Conflict, Severity::Warning, Some(&dst));
        match self.opts.policy {
            ConflictPolicy::Skip => {}
            ConflictPolicy::KeepBoth => {
                let new = self.keep_both(parent, &node.name);
                self.proceed(node, new);
            }
            ConflictPolicy::Overwrite if exists_on_disk => {
                self.ws
                    .add(WarningKind::Overwrite, Severity::Warning, Some(&dst));
                self.steps.push(Step::TrashItem { path: dst.clone() });
                self.proceed(node, dst);
            }
            ConflictPolicy::Overwrite => {
                // Two sources with the same name in one plan: never silently lose one.
                let new = self.keep_both(parent, &node.name);
                self.proceed(node, new);
            }
        }
    }

    fn note_unreadable_dir(&mut self, node: &ScanNode) {
        node.warn_unreadable(&mut self.ws);
    }

    /// `dst` is known to be free.
    fn proceed(&mut self, node: &ScanNode, dst: PathBuf) {
        self.taken.insert(self.key(&dst));
        match self.opts.mode {
            TransferMode::Copy => self.emit(node, dst, false),
            TransferMode::Move => {
                let (from_key, to_key) = (self.device_key(&node.path), self.device_key(&dst));
                let same_dev = match (&from_key, &to_key) {
                    // The folders of all the devices are one filesystem to the system, and a
                    // move from one phone to another (or to another storage of the same phone)
                    // is a rename to it. It is a copy, then.
                    (None, None) => node.meta.dev.is_some() && node.meta.dev == self.dest_dev,
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                };
                if same_dev && from_key.is_some() {
                    self.ws
                        .add(WarningKind::DeviceRename, Severity::Info, Some(&node.path));
                }
                if same_dev {
                    let st = node.stats();
                    self.totals.files += st.files;
                    self.totals.dirs += st.dirs;
                    self.totals.symlinks += st.symlinks;
                    self.totals.bytes += st.bytes;
                    self.steps.push(Step::Rename {
                        from: node.path.clone(),
                        to: dst,
                    });
                } else {
                    self.cross_device = true;
                    self.emit(node, dst, true);
                }
            }
        }
    }

    /// Extended attributes, ACLs and owner of `node`: what the copy can keep, and what the
    /// plan has to say it cannot.
    fn note_attributes(&mut self, node: &ScanNode) {
        // A device keeps none of this, and the plan has already said so.
        if self.dest_device || self.on_device(&node.path) {
            return;
        }
        let a = node.attrs;
        if a.xattrs {
            self.preserved.xattr_items += 1;
            if !self.support.xattrs {
                self.ws
                    .add(WarningKind::XattrsLost, Severity::Warning, Some(&node.path));
            }
        }
        if a.acl {
            self.preserved.acl_items += 1;
            if !self.support.acl {
                self.ws
                    .add(WarningKind::AclLost, Severity::Warning, Some(&node.path));
            }
        }
        let fid = self.engine.platform.fidelity();
        if fid.implemented()
            && let (Some(u), Some(g)) = (node.meta.uid, node.meta.gid)
            && !fid.can_set_owner(u, g)
        {
            self.ws
                .add(WarningKind::OwnerNotKept, Severity::Info, Some(&node.path));
        }
    }

    /// Children of a directory we just created cannot clash: no existence checks.
    fn emit_child(&mut self, node: &ScanNode, parent_dst: &Path, mv: bool) {
        self.emit(node, parent_dst.join(&node.name), mv);
    }

    fn emit(&mut self, node: &ScanNode, dst: PathBuf, mv: bool) {
        let fs = &self.engine.fs;
        match node.meta.kind {
            FileKind::File => {
                if !fs.can_read(&node.path) {
                    self.ws
                        .add(WarningKind::Unreadable, Severity::Warning, Some(&node.path));
                }
                let meta = &node.meta;
                self.totals.files += 1;
                self.totals.bytes += meta.size;
                let from_device = self.on_device(&node.path);
                let device = self.dest_device || from_device;
                // A device has no hard links: what has several names is copied once per name.
                if self.dest_device && meta.nlink.unwrap_or(1) > 1 {
                    self.ws.add(
                        WarningKind::DeviceHardLink,
                        Severity::Info,
                        Some(&node.path),
                    );
                }
                // Names of one file inside the selection stay names of one file.
                if !self.dest_device
                    && meta.nlink.unwrap_or(1) > 1
                    && let (Some(dev), Some(ino)) = (meta.dev, meta.ino)
                {
                    if let Some(g) = self.links.get_mut(&(dev, ino)) {
                        g.found += 1;
                        let (existing, src_existing, index) =
                            (g.dst.clone(), g.src.clone(), g.step_index);
                        if let Step::CopyFile { link_primary, .. } = &mut self.steps[index] {
                            *link_primary = true;
                        }
                        self.ws
                            .add(WarningKind::HardLinksKept, Severity::Info, Some(&node.path));
                        self.preserved.hard_links += 1;
                        self.steps.push(Step::HardLink {
                            existing,
                            link: dst,
                            src_link: mv.then(|| node.path.clone()),
                            src_existing: mv.then_some(src_existing),
                        });
                        return;
                    }
                    self.links.insert(
                        (dev, ino),
                        LinkGroup {
                            step_index: self.steps.len(),
                            dst: dst.clone(),
                            src: node.path.clone(),
                            nlink: meta.nlink.unwrap_or(1),
                            found: 1,
                        },
                    );
                }
                // What a device says about blocks and holes means nothing.
                let stored = (!device && meta.is_sparse()).then(|| meta.stored_bytes());
                if stored.is_some() {
                    self.ws
                        .add(WarningKind::SparseFiles, Severity::Info, Some(&node.path));
                    self.preserved.sparse_files += 1;
                }
                self.note_attributes(node);
                self.steps.push(Step::CopyFile {
                    src: node.path.clone(),
                    dst,
                    size: meta.size,
                    mode: file_mode(meta.mode, self.dest_device, from_device, 0o644),
                    mtime: meta.mtime.map(Into::into),
                    atime: (!device).then(|| meta.atime.map(Into::into)).flatten(),
                    verify: mv || self.opts.verify,
                    remove_source: mv,
                    uid: (!device).then_some(meta.uid).flatten(),
                    gid: (!device).then_some(meta.gid).flatten(),
                    stored,
                    link_primary: false,
                });
            }
            FileKind::Symlink if self.dest_device => {
                // A phone or a camera cannot hold a link. It is left out, and said.
                self.ws
                    .add(WarningKind::DeviceLink, Severity::Warning, Some(&node.path));
            }
            FileKind::Symlink => {
                match node.link_state {
                    Some(LinkState::Broken) => {
                        self.ws
                            .add(WarningKind::BrokenSymlink, Severity::Info, Some(&node.path))
                    }
                    Some(LinkState::Circular) => {
                        self.ws
                            .add(WarningKind::CircularLink, Severity::Info, Some(&node.path))
                    }
                    _ => self
                        .ws
                        .add(WarningKind::Symlink, Severity::Info, Some(&node.path)),
                }
                let Some(target) = node.link_target.clone() else {
                    self.ws
                        .add(WarningKind::Unreadable, Severity::Warning, Some(&node.path));
                    return;
                };
                self.totals.symlinks += 1;
                self.steps.push(Step::CopySymlink {
                    src: node.path.clone(),
                    dst,
                    target,
                    remove_source: mv,
                });
            }
            FileKind::Dir => {
                if node.loop_skipped {
                    self.ws
                        .add(WarningKind::MountLoop, Severity::Warning, Some(&node.path));
                    return;
                }
                self.note_unreadable_dir(node);
                self.totals.dirs += 1;
                let from_device = self.on_device(&node.path);
                let device = self.dest_device || from_device;
                // Owner can always write while the folder is being filled.
                self.steps.push(Step::MakeDir {
                    path: dst.clone(),
                    restore_mode: None,
                    mode: file_mode(node.meta.mode, self.dest_device, from_device, 0o755)
                        .map(|m| m | 0o700),
                });
                for c in &node.children {
                    self.emit_child(c, &dst, mv);
                }
                self.note_attributes(node);
                self.steps.push(Step::FinishDir {
                    path: dst,
                    mode: file_mode(node.meta.mode, self.dest_device, from_device, 0o755),
                    mtime: node.meta.mtime.map(Into::into),
                    src: (!device).then(|| node.path.clone()),
                    uid: (!device).then_some(node.meta.uid).flatten(),
                    gid: (!device).then_some(node.meta.gid).flatten(),
                });
                if mv {
                    self.steps.push(Step::RemoveDir {
                        path: node.path.clone(),
                    });
                }
            }
            FileKind::Other => self.ws.add(
                WarningKind::SpecialFile,
                Severity::Warning,
                Some(&node.path),
            ),
        }
    }
}
