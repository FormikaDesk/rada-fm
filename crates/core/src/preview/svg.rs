//! SVG previews: reading, parsing and drawing, with a time limit.
//!
//! The renderer cannot be told to stop, and a small file can ask for hours of drawing (a blur
//! over a large canvas, repeated). The work is done on a thread of its own and waited for as
//! long as `svg_timeout`; past that the preview says the file is too complex and the worker
//! goes on. The thread cannot be stopped, so at most [`MAX_GIVEN_UP`] of them are let to
//! run on: while that many are still busy no other SVG is started.

use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{RecvTimeoutError, bounded};
use image::{DynamicImage, RgbaImage};

use super::image::{ImageInfo, ImageLimits};

/// The largest SVG that is drawn. The whole file is held in memory (and again as a tree): a
/// bigger one is said so, not drawn.
const MAX_SVG_BYTES: u64 = 20 << 20;

/// How many drawings that were given up on may be still running.
const MAX_GIVEN_UP: usize = 2;

static GIVEN_UP: AtomicUsize = AtomicUsize::new(0);

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

/// The content of an SVG file, if it is not bigger than [`MAX_SVG_BYTES`] (checked while
/// reading too, for a file that grows meanwhile).
fn read(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let too_big = || {
        format!(
            "the SVG is bigger than {} MB: not drawn",
            MAX_SVG_BYTES >> 20
        )
    };
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_SVG_BYTES {
        return Err(too_big());
    }
    let mut data = Vec::new();
    file.take(MAX_SVG_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_SVG_BYTES {
        return Err(too_big());
    }
    Ok(data)
}

/// An SVG drawn at `edge` pixels (its longer side), and the size it declares.
fn rasterize(
    data: &[u8],
    edge: f32,
    fontdb: Arc<resvg::usvg::fontdb::Database>,
) -> Result<((f32, f32), RgbaImage), String> {
    let opt = resvg::usvg::Options {
        fontdb,
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(data, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if !(sw.is_finite() && sh.is_finite()) || sw <= 0.0 || sh <= 0.0 {
        return Err("the SVG has no usable size".into());
    }
    let scale = edge / sw.max(sh);
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
    let image = RgbaImage::from_raw(w, h, raw).ok_or("bad SVG canvas")?;
    Ok(((sw, sh), image))
}

/// An SVG drawn square at `edge` pixels (its longer side), transparent where it has no
/// ink: for the icons rada makes from its own logo.
pub fn render_svg(data: &[u8], edge: u32) -> Result<RgbaImage, String> {
    rasterize(data, edge as f32, fonts()).map(|(_, image)| image)
}

const RUNNING: u8 = 0;
const FINISHED: u8 = 1;
const GIVEN_UP_ON: u8 = 2;

/// The drawing ended: its answer is handed over unless nobody is waiting any more, and then
/// it is no longer one of those that run on after being given up on.
fn hand_over<T>(state: &AtomicU8, tx: &crossbeam_channel::Sender<T>, done: T) {
    if state
        .compare_exchange(RUNNING, FINISHED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let _ = tx.send(done);
    } else {
        GIVEN_UP.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Stop waiting. `false` when the drawing ended in the meantime: its answer is there.
fn give_up(state: &AtomicU8) -> bool {
    let gone = state
        .compare_exchange(RUNNING, GIVEN_UP_ON, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if gone {
        GIVEN_UP.fetch_add(1, Ordering::AcqRel);
    }
    gone
}

/// Run `work` on a thread and wait for it up to `limit`, or until `abandon` says the answer is
/// no longer wanted. `late` is the error when it was given up on.
fn with_deadline<T: Send + 'static>(
    limit: Duration,
    abandon: &dyn Fn() -> bool,
    late: String,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = bounded(1);
    let state = Arc::new(AtomicU8::new(RUNNING));
    let theirs = state.clone();
    let started = std::thread::Builder::new()
        .name("rada-svg".into())
        .spawn(move || {
            // A drawing that panics is an answer too, or the count of those given up on is wrong.
            let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Err("the SVG could not be drawn".into()));
            hand_over(&theirs, &tx, done);
        });
    if let Err(e) = started {
        return Err(e.to_string());
    }
    let end = Instant::now() + limit;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(done) => return done,
            Err(RecvTimeoutError::Disconnected) => return Err("the SVG could not be drawn".into()),
            Err(RecvTimeoutError::Timeout) => {}
        }
        if (Instant::now() >= end || abandon()) && give_up(&state) {
            return Err(late);
        }
    }
}

/// Read and draw the SVG at `path` for the preview, filling in the size it declares.
pub(super) fn load(
    path: &Path,
    limits: &ImageLimits,
    info: &mut ImageInfo,
    abandon: &dyn Fn() -> bool,
) -> Result<DynamicImage, String> {
    let data = read(path)?;
    if GIVEN_UP.load(Ordering::Acquire) >= MAX_GIVEN_UP {
        return Err(too_complex(limits.svg_timeout));
    }
    // Vector art is rendered at a comfortable preview size, whatever its nominal size.
    let edge = limits.max_edge.min(1024) as f32;
    // Looking for the system's fonts is done once and is not the drawing's time.
    let fonts = fonts();
    let late = too_complex(limits.svg_timeout);
    let ((sw, sh), image) = with_deadline(limits.svg_timeout, abandon, late, move || {
        rasterize(&data, edge, fonts)
    })?;
    info.width = Some(sw.round().max(1.0) as u32);
    info.height = Some(sh.round().max(1.0) as u32);
    Ok(DynamicImage::ImageRgba8(image))
}

fn too_complex(limit: Duration) -> String {
    format!(
        "the SVG is too complex to draw (given up after {} s)",
        limit.as_secs().max(1)
    )
}
