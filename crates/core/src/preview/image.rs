//! Image previews: header inspection on the cheap, decoding and downscaling in a worker.
//!
//! Nothing here runs on the UI thread. The header is enough to show format, pixel size
//! and weight immediately, and to refuse a huge image with a clear message *before*
//! any pixel is decoded (the 12000x12000 PNG that froze other file managers).

use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use crossbeam_channel::{Receiver, Sender, unbounded};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};

use super::Preview;
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
}

impl Default for ImageLimits {
    fn default() -> Self {
        ImageLimits {
            decode: true,
            max_megapixels: 50,
            max_file_bytes: 128 << 20,
            max_edge: 1600,
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
    };

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
    })
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

static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();

fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    FONTS
        .get_or_init(|| {
            // Loading system fonts takes a moment: only ever done once, in the worker.
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

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

fn load_svg(
    path: &Path,
    limits: &ImageLimits,
    info: &mut ImageInfo,
) -> Result<DynamicImage, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let opt = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(&data, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if !(sw.is_finite() && sh.is_finite()) || sw <= 0.0 || sh <= 0.0 {
        return Err("the SVG has no usable size".into());
    }
    info.width = Some(sw.round().max(1.0) as u32);
    info.height = Some(sh.round().max(1.0) as u32);
    // Vector art is rendered at a comfortable preview size, whatever its nominal size.
    let target = limits.max_edge.min(1024) as f32;
    let scale = target / sw.max(sh);
    let (w, h) = (
        ((sw * scale).round() as u32).max(1),
        ((sh * scale).round() as u32).max(1),
    );
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("cannot allocate the SVG canvas")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut raw = Vec::with_capacity((w * h * 4) as usize);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        raw.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    RgbaImage::from_raw(w, h, raw)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "bad SVG canvas".into())
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

/// Decode and downscale. Runs in the image worker.
pub fn load(path: &Path, mut info: ImageInfo, limits: &ImageLimits) -> ImagePreview {
    let result = if info.format == "SVG" {
        load_svg(path, limits, &mut info)
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
    ImagePreview { info, state, note }
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
                    let preview = load(&job.path, job.info, &job.limits);
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
