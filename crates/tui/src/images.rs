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

    pub fn show(&mut self, img: DynamicImage) {
        let p = self.picker.new_resize_protocol(img);
        self.proto.replace_protocol(p);
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
