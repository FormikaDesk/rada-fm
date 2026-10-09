//! Optional `config.toml` in the rada config folder.
//!
//! ```toml
//! icons = "nerd"      # nerd | unicode | none
//! show_hidden = false
//! sort = "name"       # name | size | date
//! reverse = false
//! theme = "auto"            # auto | rada | catppuccin | tokyo-night, or a fixed variant:
//!                            # rada-dark | rada-light | catppuccin-mocha | catppuccin-latte
//!                            # | tokyo-night-dark | tokyo-night-day
//! appearance = "auto"        # auto | light | dark: which variant a theme name uses
//! bookmarks = ["~/projects", "/mnt/data"]
//! images = "auto"            # auto | halfblocks | kitty | sixel | iterm2 | off
//! image_max_megapixels = 50   # larger images are not decoded
//! image_max_file_mb = 128
//! pdf_max_file_mb = 512      # larger PDFs are not drawn (needs poppler: pdftoppm, pdfinfo)
//! pdf_timeout_seconds = 8    # a PDF that takes longer to draw is given up on
//! mouse = true                # false: no mouse capture at all
//! keymap = "vim+classic"      # vim+classic (default) | vim | classic
//! hints = true               # false: no key hints in the bottom bar
//! sidebar = true             # shown by default; Ctrl+B toggles and remembers
//!
//! [devices]
//! hide = ["tmpfs", "/boot"]   # replaces the built-in list of mounts kept out of the disks
//!                               # (a name is a filesystem type, `fuse.*` a prefix of types;
//!                               # `/path` a mount point, `/path/*` it and everything below)
//!
//! [keys]                      # per action: replaces all its keys; [] unbinds
//! copy = ["y", "ctrl+c"]
//! quit = ["q", "ctrl+q"]
//! ```

use std::collections::HashMap;

use rada_core::model::{SortKey, SortSpec};
use rada_core::platform::Dirs;
use rada_core::preview::ImageLimits;
use serde::Deserialize;

#[derive(Debug)]
pub struct FileConfig {
    pub icons: Option<String>,
    pub show_hidden: bool,
    pub sort: SortSpec,
    pub images: Option<String>,
    pub theme: Option<String>,
    pub appearance: Option<String>,
    pub bookmarks: Vec<String>,
    pub image_limits: ImageLimits,
    pub mouse: bool,
    pub hints: bool,
    pub sidebar: bool,
    pub hide_devices: Option<Vec<String>>,
    pub keymap: Option<String>,
    pub keys: HashMap<String, Vec<String>>,
}

/// A key list may be written as one string or as an array.
#[derive(Deserialize)]
#[serde(untagged)]
enum Keys {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, Default)]
struct DevicesRaw {
    hide: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct Raw {
    icons: Option<String>,
    show_hidden: Option<bool>,
    sort: Option<String>,
    reverse: Option<bool>,
    images: Option<String>,
    theme: Option<String>,
    appearance: Option<String>,
    bookmarks: Option<Vec<String>>,
    image_max_megapixels: Option<u32>,
    image_max_file_mb: Option<u64>,
    pdf_max_file_mb: Option<u64>,
    pdf_timeout_seconds: Option<u64>,
    mouse: Option<bool>,
    hints: Option<bool>,
    sidebar: Option<bool>,
    devices: Option<DevicesRaw>,
    keymap: Option<String>,
    keys: Option<HashMap<String, Keys>>,
}

pub fn load(dirs: &Dirs) -> FileConfig {
    let path = dirs.rada_config().join("config.toml");
    let raw: Raw = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("{}: {e}", path.display());
            Raw::default()
        }),
        Err(_) => Raw::default(),
    };
    let key = match raw.sort.as_deref() {
        Some("size") => SortKey::Size,
        Some("date") | Some("modified") => SortKey::Modified,
        _ => SortKey::Name,
    };
    FileConfig {
        icons: raw.icons,
        show_hidden: raw.show_hidden.unwrap_or(false),
        sort: SortSpec {
            key,
            reverse: raw.reverse.unwrap_or(false),
            dirs_first: true,
        },
        images: raw.images,
        theme: raw.theme,
        appearance: raw.appearance,
        bookmarks: raw.bookmarks.unwrap_or_default(),
        mouse: raw.mouse.unwrap_or(true),
        hints: raw.hints.unwrap_or(true),
        sidebar: raw.sidebar.unwrap_or(true),
        hide_devices: raw.devices.and_then(|d| d.hide),
        keymap: raw.keymap,
        keys: raw
            .keys
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    match v {
                        Keys::One(s) => vec![s],
                        Keys::Many(v) => v,
                    },
                )
            })
            .collect(),
        image_limits: {
            let d = ImageLimits::default();
            ImageLimits {
                max_megapixels: raw.image_max_megapixels.unwrap_or(d.max_megapixels).max(1),
                max_file_bytes: raw
                    .image_max_file_mb
                    .map(|m| m.saturating_mul(1 << 20))
                    .unwrap_or(d.max_file_bytes),
                pdf_max_file_bytes: raw
                    .pdf_max_file_mb
                    .map(|m| m.saturating_mul(1 << 20))
                    .unwrap_or(d.pdf_max_file_bytes),
                pdf_timeout: raw
                    .pdf_timeout_seconds
                    .map(|s| std::time::Duration::from_secs(s.clamp(1, 120)))
                    .unwrap_or(d.pdf_timeout),
                pdf_cache: Some(dirs.rada_cache().join("previews")),
                ..d
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_config(text: &str) -> FileConfig {
        let root = tempfile::tempdir().unwrap();
        let dirs = Dirs::under(root.path());
        let dir = dirs.rada_config();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), text).unwrap();
        load(&dirs)
    }

    #[test]
    fn devices_hide_is_read_and_absent_means_the_built_in_list() {
        let c = with_config("[devices]\nhide = [\"tmpfs\", \"/boot/*\"]\n");
        assert_eq!(
            c.hide_devices,
            Some(vec!["tmpfs".to_string(), "/boot/*".to_string()])
        );
        assert_eq!(with_config("sidebar = true\n").hide_devices, None);
        // An empty list is a choice too: hide nothing.
        assert_eq!(
            with_config("[devices]\nhide = []\n").hide_devices,
            Some(vec![])
        );
    }
}
