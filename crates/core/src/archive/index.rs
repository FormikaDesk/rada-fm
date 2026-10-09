//! The list of an archive's members, and the folders they imply.
//!
//! Listing reads the whole archive's directory: instant for ZIP and 7z (the directory sits at
//! the end), a full pass for a compressed tar (there is no other way to know what is in a
//! stream). The result is cached in memory, keyed by the file's size and time, so moving around
//! inside an archive, previewing it and planning an extraction read it once.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use super::entry::{Entry, EntryKind, NameIssue, sanitize, zip_name_bytes};
use super::format::{Compression, Format, single_member_name, sniff};
use super::{ArchiveError, cancelled_io, external};

/// Whether to go on, and where to report.
pub struct ListControl<'a> {
    pub cancel: &'a dyn Fn() -> bool,
    /// A listing that must not take long (a preview) stops here and says so.
    pub deadline: Option<Instant>,
    pub max_entries: usize,
    pub progress: Option<&'a mut dyn FnMut(u64, u64)>,
}

impl<'a> ListControl<'a> {
    pub fn new(cancel: &'a dyn Fn() -> bool) -> Self {
        ListControl {
            cancel,
            deadline: None,
            max_entries: super::ArchiveLimits::default().max_entries,
            progress: None,
        }
    }

    pub fn with_deadline(mut self, d: Instant) -> Self {
        self.deadline = Some(d);
        self
    }

    pub fn with_max_entries(mut self, n: usize) -> Self {
        self.max_entries = n;
        self
    }

    pub fn with_progress(mut self, p: &'a mut dyn FnMut(u64, u64)) -> Self {
        self.progress = Some(p);
        self
    }

    /// `Err(Cancelled)` to stop for good; `Ok(false)` to stop and keep what was read.
    pub(super) fn tick(&mut self, entries: u64, bytes: u64) -> Result<bool, ArchiveError> {
        if (self.cancel)() {
            return Err(ArchiveError::Cancelled);
        }
        if let Some(p) = self.progress.as_mut() {
            p(entries, bytes);
        }
        if entries as usize >= self.max_entries {
            return Ok(false);
        }
        Ok(self.deadline.is_none_or(|d| Instant::now() < d))
    }
}

/// One thing in a folder of an archive.
#[derive(Clone, Debug)]
pub struct Child {
    pub name: OsString,
    pub kind: EntryKind,
    /// The member behind it, or `None` for a folder that only exists because a path runs
    /// through it.
    pub entry: Option<usize>,
    pub size: u64,
    pub mtime: Option<SystemTime>,
    pub mode: Option<u32>,
    pub link: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Index {
    pub path: PathBuf,
    pub format: Format,
    /// Size of the archive file.
    pub packed: u64,
    pub entries: Vec<Entry>,
    /// False when the listing stopped early (damage, time budget, too many members).
    pub complete: bool,
    /// Why it stopped, in words.
    pub note: Option<String>,
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    /// Uncompressed size of all files.
    pub bytes: u64,
    pub encrypted_members: u64,
    by_path: HashMap<PathBuf, usize>,
    tree: HashMap<PathBuf, BTreeMap<OsString, Child>>,
}

impl Index {
    pub(super) fn new(path: &Path, format: Format, packed: u64) -> Index {
        Index {
            path: path.to_path_buf(),
            format,
            packed,
            entries: Vec::new(),
            complete: true,
            note: None,
            files: 0,
            dirs: 0,
            symlinks: 0,
            bytes: 0,
            encrypted_members: 0,
            by_path: HashMap::new(),
            tree: HashMap::new(),
        }
    }

    /// Stopped early: keep what was read and say why.
    fn stop(&mut self, why: impl Into<String>) {
        self.complete = false;
        self.note = Some(why.into());
    }

    pub(super) fn finish(mut self) -> Index {
        (self.files, self.dirs, self.symlinks, self.bytes) = (0, 0, 0, 0);
        self.encrypted_members = 0;
        for e in &self.entries {
            match e.kind {
                EntryKind::File | EntryKind::Hardlink => {
                    self.files += 1;
                    self.bytes += e.size;
                }
                EntryKind::Dir => self.dirs += 1,
                EntryKind::Symlink => self.symlinks += 1,
                EntryKind::Special => {}
            }
            if e.encrypted {
                self.encrypted_members += 1;
            }
            if e.path.as_os_str().is_empty() {
                continue;
            }
            self.by_path.insert(e.path.clone(), e.index);
            // Every folder on the way exists, whether or not the archive lists it.
            let mut p = e.path.as_path();
            while let Some(parent) = p.parent() {
                let name = p.file_name().map(|n| n.to_os_string()).unwrap_or_default();
                let folder = self.tree.entry(parent.to_path_buf()).or_default();
                let is_leaf = std::ptr::eq(p, e.path.as_path());
                // A name that so far exists only because other paths run through it.
                let implied = folder.get(&name).is_some_and(|c| c.entry.is_none());
                if is_leaf {
                    if !(implied && e.kind != EntryKind::Dir) {
                        // The member itself (a later one with the same path replaces an
                        // earlier one).
                        folder.insert(
                            name.clone(),
                            Child {
                                name,
                                kind: e.kind,
                                entry: Some(e.index),
                                size: e.size,
                                mtime: e.mtime,
                                mode: e.mode,
                                link: e.link.clone(),
                            },
                        );
                    }
                } else {
                    folder.entry(name.clone()).or_insert_with(|| Child {
                        name,
                        kind: EntryKind::Dir,
                        entry: None,
                        size: 0,
                        mtime: None,
                        mode: None,
                        link: None,
                    });
                }
                p = parent;
                if parent.as_os_str().is_empty() {
                    break;
                }
            }
        }
        // A path used as a folder by other members is a folder, whatever its own entry says.
        let folders: Vec<PathBuf> = self.tree.keys().cloned().collect();
        for f in folders {
            if let (Some(parent), Some(name)) = (f.parent(), f.file_name())
                && let Some(c) = self.tree.get_mut(parent).and_then(|m| m.get_mut(name))
            {
                c.kind = EntryKind::Dir;
            }
        }
        self
    }

    /// The things directly inside `dir` (empty path = the archive's top), by name.
    pub fn children(&self, dir: &Path) -> Vec<&Child> {
        self.tree
            .get(dir)
            .map(|m| m.values().collect())
            .unwrap_or_default()
    }

    /// Whether `dir` is a folder of this archive (the top counts).
    pub fn is_folder(&self, dir: &Path) -> bool {
        dir.as_os_str().is_empty()
            || self.tree.contains_key(dir)
            // An empty folder has nothing under it, only its own entry.
            || self.find(dir).is_some_and(|e| e.kind == EntryKind::Dir)
    }

    /// The member at `path` (the last one, if the archive repeats a name).
    pub fn find(&self, path: &Path) -> Option<&Entry> {
        self.by_path.get(path).map(|&i| &self.entries[i])
    }

    /// The single folder at the top, when the archive has nothing else there.
    pub fn single_top_folder(&self) -> Option<OsString> {
        let top = self.tree.get(Path::new(""))?;
        if top.len() == 1 {
            let c = top.values().next()?;
            if c.kind == EntryKind::Dir {
                return Some(c.name.clone());
            }
        }
        None
    }

    /// All member indexes at or under `path`, in archive order (`path` empty = everything).
    pub fn under(&self, path: &Path) -> Vec<usize> {
        self.entries
            .iter()
            .filter(|e| e.path.starts_with(path))
            .map(|e| e.index)
            .collect()
    }
}

// ----------------------------------------------------------------------------- cache

type Key = (PathBuf, u64, Option<SystemTime>);

static CACHE: Mutex<Vec<(Key, Arc<Index>)>> = Mutex::new(Vec::new());
const CACHE_SLOTS: usize = 3;

fn key_of(path: &Path) -> Option<Key> {
    let m = std::fs::metadata(path).ok()?;
    Some((path.to_path_buf(), m.len(), m.modified().ok()))
}

/// The index of `path`, from the cache when the file has not changed.
pub fn open_cached(path: &Path, ctl: &mut ListControl<'_>) -> Result<Arc<Index>, ArchiveError> {
    let key = key_of(path);
    if let Some(k) = &key
        && let Ok(cache) = CACHE.lock()
        && let Some((_, ix)) = cache.iter().find(|(ck, _)| ck == k)
    {
        return Ok(ix.clone());
    }
    let ix = Arc::new(Index::open(path, ctl)?);
    if ix.complete
        && let (Some(k), Ok(mut cache)) = (key, CACHE.lock())
    {
        cache.retain(|(ck, _)| ck.0 != k.0);
        cache.push((k, ix.clone()));
        if cache.len() > CACHE_SLOTS {
            cache.remove(0);
        }
    }
    Ok(ix)
}

/// Forget cached indexes (tests, or after the file changed under us).
pub fn clear_cache() {
    if let Ok(mut c) = CACHE.lock() {
        c.clear();
    }
}

// ----------------------------------------------------------------------------- listing

impl Index {
    /// Read the member list of the archive at `path`. The format comes from the content.
    pub fn open(path: &Path, ctl: &mut ListControl<'_>) -> Result<Index, ArchiveError> {
        let meta = std::fs::metadata(path)?;
        if !meta.is_file() {
            return Err(ArchiveError::NotAnArchive);
        }
        let format = sniff(path)?.ok_or(ArchiveError::NotAnArchive)?;
        let mut ix = Index::new(path, format, meta.len());
        match format {
            Format::Zip => list_zip(&mut ix, ctl)?,
            Format::Tar(c) => list_tar(&mut ix, c, ctl)?,
            Format::Single(c) => list_single(&mut ix, c, ctl)?,
            Format::SevenZ => list_7z(&mut ix)?,
            Format::Rar => external::list_rar(&mut ix, ctl)?,
        }
        Ok(ix.finish())
    }

    pub(super) fn push(&mut self, mut e: Entry) {
        e.index = self.entries.len();
        self.entries.push(e);
    }

    pub(super) fn stop_with(&mut self, why: impl Into<String>) {
        self.stop(why);
    }
}

fn blank(kind: EntryKind, path: PathBuf, issue: Option<NameIssue>) -> Entry {
    Entry {
        index: 0,
        path,
        kind,
        size: 0,
        compressed: None,
        mode: None,
        mtime: None,
        link: None,
        encrypted: false,
        issue,
    }
}

const S_IFMT: u32 = 0o170000;
const S_IFLNK: u32 = 0o120000;
const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;

/// The kind a Unix mode says (`None` when the mode carries no type).
fn kind_of_mode(mode: u32) -> Option<EntryKind> {
    match mode & S_IFMT {
        S_IFLNK => Some(EntryKind::Symlink),
        S_IFDIR => Some(EntryKind::Dir),
        S_IFREG => Some(EntryKind::File),
        0 => None,
        _ => Some(EntryKind::Special),
    }
}

fn zip_error(e: zip::result::ZipError) -> ArchiveError {
    use zip::result::ZipError as Z;
    match e {
        Z::Io(e) => ArchiveError::from(e),
        Z::InvalidArchive(why) => ArchiveError::Damaged(why.to_string()),
        Z::UnsupportedArchive(why) if why.to_lowercase().contains("password") => {
            ArchiveError::Encrypted
        }
        Z::UnsupportedArchive(why) => ArchiveError::Unsupported(why.to_string()),
        other => ArchiveError::Damaged(other.to_string()),
    }
}

fn dos_time(dt: zip::DateTime) -> Option<SystemTime> {
    let civil = jiff::civil::DateTime::new(
        dt.year() as i16,
        dt.month() as i8,
        dt.day() as i8,
        dt.hour() as i8,
        dt.minute() as i8,
        dt.second() as i8,
        0,
    )
    .ok()?;
    let zoned = civil.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    Some(SystemTime::from(zoned.timestamp()))
}

pub(super) fn unix_time(secs: i64, nanos: u32) -> Option<SystemTime> {
    let t = SystemTime::UNIX_EPOCH;
    if secs >= 0 {
        t.checked_add(std::time::Duration::new(secs as u64, nanos))
    } else {
        t.checked_sub(std::time::Duration::from_secs(secs.unsigned_abs()))
    }
}

fn list_zip(ix: &mut Index, ctl: &mut ListControl<'_>) -> Result<(), ArchiveError> {
    let f = File::open(&ix.path)?;
    let mut za = zip::ZipArchive::new(BufReader::new(f)).map_err(zip_error)?;
    let mut links: Vec<usize> = Vec::new();
    let mut seen = 0u64;
    for i in 0..za.len() {
        if i % 512 == 0 && !ctl.tick(i as u64, seen)? {
            ix.stop(format!("stopped after {i} members"));
            break;
        }
        let zf = match za.by_index_raw(i) {
            Ok(z) => z,
            Err(e) => {
                ix.stop(zip_error(e).to_string());
                break;
            }
        };
        let name = zip_name_bytes(zf.name_raw(), false);
        let (path, issue) = sanitize(&name, true);
        let mode = zf.unix_mode();
        let kind = if zf.is_dir() {
            EntryKind::Dir
        } else {
            mode.and_then(kind_of_mode).unwrap_or(EntryKind::File)
        };
        let mut e = blank(kind, path, issue);
        e.size = if kind == EntryKind::File {
            zf.size()
        } else {
            0
        };
        e.compressed = Some(zf.compressed_size());
        e.mode = mode.map(|m| m & 0o7777);
        e.encrypted = zf.encrypted();
        e.mtime = zip_mtime(&zf);
        if kind == EntryKind::Symlink && !e.encrypted {
            links.push(i);
        }
        seen += e.size;
        ix.push(e);
    }
    // A symlink's target is its content.
    for i in links {
        if let Ok(mut zf) = za.by_index(i) {
            let mut target = Vec::new();
            if (&mut zf).take(4096).read_to_end(&mut target).is_ok() {
                ix.entries[i].link = Some(PathBuf::from(super::entry::bytes_to_os(&target)));
            }
        }
    }
    Ok(())
}

fn zip_mtime<R: Read>(zf: &zip::read::ZipFile<'_, R>) -> Option<SystemTime> {
    for field in zf.extra_data_fields() {
        if let zip::extra_fields::ExtraField::ExtendedTimestamp(ts) = field
            && let Some(t) = ts.mod_time()
        {
            return unix_time(t as i64, 0);
        }
    }
    zf.last_modified().and_then(dos_time)
}

/// A tar member type as an entry kind; `None` for the records that only carry metadata for
/// the next member.
pub(super) fn tar_kind(t: tar::EntryType) -> Option<EntryKind> {
    use tar::EntryType as T;
    Some(match t {
        T::Directory => EntryKind::Dir,
        T::Symlink => EntryKind::Symlink,
        T::Link => EntryKind::Hardlink,
        T::Char | T::Block | T::Fifo => EntryKind::Special,
        T::XGlobalHeader | T::XHeader | T::GNULongName | T::GNULongLink => return None,
        // Regular, contiguous, GNU sparse and unknown types: POSIX says to treat the
        // unknown ones as regular files.
        _ => EntryKind::File,
    })
}

fn list_tar(ix: &mut Index, c: Compression, ctl: &mut ListControl<'_>) -> Result<(), ArchiveError> {
    let file = File::open(&ix.path)?;
    let file_len = ix.packed;
    let mut count = 0u64;
    let mut seen = 0u64;
    // Plain tar can be skipped through without reading the data; compressed tar has to be
    // decoded.
    if c == Compression::None {
        let mut ar = tar::Archive::new(BufReader::new(file));
        let entries = ar.entries_with_seek().map_err(ArchiveError::from)?;
        for r in entries {
            match r {
                Ok(e) => {
                    let end = e.raw_file_position() + e.header().size().unwrap_or(0);
                    if end > file_len {
                        let name = String::from_utf8_lossy(&e.path_bytes()).into_owned();
                        ix.stop(format!("the archive ends in the middle of \"{name}\""));
                        add_tar_entry(ix, &e);
                        break;
                    }
                    seen += add_tar_entry(ix, &e);
                }
                Err(err) => {
                    ix.stop(err.to_string());
                    break;
                }
            }
            count += 1;
            if count.is_multiple_of(256) && !ctl.tick(count, seen)? {
                ix.stop(format!("stopped after {count} members"));
                break;
            }
        }
    } else {
        let (reader, expired) = guarded(c.reader(BufReader::new(file))?, ctl);
        let mut ar = tar::Archive::new(reader);
        let entries = ar.entries().map_err(ArchiveError::from)?;
        for r in entries {
            match r {
                Ok(e) => seen += add_tar_entry(ix, &e),
                Err(err) => {
                    let ae = ArchiveError::from(err);
                    if matches!(ae, ArchiveError::Cancelled) {
                        return Err(ae);
                    }
                    if !expired.load(Ordering::Relaxed) {
                        ix.stop(ae.to_string());
                    }
                    break;
                }
            }
            count += 1;
            if count.is_multiple_of(256) && !ctl.tick(count, seen)? {
                ix.stop(format!("stopped after {count} members"));
                break;
            }
        }
        if expired.load(Ordering::Relaxed) {
            ix.stop(format!("too large to list here: the first {count} members"));
        }
    }
    Ok(())
}

/// A reader that gives up when the listing is cancelled, so that a long decompression can be
/// stopped between two blocks.
struct Guarded<'a, 'b> {
    inner: Box<dyn Read + Send + 'a>,
    cancel: &'b dyn Fn() -> bool,
    deadline: Option<Instant>,
    expired: Arc<AtomicBool>,
    ticks: u32,
}

impl Read for Guarded<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks.is_multiple_of(64) {
            if (self.cancel)() {
                return Err(cancelled_io());
            }
            if self.deadline.is_some_and(|d| Instant::now() >= d) {
                // Time is up: end the stream here; the caller records why.
                self.expired.store(true, Ordering::Relaxed);
                return Ok(0);
            }
        }
        self.inner.read(buf)
    }
}

fn guarded<'a, 'b>(
    inner: Box<dyn Read + Send + 'a>,
    ctl: &ListControl<'b>,
) -> (Guarded<'a, 'b>, Arc<AtomicBool>) {
    let expired = Arc::new(AtomicBool::new(false));
    (
        Guarded {
            inner,
            cancel: ctl.cancel,
            deadline: ctl.deadline,
            expired: expired.clone(),
            ticks: 0,
        },
        expired,
    )
}

/// Adds the member; returns the bytes of data it holds.
fn add_tar_entry<R: Read>(ix: &mut Index, e: &tar::Entry<'_, R>) -> u64 {
    let Some(kind) = tar_kind(e.header().entry_type()) else {
        return 0;
    };
    let (path, issue) = sanitize(&e.path_bytes(), false);
    let mut en = blank(kind, path, issue);
    if matches!(kind, EntryKind::File) {
        en.size = e.size();
    }
    en.mode = e.header().mode().ok().map(|m| m & 0o7777);
    en.mtime = e.header().mtime().ok().and_then(|s| unix_time(s as i64, 0));
    if matches!(kind, EntryKind::Symlink | EntryKind::Hardlink)
        && let Some(l) = e.link_name_bytes()
    {
        en.link = Some(PathBuf::from(super::entry::bytes_to_os(&l)));
    }
    let size = en.size;
    ix.push(en);
    size
}

fn list_single(
    ix: &mut Index,
    c: Compression,
    ctl: &mut ListControl<'_>,
) -> Result<(), ArchiveError> {
    let name = ix
        .path
        .file_name()
        .map(|n| single_member_name(n, c))
        .unwrap_or_default();
    let (path, issue) = sanitize(&super::entry::os_to_bytes(&name), false);
    // How large it unpacks to is only known by unpacking it.
    let file = File::open(&ix.path)?;
    let (mut r, expired) = guarded(c.reader(BufReader::new(file))?, ctl);
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total += n as u64,
            Err(e) => {
                let ae = ArchiveError::from(e);
                if matches!(ae, ArchiveError::Cancelled) {
                    return Err(ae);
                }
                ix.stop(ae.to_string());
                break;
            }
        }
        if !ctl.tick(1, total)? {
            expired.store(true, Ordering::Relaxed);
            break;
        }
    }
    if expired.load(Ordering::Relaxed) {
        ix.stop("too large to measure here; its size is a lower bound");
    }
    let mut e = blank(EntryKind::File, path, issue);
    e.size = total;
    e.compressed = Some(ix.packed);
    e.mtime = std::fs::metadata(&ix.path)
        .ok()
        .and_then(|m| m.modified().ok());
    ix.push(e);
    Ok(())
}

fn nt_to_system(t: u64) -> Option<SystemTime> {
    const EPOCH_DIFF_SECS: u64 = 11_644_473_600;
    let secs = t / 10_000_000;
    let nanos = ((t % 10_000_000) * 100) as u32;
    let unix = secs.checked_sub(EPOCH_DIFF_SECS)?;
    unix_time(unix as i64, nanos)
}

fn sevenz_error(e: sevenz_rust2::Error) -> ArchiveError {
    use sevenz_rust2::Error as E;
    match e {
        E::PasswordRequired | E::MaybeBadPassword(_) => ArchiveError::Encrypted,
        E::Io(e, _) | E::FileOpen(e, _) => ArchiveError::from(e),
        E::Unsupported(w) => ArchiveError::Unsupported(w.to_string()),
        E::UnsupportedCompressionMethod(m) if m.contains("AES") => ArchiveError::Encrypted,
        E::UnsupportedCompressionMethod(m) => {
            ArchiveError::Unsupported(format!("compression method {m}"))
        }
        E::MaxMemLimited { max_kb, actaul_kb } => ArchiveError::Unsupported(format!(
            "it needs {} MiB of memory to unpack (the limit is {} MiB)",
            actaul_kb / 1024,
            max_kb / 1024
        )),
        other => ArchiveError::Damaged(other.to_string()),
    }
}

pub(super) fn sevenz_err(e: sevenz_rust2::Error) -> ArchiveError {
    sevenz_error(e)
}

fn list_7z(ix: &mut Index) -> Result<(), ArchiveError> {
    let ar = sevenz_rust2::Archive::open(&ix.path).map_err(sevenz_error)?;
    let order = sevenz_order(&ar);
    for &fi in &order {
        let f = &ar.files[fi];
        let attrs = f.windows_attributes();
        let unix_mode = (f.has_windows_attributes && attrs & 0x8000 != 0).then_some(attrs >> 16);
        let kind = if f.is_directory() || attrs & 0x10 != 0 {
            EntryKind::Dir
        } else {
            unix_mode.and_then(kind_of_mode).unwrap_or(EntryKind::File)
        };
        let (path, issue) = sanitize(f.name().as_bytes(), true);
        let mut e = blank(kind, path, issue);
        // Encrypted data shows only as an AES step in the chain that unpacks its block.
        e.encrypted = ar
            .stream_map
            .file_block_index
            .get(fi)
            .copied()
            .flatten()
            .and_then(|b| ar.blocks.get(b))
            .is_some_and(|b| {
                b.coders
                    .iter()
                    .any(|c| c.encoder_method_id() == [0x06, 0xF1, 0x07, 0x01])
            });
        e.size = if kind == EntryKind::File || kind == EntryKind::Symlink {
            f.size()
        } else {
            0
        };
        e.mode = unix_mode.map(|m| m & 0o7777);
        if f.has_last_modified_date {
            e.mtime = nt_to_system(u64::from(f.last_modified_date()));
        }
        ix.push(e);
    }
    Ok(())
}

/// File indexes in the order the reader delivers them: block by block, then the members that
/// have no data (folders, empty files).
pub(super) fn sevenz_order(ar: &sevenz_rust2::Archive) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..ar.files.len()).collect();
    idx.sort_by_key(|&i| {
        (
            ar.stream_map
                .file_block_index
                .get(i)
                .copied()
                .flatten()
                .unwrap_or(usize::MAX),
            i,
        )
    });
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn ix_with(paths: &[(&str, EntryKind)]) -> Index {
        let mut ix = Index::new(Path::new("/a.zip"), Format::Zip, 100);
        for (p, k) in paths {
            let (path, issue) = sanitize(p.as_bytes(), false);
            let mut e = blank(*k, path, issue);
            e.size = if *k == EntryKind::File { 10 } else { 0 };
            ix.push(e);
        }
        ix.finish()
    }

    fn names(ix: &Index, dir: &str) -> Vec<String> {
        ix.children(Path::new(dir))
            .iter()
            .map(|c| c.name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn folders_are_implied_by_the_paths_that_run_through_them() {
        let ix = ix_with(&[
            ("a/b/c.txt", EntryKind::File),
            ("a/d.txt", EntryKind::File),
            ("top.txt", EntryKind::File),
        ]);
        assert_eq!(names(&ix, ""), ["a", "top.txt"]);
        assert_eq!(names(&ix, "a"), ["b", "d.txt"]);
        assert_eq!(names(&ix, "a/b"), ["c.txt"]);
        assert!(ix.is_folder(Path::new("a/b")));
        assert!(!ix.is_folder(Path::new("a/b/c.txt")));
        assert_eq!((ix.files, ix.dirs, ix.bytes), (3, 0, 30));
    }

    #[test]
    fn an_explicit_folder_entry_is_the_same_folder() {
        let ix = ix_with(&[
            ("a", EntryKind::Dir),
            ("a/x", EntryKind::File),
            ("e", EntryKind::Dir),
        ]);
        assert_eq!(names(&ix, ""), ["a", "e"]);
        let a = ix
            .children(Path::new(""))
            .into_iter()
            .find(|c| c.name == "a");
        assert_eq!(a.unwrap().entry, Some(0));
        assert!(ix.is_folder(Path::new("e")));
    }

    #[test]
    fn a_single_top_folder_is_detected() {
        let ix = ix_with(&[("proj/a", EntryKind::File), ("proj/b/c", EntryKind::File)]);
        assert_eq!(ix.single_top_folder(), Some(OsString::from("proj")));
        let ix = ix_with(&[("proj/a", EntryKind::File), ("other", EntryKind::File)]);
        assert_eq!(ix.single_top_folder(), None);
        let ix = ix_with(&[("only-a-file", EntryKind::File)]);
        assert_eq!(ix.single_top_folder(), None);
    }

    #[test]
    fn a_path_used_as_a_folder_is_a_folder_even_if_listed_as_a_file() {
        let ix = ix_with(&[("a", EntryKind::File), ("a/b", EntryKind::File)]);
        let top = ix.children(Path::new(""));
        assert_eq!(top[0].kind, EntryKind::Dir);
    }
}
