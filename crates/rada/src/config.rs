//! Optional `config.toml` in the rada config folder.
//!
//! ```toml
//! profile = "auto"   # auto | modern | basic: how much is asked of the terminal. `basic` is
//!                     # ASCII, 16 colours, no mouse and no pictures, for terminals without
//!                     # UTF-8 or the xterm extensions; `auto` chooses from TERM and the locale
//! icons = "auto"      # auto | nerd | unicode-icons | unicode-std | none. `auto` takes the
//!                     # first the font can draw: Nerd Font symbols, Unicode symbols (needs a
//!                     # symbol font on the system), or only what every monospace font has
//! show_hidden = false
//! sort = "name"       # name | size | date
//! reverse = false
//! natural_sort = true       # file2 before file10, capitals and small letters mixed (as the
//!                           # Finder and Explorer do); false: letter by letter
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
//! cmd_keys = true            # macOS: ask the terminal to report Cmd, for Cmd+C/V/X… (classic keys)
//! show_hints = true          # false: no row of key hints at the bottom (`hints` also works)
//! sidebar = true             # the navigation pane; shown by default; Ctrl+B toggles and remembers
//! layout = "explorer"        # explorer | compact (no address bar, command bar or hints)
//! show_details_pane = true   # false: never show the details pane on its own (Alt+P still does)
//! view = "details"           # details | icons: how a new tab shows a folder
//! remember_tabs = true       # reopen the tabs of the last session
//! dates = "relative"         # relative ("Today 14:03", "3 days ago") | absolute (dd/mm/yyyy hh:mm)
//!
//! [archives]
//! max_extract_gb = 8         # an extraction that would write more asks for a typed "yes"
//! max_ratio = 200            # ...and so does one that unpacks this many times its size
//! preview_entries = 200      # items listed in the preview of an archive
//! preview_seconds = 1.5      # how long counting an archive's members may take there
//!
//! [update]
//! check = true               # look for a newer rada (at most once a day, in the background)
//! channel = "auto"           # auto (stable users see stable releases, pre-release users see the
//!                            # next pre-releases too) | stable | prerelease
//! method = "auto"            # auto | binary (download) | source (git pull + cargo build) |
//!                            # package (rada only says which tool to use)
//! source_dir = "~/rada-fm"   # the git checkout the `source` method updates
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

use rada_core::archive::ArchiveLimits;
use rada_core::model::{SortKey, SortSpec};
use rada_core::platform::Dirs;
use rada_core::preview::ImageLimits;
use rada_core::update::{Channel, Method, Settings};
use serde::Deserialize;

#[derive(Debug)]
pub struct FileConfig {
    pub profile: Option<String>,
    pub icons: Option<String>,
    pub show_hidden: bool,
    pub sort: SortSpec,
    pub images: Option<String>,
    pub theme: Option<String>,
    pub appearance: Option<String>,
    pub bookmarks: Vec<String>,
    pub image_limits: ImageLimits,
    pub archive_limits: ArchiveLimits,
    pub archive_preview_entries: usize,
    pub archive_preview_seconds: f32,
    pub mouse: bool,
    pub hints: bool,
    pub sidebar: bool,
    pub layout: Option<String>,
    pub show_details_pane: bool,
    pub view: Option<String>,
    pub remember_tabs: bool,
    pub dates: Option<String>,
    pub hide_devices: Option<Vec<String>>,
    pub keymap: Option<String>,
    /// macOS: ask the terminal to report the Cmd key.
    pub cmd_keys: bool,
    pub keys: HashMap<String, Vec<String>>,
    /// The `[update]` section as written; [`FileConfig::update_settings`] checks it.
    update: UpdateFile,
}

#[derive(Debug, Default, Clone)]
struct UpdateFile {
    check: Option<bool>,
    channel: Option<String>,
    method: Option<String>,
    source_dir: Option<String>,
}

impl FileConfig {
    /// The `[update]` section, with the choices checked.
    pub fn update_settings(&self, home: &std::path::Path) -> anyhow::Result<Settings> {
        use anyhow::Context;
        let u = &self.update;
        let channel = match u.channel.as_deref() {
            None => Channel::Auto,
            Some(n) => Channel::parse(n).with_context(|| {
                format!("unknown update channel {n:?} (use auto, stable or prerelease)")
            })?,
        };
        let method = match u.method.as_deref() {
            None => Method::Auto,
            Some(n) => Method::parse(n).with_context(|| {
                format!("unknown update method {n:?} (use auto, binary, source or package)")
            })?,
        };
        Ok(Settings {
            check: u.check.unwrap_or(true),
            channel,
            method,
            source_dir: u
                .source_dir
                .as_deref()
                .map(|d| rada_tui::palette::expand(d, home)),
        })
    }
}

/// A key list may be written as one string or as an array.
#[derive(Deserialize)]
#[serde(untagged)]
enum Keys {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, Default)]
struct ArchivesRaw {
    max_extract_gb: Option<u64>,
    max_ratio: Option<u64>,
    preview_entries: Option<usize>,
    preview_seconds: Option<f32>,
}

#[derive(Deserialize, Default)]
struct UpdateRaw {
    check: Option<bool>,
    channel: Option<String>,
    method: Option<String>,
    source_dir: Option<String>,
}

#[derive(Deserialize, Default)]
struct DevicesRaw {
    hide: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
struct Raw {
    profile: Option<String>,
    icons: Option<String>,
    show_hidden: Option<bool>,
    sort: Option<String>,
    reverse: Option<bool>,
    natural_sort: Option<bool>,
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
    show_hints: Option<bool>,
    sidebar: Option<bool>,
    layout: Option<String>,
    show_details_pane: Option<bool>,
    view: Option<String>,
    remember_tabs: Option<bool>,
    dates: Option<String>,
    devices: Option<DevicesRaw>,
    archives: Option<ArchivesRaw>,
    update: Option<UpdateRaw>,
    keymap: Option<String>,
    cmd_keys: Option<bool>,
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
        Some("type") => SortKey::Type,
        _ => SortKey::Name,
    };
    let archives = raw.archives.unwrap_or_default();
    let al = ArchiveLimits::default();
    let update = raw.update.unwrap_or_default();
    FileConfig {
        update: UpdateFile {
            check: update.check,
            channel: update.channel,
            method: update.method,
            source_dir: update.source_dir,
        },
        archive_limits: ArchiveLimits {
            max_total_bytes: archives
                .max_extract_gb
                .map(|g| g.saturating_mul(1 << 30))
                .unwrap_or(al.max_total_bytes),
            max_ratio: archives.max_ratio.unwrap_or(al.max_ratio).max(1),
            ..al
        },
        archive_preview_entries: archives.preview_entries.unwrap_or(200).clamp(10, 5000),
        archive_preview_seconds: archives.preview_seconds.unwrap_or(1.5).clamp(0.0, 30.0),
        profile: raw.profile,
        icons: raw.icons,
        show_hidden: raw.show_hidden.unwrap_or(false),
        sort: SortSpec {
            key,
            reverse: raw.reverse.unwrap_or(false),
            dirs_first: true,
            natural: raw.natural_sort.unwrap_or(true),
        },
        images: raw.images,
        theme: raw.theme,
        appearance: raw.appearance,
        bookmarks: raw.bookmarks.unwrap_or_default(),
        mouse: raw.mouse.unwrap_or(true),
        hints: raw.show_hints.or(raw.hints).unwrap_or(true),
        sidebar: raw.sidebar.unwrap_or(true),
        layout: raw.layout,
        show_details_pane: raw.show_details_pane.unwrap_or(true),
        view: raw.view,
        remember_tabs: raw.remember_tabs.unwrap_or(true),
        dates: raw.dates,
        hide_devices: raw.devices.and_then(|d| d.hide),
        keymap: raw.keymap,
        cmd_keys: raw.cmd_keys.unwrap_or(true),
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
    fn archive_limits_are_read_and_have_defaults() {
        let c = with_config("[archives]\nmax_extract_gb = 2\nmax_ratio = 50\n");
        assert_eq!(c.archive_limits.max_total_bytes, 2 << 30);
        assert_eq!(c.archive_limits.max_ratio, 50);
        let d = with_config("sidebar = true\n");
        assert_eq!(d.archive_limits, ArchiveLimits::default());
        assert_eq!(d.archive_preview_entries, 200);
    }

    #[test]
    fn the_profile_is_read_and_absent_means_auto() {
        assert_eq!(
            with_config("profile = \"basic\"\n").profile.as_deref(),
            Some("basic")
        );
        assert_eq!(with_config("sidebar = true\n").profile, None);
    }

    #[test]
    fn the_explorer_settings_are_read_and_have_defaults() {
        let d = with_config("sidebar = true\n");
        assert!(d.hints && d.show_details_pane && d.remember_tabs);
        assert_eq!((d.layout, d.view, d.dates), (None, None, None));
        let c = with_config(
            "layout = \"compact\"\nshow_details_pane = false\nview = \"icons\"\nremember_tabs = false\ndates = \"absolute\"\nshow_hints = false\n",
        );
        assert_eq!(c.layout.as_deref(), Some("compact"));
        assert!(!c.show_details_pane && !c.remember_tabs && !c.hints);
        assert_eq!(c.view.as_deref(), Some("icons"));
        assert_eq!(c.dates.as_deref(), Some("absolute"));
        // The older name of the hints switch still works.
        assert!(!with_config("hints = false\n").hints);
    }

    #[test]
    fn the_update_section_is_read_and_checked() {
        let home = std::path::Path::new("/home/u");
        let d = with_config("sidebar = true\n")
            .update_settings(home)
            .unwrap();
        assert!(d.check);
        assert_eq!(
            (d.channel, d.method, d.source_dir),
            (Channel::Auto, Method::Auto, None)
        );

        let c = with_config(
            "[update]\ncheck = false\nchannel = \"prerelease\"\nmethod = \"source\"\nsource_dir = \"~/rada-fm\"\n",
        )
        .update_settings(home)
        .unwrap();
        assert!(!c.check);
        assert_eq!((c.channel, c.method), (Channel::Prerelease, Method::Source));
        assert_eq!(c.source_dir, Some(home.join("rada-fm")));

        for bad in ["channel = \"nightly\"", "method = \"magic\""] {
            let e = with_config(&format!("[update]\n{bad}\n"))
                .update_settings(home)
                .unwrap_err()
                .to_string();
            assert!(e.contains("unknown update"), "{e}");
        }
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
