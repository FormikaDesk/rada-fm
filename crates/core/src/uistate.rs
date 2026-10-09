//! Small interface choices that survive a restart, kept in `ui.json` in the state folder
//! (as opposed to `config.toml`, which only the user edits). Today: whether the sidebar
//! is shown.
//!
//! Reading happens once at start; writing is handed to a short-lived thread so the
//! interface never waits for the disk.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiState {
    /// `None` until the user has toggled it: the configuration's value then applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar: Option<bool>,
}

const FILE: &str = "ui.json";

/// What was saved, or the empty state when there is nothing (or nothing readable).
pub fn load(state_dir: &Path) -> UiState {
    std::fs::read(state_dir.join(FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Write the state now. Errors are returned for the caller to log; losing this file only
/// costs a remembered toggle.
pub fn save(state_dir: &Path, state: &UiState) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let tmp = state_dir.join("ui.json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, state_dir.join(FILE))
}

/// Write it from a thread of its own.
pub fn save_in_background(state_dir: std::path::PathBuf, state: UiState) {
    let _ = std::thread::Builder::new()
        .name("rada-ui-state".into())
        .spawn(move || {
            if let Err(e) = save(&state_dir, &state) {
                tracing::warn!("cannot save {}: {e}", state_dir.join(FILE).display());
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_saved_means_nothing_chosen() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(d.path()), UiState::default());
        assert_eq!(load(&d.path().join("missing")), UiState::default());
    }

    #[test]
    fn a_saved_choice_comes_back() {
        let d = tempfile::tempdir().unwrap();
        save(
            d.path(),
            &UiState {
                sidebar: Some(false),
            },
        )
        .unwrap();
        assert_eq!(load(d.path()).sidebar, Some(false));
        save(
            d.path(),
            &UiState {
                sidebar: Some(true),
            },
        )
        .unwrap();
        assert_eq!(load(d.path()).sidebar, Some(true));
    }

    #[test]
    fn a_damaged_file_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("ui.json"), b"{ not json").unwrap();
        assert_eq!(load(d.path()), UiState::default());
    }
}
