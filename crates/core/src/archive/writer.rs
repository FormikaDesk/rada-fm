//! Creating an archive from a list of files that the plan already settled.
//!
//! The writer never decides anything: every member, its name in the archive, its size, mode,
//! time and (for a link) target arrive in the list. It only copies bytes, counts them for
//! progress, and stops the moment it is told to. The caller gives it a file that is not yet
//! the archive's final name.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ArchiveError;
use super::format::ArchiveKind;
use crate::fs::{FileKind, Stamp};
use crate::pathcodec;

/// One member of an archive to be created.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompressItem {
    /// Where it is on disk.
    #[serde(with = "pathcodec::path")]
    #[schemars(with = "String")]
    pub src: PathBuf,
    /// Its path inside the archive.
    #[serde(with = "pathcodec::path")]
    #[schemars(with = "String")]
    pub name: PathBuf,
    pub kind: FileKind,
    pub size: u64,
    pub mode: Option<u32>,
    pub mtime: Option<Stamp>,
    /// For a symlink: where it points.
    #[serde(with = "pathcodec::opt_path", default)]
    #[schemars(with = "Option<String>")]
    pub link: Option<PathBuf>,
}

/// Called with the bytes just read from a source; returns `false` to stop.
pub type Progress<'a> = &'a mut dyn FnMut(u64) -> bool;

/// Marks the I/O error that means "the user cancelled".
fn cancelled() -> io::Error {
    super::cancelled_io()
}

fn source_changed(path: &Path, why: &str) -> io::Error {
    io::Error::other(format!(
        "{} changed while it was being archived ({why})",
        crate::display::path(path)
    ))
}

/// Reads exactly `size` bytes of a source file, counting them.
struct Source<'a> {
    file: File,
    path: &'a Path,
    left: u64,
    progress: &'a mut dyn FnMut(u64) -> bool,
    /// Once stopped, every later read fails too.
    stopped: bool,
}

impl Read for Source<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.stopped {
            return Err(cancelled());
        }
        if self.left == 0 {
            // The file must end where it said it would.
            let mut probe = [0u8; 1];
            return match self.file.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(source_changed(self.path, "it grew")),
            };
        }
        let want = buf.len().min(self.left.min(usize::MAX as u64) as usize);
        let n = self.file.read(&mut buf[..want])?;
        if n == 0 {
            return Err(source_changed(self.path, "it shrank"));
        }
        self.left -= n as u64;
        if !(self.progress)(n as u64) {
            self.stopped = true;
            return Err(cancelled());
        }
        Ok(n)
    }
}

fn open_source(item: &CompressItem) -> io::Result<File> {
    let f = File::open(&item.src)?;
    // What is read must be what was planned.
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(source_changed(&item.src, "it is no longer a file"));
    }
    Ok(f)
}

/// Write `items` as an archive of `kind` into `out`.
pub fn write_archive<W: Write + io::Seek + Send>(
    kind: ArchiveKind,
    out: W,
    items: &[CompressItem],
    progress: Progress<'_>,
) -> Result<(), ArchiveError> {
    let out = BufWriter::with_capacity(256 * 1024, out);
    match kind {
        ArchiveKind::Zip => write_zip(out, items, progress),
        ArchiveKind::TarGz => {
            let enc = flate2::write::GzEncoder::new(out, flate2::Compression::default());
            let enc = write_tar(enc, items, progress)?;
            finish_io(enc.finish().and_then(|mut w| w.flush()))
        }
        ArchiveKind::TarXz => {
            let mut opts = lzma_rust2::XzOptions::with_preset(6);
            let block = (opts.lzma_options.dict_size as u64).max(8 << 20);
            opts.set_block_size(std::num::NonZeroU64::new(block));
            let workers = std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(2)
                .clamp(1, 8);
            let enc =
                lzma_rust2::XzWriterMt::new(out, opts, workers).map_err(ArchiveError::from)?;
            let enc = write_tar(enc, items, progress)?;
            finish_io(enc.finish().and_then(|mut w| w.flush()))
        }
        ArchiveKind::TarZst => {
            let enc = structured_zstd::encoding::StreamingEncoder::new(
                out,
                structured_zstd::encoding::CompressionLevel::Default,
            );
            let mut enc = write_tar(enc, items, progress)?;
            // Checksums let `zstd -t` and rada's own reader notice damage.
            let _ = &mut enc;
            let w = enc
                .finish()
                .map_err(|e| ArchiveError::Io(io::Error::other(e.to_string())))?;
            let mut w = w;
            finish_io(w.flush())
        }
    }
}

fn finish_io(r: io::Result<()>) -> Result<(), ArchiveError> {
    r.map_err(ArchiveError::from)
}

fn secs_of(t: Option<Stamp>) -> u64 {
    t.map(|s| s.secs.max(0) as u64).unwrap_or(0)
}

fn write_tar<W: Write>(
    w: W,
    items: &[CompressItem],
    progress: Progress<'_>,
) -> Result<W, ArchiveError> {
    let mut b = tar::Builder::new(w);
    b.mode(tar::HeaderMode::Complete);
    for it in items {
        let mut h = tar::Header::new_gnu();
        h.set_mtime(secs_of(it.mtime));
        match it.kind {
            FileKind::Dir => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(it.mode.unwrap_or(0o755) & 0o7777);
                h.set_size(0);
                let mut name = it.name.clone().into_os_string();
                name.push("/");
                b.append_data(&mut h, PathBuf::from(name), io::empty())
                    .map_err(ArchiveError::from)?;
            }
            FileKind::Symlink => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_mode(it.mode.unwrap_or(0o777) & 0o7777);
                h.set_size(0);
                let target = it.link.clone().unwrap_or_default();
                // Long names and targets use the GNU extension records.
                b.append_link(&mut h, &it.name, &target)
                    .map_err(ArchiveError::from)?;
            }
            FileKind::File => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(it.mode.unwrap_or(0o644) & 0o7777);
                h.set_size(it.size);
                let file = open_source(it).map_err(ArchiveError::from)?;
                let src = Source {
                    file,
                    path: &it.src,
                    left: it.size,
                    progress: &mut *progress,
                    stopped: false,
                };
                b.append_data(&mut h, &it.name, src)
                    .map_err(ArchiveError::from)?;
            }
            FileKind::Other => {}
        }
    }
    b.into_inner().map_err(ArchiveError::from)
}

/// A name for ZIP: always `/`, always UTF-8 (a name that is not valid UTF-8 cannot be kept
/// exactly; the plan said so).
pub fn zip_member_name(p: &Path) -> String {
    let parts: Vec<String> = p
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    parts.join("/")
}

fn dos_datetime(t: Option<Stamp>) -> zip::DateTime {
    let Some(s) = t else {
        return zip::DateTime::default();
    };
    let ts = SystemTime::from(s);
    let Ok(ts) = jiff::Timestamp::try_from(ts) else {
        return zip::DateTime::default();
    };
    let z = ts.to_zoned(jiff::tz::TimeZone::system());
    zip::DateTime::from_date_and_time(
        z.year().max(1980) as u16,
        z.month() as u8,
        z.day() as u8,
        z.hour() as u8,
        z.minute() as u8,
        z.second() as u8,
    )
    .unwrap_or_default()
}

fn write_zip<W: Write + io::Seek>(
    w: W,
    items: &[CompressItem],
    progress: Progress<'_>,
) -> Result<(), ArchiveError> {
    use zip::write::FullFileOptions;
    let mut z = zip::ZipWriter::new(w);
    for it in items {
        let name = zip_member_name(&it.name);
        let mut opts = FullFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(dos_datetime(it.mtime))
            .large_file(it.size >= 0xFFFF_FFFF);
        if let Some(m) = it.mode {
            opts = opts.unix_permissions(m & 0o7777);
        }
        if let Some(s) = it.mtime {
            // The exact time in UTC, which the DOS time above cannot say.
            let mut field = vec![1u8];
            field.extend_from_slice(
                &(s.secs.clamp(i32::MIN as i64, i32::MAX as i64) as i32).to_le_bytes(),
            );
            opts.add_extra_field(0x5455, field, false)
                .map_err(zip_write_error)?;
        }
        match it.kind {
            FileKind::Dir => z
                .add_directory(format!("{name}/"), opts)
                .map_err(zip_write_error)?,
            FileKind::Symlink => {
                let target = it.link.clone().unwrap_or_default();
                z.add_symlink(name, zip_member_name_raw(&target), opts)
                    .map_err(zip_write_error)?;
            }
            FileKind::File => {
                z.start_file(name, opts).map_err(zip_write_error)?;
                let file = open_source(it).map_err(ArchiveError::from)?;
                let mut src = Source {
                    file,
                    path: &it.src,
                    left: it.size,
                    progress: &mut *progress,
                    stopped: false,
                };
                io::copy(&mut src, &mut z).map_err(ArchiveError::from)?;
            }
            FileKind::Other => {}
        }
    }
    let mut w = z.finish().map_err(zip_write_error)?;
    w.flush().map_err(ArchiveError::from)
}

/// A link target as stored: the text, with its own separators.
fn zip_member_name_raw(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn zip_write_error(e: zip::result::ZipError) -> ArchiveError {
    match e {
        zip::result::ZipError::Io(e) => ArchiveError::from(e),
        other => ArchiveError::Io(io::Error::other(other.to_string())),
    }
}
