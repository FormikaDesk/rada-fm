//! Small interface choices that survive a restart, kept in `ui.json` in the state folder
//! (as opposed to `config.toml`, which only the user edits): whether the sidebar and the
//! details pane are shown, the view mode, and the tabs that were open.
//!
//! Reading happens once at start; writing is handed to a short-lived thread so the
//! interface never waits for the disk.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pathcodec;

/// One tab as it is remembered: where it was, and how it showed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    #[serde(with = "pathcodec::path")]
    pub path: PathBuf,
    /// `details` or `icons`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// `name`, `size`, `date` or `type`: only when the person chose the order of this tab
    /// (a click on a column title, the Sort menu, the keys). Otherwise it is left out, and
    /// the tab follows `config.toml`, whatever that says by then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    /// With `sort`: the `reverse` of the order (for a date, not reversed is newest first).
    #[serde(default)]
    pub reverse: bool,
    /// `Some(true)`: the order above was chosen. `Some(false)`: it was not, and `sort` is
    /// empty. `None`: the file comes from a version that wrote the order of every tab whether
    /// it was chosen or not, and in which a date that was reversed was the newest first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_chosen: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiState {
    /// `None` until the user has toggled it: the configuration's value then applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar: Option<bool>,
    /// The details pane (`Alt+P`); `None` until toggled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<bool>,
    /// The Tag column of the list (View ▾); `None` until toggled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags_column: Option<bool>,
    /// The layout chosen in the View menu (`auto`, `panels` or `classic`); `None` until
    /// chosen: the configuration's value then applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    /// The tabs that were open when rada last ended, and which one was in front.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active_tab: usize,
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
    crate::statefile::write_atomic(
        &state_dir.join(FILE),
        &serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?,
    )
}

/// What the last request to write each state folder's file was, by number.
static LATEST: std::sync::Mutex<std::collections::BTreeMap<PathBuf, u64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
/// One write at a time.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Write it from a thread of its own. Each write is a thread, and two close together could land
/// in either order, leaving the older state in the file: a write that a newer one has
/// replaced is not made (the newer one is), so that the file ends as the last request said.
pub fn save_in_background(state_dir: std::path::PathBuf, state: UiState) {
    let seq = {
        let mut latest = LATEST.lock().unwrap_or_else(|e| e.into_inner());
        let n = latest.entry(state_dir.clone()).or_insert(0);
        *n += 1;
        *n
    };
    let _ = std::thread::Builder::new()
        .name("rada-ui-state".into())
        .spawn(move || {
            let _one_at_a_time = WRITING.lock().unwrap_or_else(|e| e.into_inner());
            let newer = LATEST
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&state_dir)
                .is_some_and(|n| *n != seq);
            if newer {
                return;
            }
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
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(load(d.path()).sidebar, Some(false));
        save(
            d.path(),
            &UiState {
                sidebar: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(load(d.path()).sidebar, Some(true));
    }

    #[test]
    fn the_layout_chosen_in_the_view_menu_comes_back_and_an_older_file_has_none() {
        let d = tempfile::tempdir().unwrap();
        save(
            d.path(),
            &UiState {
                layout: Some("panels".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(load(d.path()).layout.as_deref(), Some("panels"));
        // A file written before there was a layout to choose.
        std::fs::write(d.path().join("ui.json"), br#"{"sidebar":true}"#).unwrap();
        assert_eq!(load(d.path()).layout, None);
        // And a state with no choice does not write the key at all.
        save(d.path(), &UiState::default()).unwrap();
        let text = std::fs::read_to_string(d.path().join("ui.json")).unwrap();
        assert!(!text.contains("layout"), "{text}");
    }

    #[test]
    fn writes_made_close_together_leave_the_last_one_in_the_file() {
        let d = tempfile::tempdir().unwrap();
        for n in 0..200usize {
            save_in_background(
                d.path().to_path_buf(),
                UiState {
                    active_tab: n,
                    ..Default::default()
                },
            );
        }
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if load(d.path()).active_tab == 199 {
                break;
            }
            assert!(std::time::Instant::now() < end, "{:?}", load(d.path()));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // And it stays the last one: no late write follows.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(load(d.path()).active_tab, 199);
    }

    #[test]
    fn a_file_from_a_version_that_wrote_every_tab_loads_without_a_choice_flag() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("ui.json"),
            br#"{"tabs":[{"path":"/x","view":"details","sort":"name","reverse":false}]}"#,
        )
        .unwrap();
        let t = &load(d.path()).tabs[0];
        assert_eq!(
            (t.sort.as_deref(), t.reverse, t.sort_chosen),
            (Some("name"), false, None)
        );
    }

    #[test]
    fn tabs_come_back_with_their_view_and_order() {
        let d = tempfile::tempdir().unwrap();
        let state = UiState {
            details: Some(false),
            tabs: vec![
                SavedTab {
                    path: PathBuf::from("/home/u/projects"),
                    view: Some("icons".into()),
                    sort: Some("date".into()),
                    reverse: true,
                    sort_chosen: Some(true),
                },
                SavedTab {
                    path: PathBuf::from("/tmp"),
                    view: None,
                    sort: None,
                    reverse: false,
                    sort_chosen: Some(false),
                },
            ],
            active_tab: 1,
            ..Default::default()
        };
        save(d.path(), &state).unwrap();
        assert_eq!(load(d.path()), state);
    }

    #[test]
    fn a_file_from_an_older_version_still_loads() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("ui.json"), br#"{"sidebar": true}"#).unwrap();
        let s = load(d.path());
        assert_eq!(s.sidebar, Some(true));
        assert!(s.tabs.is_empty());
    }

    #[test]
    fn a_damaged_file_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("ui.json"), b"{ not json").unwrap();
        assert_eq!(load(d.path()), UiState::default());
    }
}
