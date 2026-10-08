//! Read-only walk of the source items. Never follows symlinks, so a symlink cycle
//! cannot make it loop; mount/bind cycles are caught by remembering the identity
//! (device, inode) of every ancestor directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::engine::Engine;
use crate::fs::{FileKind, FsMeta};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    ToFile,
    ToDir,
    Broken,
    Circular,
}

#[derive(Clone, Debug)]
pub struct ScanNode {
    pub path: PathBuf,
    pub name: OsString,
    pub meta: FsMeta,
    pub link_target: Option<PathBuf>,
    pub link_state: Option<LinkState>,
    pub children: Vec<ScanNode>,
    /// The directory could not be listed (message already contains the full path).
    pub unreadable: Option<String>,
    /// A directory that is its own ancestor (mount cycle): not descended into.
    pub loop_skipped: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub special: u64,
    pub bytes: u64,
}

impl ScanNode {
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        self.accumulate(&mut s);
        s
    }

    fn accumulate(&self, s: &mut Stats) {
        match self.meta.kind {
            FileKind::File => {
                s.files += 1;
                s.bytes += self.meta.size;
            }
            FileKind::Dir => {
                s.dirs += 1;
                for c in &self.children {
                    c.accumulate(s);
                }
            }
            FileKind::Symlink => s.symlinks += 1,
            FileKind::Other => s.special += 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub roots: Vec<ScanNode>,
    /// Sources that vanished before they could be read.
    pub missing: Vec<PathBuf>,
    /// Sources that could not even be inspected, with the reason.
    pub errors: Vec<(PathBuf, String)>,
}

impl Scan {
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        for r in &self.roots {
            r.accumulate(&mut s);
        }
        s
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub current: PathBuf,
}

pub struct ScanControl<'a> {
    pub cancel: &'a AtomicBool,
    pub progress: &'a mut dyn FnMut(&ScanProgress),
}

struct Walker<'a, 'b> {
    engine: &'a Engine,
    ctl: ScanControl<'b>,
    prog: ScanProgress,
    last_emit: Instant,
    cancelled: bool,
}

impl Engine {
    /// Walk `sources` (usually the user's selection).
    pub fn scan(&self, sources: &[PathBuf], ctl: ScanControl<'_>) -> Scan {
        let mut w = Walker {
            engine: self,
            ctl,
            prog: ScanProgress::default(),
            last_emit: Instant::now(),
            cancelled: false,
        };
        let mut scan = Scan::default();
        for src in sources {
            if w.cancelled {
                break;
            }
            match self.fs.lstat(src) {
                Ok(meta) => {
                    let name = src.file_name().map(|n| n.to_os_string()).unwrap_or_default();
                    let mut anc = Vec::new();
                    let node = w.node(src, name, meta, &mut anc);
                    scan.roots.push(node);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => scan.missing.push(src.clone()),
                Err(e) => scan
                    .errors
                    .push((src.clone(), crate::Error::io("read", src, e).to_string())),
            }
        }
        (w.ctl.progress)(&w.prog);
        scan
    }
}

impl Walker<'_, '_> {
    fn tick(&mut self, path: &Path) {
        if self.ctl.cancel.load(Ordering::Relaxed) {
            self.cancelled = true;
        }
        if self.last_emit.elapsed() >= Duration::from_millis(60) {
            self.prog.current = path.to_path_buf();
            (self.ctl.progress)(&self.prog);
            self.last_emit = Instant::now();
        }
    }

    fn node(&mut self, path: &Path, name: OsString, meta: FsMeta, ancestors: &mut Vec<(u64, u64)>) -> ScanNode {
        self.tick(path);
        let mut node = ScanNode {
            path: path.to_path_buf(),
            name,
            meta: meta.clone(),
            link_target: None,
            link_state: None,
            children: Vec::new(),
            unreadable: None,
            loop_skipped: false,
        };
        match meta.kind {
            FileKind::File => {
                self.prog.files += 1;
                self.prog.bytes += meta.size;
            }
            FileKind::Symlink => {
                self.prog.files += 1;
                node.link_target = self.engine.fs.read_link(path).ok();
                node.link_state = Some(match self.engine.fs.stat(path) {
                    Ok(m) if m.is_dir() => LinkState::ToDir,
                    Ok(_) => LinkState::ToFile,
                    #[cfg(unix)]
                    Err(e) if e.raw_os_error() == Some(libc::ELOOP) => LinkState::Circular,
                    Err(_) => LinkState::Broken,
                });
            }
            FileKind::Dir => {
                self.prog.dirs += 1;
                let id = meta.dev.zip(meta.ino);
                if let Some(id) = id {
                    if ancestors.contains(&id) {
                        node.loop_skipped = true;
                        return node;
                    }
                    ancestors.push(id);
                }
                match self.engine.fs.read_dir(path) {
                    Ok(mut items) => {
                        items.sort_by(|a, b| a.name.cmp(&b.name));
                        for item in items {
                            if self.cancelled {
                                break;
                            }
                            match item.meta {
                                Ok(m) => {
                                    let child = self.node(&item.path, item.name, m, ancestors);
                                    node.children.push(child);
                                }
                                Err(e) => {
                                    // Keep the entry visible instead of silently dropping it.
                                    node.unreadable = Some(
                                        crate::Error::io("read", &item.path, e).to_string(),
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        node.unreadable = Some(crate::Error::io("read folder", path, e).to_string());
                    }
                }
                if id.is_some() {
                    ancestors.pop();
                }
            }
            FileKind::Other => {}
        }
        node
    }
}
