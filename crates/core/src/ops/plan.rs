//! The plan: what an operation *will* do, as data, before anything is touched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::archive::ArchiveKind;
use crate::archive::writer::CompressItem;
use crate::fs::{Fingerprint, Stamp};
use crate::i18n::{self, Key, t, tf};
use crate::platform::TrashedItem;
use crate::{display, pathcodec};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Copy,
    Move,
    Rename,
    BulkRename,
    MakeDir,
    /// Create an empty file.
    MakeFile,
    Trash,
    Delete,
    Undo,
    /// Take files out of an archive.
    Extract,
    /// Put files into a new archive.
    Compress,
    /// Change the tags of files (the Finder's tags on macOS, `user.xdg.tags` on Linux).
    SetTags,
}

impl OpKind {
    pub fn verb(self) -> &'static str {
        match self {
            OpKind::Copy => t(Key::VerbCopy),
            OpKind::Move => t(Key::VerbMove),
            OpKind::Rename => t(Key::VerbRename),
            OpKind::BulkRename => t(Key::VerbBulkRename),
            OpKind::MakeDir => t(Key::VerbNewFolder),
            OpKind::MakeFile => t(Key::VerbNewFile),
            OpKind::Trash => t(Key::VerbTrash),
            OpKind::Delete => t(Key::VerbDelete),
            OpKind::Undo => t(Key::VerbUndo),
            OpKind::Extract => t(Key::VerbExtract),
            OpKind::Compress => t(Key::VerbCompress),
            OpKind::SetTags => t(Key::VerbTags),
        }
    }
}

/// One atomic action. The whole engine (planning, execution, journal, undo) speaks
/// only this vocabulary, which is what makes "every operation is undoable" tractable:
/// each step knows its own inverse (see [`Step::inverse`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    MakeDir {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
        mode: Option<u32>,
        /// Permissions to give the folder once the whole run is over (it was made writable
        /// for the run): used when undo brings back a folder that was read-only.
        #[serde(default)]
        restore_mode: Option<u32>,
    },
    /// A new, empty file.
    MakeFile {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
        mode: Option<u32>,
    },
    /// Apply the final permissions/time of a directory created earlier in the same run
    /// (after its children exist, so a read-only source dir stays writable meanwhile).
    FinishDir {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
        mode: Option<u32>,
        mtime: Option<Stamp>,
        /// The folder this one is a copy of: its owner and extended attributes are carried
        /// over, once the folder is full.
        #[serde(with = "pathcodec::opt_path", default)]
        #[schemars(with = "Option<String>")]
        src: Option<PathBuf>,
        #[serde(default)]
        uid: Option<u32>,
        #[serde(default)]
        gid: Option<u32>,
    },
    CopyFile {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        dst: PathBuf,
        size: u64,
        mode: Option<u32>,
        mtime: Option<Stamp>,
        atime: Option<Stamp>,
        /// Re-read and compare the copy.
        verify: bool,
        /// Cross-device move: remove the source once the copy is verified.
        remove_source: bool,
        /// Owner and group of the source, given to the copy when this process may.
        #[serde(default)]
        uid: Option<u32>,
        #[serde(default)]
        gid: Option<u32>,
        /// Bytes really stored, when that is less than `size` (a sparse file): what progress
        /// and free space are counted in.
        #[serde(default)]
        stored: Option<u64>,
        /// Later steps of this plan make more names for this file: its fingerprint must not
        /// depend on its link count.
        #[serde(default)]
        link_primary: bool,
    },
    /// Another name for a file this plan copied: the copies of files that were hard links of
    /// each other stay hard links of each other.
    HardLink {
        /// The copy already made.
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        existing: PathBuf,
        /// The new name.
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        link: PathBuf,
        /// Cross-device move: the source name that goes away with this one.
        #[serde(with = "pathcodec::opt_path", default)]
        #[schemars(with = "Option<String>")]
        src_link: Option<PathBuf>,
        /// Cross-device move: where `existing` came from (for the undo).
        #[serde(with = "pathcodec::opt_path", default)]
        #[schemars(with = "Option<String>")]
        src_existing: Option<PathBuf>,
    },
    CopySymlink {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        dst: PathBuf,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        target: PathBuf,
        remove_source: bool,
    },
    /// One file out of an archive, written under a temporary name and renamed into place
    /// when complete. The members of one archive are extracted in the order the archive
    /// holds them, in a single pass.
    ExtractFile {
        /// The member as a path (`/home/u/photos.zip/2024/a.jpg`): what the user sees.
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        src: PathBuf,
        /// The archive file.
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        archive: PathBuf,
        /// The member's position in the archive's list when the plan was made.
        entry: usize,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        dst: PathBuf,
        /// Declared size: what is written never exceeds it.
        size: u64,
        mode: Option<u32>,
        mtime: Option<Stamp>,
    },
    /// A symbolic link stored in an archive, created as a link and never followed.
    ExtractSymlink {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        src: PathBuf,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        dst: PathBuf,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        target: PathBuf,
    },
    /// A whole new archive, written under a temporary name and renamed into place.
    Compress {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        dst: PathBuf,
        format: ArchiveKind,
        items: Vec<CompressItem>,
        /// Bytes of file data that will be read.
        size: u64,
    },
    Rename {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        from: PathBuf,
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        to: PathBuf,
    },
    TrashItem {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
    },
    /// Permanent removal of a file or symlink. With `expect`, refuses to remove a
    /// file that changed since it was recorded (used by undo).
    RemoveFile {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
        expect: Option<Fingerprint>,
    },
    /// Removal of an *empty* directory.
    RemoveDir {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
    },
    Restore {
        item: TrashedItem,
    },
    /// Replace the tags of a file or folder. `before` is what it has now, checked when the
    /// step runs: tags changed in the meantime are left alone, so that undo never overwrites
    /// what somebody did after the operation.
    SetTags {
        #[serde(with = "pathcodec::path")]
        #[schemars(with = "String")]
        path: PathBuf,
        before: Vec<crate::platform::Tag>,
        after: Vec<crate::platform::Tag>,
    },
}

/// Tag names for a sentence: `Red, Work`, or `none`.
pub fn tag_names(tags: &[crate::platform::Tag]) -> String {
    if tags.is_empty() {
        t(Key::TagNone).to_string()
    } else {
        tags.iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Step {
    /// The path this step is "about", for progress and error messages.
    pub fn path(&self) -> &Path {
        match self {
            Step::MakeDir { path, .. }
            | Step::MakeFile { path, .. }
            | Step::FinishDir { path, .. }
            | Step::TrashItem { path }
            | Step::RemoveFile { path, .. }
            | Step::SetTags { path, .. }
            | Step::RemoveDir { path } => path,
            Step::Compress { dst, .. } => dst,
            Step::ExtractFile { src, .. } | Step::ExtractSymlink { src, .. } => src,
            Step::CopyFile { src, .. } | Step::CopySymlink { src, .. } => src,
            Step::HardLink { link, .. } => link,
            Step::Rename { from, .. } => from,
            Step::Restore { item } => &item.original,
        }
    }

    /// Where the step writes, if anywhere.
    pub fn destination(&self) -> Option<&Path> {
        match self {
            Step::MakeDir { path, .. } | Step::MakeFile { path, .. } => Some(path),
            Step::CopyFile { dst, .. }
            | Step::CopySymlink { dst, .. }
            | Step::ExtractFile { dst, .. }
            | Step::ExtractSymlink { dst, .. }
            | Step::Compress { dst, .. } => Some(dst),
            Step::HardLink { link, .. } => Some(link),
            Step::Rename { to, .. } => Some(to),
            Step::Restore { item } => Some(&item.original),
            _ => None,
        }
    }

    pub fn bytes(&self) -> u64 {
        match self {
            Step::CopyFile { size, stored, .. } => stored.unwrap_or(*size),
            Step::ExtractFile { size, .. } | Step::Compress { size, .. } => *size,
            _ => 0,
        }
    }

    pub fn describe(&self) -> String {
        use Key::*;
        let p = |x: &Path| display::path(x);
        match self {
            Step::MakeDir { path, .. } => tf(StepMakeDir, &[&p(path)]),
            Step::MakeFile { path, .. } => tf(StepMakeFile, &[&p(path)]),
            Step::FinishDir { path, .. } => tf(StepFinishDir, &[&p(path)]),
            Step::CopyFile {
                src,
                dst,
                remove_source,
                ..
            } => tf(
                if *remove_source {
                    StepMoveFile
                } else {
                    StepCopy
                },
                &[&p(src), &p(dst)],
            ),
            Step::CopySymlink {
                src,
                dst,
                remove_source,
                ..
            } => tf(
                if *remove_source {
                    StepMoveLink
                } else {
                    StepCopyLink
                },
                &[&p(src), &p(dst)],
            ),
            Step::ExtractFile { src, dst, .. } => tf(StepExtract, &[&p(src), &p(dst)]),
            Step::ExtractSymlink { src, dst, .. } => tf(StepExtractLink, &[&p(src), &p(dst)]),
            Step::Compress { dst, items, .. } => tf(StepCompress, &[&items.len(), &p(dst)]),
            Step::HardLink { existing, link, .. } => tf(StepHardLink, &[&p(existing), &p(link)]),
            Step::Rename { from, to } => tf(StepRename, &[&p(from), &p(to)]),
            Step::TrashItem { path } => tf(StepTrash, &[&p(path)]),
            Step::RemoveFile { path, .. } => tf(StepRemoveFile, &[&p(path)]),
            Step::RemoveDir { path } => tf(StepRemoveDir, &[&p(path)]),
            Step::Restore { item } => tf(StepRestore, &[&p(&item.original)]),
            Step::SetTags {
                path,
                before,
                after,
            } => tf(StepTags, &[&p(path), &tag_names(before), &tag_names(after)]),
        }
    }

    /// The step that reverses this one, given what executing it produced.
    /// `None` when there is nothing to reverse (or it cannot be reversed).
    pub fn inverse(&self, result: &StepResult) -> Option<Step> {
        if result.status != StepStatus::Done {
            return None;
        }
        match self {
            Step::MakeDir { path, .. } if result.created => {
                Some(Step::RemoveDir { path: path.clone() })
            }
            Step::MakeFile { path, .. } => Some(Step::RemoveFile {
                path: path.clone(),
                expect: result.after.clone(),
            }),
            Step::MakeDir { .. } | Step::FinishDir { .. } => None,
            Step::CopyFile {
                src,
                dst,
                remove_source,
                size,
                mode,
                mtime,
                atime,
                uid,
                gid,
                stored,
                link_primary,
                ..
            } => {
                if *remove_source {
                    Some(Step::CopyFile {
                        src: dst.clone(),
                        dst: src.clone(),
                        size: *size,
                        mode: *mode,
                        mtime: *mtime,
                        atime: *atime,
                        verify: true,
                        remove_source: true,
                        uid: *uid,
                        gid: *gid,
                        stored: *stored,
                        link_primary: *link_primary,
                    })
                } else {
                    Some(Step::RemoveFile {
                        path: dst.clone(),
                        expect: result.after.clone(),
                    })
                }
            }
            Step::CopySymlink {
                src,
                dst,
                target,
                remove_source,
            } => {
                if *remove_source {
                    Some(Step::CopySymlink {
                        src: dst.clone(),
                        dst: src.clone(),
                        target: target.clone(),
                        remove_source: true,
                    })
                } else {
                    Some(Step::RemoveFile {
                        path: dst.clone(),
                        expect: result.after.clone(),
                    })
                }
            }
            Step::HardLink {
                existing,
                link,
                src_link,
                src_existing,
            } => match (src_link, src_existing) {
                // A moved link goes back as a link of the file it was a link of.
                (Some(back), Some(primary)) => Some(Step::HardLink {
                    existing: primary.clone(),
                    link: back.clone(),
                    src_link: Some(link.clone()),
                    src_existing: Some(existing.clone()),
                }),
                _ => Some(Step::RemoveFile {
                    path: link.clone(),
                    expect: result.after.clone(),
                }),
            },
            Step::ExtractFile { dst, .. }
            | Step::ExtractSymlink { dst, .. }
            | Step::Compress { dst, .. } => Some(Step::RemoveFile {
                path: dst.clone(),
                expect: result.after.clone(),
            }),
            Step::Rename { from, to } => Some(Step::Rename {
                from: to.clone(),
                to: from.clone(),
            }),
            Step::TrashItem { .. } => result.trashed.clone().map(|item| Step::Restore { item }),
            Step::RemoveDir { path } => {
                let mode = result.removed_dir_mode;
                Some(Step::MakeDir {
                    path: path.clone(),
                    // Writable while its contents come back; its own permissions last.
                    mode: mode.map(|m| m | 0o700),
                    restore_mode: mode.filter(|m| m & 0o700 != 0o700),
                })
            }
            // Permanent deletion and restore are not reversed here.
            Step::SetTags {
                path,
                before,
                after,
            } => Some(Step::SetTags {
                path: path.clone(),
                before: after.clone(),
                after: before.clone(),
            }),
            Step::RemoveFile { .. } | Step::Restore { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Done,
    /// Nothing to do (already in the desired state).
    NoOp,
    /// Deliberately left in place (e.g. a directory that is not empty).
    Kept,
}

/// What executing a step produced.
#[derive(Clone, Debug)]
pub struct StepResult {
    pub status: StepStatus,
    pub note: Option<String>,
    /// For `MakeDir`: this run created the directory.
    pub created: bool,
    /// Fingerprint of what was written.
    pub after: Option<Fingerprint>,
    pub trashed: Option<TrashedItem>,
    pub removed_dir_mode: Option<u32>,
    /// What could not be carried over to a copy (owner, extended attributes, ACLs, a hard
    /// link the destination cannot make...), in words.
    pub not_preserved: Vec<String>,
}

impl StepResult {
    pub fn done() -> Self {
        StepResult {
            status: StepStatus::Done,
            note: None,
            created: false,
            after: None,
            trashed: None,
            removed_dir_mode: None,
            not_preserved: Vec::new(),
        }
    }
    pub fn noop(note: impl Into<String>) -> Self {
        StepResult {
            status: StepStatus::NoOp,
            note: Some(note.into()),
            ..Self::done()
        }
    }
    pub fn kept(note: impl Into<String>) -> Self {
        StepResult {
            status: StepStatus::Kept,
            note: Some(note.into()),
            ..Self::done()
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum Severity {
    Info,
    Warning,
    /// The plan cannot be executed.
    Blocking,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum WarningKind {
    Irreversible,
    InsideItself,
    NotWritable,
    LowSpace,
    Conflict,
    Overwrite,
    Unreadable,
    SpecialFile,
    BrokenSymlink,
    Symlink,
    CircularLink,
    MountLoop,
    CrossDevice,
    SameLocation,
    /// Files with hard links outside the selection: their copies are independent.
    HardLinks,
    /// Hard links inside the selection that stay links in the copy.
    HardLinksKept,
    SparseFiles,
    /// Extended attributes the destination cannot hold.
    XattrsLost,
    AclLost,
    /// Owner and group that only their owner or root may give.
    OwnerNotKept,
    /// This platform does not carry extended attributes and ACLs over yet.
    AttributesNotKept,
    /// Items that cannot be given tags (a link, a system with no tags).
    CannotTag,
    InvalidName,
    Missing,
    Merge,
    /// An undo step was refused because the item changed since the operation.
    ModifiedSince,
    /// An undo step cannot be carried out (target occupied, source gone, ...).
    CannotUndo,
    /// Archive members whose paths would write outside the destination: not extracted.
    UnsafePath,
    /// Archive links that point outside the extracted folder (kept as links, never followed).
    LinkOutside,
    /// An archive that unpacks to far more than its size, or to more than the limit.
    ArchiveBomb,
    /// An archive that is truncated or damaged: only what could be read is listed.
    ArchiveDamaged,
    /// Members protected by a password.
    Encrypted,
    /// Symbolic links stored in a ZIP, which not every program understands.
    ZipLinks,
    /// Names that cannot be stored exactly in the chosen format.
    NamesChanged,
    /// Setuid and setgid bits dropped from extracted files.
    SetuidDropped,
    /// A hard link in an archive whose target is not being extracted.
    LinkTargetMissing,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Warning {
    pub kind: WarningKind,
    pub severity: Severity,
    pub message: String,
    pub count: u64,
    /// A few representative paths (never all of them: plans can be huge).
    #[serde(with = "pathcodec::paths")]
    #[schemars(with = "Vec<String>")]
    pub examples: Vec<PathBuf>,
}

/// Collects warnings grouped by kind so that 10 000 symlinks give one line, not 10 000.
/// (count, example paths, first detail)
type Group = (u64, Vec<PathBuf>, Option<String>);

#[derive(Default)]
pub struct WarningSet {
    by_kind: BTreeMap<(WarningKind, Severity), Group>,
}

const MAX_EXAMPLES: usize = 5;

impl WarningSet {
    pub fn add(&mut self, kind: WarningKind, severity: Severity, path: Option<&Path>) {
        self.add_with(kind, severity, path, None);
    }

    /// `detail` is appended to the example list's message the first time only.
    pub fn add_with(
        &mut self,
        kind: WarningKind,
        severity: Severity,
        path: Option<&Path>,
        detail: Option<String>,
    ) {
        let e = self.by_kind.entry((kind, severity)).or_default();
        e.0 += 1;
        if let Some(p) = path
            && e.1.len() < MAX_EXAMPLES
        {
            e.1.push(p.to_path_buf());
        }
        if e.2.is_none() {
            e.2 = detail;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.by_kind.is_empty()
    }

    pub fn has_blocking(&self) -> bool {
        self.by_kind.keys().any(|(_, s)| *s == Severity::Blocking)
    }

    pub fn finish(self) -> Vec<Warning> {
        let mut out: Vec<Warning> = self
            .by_kind
            .into_iter()
            .map(|((kind, severity), (count, examples, detail))| Warning {
                kind,
                severity,
                message: message_for(kind, count, detail.as_deref()),
                count,
                examples,
            })
            .collect();
        // Most serious first, then by kind: stable and deterministic.
        out.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.kind.cmp(&b.kind)));
        out
    }
}

/// A sentence that depends on a count: the key for one, the key for many (`{0}` is the count,
/// `{1}` the detail).
fn pl(n: u64, one: Key, many: Key, extra: &str) -> String {
    let count = i18n::thousands(n);
    i18n::tf(if n == 1 { one } else { many }, &[&count, &extra])
}

fn message_for(kind: WarningKind, n: u64, detail: Option<&str>) -> String {
    use Key::*;
    let extra = detail.map(|d| format!(" ({d})")).unwrap_or_default();
    let x = extra.as_str();
    match kind {
        WarningKind::Irreversible => t(WIrreversible).to_string(),
        WarningKind::InsideItself => pl(n, WInsideItselfOne, WInsideItselfMany, x),
        WarningKind::NotWritable => tf(WNotWritable, &[&x]),
        WarningKind::LowSpace => tf(WLowSpace, &[&x]),
        WarningKind::Conflict => pl(n, WConflictOne, WConflictMany, x),
        WarningKind::Overwrite => pl(n, WOverwriteOne, WOverwriteMany, x),
        WarningKind::Unreadable => pl(n, WUnreadableOne, WUnreadableMany, x),
        WarningKind::SpecialFile => pl(n, WSpecialFileOne, WSpecialFileMany, x),
        WarningKind::BrokenSymlink => pl(n, WBrokenSymlinkOne, WBrokenSymlinkMany, x),
        WarningKind::Symlink => pl(n, WSymlinkOne, WSymlinkMany, x),
        WarningKind::CircularLink => pl(n, WCircularLinkOne, WCircularLinkMany, x),
        WarningKind::MountLoop => pl(n, WMountLoopOne, WMountLoopMany, x),
        WarningKind::CrossDevice => t(WCrossDevice).to_string(),
        WarningKind::SameLocation => pl(n, WSameLocationOne, WSameLocationMany, x),
        WarningKind::HardLinks => pl(n, WHardLinksOne, WHardLinksMany, x),
        WarningKind::HardLinksKept => pl(n, WHardLinksKeptOne, WHardLinksKeptMany, x),
        WarningKind::SparseFiles => pl(n, WSparseOne, WSparseMany, x),
        WarningKind::XattrsLost => pl(n, WXattrsLostOne, WXattrsLostMany, x),
        WarningKind::AclLost => pl(n, WAclLostOne, WAclLostMany, x),
        WarningKind::OwnerNotKept => pl(n, WOwnerNotKeptOne, WOwnerNotKeptMany, x),
        WarningKind::AttributesNotKept => t(WAttributesNotKept).to_string(),
        WarningKind::CannotTag => pl(n, WCannotTagOne, WCannotTagMany, x),
        WarningKind::InvalidName => tf(WInvalidName, &[&x]),
        WarningKind::Missing => pl(n, WMissingOne, WMissingMany, x),
        WarningKind::Merge => pl(n, WMergeOne, WMergeMany, x),
        WarningKind::ModifiedSince => pl(n, WModifiedSinceOne, WModifiedSinceMany, x),
        WarningKind::CannotUndo => pl(n, WCannotUndoOne, WCannotUndoMany, x),
        WarningKind::UnsafePath => pl(n, WUnsafePathOne, WUnsafePathMany, x),
        WarningKind::LinkOutside => pl(n, WLinkOutsideOne, WLinkOutsideMany, x),
        WarningKind::ArchiveBomb => tf(WArchiveBomb, &[&x]),
        WarningKind::ArchiveDamaged => tf(WArchiveDamaged, &[&x]),
        WarningKind::Encrypted => pl(n, WEncryptedOne, WEncryptedMany, x),
        WarningKind::ZipLinks => pl(n, WZipLinksOne, WZipLinksMany, x),
        WarningKind::NamesChanged => pl(n, WNamesChangedOne, WNamesChangedMany, x),
        WarningKind::SetuidDropped => pl(n, WSetuidDroppedOne, WSetuidDroppedMany, x),
        WarningKind::LinkTargetMissing => pl(n, WLinkTargetMissingOne, WLinkTargetMissingMany, x),
        WarningKind::Other => detail.map_or_else(|| t(WOther).to_string(), str::to_string),
    }
}

/// How to resolve a destination that already exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Leave existing items alone, do not copy the clashing source.
    #[default]
    Skip,
    /// Copy under a new name (`name (1).ext`).
    KeepBoth,
    /// Replace; the old item is moved to the trash first so that undo can bring it back.
    Overwrite,
}

impl ConflictPolicy {
    pub fn next(self) -> Self {
        match self {
            ConflictPolicy::Skip => ConflictPolicy::KeepBoth,
            ConflictPolicy::KeepBoth => ConflictPolicy::Overwrite,
            ConflictPolicy::Overwrite => ConflictPolicy::Skip,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConflictPolicy::Skip => t(Key::PolicySkip),
            ConflictPolicy::KeepBoth => t(Key::PolicyKeepBoth),
            ConflictPolicy::Overwrite => t(Key::PolicyOverwrite),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Totals {
    /// Top-level items the user selected.
    pub items: u64,
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub bytes: u64,
}

/// What happens to one top-level item of an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ItemAction {
    Copy,
    Move,
    /// A folder merged into an existing folder of the same name.
    Merge,
    KeepBoth,
    /// Replaces an existing item (the old one goes to the trash first).
    Overwrite,
    /// Left alone: the name already exists, or it is already where it should be.
    Skip,
    Trash,
    Delete,
    Rename,
    Create,
    /// Its tags change.
    Retag,
}

impl ItemAction {
    pub fn verb(self) -> &'static str {
        match self {
            ItemAction::Copy => t(Key::ActCopy),
            ItemAction::Move => t(Key::ActMove),
            ItemAction::Merge => t(Key::ActMerge),
            ItemAction::KeepBoth => t(Key::ActKeepBoth),
            ItemAction::Overwrite => t(Key::ActReplace),
            ItemAction::Skip => t(Key::ActSkip),
            ItemAction::Trash => t(Key::ActTrash),
            ItemAction::Delete => t(Key::ActDelete),
            ItemAction::Rename => t(Key::ActRename),
            ItemAction::Create => t(Key::ActCreate),
            ItemAction::Retag => t(Key::ActRetag),
        }
    }
}

/// One selected item and what the plan does with it: the unit the plan window lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ItemSummary {
    /// The source item (or the new folder for `Create`).
    #[serde(with = "pathcodec::path")]
    #[schemars(with = "String")]
    pub path: PathBuf,
    pub kind: crate::fs::FileKind,
    pub action: ItemAction,
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub bytes: u64,
    /// Where it ends up, when that differs from where it is.
    #[serde(with = "pathcodec::opt_path", default)]
    #[schemars(with = "Option<String>")]
    pub target: Option<PathBuf>,
}

/// What a copy keeps besides the bytes, counted for the plan window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Preserved {
    /// Names that are recreated as hard links of a file copied in the same operation.
    pub hard_links: u64,
    /// Sparse files copied hole for hole.
    pub sparse_files: u64,
    /// Items with extended attributes (not counting ACLs) to carry over.
    pub xattr_items: u64,
    /// Items with ACLs to carry over.
    pub acl_items: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    pub kind: OpKind,
    pub title: String,
    #[serde(with = "pathcodec::opt_path", default)]
    #[schemars(with = "Option<String>")]
    pub destination: Option<PathBuf>,
    pub steps: Vec<Step>,
    pub totals: Totals,
    pub warnings: Vec<Warning>,
    pub policy: ConflictPolicy,
    /// False for permanent deletion: the journal records it but cannot undo it.
    pub reversible: bool,
    /// Old name -> new name, for rename previews.
    #[serde(with = "pathcodec::path_pairs")]
    #[schemars(with = "Vec<(String, String)>")]
    pub renames: Vec<(PathBuf, PathBuf)>,
    /// One entry per selected item, for a readable overview.
    pub items: Vec<ItemSummary>,
    /// What the copy keeps besides the bytes.
    #[serde(default)]
    pub preserved: Preserved,
    /// The request this plan answers, kept in the journal so the operation can be
    /// planned again later (redo).
    #[serde(default)]
    pub request: Option<crate::ops::request::OpRequest>,
    /// Set when this plan re-does an operation that was undone.
    #[serde(default)]
    pub redo_of: Option<String>,
    /// For a new archive: about how large it will be (a guess from the kind of files).
    #[serde(default)]
    pub estimated_bytes: Option<u64>,
}

impl Plan {
    pub fn empty(kind: OpKind, title: impl Into<String>) -> Plan {
        Plan {
            kind,
            title: title.into(),
            destination: None,
            steps: Vec::new(),
            totals: Totals::default(),
            warnings: Vec::new(),
            policy: ConflictPolicy::Skip,
            reversible: true,
            renames: Vec::new(),
            items: Vec::new(),
            preserved: Preserved::default(),
            request: None,
            redo_of: None,
            estimated_bytes: None,
        }
    }

    pub fn blocking(&self) -> impl Iterator<Item = &Warning> {
        self.warnings
            .iter()
            .filter(|w| w.severity == Severity::Blocking)
    }

    pub fn is_executable(&self) -> bool {
        self.blocking().next().is_none() && !self.steps.is_empty()
    }

    pub fn has_conflicts(&self) -> bool {
        self.warnings
            .iter()
            .any(|w| matches!(w.kind, WarningKind::Conflict | WarningKind::Overwrite))
    }

    /// Bytes that will be moved through memory/IO.
    pub fn total_bytes(&self) -> u64 {
        self.steps.iter().map(Step::bytes).sum()
    }
}
