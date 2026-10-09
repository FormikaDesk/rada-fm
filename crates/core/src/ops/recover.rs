//! Settling operations that were cut short: the process was killed, the machine lost power
//! or the terminal was closed with `kill -9` in the middle of a copy or a move.
//!
//! The journal says which steps completed (their `undo` records are written as the steps
//! finish) and which step was about to run (the `pending` record before it). That leaves one
//! step in doubt, the last one, and the disk answers it:
//!
//! * the step's temporary file (`.rada-part-<pid>-<n>`, a sibling of the destination that is
//!   renamed into place only when complete) is removed;
//! * a destination that is there but has no `undo` record is a completed step the journal
//!   missed: its undo is recorded, so that undoing the operation still brings everything
//!   back;
//! * a move that was cut between "copy in place" and "remove the source" has the file in two
//!   places: the copy is treated as not made (undo removes it), the source is untouched.
//!
//! Then the operation gets an `end` record (status `interrupted`) and is not looked at again.
//! Operations of a process that is still alive are left alone.

use std::path::{Path, PathBuf};

use super::engine::Engine;
use super::plan::Step;
use crate::Result;
use crate::fs::FsMeta;
use crate::journal::{Intent, Interrupted, Journal};

/// What settling one interrupted operation did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovered {
    pub id: String,
    pub title: String,
    /// Leftover temporary files that were removed.
    pub removed_temps: Vec<PathBuf>,
    /// Completed steps the journal had missed, now recorded (so undo covers them).
    pub adopted: Vec<PathBuf>,
    /// Things worth telling the user that nothing could be done about.
    pub notes: Vec<String>,
}

/// The process id inside a journal id (`<ms hex>-<pid hex>-<n>`).
fn pid_of(id: &str) -> Option<u32> {
    u32::from_str_radix(id.split('-').nth(1)?, 16).ok()
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists and may be signalled.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    // Without a way to ask, never touch what might still be running.
    true
}

/// Older than this, an interrupted operation is settled even if its process id is in use
/// again (the number was reused by something else).
const STALE_AFTER_SECS: i64 = 24 * 3600;

impl Engine {
    /// Settle every interrupted operation of `journal` (see the module documentation).
    pub fn recover_interrupted(&self, journal: &Journal) -> Result<Vec<Recovered>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut out = Vec::new();
        for it in journal.interrupted()? {
            let Some(pid) = pid_of(&it.entry.id) else {
                continue;
            };
            let fresh = now - it.entry.time < STALE_AFTER_SECS;
            if fresh && (pid == std::process::id() || alive(pid)) {
                continue; // still running (here or in another rada)
            }
            let r = self.settle_one(journal, &it, pid)?;
            out.push(r);
        }
        Ok(out)
    }

    fn settle_one(&self, journal: &Journal, it: &Interrupted, pid: u32) -> Result<Recovered> {
        let mut rec = Recovered {
            id: it.entry.id.clone(),
            title: it.entry.title.clone(),
            ..Default::default()
        };
        let mut adopted: Vec<Step> = Vec::new();
        if let Some(intent) = &it.in_doubt {
            self.remove_temps(intent, pid, &mut rec);
            if it.entry.reversible
                && let Some(step) = self.adopt(intent, &mut rec)
            {
                adopted.push(step);
            }
        }
        journal.settle(&it.entry, adopted)?;
        Ok(rec)
    }

    /// The temporary files of the step that was running: `.rada-part-<pid>-*` next to its
    /// destination.
    fn remove_temps(&self, intent: &Intent, pid: u32, rec: &mut Recovered) {
        let dst = match intent {
            Intent::Create { dst } | Intent::Move { dst, .. } => dst,
            Intent::Link { link, .. } => link,
            _ => return,
        };
        let Some(dir) = dst.parent() else { return };
        let prefix = format!(".rada-part-{pid}-");
        let Ok(items) = self.fs.read_dir(dir) else {
            return;
        };
        for item in items {
            if item.name.to_string_lossy().starts_with(&prefix) {
                match self.fs.remove_file(&item.path) {
                    Ok(()) => rec.removed_temps.push(item.path),
                    Err(e) => rec
                        .notes
                        .push(format!("cannot remove {}: {e}", item.path.display())),
                }
            }
        }
    }

    /// If the step in doubt turns out to have completed, the inverse the journal missed.
    fn adopt(&self, intent: &Intent, rec: &mut Recovered) -> Option<Step> {
        let fs = &self.fs;
        let exists = |p: &Path| fs.lstat(p).is_ok();
        let step = match intent {
            Intent::Create { dst } => {
                let meta = fs.lstat(dst).ok()?;
                Step::RemoveFile {
                    path: dst.clone(),
                    expect: Some(meta.fingerprint()),
                }
            }
            Intent::Move { src, dst, symlink } => {
                let meta = fs.lstat(dst).ok()?;
                if exists(src) {
                    // In two places: the copy counts as not made.
                    rec.notes.push(format!(
                        "{} was copied but not yet removed from its old place; both are kept, and undo removes the copy",
                        dst.display()
                    ));
                    Step::RemoveFile {
                        path: dst.clone(),
                        expect: Some(meta.fingerprint()),
                    }
                } else if *symlink {
                    Step::CopySymlink {
                        src: dst.clone(),
                        dst: src.clone(),
                        target: fs.read_link(dst).ok()?,
                        remove_source: true,
                    }
                } else {
                    back_copy(src, dst, &meta)
                }
            }
            Intent::Dir { path } => {
                let meta = fs.lstat(path).ok()?;
                if !meta.is_dir() {
                    return None;
                }
                Step::RemoveDir { path: path.clone() }
            }
            Intent::Rename { from, to } => {
                if exists(from) || !exists(to) {
                    return None;
                }
                Step::Rename {
                    from: to.clone(),
                    to: from.clone(),
                }
            }
            Intent::Link {
                existing,
                link,
                src_link,
                src_existing,
            } => {
                let meta = fs.lstat(link).ok()?;
                match (src_link, src_existing) {
                    (Some(s), Some(primary)) if !exists(s) => Step::HardLink {
                        existing: primary.clone(),
                        link: s.clone(),
                        src_link: Some(link.clone()),
                        src_existing: Some(existing.clone()),
                    },
                    _ => Step::RemoveFile {
                        path: link.clone(),
                        expect: Some(meta.fingerprint()),
                    },
                }
            }
        };
        rec.adopted.push(match &step {
            Step::RemoveFile { path, .. } | Step::RemoveDir { path } => path.clone(),
            Step::CopyFile { src, .. } | Step::CopySymlink { src, .. } => src.clone(),
            Step::Rename { from, .. } => from.clone(),
            Step::HardLink { link, .. } => link.clone(),
            _ => PathBuf::new(),
        });
        Some(step)
    }
}

/// The step that moves a completed cross-device move back, from what the copy looks like now.
fn back_copy(src: &Path, dst: &Path, meta: &FsMeta) -> Step {
    Step::CopyFile {
        src: dst.to_path_buf(),
        dst: src.to_path_buf(),
        size: meta.size,
        mode: meta.mode,
        mtime: meta.mtime.map(Into::into),
        atime: meta.atime.map(Into::into),
        verify: true,
        remove_source: true,
        uid: meta.uid,
        gid: meta.gid,
        stored: meta.is_sparse().then(|| meta.stored_bytes()),
        link_primary: false,
    }
}
