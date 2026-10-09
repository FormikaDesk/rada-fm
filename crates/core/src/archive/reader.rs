//! Reading the data of members, in the order the archive holds them.
//!
//! A stream (tar.gz, tar.xz, solid 7z) can only be read from its beginning. Asking for
//! member 40, then 3, then 41 would decompress the archive three times; so a [`Session`]
//! reads the archive **once, in order**, on a helper thread, and hands each wanted member to
//! the caller as it goes by. Members the caller skips are not stored: they are dropped as they
//! pass. Asking for a member that has already gone by starts the pass again (a retry).
//!
//! The helper thread is told to stop when the session is dropped; it notices at the next
//! chunk, so abandoning an extraction (cancel, quit) does not leave work running.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::Arc;

use super::entry::{EntryKind, sanitize};
use super::format::{Compression, Format};
use super::index::{Index, sevenz_err, tar_kind};
use super::{ArchiveError, external};

const CHUNK: usize = 64 * 1024;

enum Msg {
    Start(usize),
    Data(Vec<u8>),
    End,
    Failed(ArchiveError),
}

/// What the helper thread sends through.
pub(super) struct Emitter {
    tx: SyncSender<Msg>,
    stop: Arc<AtomicBool>,
}

impl Emitter {
    fn alive(&self) -> bool {
        !self.stop.load(Ordering::Relaxed)
    }

    fn send(&self, m: Msg) -> Result<(), ArchiveError> {
        if !self.alive() || self.tx.send(m).is_err() {
            return Err(ArchiveError::Cancelled);
        }
        Ok(())
    }

    pub(super) fn start(&self, idx: usize) -> Result<(), ArchiveError> {
        self.send(Msg::Start(idx))
    }

    pub(super) fn end(&self) -> Result<(), ArchiveError> {
        self.send(Msg::End)
    }

    /// Everything `r` holds, as chunks.
    pub(super) fn copy(&self, r: &mut dyn Read) -> Result<(), ArchiveError> {
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = loop {
                match r.read(&mut buf) {
                    Ok(n) => break n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e.into()),
                }
            };
            if n == 0 {
                return Ok(());
            }
            self.send(Msg::Data(buf[..n].to_vec()))?;
        }
    }
}

pub struct Session {
    index: Arc<Index>,
    wanted: Vec<bool>,
    rx: Receiver<Msg>,
    stop: Arc<AtomicBool>,
    /// The last member the helper announced.
    last_started: Option<usize>,
    failed: Option<String>,
    done: bool,
}

impl Session {
    /// A pass over the archive that delivers `wanted` (indexes of files, any order).
    pub fn open(index: Arc<Index>, wanted: &[usize]) -> Session {
        let mut flags = vec![false; index.entries.len()];
        for &w in wanted {
            if let Some(f) = flags.get_mut(w) {
                *f = true;
            }
        }
        Session::spawn(index, flags, 0)
    }

    fn spawn(index: Arc<Index>, wanted: Vec<bool>, from: usize) -> Session {
        let (tx, rx) = sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let em = Emitter {
            tx,
            stop: stop.clone(),
        };
        let mut flags = wanted.clone();
        for f in flags.iter_mut().take(from) {
            *f = false;
        }
        let ix = index.clone();
        let _ = std::thread::Builder::new()
            .name("rada-archive".into())
            .spawn(move || {
                if let Err(e) = produce(&ix, &flags, &em) {
                    // The receiver may be gone already; nobody is waiting for the news.
                    let _ = em.tx.send(Msg::Failed(e));
                }
            });
        Session {
            index,
            wanted,
            rx,
            stop,
            last_started: None,
            failed: None,
            done: false,
        }
    }

    /// Hand the data of member `idx` to `sink`. The sink returns `Ok(false)` to stop.
    /// Returns the number of bytes delivered.
    pub fn read(
        &mut self,
        idx: usize,
        sink: &mut dyn FnMut(&[u8]) -> io::Result<bool>,
    ) -> Result<u64, ArchiveError> {
        if let Some(why) = &self.failed {
            return Err(ArchiveError::Damaged(why.clone()));
        }
        // Already past it (a retry): read the archive again from the start.
        if self.last_started.is_some_and(|l| l >= idx) {
            self.stop.store(true, Ordering::Relaxed);
            let fresh = Session::spawn(self.index.clone(), self.wanted.clone(), idx);
            *self = fresh;
        }
        let mut delivering = false;
        let mut total = 0u64;
        loop {
            let msg = match self.rx.recv() {
                Ok(m) => m,
                Err(_) => {
                    self.done = true;
                    return Err(ArchiveError::Damaged(
                        "the archive ended before this item was reached".into(),
                    ));
                }
            };
            match msg {
                Msg::Start(i) => {
                    self.last_started = Some(i);
                    if i > idx {
                        return Err(ArchiveError::Damaged(
                            "this item is not where the archive's list says it is".into(),
                        ));
                    }
                    delivering = i == idx;
                }
                Msg::Data(d) => {
                    if delivering {
                        total += d.len() as u64;
                        if !sink(&d)? {
                            return Err(ArchiveError::Cancelled);
                        }
                    }
                }
                Msg::End => {
                    if delivering {
                        return Ok(total);
                    }
                }
                Msg::Failed(e) => {
                    if let ArchiveError::Cancelled = e {
                        return Err(e);
                    }
                    self.failed = Some(e.to_string());
                    return Err(e);
                }
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Up to `limit` bytes of member `idx`.
pub fn read_prefix(index: Arc<Index>, idx: usize, limit: usize) -> Result<Vec<u8>, ArchiveError> {
    let mut s = Session::open(index, &[idx]);
    let mut out = Vec::with_capacity(limit.min(1 << 20));
    let r = s.read(idx, &mut |d| {
        let room = limit - out.len();
        out.extend_from_slice(&d[..d.len().min(room)]);
        Ok(out.len() < limit)
    });
    match r {
        Ok(_) | Err(ArchiveError::Cancelled) => Ok(out),
        Err(e) => Err(e),
    }
}

// ----------------------------------------------------------------------------- helper thread

fn produce(ix: &Index, wanted: &[bool], em: &Emitter) -> Result<(), ArchiveError> {
    let last = match wanted.iter().rposition(|w| *w) {
        Some(l) => l,
        None => return Ok(()),
    };
    match ix.format {
        Format::Zip => produce_zip(ix, wanted, em),
        Format::Tar(c) => produce_tar(ix, c, wanted, last, em),
        Format::Single(c) => produce_single(ix, c, em),
        Format::SevenZ => produce_7z(ix, wanted, last, em),
        Format::Rar => external::produce_rar(ix, wanted, last, em),
    }
}

fn produce_zip(ix: &Index, wanted: &[bool], em: &Emitter) -> Result<(), ArchiveError> {
    let f = File::open(&ix.path)?;
    let mut za = zip::ZipArchive::new(BufReader::new(f)).map_err(zip_read_error)?;
    for (i, w) in wanted.iter().enumerate() {
        if !*w {
            continue;
        }
        let mut zf = za.by_index(i).map_err(zip_read_error)?;
        em.start(i)?;
        em.copy(&mut zf)?;
        em.end()?;
    }
    Ok(())
}

fn zip_read_error(e: zip::result::ZipError) -> ArchiveError {
    use zip::result::ZipError as Z;
    match e {
        Z::Io(e) => ArchiveError::from(e),
        Z::UnsupportedArchive(w) if w.to_lowercase().contains("password") => {
            ArchiveError::Encrypted
        }
        Z::UnsupportedArchive(w) => ArchiveError::Unsupported(w.to_string()),
        Z::InvalidPassword => ArchiveError::Encrypted,
        other => ArchiveError::Damaged(other.to_string()),
    }
}

fn produce_tar(
    ix: &Index,
    c: Compression,
    wanted: &[bool],
    last: usize,
    em: &Emitter,
) -> Result<(), ArchiveError> {
    let file = File::open(&ix.path)?;
    let reader = c.reader(BufReader::new(file))?;
    let mut ar = tar::Archive::new(reader);
    let mut n = 0usize;
    for r in ar.entries()? {
        let mut e = r?;
        if tar_kind(e.header().entry_type()).is_none() {
            continue;
        }
        if n > last {
            break;
        }
        if wanted[n] {
            // The list was made from this very file; a different member here means the file
            // changed in between.
            let (p, _) = sanitize(&e.path_bytes(), false);
            if ix.entries.get(n).is_none_or(|x| x.path != p) {
                return Err(ArchiveError::Damaged(
                    "the archive changed while it was being read".into(),
                ));
            }
            em.start(n)?;
            em.copy(&mut e)?;
            em.end()?;
        }
        n += 1;
    }
    Ok(())
}

fn produce_single(ix: &Index, c: Compression, em: &Emitter) -> Result<(), ArchiveError> {
    let file = File::open(&ix.path)?;
    let mut r = c.reader(BufReader::new(file))?;
    em.start(0)?;
    em.copy(&mut r)?;
    em.end()
}

fn produce_7z(ix: &Index, wanted: &[bool], last: usize, em: &Emitter) -> Result<(), ArchiveError> {
    let mut ar = sevenz_rust2::ArchiveReader::open(&ix.path, sevenz_rust2::Password::empty())
        .map_err(sevenz_err)?;
    let mut n = 0usize;
    let mut failure: Option<ArchiveError> = None;
    let r = ar.for_each_entries(|_, reader| {
        let i = n;
        n += 1;
        let go = (|| -> Result<(), ArchiveError> {
            if wanted.get(i).copied().unwrap_or(false) {
                em.start(i)?;
                em.copy(reader)?;
                em.end()?;
            } else {
                io::copy(reader, &mut io::sink())?;
            }
            Ok(())
        })();
        match go {
            Ok(()) => Ok(i < last),
            Err(e) => {
                failure = Some(e);
                Ok(false)
            }
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    r.map_err(sevenz_err)
}

/// Whether a kind carries data in the stream.
pub fn has_data(kind: EntryKind) -> bool {
    kind == EntryKind::File
}
