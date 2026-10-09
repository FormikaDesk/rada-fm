//! Developer aid for screenshots: a filesystem that copies slowly, so that the progress
//! bar can be photographed. Enabled only by `RADA_DEMO_SLOW_MBPS`.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rada_core::fs::{CopyControl, CopyOutcome, CopyRequest, DirItem, FsEngine, FsMeta, LocalFs};

pub struct SlowFs {
    inner: LocalFs,
    bytes_per_sec: f64,
}

pub fn slow_fs_from_env() -> Option<SlowFs> {
    let mbps: f64 = std::env::var("RADA_DEMO_SLOW_MBPS").ok()?.parse().ok()?;
    Some(SlowFs {
        inner: LocalFs,
        bytes_per_sec: mbps * 1_048_576.0,
    })
}

impl FsEngine for SlowFs {
    fn lstat(&self, p: &Path) -> io::Result<FsMeta> {
        self.inner.lstat(p)
    }
    fn stat(&self, p: &Path) -> io::Result<FsMeta> {
        self.inner.stat(p)
    }
    fn read_dir(&self, p: &Path) -> io::Result<Vec<DirItem>> {
        self.inner.read_dir(p)
    }
    fn read_link(&self, p: &Path) -> io::Result<PathBuf> {
        self.inner.read_link(p)
    }
    fn create_dir(&self, p: &Path, mode: Option<u32>) -> io::Result<()> {
        self.inner.create_dir(p, mode)
    }
    fn create_symlink(&self, t: &Path, l: &Path) -> io::Result<()> {
        self.inner.create_symlink(t, l)
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        self.inner.rename(a, b)
    }
    fn rename_noreplace(&self, a: &Path, b: &Path) -> io::Result<()> {
        self.inner.rename_noreplace(a, b)
    }
    fn remove_file(&self, p: &Path) -> io::Result<()> {
        self.inner.remove_file(p)
    }
    fn remove_dir(&self, p: &Path) -> io::Result<()> {
        self.inner.remove_dir(p)
    }
    fn hard_link(&self, existing: &Path, link: &Path) -> io::Result<()> {
        self.inner.hard_link(existing, link)
    }
    fn set_mode(&self, p: &Path, m: u32) -> io::Result<()> {
        self.inner.set_mode(p, m)
    }
    fn set_mtime(&self, p: &Path, m: SystemTime, a: Option<SystemTime>) -> io::Result<()> {
        self.inner.set_mtime(p, m, a)
    }
    fn copy_file(&self, req: &CopyRequest, ctl: &mut dyn CopyControl) -> io::Result<CopyOutcome> {
        let rate = self.bytes_per_sec;
        self.inner.copy_file(req, &mut |n: u64| {
            std::thread::sleep(Duration::from_secs_f64(n as f64 / rate));
            ctl.advance(n)
        })
    }
    fn available_space(&self, p: &Path) -> io::Result<u64> {
        self.inner.available_space(p)
    }
    fn can_read(&self, p: &Path) -> bool {
        self.inner.can_read(p)
    }
    fn can_write(&self, p: &Path) -> bool {
        self.inner.can_write(p)
    }
}
