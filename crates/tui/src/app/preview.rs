//! The preview of the item under the cursor: what was asked of the worker, what it answered,
//! the picture that goes with it and whether the keyboard is in the pane.

use crate::images::ImageUi;
use rada_core::events::PreviewEvent;
use rada_core::preview::{ImageInfo, ImagePreview, ImageState, Limits, Preview};
use std::path::{Path, PathBuf};

pub struct ImageView {
    pub info: ImageInfo,
    pub status: ImageStatus,
    pub note: Option<String>,
    /// A PDF that could not be drawn: the text of its first page.
    pub text: Vec<String>,
}

pub enum ImageStatus {
    /// Header read, pixels being decoded in the worker.
    Decoding,
    /// The picture is in the graphics protocol and drawn by the renderer.
    Shown,
    /// Image rendering is off (`--images off`): information only.
    NoGraphics,
    TooLarge(String),
    Failed(String),
}

pub struct PreviewPane {
    pub name: String,
    pub content: Option<Preview>,
    pub scroll: usize,
    /// For image files: what to show besides the picture itself.
    pub image: Option<ImageView>,
    /// Binary files show a card; the hex dump only on request (`H`).
    pub hex: bool,
    pub image_ui: Option<ImageUi>,
    /// The keyboard is in the preview of the panels view.
    pub focus: bool,
    /// The file `content` is the preview of.
    pub(super) path: Option<PathBuf>,
    limits: Limits,
    /// The number of the last request: an older answer is dropped.
    generation: u64,
    /// The file last asked for, so that the same one is not asked twice.
    asked: Option<PathBuf>,
}

impl PreviewPane {
    pub fn new(image_ui: Option<ImageUi>, mut limits: Limits) -> PreviewPane {
        // No graphics, no point in decoding pictures.
        limits.image.decode = limits.image.decode && image_ui.is_some();
        PreviewPane {
            name: String::new(),
            content: None,
            scroll: 0,
            image: None,
            hex: false,
            image_ui,
            focus: false,
            path: None,
            limits,
            generation: 0,
            asked: None,
        }
    }

    /// Nothing is under the cursor: show nothing.
    pub(super) fn clear(&mut self) {
        self.content = None;
        self.name.clear();
        self.asked = None;
    }

    /// What was shown is out of date: ask again the next time.
    pub(super) fn invalidate(&mut self) {
        self.asked = None;
    }

    /// Note that `path` is asked for. Its request number and the limits to ask with, or `None`
    /// when it is the file already asked for.
    pub(super) fn ask(&mut self, path: &Path, show_hidden: bool) -> Option<(u64, Limits)> {
        if self.asked.as_deref() == Some(path) {
            return None;
        }
        self.asked = Some(path.to_path_buf());
        self.generation += 1;
        let limits = Limits {
            show_hidden,
            ..self.limits.clone()
        };
        Some((self.generation, limits))
    }

    pub fn scroll_up(&mut self, by: usize) {
        self.scroll = self.scroll.saturating_sub(by);
    }

    pub fn scroll_down(&mut self, by: usize) {
        self.scroll = self.scroll.saturating_add(by);
    }

    /// Switch a binary file between its card and its hex dump; false for anything else.
    pub(super) fn toggle_hex(&mut self) -> bool {
        let binary = matches!(self.content, Some(Preview::Binary(_)));
        if binary {
            self.hex = !self.hex;
            self.scroll = 0;
        }
        binary
    }

    /// The cell is `cell` pixels now; true when the pictures have to be made again.
    pub(super) fn use_cell_px(&mut self, cell: (u16, u16)) -> bool {
        let changed = self
            .image_ui
            .as_mut()
            .is_some_and(|ui| ui.set_cell_px(cell));
        if changed {
            self.asked = None;
        }
        changed
    }

    pub(super) fn clear_image(&mut self) {
        if let Some(ui) = &mut self.image_ui {
            ui.clear();
        }
    }

    /// Take the worker's answer, if it is to the last request. `bg` is the colour behind a
    /// picture, for its corners.
    pub(super) fn show(&mut self, ev: PreviewEvent, bg: Option<[u8; 3]>) {
        if ev.generation != self.generation {
            return;
        }
        if self.path.as_ref() != Some(&ev.path) {
            self.hex = false;
            self.scroll = 0;
            self.path = Some(ev.path.clone());
        }
        self.name = rada_core::display::name(&ev.name);
        match ev.preview {
            Preview::Image(ip) => {
                self.image = Some(self.image_view(ip, bg));
                self.content = None;
            }
            other => {
                self.clear_image();
                self.image = None;
                self.content = Some(other);
            }
        }
    }

    fn image_view(&mut self, ip: ImagePreview, bg: Option<[u8; 3]>) -> ImageView {
        let status = match ip.state {
            ImageState::Disabled => {
                self.clear_image();
                ImageStatus::NoGraphics
            }
            ImageState::Loading => {
                self.clear_image();
                ImageStatus::Decoding
            }
            ImageState::Ready(px) => match &mut self.image_ui {
                Some(ui) => {
                    // The event owns the only reference: no pixel copy.
                    let img =
                        std::sync::Arc::try_unwrap(px).unwrap_or_else(|shared| (*shared).clone());
                    ui.show(img, bg);
                    ImageStatus::Shown
                }
                None => ImageStatus::NoGraphics,
            },
            ImageState::TooLarge(m) => {
                self.clear_image();
                ImageStatus::TooLarge(m)
            }
            ImageState::Failed(m) => {
                self.clear_image();
                ImageStatus::Failed(m)
            }
        };
        ImageView {
            info: ip.info,
            status,
            note: ip.note,
            text: ip.text,
        }
    }
}
