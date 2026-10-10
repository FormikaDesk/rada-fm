//! The user's standard folders (Desktop, Documents, Downloads…) under the names the
//! system really uses: "Scaricati" on an Italian Linux desktop, "Downloads" elsewhere.
//!
//! Linux reads `user-dirs.dirs` (xdg-user-dirs). Windows (Known Folders) and macOS will
//! ask the system in a later phase; until then they use the conventional names under
//! the home folder.

use std::path::{Path, PathBuf};

/// Which standard place a folder is, for its icon and its position in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    Music,
    Pictures,
    Videos,
    Trash,
}

/// The standard folders of a user. A `None` is a folder the user has switched off (or
/// that the system does not define); whether a folder exists is for the caller to check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserDirs {
    pub desktop: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub pictures: Option<PathBuf>,
    pub videos: Option<PathBuf>,
}

impl UserDirs {
    /// The usual names under `home`, in English, one per kind: what a fresh system has
    /// before any localisation, and the stand-in on platforms not implemented yet.
    pub fn conventional(home: &Path, movies: &str) -> UserDirs {
        let j = |n: &str| Some(home.join(n));
        UserDirs {
            desktop: j("Desktop"),
            documents: j("Documents"),
            downloads: j("Downloads"),
            music: j("Music"),
            pictures: j("Pictures"),
            videos: j(movies),
        }
    }

    /// The folders `$XDG_CONFIG_HOME/user-dirs.dirs` names, or `None` when there is no such
    /// file (a system never localised: the caller falls back to the English names).
    pub fn from_xdg_file(config: &Path, home: &Path) -> Option<UserDirs> {
        let text = std::fs::read_to_string(config.join("user-dirs.dirs")).ok()?;
        Some(UserDirs::parse_xdg(&text, home))
    }

    /// In display order.
    pub fn entries(&self) -> Vec<(PlaceKind, &Path)> {
        [
            (PlaceKind::Desktop, &self.desktop),
            (PlaceKind::Documents, &self.documents),
            (PlaceKind::Downloads, &self.downloads),
            (PlaceKind::Pictures, &self.pictures),
            (PlaceKind::Music, &self.music),
            (PlaceKind::Videos, &self.videos),
        ]
        .into_iter()
        .filter_map(|(k, p)| p.as_deref().map(|p| (k, p)))
        .collect()
    }

    /// Parse the text of `user-dirs.dirs`:
    ///
    /// ```text
    /// XDG_DOWNLOAD_DIR="$HOME/Scaricati"
    /// ```
    ///
    /// Only absolute results are kept, `$HOME` and `${HOME}` are expanded, and a folder
    /// that points at the home folder itself counts as switched off (that is how
    /// xdg-user-dirs disables one).
    pub fn parse_xdg(text: &str, home: &Path) -> UserDirs {
        let mut out = UserDirs::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let slot = match key.trim() {
                "XDG_DESKTOP_DIR" => &mut out.desktop,
                "XDG_DOCUMENTS_DIR" => &mut out.documents,
                "XDG_DOWNLOAD_DIR" => &mut out.downloads,
                "XDG_MUSIC_DIR" => &mut out.music,
                "XDG_PICTURES_DIR" => &mut out.pictures,
                "XDG_VIDEOS_DIR" => &mut out.videos,
                _ => continue,
            };
            *slot = xdg_value(value, home).filter(|p| !same_folder(p, home));
        }
        out
    }
}

fn same_folder(a: &Path, b: &Path) -> bool {
    a.components().eq(b.components())
}

/// One value of the file: quotes removed, `$HOME` expanded, escapes resolved.
fn xdg_value(raw: &str, home: &Path) -> Option<PathBuf> {
    let v = raw.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(v);
    let mut unescaped = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                unescaped.push(n);
            }
        } else {
            unescaped.push(c);
        }
    }
    let home_str = home.to_string_lossy();
    let expanded = if let Some(rest) = unescaped.strip_prefix("${HOME}") {
        format!("{home_str}{rest}")
    } else if let Some(rest) = unescaped.strip_prefix("$HOME") {
        format!("{home_str}{rest}")
    } else {
        unescaped
    };
    let p = PathBuf::from(expanded);
    p.is_absolute().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on every system: `/home/user`, or `C:/home/user` on Windows (written
    /// with slashes: in the file under test a backslash is an escape).
    fn abs(rest: &str) -> PathBuf {
        let root = if cfg!(windows) { "C:/" } else { "/" };
        PathBuf::from(format!("{root}{rest}"))
    }

    fn home() -> PathBuf {
        abs("home/user")
    }

    fn names(u: &UserDirs) -> Vec<String> {
        u.entries()
            .iter()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn an_italian_desktop_keeps_its_own_names() {
        let text = r#"# This file is written by xdg-user-dirs-update
XDG_DESKTOP_DIR="$HOME/Scrivania"
XDG_DOWNLOAD_DIR="$HOME/Scaricati"
XDG_TEMPLATES_DIR="$HOME/Modelli"
XDG_PUBLICSHARE_DIR="$HOME/Pubblici"
XDG_DOCUMENTS_DIR="$HOME/Documenti"
XDG_MUSIC_DIR="$HOME/Musica"
XDG_PICTURES_DIR="$HOME/Immagini"
XDG_VIDEOS_DIR="$HOME/Video"
"#;
        let u = UserDirs::parse_xdg(text, &home());
        assert_eq!(
            names(&u),
            [
                "Scrivania",
                "Documenti",
                "Scaricati",
                "Immagini",
                "Musica",
                "Video"
            ]
        );
        assert_eq!(u.downloads, Some(home().join("Scaricati")));
    }

    #[test]
    fn braces_spaces_absolute_paths_and_escapes() {
        let text = format!(
            "XDG_DOCUMENTS_DIR=\"${{HOME}}/My Docs\"\n\
             XDG_MUSIC_DIR=\"{}\"\n\
             XDG_PICTURES_DIR=\"$HOME/Foto \\\"vecchie\\\"\"\n",
            abs("data/Media/Music").display()
        );
        let u = UserDirs::parse_xdg(&text, &home());
        assert_eq!(u.documents, Some(home().join("My Docs")));
        assert_eq!(u.music, Some(abs("data/Media/Music")));
        assert_eq!(u.pictures, Some(home().join("Foto \"vecchie\"")));
    }

    #[test]
    fn a_folder_pointing_at_home_is_switched_off_and_junk_is_ignored() {
        let text = "XDG_DESKTOP_DIR=\"$HOME/\"\n\
                    XDG_VIDEOS_DIR=\"$HOME\"\n\
                    XDG_MUSIC_DIR=\"relative/Music\"\n\
                    this is not a setting\n\
                    # XDG_DOWNLOAD_DIR=\"$HOME/Commented\"\n\
                    XDG_DOCUMENTS_DIR=\"$HOME/Documents\"\n";
        let u = UserDirs::parse_xdg(text, &home());
        assert_eq!(names(&u), ["Documents"]);
    }

    #[test]
    fn the_conventional_names_follow_the_display_order() {
        let u = UserDirs::conventional(&home(), "Videos");
        assert_eq!(
            names(&u),
            [
                "Desktop",
                "Documents",
                "Downloads",
                "Pictures",
                "Music",
                "Videos"
            ]
        );
        assert_eq!(
            UserDirs::conventional(&home(), "Movies").videos,
            Some(home().join("Movies"))
        );
    }
}
