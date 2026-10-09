//! Planning the extraction of an archive and the creation of one.
//!
//! Both only read. An extraction plan is made of the usual steps (create folder, finish
//! folder, create link) plus `ExtractFile`, one per file, in the order the archive holds
//! them. Every member goes through the same questions before it becomes a step: is its path
//! safe, does it go through a link the archive itself made, is it protected by a password, is
//! its name valid here, is something already there. The answers that stop a member are
//! warnings that say why.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::engine::Engine;
use super::names;
use super::plan::*;
use super::scan::{Scan, ScanControl, ScanNode, ScanProgress};
use crate::archive::index::{Index, ListControl, open_cached};
use crate::archive::writer::CompressItem;
use crate::archive::{
    ArchiveKind, EntryKind, Format, NameIssue, archive_stem, link_escapes,
};
use crate::fs::{FileKind, Stamp};
use crate::{Error, Result, display};

/// Where the members of an archive land.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ExtractInto {
    /// Straight into the destination folder.
    Here,
    /// Into a folder named after the archive (without its extensions) — unless the archive
    /// already has a single folder at its top, which then is the folder.
    #[default]
    Auto,
    /// Into a folder with this name.
    Folder { name: String },
}

fn not_found(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound
}

/// A path relative to the archive's top, cleaned: only plain components.
pub fn clean_inner(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

struct Ex<'a> {
    engine: &'a Engine,
    policy: ConflictPolicy,
    ws: WarningSet,
    steps: Vec<Step>,
    /// Folders this plan makes: nothing in them can clash with what is on the disk.
    made: HashSet<PathBuf>,
    taken: HashSet<OsString>,
    case_insensitive: bool,
    /// Folders the archive lists explicitly, with the permissions and time it gives them.
    explicit_dirs: HashMap<PathBuf, (Option<u32>, Option<Stamp>)>,
    /// Where each extracted member went, by its path in the archive (for hard links).
    placed: HashMap<PathBuf, PathBuf>,
    /// Folders that exist on the disk and were checked.
    existing: HashSet<PathBuf>,
    /// Paths that could not be made (something else is there): what lies under is skipped.
    refused: HashSet<PathBuf>,
}

impl Ex<'_> {
    fn key(&self, p: &Path) -> OsString {
        if self.case_insensitive {
            OsString::from(p.as_os_str().to_string_lossy().to_lowercase())
        } else {
            p.as_os_str().to_os_string()
        }
    }

    /// Make sure the folder `dir` exists when the plan runs. `Err(path)` names what stands in
    /// the way.
    fn ensure_dir(&mut self, dir: &Path, mode: Option<u32>) -> std::result::Result<(), PathBuf> {
        if self.made.contains(dir) || self.existing.contains(dir) {
            return Ok(());
        }
        if self.refused.contains(dir) {
            return Err(dir.to_path_buf());
        }
        match self.engine.fs.lstat(dir) {
            Ok(m) if m.is_dir() => {
                self.existing.insert(dir.to_path_buf());
                Ok(())
            }
            Ok(_) => {
                if self.policy == ConflictPolicy::Overwrite {
                    self.ws
                        .add(WarningKind::Overwrite, Severity::Warning, Some(dir));
                    self.steps.push(Step::TrashItem {
                        path: dir.to_path_buf(),
                    });
                    self.make(dir, mode)
                } else {
                    self.ws
                        .add(WarningKind::Conflict, Severity::Warning, Some(dir));
                    self.refused.insert(dir.to_path_buf());
                    Err(dir.to_path_buf())
                }
            }
            Err(e) if not_found(&e) => self.make(dir, mode),
            Err(e) => {
                self.ws.add_with(
                    WarningKind::Unreadable,
                    Severity::Warning,
                    Some(dir),
                    Some(Error::io("inspect", dir, e).to_string()),
                );
                self.refused.insert(dir.to_path_buf());
                Err(dir.to_path_buf())
            }
        }
    }

    fn make(&mut self, dir: &Path, mode: Option<u32>) -> std::result::Result<(), PathBuf> {
        if let Some(parent) = dir.parent()
            && !parent.as_os_str().is_empty()
            && !self.is_base(parent)
        {
            self.ensure_dir(parent, None)?;
        }
        self.steps.push(Step::MakeDir {
            path: dir.to_path_buf(),
            // Writable while it is filled; its own permissions come last.
            mode: mode.map(|m| m | 0o700),
            restore_mode: None,
        });
        self.made.insert(dir.to_path_buf());
        self.taken.insert(self.key(dir));
        Ok(())
    }

    /// The folder the user chose to extract into: it must already be there.
    fn is_base(&self, p: &Path) -> bool {
        self.existing.contains(p)
    }

    /// The first free `name (n).ext` in `parent`.
    fn keep_both(&self, parent: &Path, name: &OsStr) -> PathBuf {
        for n in 1u32.. {
            let cand = parent.join(names::numbered(name, n));
            if self.engine.fs.lstat(&cand).is_err() && !self.taken.contains(&self.key(&cand)) {
                return cand;
            }
        }
        unreachable!("u32 range exhausted")
    }

    /// Where a new file or link at `dst` goes, given what is already there and the policy.
    /// `None` means the member is not extracted.
    fn claim(&mut self, dst: &Path) -> Option<PathBuf> {
        let parent = dst.parent().unwrap_or(Path::new(""));
        let in_new_folder = self.made.contains(parent);
        let planned = self.taken.contains(&self.key(dst));
        let exists = !in_new_folder && self.engine.fs.lstat(dst).is_ok();
        if !planned && !exists {
            self.taken.insert(self.key(dst));
            return Some(dst.to_path_buf());
        }
        self.ws
            .add(WarningKind::Conflict, Severity::Warning, Some(dst));
        let name = dst.file_name().unwrap_or_default();
        match self.policy {
            ConflictPolicy::Skip => None,
            ConflictPolicy::KeepBoth => {
                let new = self.keep_both(parent, name);
                self.taken.insert(self.key(&new));
                Some(new)
            }
            ConflictPolicy::Overwrite if exists && !planned => {
                self.ws
                    .add(WarningKind::Overwrite, Severity::Warning, Some(dst));
                self.steps.push(Step::TrashItem {
                    path: dst.to_path_buf(),
                });
                self.taken.insert(self.key(dst));
                Some(dst.to_path_buf())
            }
            // Two members with one name in one archive: never silently lose one.
            ConflictPolicy::Overwrite => {
                let new = self.keep_both(parent, name);
                self.taken.insert(self.key(&new));
                Some(new)
            }
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Count {
    files: u64,
    dirs: u64,
    symlinks: u64,
    bytes: u64,
}

impl Engine {
    /// Plan taking `only` (paths inside the archive; empty = everything) out of `archive`.
    pub fn plan_extract(
        &self,
        archive: &Path,
        only: &[PathBuf],
        dest: &Path,
        into: &ExtractInto,
        policy: ConflictPolicy,
        ctl: ScanControl<'_>,
    ) -> Result<Plan> {
        let ScanControl { cancel, progress } = ctl;
        let cancelled = || cancel.load(Ordering::Relaxed);
        let mut report = |entries: u64, bytes: u64| {
            progress(&ScanProgress {
                files: entries,
                dirs: 0,
                bytes,
                current: archive.to_path_buf(),
            })
        };
        let ix = {
            let mut lc = ListControl::new(&cancelled)
                .with_max_entries(self.archive_limits.max_entries)
                .with_progress(&mut report);
            open_cached(archive, &mut lc).map_err(|e| e.into_error(archive))?
        };
        let archive_name = archive.file_name().unwrap_or_default().to_os_string();

        // The selection, as cleaned inner paths.
        let mut selection: Vec<PathBuf> = Vec::new();
        for o in only {
            match clean_inner(o) {
                Some(p) if !p.as_os_str().is_empty() => selection.push(p),
                _ => {
                    return Err(Error::Invalid(format!(
                        "{} is not a path inside the archive",
                        display::path(o)
                    )));
                }
            }
        }

        // Where the members go.
        let mut root: PathBuf = if !selection.is_empty() {
            dest.to_path_buf()
        } else {
            match into {
                ExtractInto::Here => dest.to_path_buf(),
                ExtractInto::Auto => {
                    if ix.single_top_folder().is_some() || matches!(ix.format, Format::Single(_)) {
                        dest.to_path_buf()
                    } else {
                        dest.join(archive_stem(&archive_name))
                    }
                }
                ExtractInto::Folder { name } => {
                    names::validate(&self.platform.path_rules(), OsStr::new(name))
                        .map_err(Error::Invalid)?;
                    dest.join(name)
                }
            }
        };

        let mut ex = Ex {
            engine: self,
            policy,
            ws: WarningSet::default(),
            steps: Vec::new(),
            made: HashSet::new(),
            taken: HashSet::new(),
            case_insensitive: self.platform.path_rules().case_insensitive,
            explicit_dirs: HashMap::new(),
            placed: HashMap::new(),
            existing: HashSet::new(),
            refused: HashSet::new(),
        };

        // The destination folder must be there and writable.
        let problem = match self.fs.stat(dest) {
            Ok(m) if m.is_dir() => {
                ex.existing.insert(dest.to_path_buf());
                (!self.fs.can_write(dest)).then(String::new)
            }
            Ok(_) => Some("not a folder".to_string()),
            Err(e) => Some(Error::io("open", dest, e).to_string()),
        };
        if let Some(detail) = problem {
            ex.ws.add_with(
                WarningKind::NotWritable,
                Severity::Blocking,
                Some(dest),
                (!detail.is_empty()).then_some(detail),
            );
        } else if root != dest {
            // A wrapper folder is made before anything lands in it.
            match self.fs.lstat(&root) {
                Ok(m) if m.is_dir() => {
                    ex.existing.insert(root.clone());
                    ex.ws.add(WarningKind::Merge, Severity::Info, Some(&root));
                }
                // A file has the folder's name (the archive itself, say): take the next free
                // name, unless the user named the folder.
                Ok(_) if matches!(into, ExtractInto::Auto) => {
                    let name = root.file_name().unwrap_or_default().to_os_string();
                    root = ex.keep_both(dest, &name);
                    let _ = ex.ensure_dir(&root, None);
                }
                Ok(_) => ex
                    .ws
                    .add(WarningKind::Conflict, Severity::Blocking, Some(&root)),
                Err(e) if not_found(&e) => {
                    let _ = ex.ensure_dir(&root, None);
                }
                Err(e) => ex.ws.add_with(
                    WarningKind::Unreadable,
                    Severity::Blocking,
                    Some(&root),
                    Some(Error::io("inspect", &root, e).to_string()),
                ),
            }
        }
        if ex.ws.has_blocking() {
            let mut plan = Plan::empty(
                OpKind::Extract,
                format!("Extract {}", display::name(&archive_name)),
            );
            plan.destination = Some(root);
            plan.policy = policy;
            plan.warnings = ex.ws.finish();
            return Ok(plan);
        }
        self.plan_extract_at(archive, dest, policy, &ix, &selection, root, ex)
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_extract_at(
        &self,
        archive: &Path,
        dest: &Path,
        policy: ConflictPolicy,
        ix: &Index,
        selection: &[PathBuf],
        root: PathBuf,
        mut ex: Ex<'_>,
    ) -> Result<Plan> {
        let archive_name = archive.file_name().unwrap_or_default().to_os_string();
        let rules = self.platform.path_rules();
        let rules = &rules;
        // The archive's own links, so that nothing is written through one.
        let link_paths: HashSet<&Path> = ix
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Symlink)
            .map(|e| e.path.as_path())
            .collect();
        let mut counts: Vec<Count> = vec![Count::default(); selection.len().max(1)];
        let mut setuid = 0u64;
        let mut written = 0u64;
        let mut hardlink_count = 0u64;

        for e in &ix.entries {
            // Which part of the selection does it belong to, and where does it land?
            let (rel, sel_ix) = if selection.is_empty() {
                (e.path.clone(), 0)
            } else {
                let Some((i, sel)) = selection
                    .iter()
                    .enumerate()
                    .find(|(_, s)| e.path.starts_with(s))
                else {
                    continue;
                };
                let tail = e.path.strip_prefix(sel).unwrap_or(Path::new(""));
                let base = sel.file_name().map(PathBuf::from).unwrap_or_default();
                if tail.as_os_str().is_empty() {
                    (base, i)
                } else {
                    (base.join(tail), i)
                }
            };
            if rel.as_os_str().is_empty() && e.issue.is_none() {
                continue; // the archive's own top
            }
            if let Some(issue) = e.issue {
                ex.ws.add_with(
                    WarningKind::UnsafePath,
                    Severity::Warning,
                    Some(&archive.join(&e.path)),
                    Some(issue.reason().to_string()),
                );
                continue;
            }
            if e.path
                .ancestors()
                .skip(1)
                .any(|a| !a.as_os_str().is_empty() && link_paths.contains(a))
            {
                ex.ws.add_with(
                    WarningKind::UnsafePath,
                    Severity::Warning,
                    Some(&archive.join(&e.path)),
                    Some("its path goes through a link stored in the archive".into()),
                );
                continue;
            }
            if e.kind == EntryKind::Special {
                ex.ws.add(
                    WarningKind::SpecialFile,
                    Severity::Warning,
                    Some(&archive.join(&e.path)),
                );
                continue;
            }
            if e.encrypted && e.kind != EntryKind::Dir {
                ex.ws.add(
                    WarningKind::Encrypted,
                    Severity::Blocking,
                    Some(&archive.join(&e.path)),
                );
                continue;
            }
            // Every name must be valid here.
            if let Some(bad) = rel
                .components()
                .filter_map(|c| match c {
                    Component::Normal(n) => Some(n),
                    _ => None,
                })
                .find_map(|n| names::validate(rules, n).err())
            {
                ex.ws.add_with(
                    WarningKind::InvalidName,
                    Severity::Warning,
                    Some(&archive.join(&e.path)),
                    Some(bad),
                );
                continue;
            }
            let dst = root.join(&rel);
            let src = archive.join(&e.path);
            let parent = dst.parent().unwrap_or(&root).to_path_buf();
            match e.kind {
                EntryKind::Dir => {
                    let was_there = ex.made.contains(&dst);
                    if ex.ensure_dir(&dst, e.mode).is_err() {
                        continue;
                    }
                    if ex.made.contains(&dst) {
                        if !was_there {
                            counts[sel_ix].dirs += 1;
                        }
                        ex.explicit_dirs
                            .insert(dst.clone(), (e.mode, e.mtime.map(Stamp::from)));
                    } else {
                        ex.ws.add(WarningKind::Merge, Severity::Info, Some(&dst));
                    }
                }
                EntryKind::File | EntryKind::Symlink | EntryKind::Hardlink => {
                    if ex.ensure_dir(&parent, None).is_err() {
                        continue;
                    }
                    if parent != root || !ex.existing.contains(&parent) {
                        // Folders made on the way count as folders of this item.
                    }
                    let Some(dst) = ex.claim(&dst) else { continue };
                    match e.kind {
                        EntryKind::File => {
                            let mode = e.mode.map(|m| m & 0o1777);
                            if e.mode.is_some_and(|m| m & 0o6000 != 0) {
                                setuid += 1;
                            }
                            ex.steps.push(Step::ExtractFile {
                                src,
                                archive: archive.to_path_buf(),
                                entry: e.index,
                                dst: dst.clone(),
                                size: e.size,
                                mode,
                                mtime: e.mtime.map(Stamp::from),
                            });
                            ex.placed.insert(e.path.clone(), dst);
                            counts[sel_ix].files += 1;
                            counts[sel_ix].bytes += e.size;
                            written += e.size;
                        }
                        EntryKind::Symlink => {
                            let Some(target) = e.link.clone() else {
                                ex.ws.add(WarningKind::Unreadable, Severity::Warning, Some(&src));
                                continue;
                            };
                            ex.ws.add(WarningKind::Symlink, Severity::Info, Some(&src));
                            if link_escapes(&e.path, &target) {
                                ex.ws.add_with(
                                    WarningKind::LinkOutside,
                                    Severity::Warning,
                                    Some(&src),
                                    Some(format!("to {}", display::path(&target))),
                                );
                            }
                            ex.steps.push(Step::ExtractSymlink { src, dst, target });
                            counts[sel_ix].symlinks += 1;
                        }
                        _ => {
                            // A hard link: another name for a file extracted earlier.
                            let target = e.link.clone().unwrap_or_default();
                            let (t, issue) =
                                crate::archive::entry::sanitize(&crate::archive::entry::os_to_bytes(target.as_os_str()), false);
                            if issue.is_some() {
                                ex.ws.add_with(
                                    WarningKind::UnsafePath,
                                    Severity::Warning,
                                    Some(&src),
                                    Some(NameIssue::Traversal.reason().to_string()),
                                );
                                continue;
                            }
                            match ex.placed.get(&t).cloned() {
                                Some(existing) => {
                                    ex.steps.push(Step::HardLink {
                                        existing,
                                        link: dst,
                                        src_link: None,
                                        src_existing: None,
                                    });
                                    hardlink_count += 1;
                                    counts[sel_ix].files += 1;
                                }
                                None => ex.ws.add(
                                    WarningKind::LinkTargetMissing,
                                    Severity::Warning,
                                    Some(&src),
                                ),
                            }
                        }
                    }
                }
                EntryKind::Special => {}
            }
        }

        // Folders get the permissions and times the archive gives them once they are full,
        // innermost first.
        type Finish<'a> = (&'a PathBuf, &'a (Option<u32>, Option<Stamp>));
        let mut finish: Vec<Finish<'_>> = ex
            .explicit_dirs
            .iter()
            .filter(|(p, _)| ex.made.contains(*p))
            .collect();
        finish.sort_by(|a, b| {
            b.0.components()
                .count()
                .cmp(&a.0.components().count())
                .then_with(|| a.0.cmp(b.0))
        });
        let finish_steps: Vec<Step> = finish
            .into_iter()
            .map(|(p, (mode, mtime))| Step::FinishDir {
                path: p.clone(),
                mode: *mode,
                mtime: *mtime,
                src: None,
                uid: None,
                gid: None,
            })
            .collect();
        ex.steps.extend(finish_steps);
        if hardlink_count > 0 {
            ex.ws.add(WarningKind::HardLinksKept, Severity::Info, None);
        }
        if setuid > 0 {
            for _ in 0..setuid {
                ex.ws.add(WarningKind::SetuidDropped, Severity::Info, None);
            }
        }

        // The archive itself.
        if !ix.complete {
            ex.ws.add_with(
                WarningKind::ArchiveDamaged,
                Severity::Warning,
                Some(archive),
                ix.note.clone().map(|n| format!(": {n}")),
            );
        }
        if ix.format == Format::Rar {
            // nothing extra: the tool was already needed to list it
        }
        let total = Count {
            files: counts.iter().map(|c| c.files).sum(),
            dirs: counts.iter().map(|c| c.dirs).sum(),
            symlinks: counts.iter().map(|c| c.symlinks).sum(),
            bytes: counts.iter().map(|c| c.bytes).sum(),
        };
        if let Some(why) = self.archive_limits.suspicious(ix.packed, written) {
            ex.ws.add_with(
                WarningKind::ArchiveBomb,
                Severity::Warning,
                Some(archive),
                Some(format!(": {why}")),
            );
        }

        // Room for what will be written.
        let probe = nearest_existing(&*self.fs, &root).unwrap_or_else(|| dest.to_path_buf());
        if written > 0
            && let Ok(avail) = self.fs.available_space(&probe)
            && written > avail
        {
            ex.ws.add_with(
                WarningKind::LowSpace,
                Severity::Warning,
                Some(&probe),
                Some(format!(
                    "need {}, {} free",
                    display::bytes(written),
                    display::bytes(avail)
                )),
            );
        }

        let title = if selection.is_empty() {
            format!(
                "Extract {} to {}",
                display::name(&archive_name),
                display::path(&root)
            )
        } else {
            format!(
                "Extract {} from {} to {}",
                if selection.len() == 1 {
                    "1 item".to_string()
                } else {
                    format!("{} items", selection.len())
                },
                display::name(&archive_name),
                display::path(&root)
            )
        };
        let mut plan = Plan::empty(OpKind::Extract, title);
        plan.destination = Some(root.clone());
        plan.policy = policy;
        plan.steps = ex.steps;
        plan.totals = Totals {
            items: counts.len() as u64,
            files: total.files,
            dirs: total.dirs,
            symlinks: total.symlinks,
            bytes: total.bytes,
        };
        if selection.is_empty() {
            plan.items.push(ItemSummary {
                path: archive.to_path_buf(),
                kind: FileKind::File,
                action: if plan.steps.is_empty() {
                    ItemAction::Skip
                } else {
                    ItemAction::Copy
                },
                files: total.files,
                dirs: total.dirs,
                symlinks: total.symlinks,
                bytes: total.bytes,
                target: Some(root),
            });
        } else {
            for (i, sel) in selection.iter().enumerate() {
                let c = counts[i];
                let kind = match ix.find(sel).map(|e| e.kind) {
                    Some(EntryKind::Dir) | None => FileKind::Dir,
                    Some(EntryKind::Symlink) => FileKind::Symlink,
                    Some(_) => FileKind::File,
                };
                plan.items.push(ItemSummary {
                    path: archive.join(sel),
                    kind,
                    action: if c.files + c.dirs + c.symlinks == 0 {
                        ItemAction::Skip
                    } else {
                        ItemAction::Copy
                    },
                    files: c.files,
                    dirs: c.dirs,
                    symlinks: c.symlinks,
                    bytes: c.bytes,
                    target: sel.file_name().map(|n| dest.join(n)),
                });
            }
        }
        plan.warnings = ex.ws.finish();
        Ok(plan)
    }

    // ------------------------------------------------------------------ compress

    /// Plan putting `scan`'s items into a new archive at `archive`.
    pub fn plan_compress(
        &self,
        scan: &Scan,
        archive: &Path,
        kind: ArchiveKind,
        policy: ConflictPolicy,
    ) -> Plan {
        let name = archive.file_name().unwrap_or_default().to_os_string();
        let dir = archive.parent().unwrap_or(Path::new("."));
        let mut plan = Plan::empty(
            OpKind::Compress,
            format!(
                "Compress {} into {}",
                if scan.roots.len() == 1 {
                    "1 item".to_string()
                } else {
                    format!("{} items", scan.roots.len())
                },
                display::name(&name)
            ),
        );
        plan.policy = policy;
        let mut ws = WarningSet::default();
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

        // The archive's name and place.
        if let Err(why) = names::validate(&self.platform.path_rules(), &name) {
            ws.add_with(
                WarningKind::InvalidName,
                Severity::Blocking,
                Some(archive),
                Some(why),
            );
        }
        match self.fs.stat(dir) {
            Ok(m) if m.is_dir() && self.fs.can_write(dir) => {}
            Ok(m) if m.is_dir() => ws.add(WarningKind::NotWritable, Severity::Blocking, Some(dir)),
            Ok(_) => ws.add_with(
                WarningKind::NotWritable,
                Severity::Blocking,
                Some(dir),
                Some("not a folder".into()),
            ),
            Err(e) => ws.add_with(
                WarningKind::NotWritable,
                Severity::Blocking,
                Some(dir),
                Some(Error::io("open", dir, e).to_string()),
            ),
        }
        let mut dst = archive.to_path_buf();
        let mut trash_old = false;
        if self.fs.lstat(archive).is_ok() {
            ws.add(WarningKind::Conflict, Severity::Warning, Some(archive));
            match policy {
                ConflictPolicy::Skip => {
                    ws.add(WarningKind::Conflict, Severity::Blocking, Some(archive));
                }
                ConflictPolicy::KeepBoth => {
                    for n in 1u32.. {
                        let cand = dir.join(names::numbered(&name, n));
                        if self.fs.lstat(&cand).is_err() {
                            dst = cand;
                            break;
                        }
                    }
                }
                ConflictPolicy::Overwrite => {
                    ws.add(WarningKind::Overwrite, Severity::Warning, Some(archive));
                    trash_old = true;
                }
            }
        }
        plan.destination = Some(dst.clone());

        // Two items with one name would make one member overwrite the other.
        let mut top_names: HashSet<OsString> = HashSet::new();
        for r in &scan.roots {
            if !top_names.insert(r.name.clone()) {
                ws.add_with(
                    WarningKind::InvalidName,
                    Severity::Blocking,
                    Some(&r.path),
                    Some("two items have the same name".into()),
                );
            }
        }

        let mut items: Vec<CompressItem> = Vec::new();
        let mut totals = Totals {
            items: scan.roots.len() as u64,
            ..Default::default()
        };
        let mut est = 0f64;
        let mut non_utf8 = 0u64;
        let mut links = 0u64;
        for r in &scan.roots {
            let before = (totals.files, totals.dirs, totals.symlinks, totals.bytes);
            if r.path.parent().is_none() {
                ws.add_with(
                    WarningKind::InvalidName,
                    Severity::Blocking,
                    Some(&r.path),
                    Some("cannot archive the root".into()),
                );
                continue;
            }
            walk_compress(
                r,
                Path::new(""),
                &dst,
                archive,
                &mut items,
                &mut totals,
                &mut ws,
                &mut est,
                kind,
                &mut non_utf8,
                &mut links,
            );
            plan.items.push(ItemSummary {
                path: r.path.clone(),
                kind: r.meta.kind,
                action: ItemAction::Copy,
                files: totals.files - before.0,
                dirs: totals.dirs - before.1,
                symlinks: totals.symlinks - before.2,
                bytes: totals.bytes - before.3,
                target: Some(dst.clone()),
            });
        }
        if kind == ArchiveKind::Zip && links > 0 {
            ws.add(WarningKind::ZipLinks, Severity::Info, None);
        }
        if kind == ArchiveKind::Zip && non_utf8 > 0 {
            for _ in 0..non_utf8 {
                ws.add_with(
                    WarningKind::NamesChanged,
                    Severity::Warning,
                    None,
                    Some("ZIP stores names as text".into()),
                );
            }
        }
        let estimate = (est as u64).max(if totals.bytes > 0 { 1 } else { 0 });
        plan.estimated_bytes = Some(estimate);
        if estimate > 0
            && let Ok(avail) = self.fs.available_space(dir)
            && estimate > avail
        {
            ws.add_with(
                WarningKind::LowSpace,
                Severity::Warning,
                Some(dir),
                Some(format!(
                    "about {} needed, {} free",
                    display::bytes(estimate),
                    display::bytes(avail)
                )),
            );
        }
        if trash_old {
            plan.steps.push(Step::TrashItem {
                path: archive.to_path_buf(),
            });
        }
        if items.is_empty() {
            ws.add_with(
                WarningKind::Other,
                Severity::Blocking,
                None,
                Some("nothing to put in the archive".into()),
            );
        } else {
            plan.steps.push(Step::Compress {
                dst,
                format: kind,
                items,
                size: totals.bytes,
            });
        }
        plan.totals = totals;
        plan.warnings = ws.finish();
        plan
    }
}

/// A guess at how large an item becomes: files that are already compressed stay as they
/// are, the rest shrinks by what the format usually achieves.
fn estimate_of(name: &OsStr, size: u64, kind: ArchiveKind) -> f64 {
    const PACKED: &[&str] = &[
        "jpg", "jpeg", "png", "gif", "webp", "heic", "avif", "mp3", "m4a", "aac", "ogg", "opus",
        "flac", "mp4", "mkv", "mov", "webm", "avi", "zip", "gz", "tgz", "bz2", "xz", "zst", "7z",
        "rar", "jar", "docx", "xlsx", "pptx", "odt", "pdf", "apk", "deb", "rpm",
    ];
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if PACKED.contains(&ext.as_str()) {
        return size as f64;
    }
    let ratio = match kind {
        ArchiveKind::Zip | ArchiveKind::TarGz => 0.45,
        ArchiveKind::TarZst => 0.42,
        ArchiveKind::TarXz => 0.32,
    };
    size as f64 * ratio
}

#[allow(clippy::too_many_arguments)]
fn walk_compress(
    n: &ScanNode,
    parent: &Path,
    dst: &Path,
    old_dst: &Path,
    items: &mut Vec<CompressItem>,
    totals: &mut Totals,
    ws: &mut WarningSet,
    est: &mut f64,
    kind: ArchiveKind,
    non_utf8: &mut u64,
    links: &mut u64,
) {
    // The archive being replaced (or about to be made) is never part of itself.
    if n.path == dst || n.path == old_dst {
        return;
    }
    let name = parent.join(&n.name);
    if name.to_str().is_none() {
        *non_utf8 += 1;
    }
    let mtime = n.meta.mtime.map(Stamp::from);
    match n.meta.kind {
        FileKind::File => {
            totals.files += 1;
            totals.bytes += n.meta.size;
            *est += estimate_of(&n.name, n.meta.size, kind);
            items.push(CompressItem {
                src: n.path.clone(),
                name,
                kind: FileKind::File,
                size: n.meta.size,
                mode: n.meta.mode.map(|m| m & 0o7777),
                mtime,
                link: None,
            });
        }
        FileKind::Symlink => {
            let Some(target) = n.link_target.clone() else {
                ws.add(WarningKind::Unreadable, Severity::Warning, Some(&n.path));
                return;
            };
            totals.symlinks += 1;
            *links += 1;
            ws.add(WarningKind::Symlink, Severity::Info, Some(&n.path));
            items.push(CompressItem {
                src: n.path.clone(),
                name,
                kind: FileKind::Symlink,
                size: 0,
                mode: n.meta.mode.map(|m| m & 0o7777),
                mtime,
                link: Some(target),
            });
        }
        FileKind::Dir => {
            if n.loop_skipped {
                ws.add(WarningKind::MountLoop, Severity::Warning, Some(&n.path));
                return;
            }
            if let Some(msg) = &n.unreadable {
                ws.add_with(
                    WarningKind::Unreadable,
                    Severity::Warning,
                    Some(&n.path),
                    Some(msg.clone()),
                );
            }
            totals.dirs += 1;
            items.push(CompressItem {
                src: n.path.clone(),
                name: name.clone(),
                kind: FileKind::Dir,
                size: 0,
                mode: n.meta.mode.map(|m| m & 0o7777),
                mtime,
                link: None,
            });
            for c in &n.children {
                walk_compress(
                    c, &name, dst, old_dst, items, totals, ws, est, kind, non_utf8, links,
                );
            }
        }
        FileKind::Other => ws.add(WarningKind::SpecialFile, Severity::Warning, Some(&n.path)),
    }
}

/// The nearest ancestor of `p` (or `p`) that exists, for asking about free space.
fn nearest_existing(fs: &dyn crate::fs::FsEngine, p: &Path) -> Option<PathBuf> {
    p.ancestors()
        .find(|a| !a.as_os_str().is_empty() && fs.stat(a).is_ok())
        .map(Path::to_path_buf)
}
