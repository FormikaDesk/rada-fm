//! Image previews: header inspection on the cheap, decoding and downscaling in a worker.
//!
//! Nothing here runs on the UI thread. The header is enough to show format, pixel size
//! and weight immediately, and to refuse a huge image with a clear message *before*
//! any pixel is decoded (the 12000x12000 PNG that froze other file managers).

use std::ffi::OsString;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use crossbeam_channel::{Receiver, Sender, unbounded};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use super::{Preview, pdf};
use crate::display;
use crate::events::{CoreEvent, PreviewEvent};

#[derive(Clone, Debug)]
pub struct ImageLimits {
    /// `false` when images are not drawn at all: nothing is decoded then.
    pub decode: bool,
    /// Images with more pixels than this are not decoded (megapixels).
    pub max_megapixels: u32,
    /// Files bigger than this are not decoded.
    pub max_file_bytes: u64,
    /// Decoded images are downscaled so that neither side exceeds this.
    pub max_edge: u32,
    /// PDFs bigger than this are not rendered.
    pub pdf_max_file_bytes: u64,
    /// Each poppler tool may run this long before it is stopped.
    pub pdf_timeout: Duration,
    /// An SVG that takes longer than this to read and draw is given up on: the file is small,
    /// the drawing can be endless (filters over a large canvas, patterns within patterns).
    pub svg_timeout: Duration,
    /// Where rendered first pages are kept; `None` is a folder under the system's temporary one.
    pub pdf_cache: Option<PathBuf>,
    /// The cache is pruned down to this size, and of anything older than `pdf_cache_max_age`.
    pub pdf_cache_max_bytes: u64,
    pub pdf_cache_max_age: Duration,
    /// Where to look for `pdftoppm`, `pdftotext` and `pdfinfo` (a `PATH`-style list); `None`
    /// is the process's own `PATH`. The system's own tools (`sips`, `qlmanage`) are looked for
    /// there too.
    pub pdf_tools_path: Option<OsString>,
    /// What the system draws documents with when poppler is not there: `sips` and Quick Look
    /// on a Mac, Windows' PDF renderer and the shell's thumbnails on Windows. `None` on
    /// Linux; the tests set it to try the logic of another system.
    pub system_renderer: Option<super::system::SystemRenderer>,
}

impl Default for ImageLimits {
    fn default() -> Self {
        ImageLimits {
            decode: true,
            max_megapixels: 50,
            max_file_bytes: 128 << 20,
            max_edge: 1600,
            pdf_max_file_bytes: 512 << 20,
            pdf_timeout: Duration::from_secs(8),
            svg_timeout: Duration::from_secs(8),
            pdf_cache: None,
            pdf_cache_max_bytes: 200 << 20,
            pdf_cache_max_age: Duration::from_secs(30 * 24 * 3600),
            pdf_tools_path: None,
            system_renderer: super::system::SystemRenderer::native(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImageInfo {
    pub format: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// For a PDF: page count, title and author, when known.
    pub doc: Option<pdf::DocFacts>,
    /// Drawn by the system's thumbnailer (a document, a video…), not decoded: `format` then
    /// says what kind of document it is.
    pub thumb: bool,
}

#[derive(Clone, Debug)]
pub enum ImageState {
    /// Decoding is in progress in the worker.
    Loading,
    /// Image rendering is switched off: only the header information is available.
    Disabled,
    /// Decoded and already downscaled to `ImageLimits::max_edge`.
    Ready(Arc<DynamicImage>),
    TooLarge(String),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct ImagePreview {
    pub info: ImageInfo,
    pub state: ImageState,
    /// Something worth telling the user even though the image is shown
    /// (for instance a truncated file).
    pub note: Option<String>,
    /// For a PDF that could not be drawn: the text of its first page instead.
    pub text: Vec<String>,
    /// The real file to decode when the thing shown is not itself a file (a picture inside an
    /// archive, taken out into the cache).
    pub source: Option<PathBuf>,
}

fn raster_format(f: ImageFormat) -> Option<&'static str> {
    Some(match f {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Tiff => "TIFF",
        _ => return None,
    })
}

fn looks_like_svg(head: &[u8]) -> bool {
    let head = &head[..head.len().min(4096)];
    if head.contains(&0) {
        return false;
    }
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    (t.starts_with("<svg")
        || t.starts_with("<?xml")
        || t.starts_with("<!--")
        || t.starts_with("<!DOCTYPE svg"))
        && text.contains("<svg")
}

/// Is this an image we can show? Looks at the content, never the extension.
/// Returns the cheap, header-only preview (state `Loading`, `TooLarge` or `Failed`).
pub(super) fn detect(
    path: &Path,
    head: &[u8],
    size: u64,
    modified: Option<SystemTime>,
    limits: &ImageLimits,
) -> Option<ImagePreview> {
    let mut info = ImageInfo {
        format: String::new(),
        width: None,
        height: None,
        size,
        modified,
        doc: None,
        thumb: false,
    };

    if looks_like_pdf(head) {
        return Some(detect_pdf(info, size, limits));
    }

    if looks_like_svg(head) {
        info.format = "SVG".into();
        let state = if size > limits.max_file_bytes.min(32 << 20) {
            ImageState::TooLarge(format!(
                "SVG file is {} (limit {})",
                display::bytes(size),
                display::bytes(limits.max_file_bytes.min(32 << 20))
            ))
        } else {
            ImageState::Loading
        };
        return Some(ImagePreview {
            info,
            state: disable_if_off(state, limits),
            note: None,
            text: Vec::new(),
            source: None,
        });
    }

    let fmt = image::guess_format(head).ok()?;
    info.format = raster_format(fmt)?.to_string();
    // Sizes come from the header only; no pixel is decoded here.
    let dims = header_dims(path, fmt);
    let state = match dims {
        Err(e) => ImageState::Failed(format!("cannot read the image header: {e}")),
        Ok((w, h)) => {
            info.width = Some(w);
            info.height = Some(h);
            let mp = (w as f64 * h as f64) / 1_000_000.0;
            if mp > limits.max_megapixels as f64 {
                ImageState::TooLarge(format!(
                    "{w}×{h} px is {mp:.0} megapixels; the preview limit is {} MP (image_max_megapixels in config.toml)",
                    limits.max_megapixels
                ))
            } else if size > limits.max_file_bytes {
                ImageState::TooLarge(format!(
                    "the file is {}; the preview limit is {} (image_max_file_mb in config.toml)",
                    display::bytes(size),
                    display::bytes(limits.max_file_bytes)
                ))
            } else {
                ImageState::Loading
            }
        }
    };
    Some(ImagePreview {
        info,
        state: disable_if_off(state, limits),
        note: None,
        text: Vec::new(),
        source: None,
    })
}

/// A PDF announces itself with `%PDF-` in its first kilobyte.
/// Keep the preview cache within its limits (it is also used for archive members).
pub(super) fn prune_if_due(dir: &Path, limits: &ImageLimits) {
    pdf::maybe_prune(dir, limits);
}

/// Whether the first bytes are a picture or a PDF, the things this module can draw.
pub(super) fn is_drawable(head: &[u8]) -> bool {
    looks_like_pdf(head)
        || looks_like_svg(head)
        || image::guess_format(head).is_ok_and(|f| raster_format(f).is_some())
}

fn looks_like_pdf(head: &[u8]) -> bool {
    head[..head.len().min(1024)]
        .windows(5)
        .any(|w| w == b"%PDF-")
}

/// The cheap answer for a document the system draws a thumbnail of: nothing is asked of the
/// system yet.
pub(super) fn detect_thumbnail(
    label: &str,
    size: u64,
    modified: Option<SystemTime>,
    limits: &ImageLimits,
) -> ImagePreview {
    ImagePreview {
        info: ImageInfo {
            format: label.to_string(),
            width: None,
            height: None,
            size,
            modified,
            doc: None,
            thumb: true,
        },
        state: disable_if_off(ImageState::Loading, limits),
        note: None,
        text: Vec::new(),
        source: None,
    }
}

/// The cheap answer for a PDF: refused when too big or when nothing can read it.
fn detect_pdf(mut info: ImageInfo, size: u64, limits: &ImageLimits) -> ImagePreview {
    info.format = "PDF".into();
    let tools = pdf::tools_for(limits);
    let (state, note) = if size > limits.pdf_max_file_bytes {
        (
            ImageState::TooLarge(format!(
                "the file is {}; the preview limit for PDFs is {} (pdf_max_file_mb in config.toml)",
                display::bytes(size),
                display::bytes(limits.pdf_max_file_bytes)
            )),
            None,
        )
    } else if !tools.any() && limits.system_renderer.is_none() {
        (
            ImageState::Failed("poppler is not installed".into()),
            Some(pdf::install_hint()),
        )
    } else {
        (ImageState::Loading, None)
    };
    ImagePreview {
        info,
        state: disable_if_off(state, limits),
        note,
        text: Vec::new(),
        source: None,
    }
}

/// With image rendering off nothing may be decoded: report the header only.
fn disable_if_off(state: ImageState, limits: &ImageLimits) -> ImageState {
    match state {
        ImageState::Loading if !limits.decode => ImageState::Disabled,
        other => other,
    }
}

fn header_dims(path: &Path, fmt: ImageFormat) -> Result<(u32, u32), String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let r = ImageReader::with_format(BufReader::new(f), fmt);
    r.into_dimensions().map_err(|e| e.to_string())
}

// --------------------------------------------------------------------------- decoding

/// Pictures with transparency (PNG logos, every SVG) are drawn over a discreet light
/// checkerboard, so that a black shape stays visible on a dark terminal.
fn on_checkerboard(img: DynamicImage) -> DynamicImage {
    if !img.color().has_alpha() {
        return img;
    }
    let mut rgba = img.into_rgba8();
    if rgba.pixels().all(|p| p.0[3] == 255) {
        return DynamicImage::ImageRgba8(rgba);
    }
    let tile = (rgba.width().max(rgba.height()) / 48).max(8);
    for (x, y, p) in rgba.enumerate_pixels_mut() {
        let a = p.0[3] as u32;
        if a == 255 {
            continue;
        }
        let back: u32 = if ((x / tile) + (y / tile)).is_multiple_of(2) {
            232
        } else {
            206
        };
        for c in &mut p.0[..3] {
            *c = ((*c as u32 * a + back * (255 - a)) / 255) as u8;
        }
        p.0[3] = 255;
    }
    DynamicImage::ImageRgba8(rgba)
}

fn downscale(img: DynamicImage, max_edge: u32) -> DynamicImage {
    if img.width().max(img.height()) <= max_edge {
        img
    } else {
        img.resize(max_edge, max_edge, FilterType::Triangle)
    }
}

fn load_raster(
    path: &Path,
    limits: &ImageLimits,
    info: &mut ImageInfo,
) -> Result<DynamicImage, String> {
    let bytes_limit = (limits.max_megapixels as u64 * 1_000_000 * 4).saturating_mul(3);
    let mut reader = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut lim = image::Limits::default();
    lim.max_alloc = Some(bytes_limit);
    reader.limits(lim);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    // Report the size as displayed (a rotated photo swaps its sides).
    info.width = Some(img.width());
    info.height = Some(img.height());
    Ok(downscale(img, limits.max_edge))
}

/// Decode and downscale. Runs in the image worker; `abandon` says when the user has moved on
/// (a PDF render is stopped at once then).
pub fn load(
    path: &Path,
    mut info: ImageInfo,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> ImagePreview {
    if info.thumb {
        return load_thumbnail(path, info, limits, abandon);
    }
    if info.format == "PDF" {
        return load_pdf(path, info, limits, abandon);
    }
    let result = if info.format == "SVG" {
        super::svg::load(path, limits, &mut info, abandon)
    } else {
        load_raster(path, limits, &mut info)
    };
    let state = match result {
        Ok(img) => ImageState::Ready(Arc::new(on_checkerboard(img))),
        Err(e) => ImageState::Failed(format!("cannot decode the image: {e}")),
    };
    // Lenient decoders happily draw half a JPEG; say so instead of pretending.
    let note =
        (matches!(state, ImageState::Ready(_)) && info.format == "JPEG" && !jpeg_is_complete(path))
            .then(|| "the file is truncated or damaged: showing what could be decoded".to_string());
    ImagePreview {
        info,
        state,
        note,
        text: Vec::new(),
        source: None,
    }
}

/// A document's thumbnail from the system (Quick Look, the shell).
fn load_thumbnail(
    path: &Path,
    info: ImageInfo,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> ImagePreview {
    let rendered = match limits.system_renderer {
        Some(r) => {
            let mut lim = limits.clone();
            lim.pdf_timeout = super::system::thumbnail_timeout(limits);
            super::system::render_thumbnail(r, path, &info, &lim, abandon)
        }
        None => Err(pdf::PdfProblem::NoTool("a thumbnailer")),
    };
    let state = match rendered {
        Ok(png) => {
            let mut sub = info.clone();
            sub.format = "PNG".into();
            match load_raster(&png, limits, &mut sub) {
                Ok(img) => ImageState::Ready(Arc::new(img)),
                Err(e) => {
                    let _ = std::fs::remove_file(&png);
                    ImageState::Failed(format!("cannot decode the thumbnail: {e}"))
                }
            }
        }
        Err(p) => ImageState::Failed(p.message()),
    };
    ImagePreview {
        info,
        state,
        note: None,
        text: Vec::new(),
        source: None,
    }
}

/// The first page of a PDF, with its facts; the page's text when it cannot be drawn. Poppler
/// when it is installed; otherwise what the system has.
fn load_pdf(
    path: &Path,
    mut info: ImageInfo,
    limits: &ImageLimits,
    abandon: &dyn Fn() -> bool,
) -> ImagePreview {
    let tools = pdf::tools_for(limits);
    let mut note = None;
    let mut text = Vec::new();
    // Facts first: they are quick, and say at once if the file needs a password. `pdfinfo`
    // when there is one; else read from the file itself.
    let facts = match pdf::facts(&tools, path, limits, abandon) {
        Err(pdf::PdfProblem::NoTool(_)) => pdf::facts_in_process(path),
        other => other,
    };
    let mut problem = None;
    match facts {
        Ok(f) => info.doc = Some(f),
        Err(pdf::PdfProblem::Password) => problem = Some(pdf::PdfProblem::Password),
        // Our own reader being puzzled is no reason to refuse a file the renderer may draw.
        Err(pdf::PdfProblem::Damaged(_)) if tools.pdftoppm.is_none() => {}
        Err(p) => problem = Some(p),
    }
    let rendered = match problem {
        Some(p) => Err(p),
        None if tools.pdftoppm.is_some() => {
            pdf::render_first_page(&tools, path, &info, limits, abandon)
        }
        None => match limits.system_renderer {
            Some(r) => super::system::render_pdf(r, path, &info, limits, abandon),
            None => Err(pdf::PdfProblem::NoTool("pdftoppm")),
        },
    };
    let state = match rendered {
        Ok(png) => {
            let mut sub = info.clone();
            sub.format = "PNG".into();
            match load_raster(&png, limits, &mut sub) {
                Ok(img) => ImageState::Ready(Arc::new(img)),
                Err(e) => {
                    // A cached picture that cannot be read: forget it.
                    let _ = std::fs::remove_file(&png);
                    ImageState::Failed(format!("cannot decode the page: {e}"))
                }
            }
        }
        Err(p) => {
            if matches!(p, pdf::PdfProblem::NoTool(_)) {
                note = Some(pdf::install_hint());
            }
            // Without the picture, the words are the next best thing.
            if !matches!(
                p,
                pdf::PdfProblem::Password
                    | pdf::PdfProblem::Abandoned
                    | pdf::PdfProblem::TooSlow(_)
            ) && let Ok(t) = pdf::first_page_text(&tools, path, limits, 16 * 1024, abandon)
            {
                text = t
                    .lines()
                    .map(|l| display::line(l.trim_end(), 4))
                    .take(200)
                    .collect();
                while text.last().is_some_and(String::is_empty) {
                    text.pop();
                }
            }
            ImageState::Failed(p.message())
        }
    };
    ImagePreview {
        info,
        state,
        note,
        text,
        source: None,
    }
}

/// A complete JPEG ends with the End-Of-Image marker (trailing padding is tolerated).
fn jpeg_is_complete(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return true;
    };
    let Ok(len) = f.seek(SeekFrom::End(0)) else {
        return true;
    };
    let from = len.saturating_sub(64);
    let mut tail = Vec::new();
    if f.seek(SeekFrom::Start(from)).is_err() || f.read_to_end(&mut tail).is_err() {
        return true;
    }
    tail.windows(2).any(|w| w == [0xff, 0xd9])
}

/// Decode a file already in memory (used by tests).
#[doc(hidden)]
pub fn dimensions_of(bytes: &[u8]) -> Option<(u32, u32)> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

// --------------------------------------------------------------------------- worker

struct Job {
    path: PathBuf,
    generation: u64,
    info: ImageInfo,
    limits: ImageLimits,
}

/// Decodes images on its own thread, newest request first, so that browsing a folder of
/// photos never queues up work for pictures that have already scrolled past.
#[derive(Clone)]
pub struct ImageWorker {
    tx: Sender<Job>,
    newest: Arc<AtomicU64>,
}

impl ImageWorker {
    pub fn spawn(out: Sender<CoreEvent>) -> ImageWorker {
        let (tx, rx): (Sender<Job>, Receiver<Job>) = unbounded();
        let newest = Arc::new(AtomicU64::new(0));
        let n = newest.clone();
        std::thread::Builder::new()
            .name("rada-image".into())
            .spawn(move || {
                while let Ok(mut job) = rx.recv() {
                    while let Ok(newer) = rx.try_recv() {
                        job = newer;
                    }
                    if job.generation < n.load(Ordering::Relaxed) {
                        continue; // the user has moved on
                    }
                    let gen_ = job.generation;
                    let preview = load(&job.path, job.info, &job.limits, &|| {
                        gen_ < n.load(Ordering::Relaxed)
                    });
                    let name = job
                        .path
                        .file_name()
                        .map(|s| s.to_os_string())
                        .unwrap_or_default();
                    let ev = PreviewEvent {
                        generation: job.generation,
                        path: job.path,
                        name,
                        preview: Preview::Image(preview),
                    };
                    if out.send(CoreEvent::Preview(ev)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn image worker");
        ImageWorker { tx, newest }
    }

    /// Note that a newer preview was requested: older pending decodes are dropped.
    pub fn newest(&self, generation: u64) {
        self.newest.fetch_max(generation, Ordering::Relaxed);
    }

    pub fn submit(&self, path: PathBuf, generation: u64, info: ImageInfo, limits: ImageLimits) {
        let _ = self.tx.send(Job {
            path,
            generation,
            info,
            limits,
        });
    }
}
