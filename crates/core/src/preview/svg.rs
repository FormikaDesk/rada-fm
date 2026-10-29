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
fn hand_over<T>(
    state: &AtomicU8,
    given_up: &AtomicUsize,
    tx: &crossbeam_channel::Sender<T>,
    done: T,
) {
    if state
        .compare_exchange(RUNNING, FINISHED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let _ = tx.send(done);
    } else {
        given_up.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Stop waiting. `false` when the drawing ended in the meantime: its answer is there.
fn give_up(state: &AtomicU8, given_up: &AtomicUsize) -> bool {
    let gone = state
        .compare_exchange(RUNNING, GIVEN_UP_ON, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    if gone {
        given_up.fetch_add(1, Ordering::AcqRel);
    }
    gone
}

/// Too many drawings that were given up on are still running for another to be started.
fn crowded(given_up: &AtomicUsize) -> bool {
    given_up.load(Ordering::Acquire) >= MAX_GIVEN_UP
}

/// Run `work` on a thread and wait for it up to `limit`, or until `abandon` says the answer is
/// no longer wanted. `late` is the error when it was given up on, and `given_up` counts the
/// drawings that are.
fn with_deadline<T: Send + 'static>(
    given_up: &'static AtomicUsize,
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
            hand_over(&theirs, given_up, &tx, done);
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
        if (Instant::now() >= end || abandon()) && give_up(&state, given_up) {
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
    if crowded(&GIVEN_UP) {
        return Err(too_complex(limits.svg_timeout));
    }
    // Vector art is rendered at a comfortable preview size, whatever its nominal size.
    let edge = limits.max_edge.min(1024) as f32;
    // Looking for the system's fonts is done once and is not the drawing's time.
    let fonts = fonts();
    draw(&GIVEN_UP, limits, info, abandon, move || {
        rasterize(&data, edge, fonts)
    })
}

/// Run `work`, the drawing, within the time the limits allow, and fill in the size it declares.
fn draw(
    given_up: &'static AtomicUsize,
    limits: &ImageLimits,
    info: &mut ImageInfo,
    abandon: &dyn Fn() -> bool,
    work: impl FnOnce() -> Result<((f32, f32), RgbaImage), String> + Send + 'static,
) -> Result<DynamicImage, String> {
    let late = too_complex(limits.svg_timeout);
    let ((sw, sh), image) = with_deadline(given_up, limits.svg_timeout, abandon, late, work)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::Sender;

    /// A drawing that goes on until the test lets it end, whatever the speed of the machine.
    fn endless<T: Send + 'static>(answer: T) -> (impl FnOnce() -> Result<T, String>, Sender<()>) {
        let (release, wait) = bounded::<()>(1);
        (
            move || {
                let _ = wait.recv();
                Ok(answer)
            },
            release,
        )
    }

    /// Wait until `count` is `to`: the thread of a drawing that ended has put it right.
    fn settles_at(count: &AtomicUsize, to: usize) {
        let end = Instant::now() + Duration::from_secs(60);
        while count.load(Ordering::Acquire) != to {
            assert!(
                Instant::now() < end,
                "stuck at {}",
                count.load(Ordering::Acquire)
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn quick() -> Duration {
        Duration::from_millis(20)
    }

    #[test]
    fn a_drawing_that_does_not_end_is_given_up_on_and_counted_until_it_ends() {
        static N: AtomicUsize = AtomicUsize::new(0);
        let (work, release) = endless(1u8);
        let r = with_deadline(&N, quick(), &|| false, "late".into(), work);
        assert_eq!(r, Err("late".into()));
        assert_eq!(N.load(Ordering::Acquire), 1);
        release.send(()).unwrap();
        settles_at(&N, 0);
    }

    #[test]
    fn a_drawing_that_ends_in_time_is_handed_over_and_never_counted() {
        static N: AtomicUsize = AtomicUsize::new(0);
        let r = with_deadline(
            &N,
            Duration::from_secs(60),
            &|| false,
            "late".into(),
            || Ok(7u8),
        );
        assert_eq!(r, Ok(7));
        assert_eq!(N.load(Ordering::Acquire), 0);
    }

    #[test]
    fn a_drawing_nobody_waits_for_any_more_is_given_up_before_the_limit() {
        static N: AtomicUsize = AtomicUsize::new(0);
        let (work, release) = endless(1u8);
        let r = with_deadline(&N, Duration::from_secs(600), &|| true, "late".into(), work);
        assert_eq!(r, Err("late".into()));
        release.send(()).unwrap();
        settles_at(&N, 0);
    }

    #[test]
    fn a_drawing_that_fails_or_panics_is_an_answer_and_not_counted() {
        static N: AtomicUsize = AtomicUsize::new(0);
        let wait = Duration::from_secs(60);
        let r: Result<u8, String> =
            with_deadline(
                &N,
                wait,
                &|| false,
                "late".into(),
                || Err("bad file".into()),
            );
        assert_eq!(r, Err("bad file".into()));
        let r: Result<u8, String> = with_deadline(&N, wait, &|| false, "late".into(), || {
            panic!("in the renderer")
        });
        assert_eq!(r, Err("the SVG could not be drawn".into()));
        assert_eq!(N.load(Ordering::Acquire), 0);
    }

    #[test]
    fn no_other_drawing_starts_while_enough_of_those_given_up_on_run() {
        let n = AtomicUsize::new(MAX_GIVEN_UP - 1);
        assert!(!crowded(&n));
        n.fetch_add(1, Ordering::AcqRel);
        assert!(crowded(&n));
    }

    fn info() -> ImageInfo {
        ImageInfo {
            format: "SVG".into(),
            width: None,
            height: None,
            size: 0,
            modified: None,
            doc: None,
            thumb: false,
        }
    }

    #[test]
    fn a_slow_svg_says_it_is_too_complex_and_the_next_one_is_drawn() {
        static N: AtomicUsize = AtomicUsize::new(0);
        let limits = ImageLimits {
            svg_timeout: quick(),
            ..Default::default()
        };
        let mut i = info();
        let (work, release) = endless(((10.0, 10.0), RgbaImage::new(1, 1)));
        let err = draw(&N, &limits, &mut i, &|| false, work).unwrap_err();
        assert!(err.contains("too complex"), "{err}");
        assert_eq!(
            i.width, None,
            "nothing is said of a file that was not drawn"
        );
        // The worker is free: a simple one right after is drawn, and its size is the declared one.
        let long = ImageLimits {
            svg_timeout: Duration::from_secs(60),
            ..Default::default()
        };
        let image = draw(&N, &long, &mut i, &|| false, || {
            Ok(((30.0, 20.0), RgbaImage::new(3, 2)))
        })
        .unwrap();
        assert_eq!((image.width(), image.height()), (3, 2));
        assert_eq!((i.width, i.height), (Some(30), Some(20)));
        release.send(()).unwrap();
        settles_at(&N, 0);
    }
}
