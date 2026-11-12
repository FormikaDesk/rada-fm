//! A word on each operation of the history: whether its undo can be expected to put back
//! everything. It is an opinion, from one `lstat` per item and no hash: the plan of the undo
//! ([`Engine::plan_undo`](crate::ops::Engine::plan_undo)) decides, with checks of its own.
//!
//! - **undoable**: every step was recorded, and what the operation left is as it left it;
//! - **to check**: some of what it left changed or is gone (the undo leaves those alone), or
//!   it left more than [`MAX_LOOKS`] items, too many to look at here;
//! - **not guaranteed**: it ran other programs, it worked on a phone or a camera, or some of
//!   its steps ran without their way back being written. Said without looking at the disk:
//!   for the first two the plan wrote it in the record of the start.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fs::{FileKind, Fingerprint, FsEngine, Stamp};
use crate::journal::JournalEntry;
use crate::ops::Step;
use crate::platform::trash::TrashHandle;

/// Items looked at, at most, for one operation.
pub const MAX_LOOKS: usize = 2000;

/// Why the undo of an operation cannot be promised, as its plan knew it when it began.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UndoLimit {
    /// It ran other programs: rada sees only the file it published.
    Action,
    /// It worked on a phone or a camera, which keeps no promise of its own.
    Device,
}

/// Why an undo is not guaranteed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsure {
    Action,
    Device,
    /// Steps ran without their way back being written.
    Gaps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Undoable,
    /// This many of the items it left changed or are gone.
    ToCheck {
        changed: usize,
    },
    /// More items than [`MAX_LOOKS`]: not looked at.
    TooMany,
    NotGuaranteed(Unsure),
}

/// An opinion, with the folders it looked into and their times then: it holds while they
/// do not change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub health: Health,
    pub folders: Vec<(PathBuf, Option<Stamp>)>,
}

/// What an item left by an operation must still be for its undo to go as recorded.
enum Want {
    Same(Fingerprint),
    Dir,
    There,
}

/// The item a step of undo acts on, and what it expects of it; `None` for a step that
/// nothing on the disk can say anything about (tags, the Recycle Bin).
fn look_of(step: &Step) -> Option<(&Path, Want)> {
    let want = |fp: &Option<Fingerprint>| fp.clone().map_or(Want::There, Want::Same);
    match step {
        Step::Rename {
            from, expect_from, ..
        } => Some((from, want(expect_from))),
        Step::RemoveFile { path, expect } => Some((path, want(expect))),
        Step::RemoveDir { path } => Some((path, Want::Dir)),
        Step::Restore { item } => match &item.handle {
            TrashHandle::Freedesktop { stored, .. } | TrashHandle::Directory { stored } => {
                Some((stored, Want::There))
            }
            TrashHandle::Opaque { .. } => None,
        },
        _ => None,
    }
}

/// Whether the item is not what the undo expects any more.
fn changed(fs: &dyn FsEngine, path: &Path, want: &Want) -> bool {
    let Ok(meta) = fs.lstat(path) else {
        return true;
    };
    match want {
        Want::Same(fp) => fp.diff(&meta.fingerprint()).is_some(),
        Want::Dir => meta.kind != FileKind::Dir,
        Want::There => false,
    }
}

/// What can be said of `entry` without the disk. `None` when there is nothing to say (it
/// cannot be undone at all: permanent, damaged, undone, empty) and when its files must be
/// looked at.
pub fn without_disk(entry: &JournalEntry) -> Option<Health> {
    if !(entry.is_undoable() || entry.can_retry_undo()) {
        return None;
    }
    let unsure = match entry.undo_limit {
        _ if entry.unrecorded_from.is_some() => Unsure::Gaps,
        Some(UndoLimit::Action) => Unsure::Action,
        Some(UndoLimit::Device) => Unsure::Device,
        None => return None,
    };
    Some(Health::NotGuaranteed(unsure))
}

/// Look at what the steps `remaining` of the undo `steps` expect. `None` if `cancelled` said
/// so before the end.
pub fn look(
    fs: &dyn FsEngine,
    steps: &[Step],
    remaining: &[usize],
    cancelled: &dyn Fn() -> bool,
) -> Option<Verdict> {
    let looks: Vec<(&Path, Want)> = remaining
        .iter()
        .filter_map(|i| steps.get(*i).and_then(look_of))
        .collect();
    if looks.len() > MAX_LOOKS {
        return Some(Verdict {
            health: Health::TooMany,
            folders: Vec::new(),
        });
    }
    let mut count = 0;
    for (n, (path, want)) in looks.iter().enumerate() {
        if n % 64 == 0 && cancelled() {
            return None;
        }
        count += usize::from(changed(fs, path, want));
    }
    let health = match count {
        0 => Health::Undoable,
        changed => Health::ToCheck { changed },
    };
    let folders: BTreeSet<&Path> = looks.iter().filter_map(|(p, _)| p.parent()).collect();
    Some(Verdict {
        health,
        folders: folders
            .into_iter()
            .map(|d| (d.into(), mtime(fs, d)))
            .collect(),
    })
}

fn mtime(fs: &dyn FsEngine, dir: &Path) -> Option<Stamp> {
    fs.lstat(dir).ok()?.mtime.map(Stamp::from)
}

/// Whether an opinion still holds: none of the folders it looked into changed since.
pub fn still_holds(fs: &dyn FsEngine, verdict: &Verdict) -> bool {
    verdict
        .folders
        .iter()
        .all(|(dir, then)| mtime(fs, dir) == *then)
}
