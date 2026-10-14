//! Terminal graphics: protocol detection (Kitty, Sixel, iTerm2, halfblocks) and the
//! resize/encode work, which always runs on a thread of its own. Which terminal draws what
//! is decided in [`caps`](crate::caps); here the choice is turned into a working encoder.

use std::sync::mpsc;
use std::time::Duration;

use crossbeam_channel::Receiver;
use image::DynamicImage;
use ratatui_image::FontSize;
use ratatui_image::errors::Errors;
use ratatui_image::picker::cap_parser::QueryStdioOptions;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::thread::{ResizeRequest, ResizeResponse, ThreadProtocol};

pub type ResizeResult = Result<ResizeResponse, Errors>;

/// How images are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageMode {
    /// Ask the terminal what it supports (Kitty, Sixel, iTerm2), fall back to half blocks.
    Auto,
    /// Always coloured half blocks; works everywhere.
    Halfblocks,
    /// Force one protocol (for terminals that cannot be detected, or tmux with passthrough).
    Kitty,
    Sixel,
    Iterm2,
    /// No image rendering: only the information card.
    Off,
}

impl ImageMode {
    pub fn parse(s: &str) -> Option<ImageMode> {
        match s.to_ascii_lowercase().as_str() {
            "auto" | "on" => Some(ImageMode::Auto),
            "halfblocks" | "half-blocks" | "blocks" => Some(ImageMode::Halfblocks),
            "kitty" => Some(ImageMode::Kitty),
            "sixel" => Some(ImageMode::Sixel),
            "iterm2" | "iterm" => Some(ImageMode::Iterm2),
            "off" | "none" | "no" => Some(ImageMode::Off),
            _ => None,
        }
    }
}

pub struct ImageUi {
    picker: Picker,
    pub proto: ThreadProtocol,
    results: Receiver<ResizeResult>,
}

impl ImageUi {
    /// Build from a picker; starts the resize/encode thread.
    pub fn new(picker: Picker) -> ImageUi {
        let (req_tx, req_rx) = mpsc::channel::<ResizeRequest>();
        let (res_tx, res_rx) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("rada-image-encode".into())
            .spawn(move || {
                while let Ok(mut req) = req_rx.recv() {
                    // Scrolling through photos queues requests for pictures already gone.
                    while let Ok(newer) = req_rx.try_recv() {
                        req = newer;
                    }
                    if res_tx.send(req.resize_encode()).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn image encoder thread");
        ImageUi {
            proto: ThreadProtocol::new(req_tx, None),
            picker,
            results: res_rx,
        }
    }

    /// Choose how to draw images, without ever leaving the keyboard reader in doubt.
    /// Also returns the keys the user typed while the terminal was being asked.
    ///
    /// What the environment says comes first ([`TermCaps`](crate::caps::TermCaps)):
    ///
    /// 1. Under tmux, and on terminals known to have no graphics protocol (GNOME Terminal,
    ///    Terminal.app): half blocks, and nothing is asked.
    /// 2. Terminals that announce themselves (Windows Terminal, Konsole, Ghostty, kitty,
    ///    WezTerm, iTerm2, foot): the protocol follows from the environment and the cell
    ///    size from the window size; only where the system does not know the window's size
    ///    in pixels (Windows) is the terminal asked for it.
    /// 3. Anything else is asked, with a bounded wait.
    pub fn detect(caps: &crate::caps::TermCaps) -> (ImageUi, Vec<u8>) {
        use crate::caps::ImagePlan;
        match caps.images {
            ImagePlan::Off | ImagePlan::Halfblocks => (ImageUi::halfblocks(), Vec::new()),
            ImagePlan::Protocol(proto) => ImageUi::for_protocol(proto, caps.may_query),
            ImagePlan::Ask if !caps.may_query => (ImageUi::halfblocks(), Vec::new()),
            // The library's own query reads standard input, which on the Windows console
            // does not deliver the terminal's answers.
            ImagePlan::Ask if cfg!(windows) => ImageUi::asked(None),
            ImagePlan::Ask => {
                let opts = QueryStdioOptions {
                    timeout: Duration::from_secs(1),
                    ..Default::default()
                };
                match Picker::from_query_stdio_with_options(opts) {
                    Ok(p) => (ImageUi::new(p), Vec::new()),
                    Err(e) => {
                        tracing::warn!(
                            "terminal graphics detection failed: {e}; using half blocks"
                        );
                        (ImageUi::halfblocks(), Vec::new())
                    }
                }
            }
        }
    }

    /// A given protocol, with the cell size taken from the window's pixel size.
    /// `None` when the system does not report pixel sizes.
    pub fn forced(proto: ProtocolType) -> Option<ImageUi> {
        let ws = crossterm::terminal::window_size().ok()?;
        if ws.columns == 0 || ws.rows == 0 || ws.width == 0 || ws.height == 0 {
            return None;
        }
        Some(ImageUi::sized(
            proto,
            (ws.width / ws.columns, ws.height / ws.rows),
        ))
    }

    fn sized(proto: ProtocolType, cell: (u16, u16)) -> ImageUi {
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize(FontSize::new(cell.0, cell.1));
        picker.set_protocol_type(proto);
        ImageUi::new(picker)
    }

    /// Ask the terminal for its cell size (and, when no protocol is given, whether it draws
    /// Sixel). Half blocks when it does not say. Must run before the keyboard reader starts.
    fn asked(proto: Option<ProtocolType>) -> (ImageUi, Vec<u8>) {
        let grid = crossterm::terminal::size().unwrap_or((0, 0));
        let answer = crate::termtheme::with_terminal(|io| {
            crate::termtheme::query_cell_size(io, grid, Duration::from_millis(300))
        });
        let Some(Ok(q)) = answer else {
            tracing::warn!("the terminal cannot be asked for its cell size; using half blocks");
            return (ImageUi::halfblocks(), Vec::new());
        };
        let proto = proto.or(q.sixel.then_some(ProtocolType::Sixel));
        match (proto, q.cell) {
            (Some(proto), Some(cell)) => (ImageUi::sized(proto, cell), q.leftover),
            _ => {
                tracing::warn!(
                    "the terminal reports no cell size (answered: {}); using half blocks",
                    q.completed
                );
                (ImageUi::halfblocks(), q.leftover)
            }
        }
    }

    /// A protocol known from the environment or chosen by the user: the cell size comes
    /// from the system and, failing that, from the terminal itself when it may be asked.
    pub fn for_protocol(proto: ProtocolType, may_query: bool) -> (ImageUi, Vec<u8>) {
        if let Some(ui) = ImageUi::forced(proto) {
            return (ui, Vec::new());
        }
        if may_query {
            return ImageUi::asked(Some(proto));
        }
        tracing::warn!("the terminal reports no pixel size; using half blocks");
        (ImageUi::halfblocks(), Vec::new())
    }

    /// For `--images kitty|sixel|iterm2`, without asking the terminal anything.
    pub fn with_protocol(proto: ProtocolType) -> ImageUi {
        ImageUi::for_protocol(proto, false).0
    }

    pub fn halfblocks() -> ImageUi {
        ImageUi::new(Picker::halfblocks())
    }

    pub fn results(&self) -> Receiver<ResizeResult> {
        self.results.clone()
    }

    pub fn protocol_name(&self) -> &'static str {
        match self.picker.protocol_type() {
            ProtocolType::Kitty => "kitty graphics",
            ProtocolType::Sixel => "sixel",
            ProtocolType::Iterm2 => "iTerm2 images",
            ProtocolType::Halfblocks => "half blocks",
        }
    }

    /// Hand a decoded picture over for drawing. With `bg`, the colour of what lies behind it,
    /// its four corners are rounded first (see [`round_corners`]).
    pub fn show(&mut self, mut img: DynamicImage, bg: Option<[u8; 3]>) {
        if let Some(bg) = bg {
            // Half blocks have a few pixels across the picture: their corner needs a larger
            // share of it to show at all.
            let share = if self.picker.protocol_type() == ProtocolType::Halfblocks {
                CORNER_SHARE_BLOCKS
            } else {
                CORNER_SHARE
            };
            round_corners(&mut img, bg, share);
        }
        // What the picture does not fill of its last cells is the background, not black.
        self.picker
            .set_background_color(bg.map(|c| [c[0], c[1], c[2], 255]));
        let p = self.picker.new_resize_protocol(img);
        self.proto.replace_protocol(p);
    }

    /// Whether the picture is drawn by the terminal (Kitty, Sixel, iTerm2) and not out of
    /// ordinary cells: such a picture lies over the cells, so it has to be left out where a
    /// popup is drawn.
    pub fn is_graphics(&self) -> bool {
        self.picker.protocol_type() != ProtocolType::Halfblocks
    }

    pub fn clear(&mut self) {
        self.proto.empty_protocol();
    }

    pub fn on_resized(&mut self, r: ResizeResult) {
        match r {
            Ok(done) => {
                self.proto.update_resized_protocol(done);
            }
            Err(e) => tracing::warn!("image encoding failed: {e}"),
        }
    }
}

/// The radius of the rounded corners, as a share of the shorter side of the picture: the
/// same look at any size (and at any scale it is drawn at later). A little more for half
/// blocks, which draw the picture a few dozen pixels wide.
pub const CORNER_SHARE: f32 = 0.04;
pub const CORNER_SHARE_BLOCKS: f32 = 0.12;

/// Round the four corners of a picture, in place: the pixels outside a quarter circle in each
/// corner are given the colour of the background `bg`, blended along the edge so that it
/// stays smooth when the picture is scaled down. This is done on the pixels, before they are
/// sent to the terminal, so that every graphics protocol (and the half blocks) shows it.
pub fn round_corners(img: &mut DynamicImage, bg: [u8; 3], share: f32) {
    let (w, h) = (img.width(), img.height());
    let radius = ((w.min(h) as f32 * share).round() as u32).max(3);
    if w < radius * 2 + 2 || h < radius * 2 + 2 {
        return;
    }
    // Only the corners are touched; a picture that is not 8-bit RGB(A) is converted first.
    if !matches!(
        img,
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_)
    ) {
        *img = DynamicImage::ImageRgba8(img.to_rgba8());
    }
    let r = radius as f32;
    // How much of the pixel at (dx, dy) from the corner square's inner corner is inside the
    // quarter circle: 1.0 inside, 0.0 outside, in between along the edge.
    let coverage = |dx: u32, dy: u32| -> f32 {
        let (px, py) = (r - dx as f32 - 0.5, r - dy as f32 - 0.5);
        (r - (px * px + py * py).sqrt() + 0.5).clamp(0.0, 1.0)
    };
    let blend = |c: u8, b: u8, cov: f32| (c as f32 * cov + b as f32 * (1.0 - cov)).round() as u8;
    for dy in 0..radius {
        for dx in 0..radius {
            let cov = coverage(dx, dy);
            if cov >= 1.0 {
                continue;
            }
            for (x, y) in [
                (dx, dy),
                (w - 1 - dx, dy),
                (dx, h - 1 - dy),
                (w - 1 - dx, h - 1 - dy),
            ] {
                match img {
                    DynamicImage::ImageRgb8(p) => {
                        let px = p.get_pixel_mut(x, y);
                        for i in 0..3 {
                            px[i] = blend(px[i], bg[i], cov);
                        }
                    }
                    DynamicImage::ImageRgba8(p) => {
                        let px = p.get_pixel_mut(x, y);
                        for i in 0..3 {
                            px[i] = blend(px[i], bg[i], cov);
                        }
                        // The corner is no longer see-through: it is the background.
                        px[3] = blend(px[3], 255, cov);
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    const BG: [u8; 3] = [26, 27, 38];

    #[test]
    fn the_corners_of_a_picture_take_the_background_and_the_middle_is_untouched() {
        let mut img =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(100, 80, Rgb::from([200u8, 100, 50])));
        round_corners(&mut img, BG, CORNER_SHARE);
        let p = img.to_rgb8();
        for (x, y) in [(0, 0), (99, 0), (0, 79), (99, 79)] {
            assert_eq!(p.get_pixel(x, y).0, BG, "corner {x},{y}");
        }
        assert_eq!(p.get_pixel(50, 40).0, [200, 100, 50]);
        // Along the edges, away from the corners, nothing changes.
        assert_eq!(p.get_pixel(50, 0).0, [200, 100, 50]);
        assert_eq!(p.get_pixel(0, 40).0, [200, 100, 50]);
    }

    #[test]
    fn the_edge_of_the_curve_is_blended_not_cut() {
        let mut img =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(200, 200, Rgb::from([255u8, 255, 255])));
        round_corners(&mut img, [0, 0, 0], CORNER_SHARE);
        let p = img.to_rgb8();
        let radius = (200.0 * CORNER_SHARE).round() as u32;
        // Somewhere along the diagonal of the corner there is a pixel that is neither.
        let mixed = (0..radius).any(|i| {
            let v = p.get_pixel(i, i)[0];
            v > 0 && v < 255
        });
        assert!(mixed, "the curve has an anti-aliased edge");
        // A pixel well inside the quarter circle is the picture's own.
        assert_eq!(p.get_pixel(radius, radius).0, [255, 255, 255]);
    }

    #[test]
    fn a_picture_with_transparency_becomes_opaque_in_its_corners() {
        let mut img =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(60, 60, Rgba([10, 20, 30, 0])));
        round_corners(&mut img, BG, CORNER_SHARE);
        let p = img.to_rgba8();
        assert_eq!(p.get_pixel(0, 0).0, [BG[0], BG[1], BG[2], 255]);
        assert_eq!(p.get_pixel(30, 30).0, [10, 20, 30, 0]);
    }

    #[test]
    fn other_pixel_formats_are_converted_and_tiny_pictures_left_alone() {
        let mut grey =
            DynamicImage::ImageLuma8(image::GrayImage::from_pixel(80, 80, image::Luma([128])));
        round_corners(&mut grey, BG, CORNER_SHARE);
        assert!(matches!(grey, DynamicImage::ImageRgba8(_)));
        assert_eq!(grey.to_rgb8().get_pixel(0, 0).0, BG);
        let mut tiny = DynamicImage::ImageRgb8(RgbImage::from_pixel(6, 6, Rgb::from([9u8, 9, 9])));
        round_corners(&mut tiny, BG, CORNER_SHARE);
        assert_eq!(
            tiny.to_rgb8().get_pixel(0, 0).0,
            [9, 9, 9],
            "too small to round"
        );
    }
}
