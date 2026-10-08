//! Where rada keeps its own files. Passed around explicitly (never read from the
//! environment deep inside the engine), which is what lets tests sandbox everything.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl Dirs {
    /// Resolve from the process environment.
    pub fn from_env() -> Result<Dirs> {
        Self::from_vars(|k| std::env::var_os(k))
    }

    /// Resolve from an arbitrary variable source (XDG on Linux).
    pub fn from_vars(get: impl Fn(&str) -> Option<OsString>) -> Result<Dirs> {
        // XDG: relative paths in these variables must be ignored.
        let abs = |k: &str| get(k).map(PathBuf::from).filter(|p| p.is_absolute());
        let home = abs("HOME")
            .or_else(|| abs("USERPROFILE"))
            .ok_or_else(|| Error::Invalid("cannot determine the home directory".into()))?;
        #[cfg(target_os = "macos")]
        let (def_config, def_data, def_state, def_cache) = (
            home.join("Library/Application Support"),
            home.join("Library/Application Support"),
            home.join("Library/Application Support"),
            home.join("Library/Caches"),
        );
        #[cfg(windows)]
        let (def_config, def_data, def_state, def_cache) = {
            let appdata = abs("APPDATA").unwrap_or_else(|| home.join("AppData/Roaming"));
            let local = abs("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData/Local"));
            (appdata, local.clone(), local.clone(), local)
        };
        #[cfg(not(any(target_os = "macos", windows)))]
        let (def_config, def_data, def_state, def_cache) = (
            home.join(".config"),
            home.join(".local/share"),
            home.join(".local/state"),
            home.join(".cache"),
        );
        Ok(Dirs {
            config: abs("XDG_CONFIG_HOME").unwrap_or(def_config),
            data: abs("XDG_DATA_HOME").unwrap_or(def_data),
            state: abs("XDG_STATE_HOME").unwrap_or(def_state),
            cache: abs("XDG_CACHE_HOME").unwrap_or(def_cache),
            home,
        })
    }

    /// Everything under one root (tests, `--sandbox`).
    pub fn under(root: &Path) -> Dirs {
        Dirs {
            home: root.join("home"),
            config: root.join("xdg/config"),
            data: root.join("xdg/data"),
            state: root.join("xdg/state"),
            cache: root.join("xdg/cache"),
        }
    }

    pub fn rada_state(&self) -> PathBuf {
        self.state.join("rada")
    }

    pub fn rada_config(&self) -> PathBuf {
        self.config.join("rada")
    }

    pub fn rada_cache(&self) -> PathBuf {
        self.cache.join("rada")
    }

    pub fn journal_path(&self) -> PathBuf {
        self.rada_state().join("journal.jsonl")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.rada_state().join("log")
    }

    /// The user's own trash directory (`$XDG_DATA_HOME/Trash` on Linux).
    pub fn home_trash(&self) -> PathBuf {
        self.data.join("Trash")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars(m: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, OsString> = m
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn xdg_defaults_and_overrides() {
        let d = Dirs::from_vars(vars(&[("HOME", "/h"), ("XDG_STATE_HOME", "/s")])).unwrap();
        assert_eq!(d.state, PathBuf::from("/s"));
        assert_eq!(d.data, PathBuf::from("/h/.local/share"));
        assert_eq!(d.journal_path(), PathBuf::from("/s/rada/journal.jsonl"));
        assert_eq!(d.home_trash(), PathBuf::from("/h/.local/share/Trash"));
    }

    #[test]
    fn relative_xdg_values_are_ignored() {
        let d = Dirs::from_vars(vars(&[("HOME", "/h"), ("XDG_STATE_HOME", "relative/x")])).unwrap();
        assert!(d.state.is_absolute());
        assert_ne!(d.state, PathBuf::from("relative/x"));
    }

    #[test]
    fn missing_home_is_an_error() {
        assert!(Dirs::from_vars(vars(&[])).is_err());
    }
}
