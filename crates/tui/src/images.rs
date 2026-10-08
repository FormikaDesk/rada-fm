//! Terminal graphics: protocol detection (Kitty, Sixel, iTerm2, halfblocks) and the
//! resize/encode work, which always runs on a thread of its own.

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
            .name("vela-image-encode".into())
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
    ///
    /// 1. Under tmux: half blocks. Graphics queries are wrapped for passthrough and are
    ///    simply never answered unless `allow-passthrough` is on; waiting for them is slow
    ///    and the abandoned query thread keeps reading (and eating) keystrokes.
    /// 2. Terminals that announce themselves (Ghostty, kitty, WezTerm, iTerm2, foot): the
    ///    protocol follows from the environment and the cell size from the window size, so
    ///    nothing is asked and nothing can go wrong.
    /// 3. `linux`/`dumb` terminals never answer: half blocks.
    /// 4. Anything else is asked, with a bounded wait.
    pub fn detect() -> ImageUi {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let (term, prog) = (env("TERM"), env("TERM_PROGRAM"));
        if !env("TMUX").is_empty() {
            return ImageUi::halfblocks();
        }
        if let Some(proto) = known_protocol(&term, &prog) {
            if let Some(ui) = ImageUi::forced(proto) {
                return ui;
            }
        }
        if matches!(term.as_str(), "" | "dumb" | "linux") {
            return ImageUi::halfblocks();
        }
        let opts = QueryStdioOptions {
            timeout: Duration::from_secs(1),
            ..Default::default()
        };
        match Picker::from_query_stdio_with_options(opts) {
            Ok(p) => ImageUi::new(p),
            Err(e) => {
                tracing::warn!("terminal graphics detection failed: {e}; using half blocks");
                ImageUi::halfblocks()
            }
        }
    }

    /// A given protocol, with the cell size taken from the window's pixel size.
    /// `None` when the terminal does not report pixel sizes.
    pub fn forced(proto: ProtocolType) -> Option<ImageUi> {
        let ws = crossterm::terminal::window_size().ok()?;
        if ws.columns == 0 || ws.rows == 0 || ws.width == 0 || ws.height == 0 {
            return None;
        }
        let fs = FontSize::new(ws.width / ws.columns, ws.height / ws.rows);
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize(fs);
        picker.set_protocol_type(proto);
        Some(ImageUi::new(picker))
    }

    /// For `--images kitty|sixel|iterm2`.
    pub fn with_protocol(proto: ProtocolType) -> ImageUi {
        ImageUi::forced(proto).unwrap_or_else(|| {
            tracing::warn!("the terminal reports no pixel size; using half blocks");
            ImageUi::halfblocks()
        })
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

/// Protocol implied by the environment, for terminals that identify themselves.
fn known_protocol(term: &str, prog: &str) -> Option<ProtocolType> {
    let has = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    if term == "xterm-ghostty"
        || prog.eq_ignore_ascii_case("ghostty")
        || has("GHOSTTY_RESOURCES_DIR")
    {
        return Some(ProtocolType::Kitty);
    }
    if term == "xterm-kitty" || has("KITTY_WINDOW_ID") {
        return Some(ProtocolType::Kitty);
    }
    if prog.eq_ignore_ascii_case("wezterm") || has("WEZTERM_EXECUTABLE") || prog == "iTerm.app" {
        return Some(ProtocolType::Iterm2);
    }
    if term.starts_with("foot") {
        return Some(ProtocolType::Sixel);
    }
    None
}
