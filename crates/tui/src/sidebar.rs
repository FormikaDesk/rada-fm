//! The sidebar's contents: standard places, bookmarks and disks, as a list of items. Drawing
//! is in `ui/sidebar.rs`; this is what is in it and how wide it gets.

use std::path::{Path, PathBuf};

use rada_core::platform::{PlaceKind, VolumeKind};

use crate::app::App;
use crate::icons::IconSet;

/// Width of the full sidebar, and of the column of icons it shrinks to.
pub const FULL_WIDTH: u16 = 26;
pub const RAIL_WIDTH: u16 = 5;

/// Terminal widths from which the sidebar is shown in full / as icons. Below the second,
/// it is not shown at all: it gives way before the preview does (the preview needs 88
/// columns of its own).
const FULL_FROM: u16 = 124;
const RAIL_FROM: u16 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hidden,
    /// Icons only, and only the standard places.
    Rail,
    Full,
}

impl Mode {
    /// How much of the sidebar a terminal of `width` columns gets; `on` is the user's
    /// choice to have it at all.
    pub fn for_width(width: u16, on: bool) -> Mode {
        if !on || width < RAIL_FROM {
            Mode::Hidden
        } else if width < FULL_FROM {
            Mode::Rail
        } else {
            Mode::Full
        }
    }

    /// Columns taken, divider included.
    pub fn columns(self) -> u16 {
        match self {
            Mode::Hidden => 0,
            Mode::Rail => RAIL_WIDTH + 1,
            Mode::Full => FULL_WIDTH + 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Places,
    Bookmarks,
    Devices,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Places => "PLACES",
            Section::Bookmarks => "BOOKMARKS",
            Section::Devices => "DEVICES",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Place(PlaceKind),
    /// `removable`: kept in rada's own state, so the sidebar can take it out (a bookmark
    /// from the configuration file can only be removed there).
    Bookmark {
        removable: bool,
    },
    Device(VolumeKind),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    /// How full the disk is, 0.0 to 1.0.
    pub used: f64,
    pub free: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub section: Section,
    pub origin: Origin,
    pub name: String,
    pub path: PathBuf,
    pub usage: Option<Usage>,
}

impl Item {
    /// The icon: a Nerd Font glyph, a plain Unicode mark, or nothing.
    pub fn glyph(&self, set: IconSet) -> &'static str {
        let (nerd, plain) = match &self.origin {
            Origin::Place(PlaceKind::Home) => ("\u{f015}", "⌂"),
            Origin::Place(PlaceKind::Desktop) => ("\u{f108}", "▭"),
            Origin::Place(PlaceKind::Documents) => ("\u{f15c}", "▤"),
            Origin::Place(PlaceKind::Downloads) => ("\u{f019}", "↓"),
            Origin::Place(PlaceKind::Music) => ("\u{f001}", "♪"),
            Origin::Place(PlaceKind::Pictures) => ("\u{f03e}", "▣"),
            Origin::Place(PlaceKind::Videos) => ("\u{f03d}", "▶"),
            Origin::Place(PlaceKind::Trash) => ("\u{f1f8}", "⌫"),
            Origin::Bookmark { .. } => ("\u{f02e}", "★"),
            Origin::Device(VolumeKind::Removable) => ("\u{f287}", "⏏"),
            Origin::Device(VolumeKind::Network) => ("\u{f0ac}", "≈"),
            Origin::Device(_) => ("\u{f0a0}", "▥"),
        };
        match set {
            IconSet::Nerd => nerd,
            IconSet::Unicode => plain,
            IconSet::None => "",
        }
    }

    /// What stands in for the icon when there is none: the first letter.
    pub fn initial(&self) -> String {
        self.name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default()
    }
}

fn base_name(p: &Path) -> String {
    p.file_name()
        .map(rada_core::display::name)
        .unwrap_or_else(|| rada_core::display::path(p))
}

/// Everything the sidebar lists, in order. `places_only` is the narrow mode: bookmarks and
/// disks are left out (they stay reachable with the jump palette).
pub fn items(app: &App, places_only: bool) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for p in &app.paths.places {
        out.push(Item {
            section: Section::Places,
            origin: Origin::Place(p.kind),
            name: p.name(),
            path: p.path.clone(),
            usage: None,
        });
    }
    if places_only {
        return out;
    }

    let state_bookmarks = &app.paths.bookmarks;
    let mut seen: Vec<&PathBuf> = Vec::new();
    for (path, removable) in app
        .config_bookmarks()
        .iter()
        .map(|p| (p, false))
        .chain(state_bookmarks.iter().map(|p| (p, true)))
    {
        if seen.contains(&path) {
            continue;
        }
        seen.push(path);
        // A bookmark in both places can be removed from the state, but would stay.
        let removable = removable && !app.config_bookmarks().contains(path);
        out.push(Item {
            section: Section::Bookmarks,
            origin: Origin::Bookmark { removable },
            name: base_name(path),
            path: path.clone(),
            usage: None,
        });
    }

    for v in app.volumes.iter().filter(|v| v.kind != VolumeKind::Virtual) {
        let name = device_name(v);
        let usage = match (v.total, v.available) {
            (Some(t), Some(a)) if t > 0 && v.responsive => Some(Usage {
                used: (1.0 - a as f64 / t as f64).clamp(0.0, 1.0),
                free: a,
            }),
            _ => None,
        };
        out.push(Item {
            section: Section::Devices,
            origin: Origin::Device(v.kind),
            name,
            path: v.mount_point.clone(),
            usage,
        });
    }
    out
}

/// A disk's name in the sidebar: its label, "System" for the root, else its mount folder.
pub fn device_name(v: &rada_core::platform::Volume) -> String {
    v.label.clone().unwrap_or_else(|| {
        if v.mount_point == Path::new(std::path::MAIN_SEPARATOR_STR) {
            "System".to_string()
        } else {
            base_name(&v.mount_point)
        }
    })
}

/// The item the current folder belongs to, for the highlight: the first one that is that
/// folder (a place before a bookmark of the same folder).
pub fn current_index(items: &[Item], cwd: &Path) -> Option<usize> {
    items.iter().position(|i| i.path == cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidebar_gives_way_before_the_preview_does() {
        assert_eq!(Mode::for_width(200, true), Mode::Full);
        assert_eq!(Mode::for_width(124, true), Mode::Full);
        assert_eq!(Mode::for_width(123, true), Mode::Rail);
        assert_eq!(Mode::for_width(100, true), Mode::Rail);
        assert_eq!(Mode::for_width(99, true), Mode::Hidden);
        assert_eq!(Mode::for_width(200, false), Mode::Hidden);
        // Full or rail, the 88 columns the preview needs are still there (less the margins).
        assert!(124 - Mode::Full.columns() - 6 >= 88);
        assert!(100 - Mode::Rail.columns() - 6 >= 88);
    }

    #[test]
    fn the_root_of_the_system_is_called_system() {
        use rada_core::platform::Volume;
        let root = Volume {
            mount_point: PathBuf::from("/"),
            label: None,
            fs_type: "ext4".into(),
            device: "/dev/sda1".into(),
            kind: VolumeKind::Fixed,
            drive_letter: None,
            total: Some(100),
            available: Some(40),
            read_only: false,
            responsive: true,
        };
        assert_eq!(device_name(&root), "System");
        let usb = Volume {
            mount_point: PathBuf::from("/run/media/u/USB"),
            ..root
        };
        assert_eq!(device_name(&usb), "USB");
    }

    #[test]
    fn a_device_without_a_label_is_named_after_its_mount() {
        assert_eq!(base_name(Path::new("/run/media/user/USB")), "USB");
    }
}
