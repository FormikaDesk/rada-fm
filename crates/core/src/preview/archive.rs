//! Previews of archives and of what is inside them.
//!
//! An archive that is not opened shows what it holds: how many files, how large packed and
//! unpacked, and the first things at its top. A member shows like a file would: text and
//! pictures are taken out of the archive for the preview only, with size limits, and a picture
//! goes to the preview cache because the image decoder works on files.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use super::{DirPreview, FileCard, Limits, Preview};
use crate::archive::index::{ListControl, open_cached};
use crate::archive::reader::{Session, read_prefix};
use crate::archive::{ArchiveError, EntryKind, Index, Location};
use crate::display;
use crate::ops::LinkState;

#[derive(Clone, Debug)]
pub struct ArchivePreview {
    pub format: String,
    pub packed: u64,
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    /// Uncompressed size of the files.
    pub bytes: u64,
    /// False when the whole archive could not be read in the time allowed: the counts are a
    /// lower bound.
    pub complete: bool,
    /// Why not (damage, time).
    pub note: Option<String>,
    /// The first things at the top: (escaped name, is a folder), folders first.
    pub first: Vec<(String, bool)>,
    pub more: bool,
    pub encrypted: u64,
    /// The archive could not be opened at all, in words.
    pub problem: Option<String>,
}

fn problem(format: String, packed: u64, why: String) -> Preview {
    Preview::Archive(ArchivePreview {
        format,
        packed,
        files: 0,
        dirs: 0,
        symlinks: 0,
        bytes: 0,
        complete: false,
        note: None,
        first: Vec::new(),
        more: false,
        encrypted: 0,
        problem: Some(why),
    })
}

fn top_listing(ix: &Index, dir: &Path, limit: usize) -> (Vec<(String, bool)>, bool) {
    let mut kids = ix.children(dir);
    kids.sort_by(|a, b| {
        (b.kind == EntryKind::Dir)
            .cmp(&(a.kind == EntryKind::Dir))
            .then_with(|| display::name(&a.name).to_lowercase().cmp(&display::name(&b.name).to_lowercase()))
    });
    let more = kids.len() > limit;
    (
        kids.into_iter()
            .take(limit)
            .map(|c| (display::name(&c.name), c.kind == EntryKind::Dir))
            .collect(),
        more,
    )
}

/// What the archive file at `path` holds.
pub fn summarize(path: &Path, packed: u64, limits: &Limits, cancel: &dyn Fn() -> bool) -> Preview {
    let deadline = Instant::now() + Duration::from_secs_f32(limits.archive_seconds);
    let mut lc = ListControl::new(cancel).with_deadline(deadline);
    let label = || crate::archive::format::sniff(path).ok().flatten().map(|f| f.label());
    match open_cached(path, &mut lc) {
        Ok(ix) => {
            let (first, more) = top_listing(&ix, Path::new(""), limits.archive_entries);
            Preview::Archive(ArchivePreview {
                format: ix.format.label(),
                packed,
                files: ix.files,
                dirs: ix.dirs,
                symlinks: ix.symlinks,
                bytes: ix.bytes,
                complete: ix.complete,
                note: ix.note.clone(),
                first,
                more,
                encrypted: ix.encrypted_members,
                problem: None,
            })
        }
        Err(ArchiveError::Cancelled) => Preview::Empty,
        Err(e) => problem(label().unwrap_or_else(|| "archive".into()), packed, e.to_string()),
    }
}

/// A cache file for one member: it changes when the archive or the member changes.
fn cache_name(dir: &Path, archive: &Path, ix: &Index, inner: &Path) -> PathBuf {
    let mut key: Vec<u8> = archive.as_os_str().as_encoded_bytes().to_vec();
    key.extend_from_slice(&ix.packed.to_le_bytes());
    if let Ok(m) = std::fs::metadata(archive).and_then(|m| m.modified())
        && let Ok(d) = m.duration_since(SystemTime::UNIX_EPOCH)
    {
        key.extend_from_slice(&d.as_secs().to_le_bytes());
        key.extend_from_slice(&d.subsec_nanos().to_le_bytes());
    }
    key.push(0);
    key.extend_from_slice(inner.as_os_str().as_encoded_bytes());
    key.extend_from_slice(b"arc-v1");
    dir.join(format!("arc-{:016x}", xxhash_rust::xxh3::xxh3_64(&key)))
}

/// Take a member out into the cache (once; the file stays for the next time).
fn extract_to_cache(
    ix: &std::sync::Arc<Index>,
    idx: usize,
    target: &Path,
    cancel: &dyn Fn() -> bool,
) -> Result<(), String> {
    if target.is_file() {
        if let Ok(f) = std::fs::File::options().append(true).open(target) {
            let _ = f.set_modified(SystemTime::now());
        }
        return Ok(());
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot use the preview cache: {e}"))?;
    }
    let part = target.with_extension("part");
    let mut f = std::fs::File::create(&part).map_err(|e| format!("cannot use the preview cache: {e}"))?;
    let mut s = Session::open(ix.clone(), &[idx]);
    let mut fail: Option<std::io::Error> = None;
    let r = s.read(idx, &mut |d| {
        if cancel() {
            return Ok(false);
        }
        match std::io::Write::write_all(&mut f, d) {
            Ok(()) => Ok(true),
            Err(e) => {
                fail = Some(e);
                Ok(false)
            }
        }
    });
    drop(f);
    let ok = fail.is_none() && r.is_ok();
    if ok {
        std::fs::rename(&part, target).map_err(|e| format!("cannot store the preview: {e}"))?;
        Ok(())
    } else {
        let _ = std::fs::remove_file(&part);
        Err(match (fail, r) {
            (Some(e), _) => e.to_string(),
            (None, Err(e)) => e.to_string(),
            _ => "stopped".to_string(),
        })
    }
}

/// What the member at `loc` looks like.
pub fn member(loc: &Location, limits: &Limits, cancel: &dyn Fn() -> bool) -> Preview {
    let mut lc = ListControl::new(cancel);
    let ix = match open_cached(&loc.archive, &mut lc) {
        Ok(ix) => ix,
        Err(ArchiveError::Cancelled) => return Preview::Empty,
        Err(e) => return Preview::Error(e.to_string()),
    };
    if ix.is_folder(&loc.inner) {
        let (entries, more) = top_listing(&ix, &loc.inner, limits.dir_entries);
        return Preview::Dir(DirPreview {
            entries,
            truncated: more,
        });
    }
    let Some(e) = ix.find(&loc.inner) else {
        return Preview::Error("not in the archive".into());
    };
    match e.kind {
        EntryKind::Symlink => Preview::Symlink {
            target: e.link.clone().unwrap_or_default(),
            state: LinkState::ToFile,
            inner: None,
        },
        EntryKind::Special => Preview::Special("device or pipe stored in the archive (never extracted)".into()),
        EntryKind::Hardlink => Preview::Special(format!(
            "another name for {}",
            e.link.as_ref().map(|l| display::path(l)).unwrap_or_default()
        )),
        EntryKind::Dir => Preview::Empty,
        EntryKind::File => {
            if e.encrypted {
                return Preview::Error("protected by a password".into());
            }
            if e.size == 0 {
                return Preview::Empty;
            }
            let head = match read_prefix(ix.clone(), e.index, limits.max_bytes) {
                Ok(h) => h,
                Err(ArchiveError::Cancelled) => return Preview::Empty,
                Err(err) => return Preview::Error(err.to_string()),
            };
            // A picture goes through the cache, because the decoder reads files.
            if super::image::is_drawable(&head) && e.size <= limits.image.max_file_bytes {
                let cache = limits
                    .image
                    .pdf_cache
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().join("rada-previews"));
                let target = cache_name(&cache, &loc.archive, &ix, &loc.inner);
                match extract_to_cache(&ix, e.index, &target, cancel) {
                    Ok(()) => {
                        if let Some(mut img) =
                            super::image::detect(&target, &head, e.size, e.mtime, &limits.image)
                        {
                            img.source = Some(target);
                            super::image::prune_if_due(&cache, &limits.image);
                            return Preview::Image(img);
                        }
                    }
                    Err(_) if cancel() => return Preview::Empty,
                    Err(why) => return Preview::Error(format!("cannot take it out of the archive: {why}")),
                }
            }
            let card_time = e.mtime;
            let mode = e.mode;
            super::bytes_preview(&head, e.size, limits, move |kind, exec| {
                FileCard {
                    kind,
                    size: e.size,
                    modified: card_time,
                    accessed: None,
                    created: None,
                    mode,
                    exec,
                }
            })
        }
    }
}
