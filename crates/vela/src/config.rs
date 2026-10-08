//! Optional `config.toml` in the vela config folder.
//!
//! ```toml
//! icons = "nerd"      # nerd | unicode | none
//! show_hidden = false
//! sort = "name"       # name | size | date
//! reverse = false
//! ```

use serde::Deserialize;
use vela_core::model::{SortKey, SortSpec};
use vela_core::platform::Dirs;

#[derive(Debug)]
pub struct FileConfig {
    pub icons: Option<String>,
    pub show_hidden: bool,
    pub sort: SortSpec,
}

#[derive(Deserialize, Default)]
struct Raw {
    icons: Option<String>,
    show_hidden: Option<bool>,
    sort: Option<String>,
    reverse: Option<bool>,
}

pub fn load(dirs: &Dirs) -> FileConfig {
    let path = dirs.vela_config().join("config.toml");
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
    }
}
