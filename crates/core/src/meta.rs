//! What a file says about itself: when and with what a photo was taken, who sings a song, the
//! title of a document, how long a video lasts.
//!
//! Every field is optional, and reading never fails: a file that is too big, of a kind that is
//! not known, damaged or slow to read gives empty fields, never an error. The reading is
//! bounded everywhere:
//!
//! * a photo is read from its first [`MetaLimits::prefix`] bytes only (that is where EXIF and
//!   XMP are), never as a whole;
//! * a song is read through its tags only, not its sound;
//! * a PDF has to be read whole to find its catalogue, so only up to
//!   [`MetaLimits::pdf_max_bytes`];
//! * a video is asked to `ffprobe`, run with the process runner and a timeout; without it the
//!   video fields stay empty;
//! * every file is given [`MetaLimits::timeout`]; what has not answered by then is empty.
//!
//! The place of a photo is the one written in the file (XMP or IPTC). Coordinates alone do not
//! give a place: there is no table of cities in rada.
//!
//! Text from the file goes through the same cleaning as every text that does not come from
//! rada ([`display::text`](crate::display::text)).

use std::fs::File;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::ops::Programs;

mod audio;
mod photo;
mod place;
mod video;
mod worker;

pub use video::Ffprobe;
pub use worker::{MetaCache, MetaEvent, MetaWorker};

/// What a file says about itself. Only the part that fits its kind is ever filled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub exif: Exif,
    pub audio: Audio,
    pub pdf: Pdf,
    pub video: Video,
}

impl Meta {
    /// Nothing was found.
    pub fn is_empty(&self) -> bool {
        *self == Meta::default()
    }
}

/// What a camera wrote in a photo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Exif {
    /// When the photo was taken, as the camera's clock said (no time zone).
    pub date: Option<jiff::civil::Date>,
    pub time: Option<jiff::civil::Time>,
    /// Maker and model, the maker left out when the model already names it.
    pub camera: Option<String>,
    /// The EXIF orientation, 1 to 8.
    pub orientation: Option<u16>,
    pub gps: Option<Gps>,
    /// The place written in the file (XMP or IPTC), never one worked out from `gps`.
    pub place: Option<String>,
}

/// A position in degrees, north and east positive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
}

/// The tags of a song.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub track: Option<u32>,
    pub year: Option<i32>,
}

/// What a PDF says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pdf {
    pub title: Option<String>,
    pub author: Option<String>,
    pub pages: Option<u32>,
}

/// What `ffprobe` says of a video.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Video {
    pub duration: Option<Duration>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The kinds of file something is read from, told by the name's extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaKind {
    Image,
    Audio,
    Pdf,
    Video,
}

impl MetaKind {
    pub fn of(path: &Path) -> Option<MetaKind> {
        const IMAGE: &[&str] = &[
            "jpg", "jpeg", "jpe", "jfif", "tif", "tiff", "png", "webp", "heic", "heif", "avif",
            "dng", "cr2", "nef", "arw", "orf", "rw2", "pef", "srw",
        ];
        const AUDIO: &[&str] = &[
            "mp3", "flac", "ogg", "oga", "opus", "spx", "m4a", "m4b", "aac", "wav", "aif", "aiff",
            "wv", "ape", "mpc",
        ];
        const VIDEO: &[&str] = &[
            "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts",
            "mts", "3gp", "ogv",
        ];
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        let ext = ext.as_str();
        if IMAGE.contains(&ext) {
            Some(MetaKind::Image)
        } else if AUDIO.contains(&ext) {
            Some(MetaKind::Audio)
        } else if VIDEO.contains(&ext) {
            Some(MetaKind::Video)
        } else {
            (ext == "pdf").then_some(MetaKind::Pdf)
        }
    }
}

/// How far reading may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetaLimits {
    /// A file bigger than this is not read at all.
    pub max_bytes: u64,
    /// A PDF is read whole: a bigger one is not read.
    pub pdf_max_bytes: u64,
    /// The bytes of a photo that are looked at, from its start.
    pub prefix: usize,
    /// How long one file may take.
    pub timeout: Duration,
}

impl Default for MetaLimits {
    fn default() -> Self {
        MetaLimits {
            max_bytes: 16 << 30,
            pdf_max_bytes: 64 << 20,
            prefix: 1 << 20,
            timeout: Duration::from_secs(5),
        }
    }
}

/// Where `ffprobe` is found, if it is to be used at all.
#[derive(Clone, Debug, Default)]
enum Prober {
    /// Videos are not looked into.
    #[default]
    Off,
    /// Looked up when a video is read, as the actions look up their programs.
    Lookup(Programs),
    Fixed(Ffprobe),
}

/// The programs reading may use: only `ffprobe`, for videos.
#[derive(Clone, Debug, Default)]
pub struct MetaTools {
    prober: Prober,
}

impl MetaTools {
    /// No program: the video fields stay empty.
    pub fn none() -> MetaTools {
        MetaTools::default()
    }

    /// `ffprobe` from the configuration or the `PATH`, looked for when a video is read.
    pub fn with_programs(programs: Programs) -> MetaTools {
        MetaTools {
            prober: Prober::Lookup(programs),
        }
    }

    /// This `ffprobe`.
    pub fn with_ffprobe(ffprobe: Ffprobe) -> MetaTools {
        MetaTools {
            prober: Prober::Fixed(ffprobe),
        }
    }

    fn ffprobe(&self) -> Option<Ffprobe> {
        match &self.prober {
            Prober::Off => None,
            Prober::Lookup(programs) => programs.resolve("ffprobe").ok().map(Ffprobe::at),
            Prober::Fixed(f) => Some(f.clone()),
        }
    }
}

/// Read what `path` says about itself. Never fails and never takes much longer than
/// `limits.timeout`; `cancel` is asked while waiting, and a yes gives empty fields.
pub fn read(
    path: &Path,
    tools: &MetaTools,
    limits: &MetaLimits,
    cancel: &dyn Fn() -> bool,
) -> Meta {
    let Some(kind) = MetaKind::of(path) else {
        return Meta::default();
    };
    let Some(len) = regular_len(path) else {
        return Meta::default();
    };
    let max = if kind == MetaKind::Pdf {
        limits.pdf_max_bytes.min(limits.max_bytes)
    } else {
        limits.max_bytes
    };
    if len == 0 || len > max {
        return Meta::default();
    }
    if kind == MetaKind::Video {
        return match tools.ffprobe() {
            Some(f) => video::read(&f, path, limits.timeout, cancel),
            None => Meta::default(),
        };
    }
    let owned = path.to_path_buf();
    let limits = *limits;
    in_thread(limits.timeout, cancel, move || {
        in_process(kind, &owned, &limits)
    })
}

/// The size of `path` when it is a regular file (a link is followed): never a device, a pipe
/// or a folder, which could block or never end.
fn regular_len(path: &Path) -> Option<u64> {
    let m = std::fs::metadata(path).ok()?;
    m.is_file().then_some(m.len())
}

/// The readers that run inside rada.
fn in_process(kind: MetaKind, path: &Path, limits: &MetaLimits) -> Meta {
    let Ok(file) = open_regular(path) else {
        return Meta::default();
    };
    match kind {
        MetaKind::Image => Meta {
            exif: photo::read(file, limits.prefix),
            ..Meta::default()
        },
        MetaKind::Audio => Meta {
            audio: audio::read(file),
            ..Meta::default()
        },
        MetaKind::Pdf => Meta {
            pdf: pdf(file, limits.pdf_max_bytes),
            ..Meta::default()
        },
        MetaKind::Video => Meta::default(),
    }
}

/// Open `path` for reading, refusing anything but a regular file. The check is made on what
/// was opened: a file swapped for a pipe after it was looked at cannot block the reader.
fn open_regular(path: &Path) -> io::Result<File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Opening a pipe would wait for a writer; with this it returns at once.
        opts.custom_flags(libc::O_NONBLOCK);
    }
    let file = opts.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// Title, author and pages of a PDF: lopdf reads the file whole, at most `max` bytes of it.
fn pdf(file: File, max: u64) -> Pdf {
    use std::io::Read;
    let Ok(m) = lopdf::Document::load_metadata_from(file.take(max)) else {
        return Pdf::default();
    };
    Pdf {
        title: m.title.as_deref().and_then(tidy),
        author: m.author.as_deref().and_then(tidy),
        pages: Some(m.page_count).filter(|n| *n > 0),
    }
}

/// Run `read` on a thread of its own and wait for it at most `timeout`. The readers inside
/// rada cannot be interrupted; one that is late is left to finish, its answer dropped, and the
/// file has empty fields.
fn in_thread<F>(timeout: Duration, cancel: &dyn Fn() -> bool, read: F) -> Meta
where
    F: FnOnce() -> Meta + Send + 'static,
{
    let (tx, rx) = crossbeam_channel::bounded(1);
    let spawned = std::thread::Builder::new()
        .name("rada-meta-read".into())
        .spawn(move || {
            let _ = tx.send(read());
        });
    if spawned.is_err() {
        return Meta::default();
    }
    let deadline = Instant::now() + timeout;
    loop {
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(meta) => return meta,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Meta::default(),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if cancel() || Instant::now() >= deadline {
                    return Meta::default();
                }
            }
        }
    }
}

/// Most characters kept of one field.
const MAX_FIELD: usize = 256;

/// A field as it may be shown: cleaned like every outside text, trimmed, cut to
/// [`MAX_FIELD`] characters; `None` when nothing is left.
pub(crate) fn tidy(raw: &str) -> Option<String> {
    let cut: String = raw.trim().chars().take(MAX_FIELD).collect();
    let clean = crate::display::text(cut.trim());
    (!clean.is_empty()).then_some(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_told_by_the_extension_whatever_its_case() {
        assert_eq!(MetaKind::of(Path::new("/a/b.JPG")), Some(MetaKind::Image));
        assert_eq!(MetaKind::of(Path::new("/a/b.flac")), Some(MetaKind::Audio));
        assert_eq!(MetaKind::of(Path::new("/a/b.Pdf")), Some(MetaKind::Pdf));
        assert_eq!(MetaKind::of(Path::new("/a/b.mkv")), Some(MetaKind::Video));
        assert_eq!(MetaKind::of(Path::new("/a/b.txt")), None);
        assert_eq!(MetaKind::of(Path::new("/a/jpg")), None);
    }

    #[test]
    fn a_field_is_cleaned_trimmed_and_cut() {
        assert_eq!(tidy("  \u{1b}[31mred  ").as_deref(), Some("\\e[31mred"));
        assert_eq!(tidy("   "), None);
        assert_eq!(tidy(&"x".repeat(1000)).unwrap().chars().count(), MAX_FIELD);
    }

    #[test]
    fn a_reader_that_does_not_answer_in_time_gives_empty_fields() {
        let started = Instant::now();
        let meta = in_thread(Duration::from_millis(100), &|| false, || {
            std::thread::sleep(Duration::from_secs(3));
            Meta {
                pdf: Pdf {
                    pages: Some(1),
                    ..Pdf::default()
                },
                ..Meta::default()
            }
        });
        assert!(meta.is_empty());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_reader_is_given_up_when_the_caller_cancels() {
        let started = Instant::now();
        let meta = in_thread(Duration::from_secs(30), &|| true, || {
            std::thread::sleep(Duration::from_secs(3));
            Meta::default()
        });
        assert!(meta.is_empty());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
