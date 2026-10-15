//! The sidebar's contents: standard places, bookmarks and disks, as a list of items. Drawing
//! is in `ui/sidebar.rs`; this is what is in it and how wide it gets.

use std::path::{Path, PathBuf};

use rada_core::platform::{PlaceKind, TagColor as Hue, VolumeKind};

use crate::app::App;
use crate::icons::IconSet;

/// Width of the full navigation pane, and of the column of icons it shrinks to.
pub const FULL_WIDTH: u16 = 28;
pub const RAIL_WIDTH: u16 = 5;

/// Terminal widths from which the pane is shown in full / as icons. Below the second it is
/// not shown at all.
const FULL_FROM: u16 = 100;
const RAIL_FROM: u16 = 60;

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
    /// Home, alone at the top.
    Home,
    /// The user's standard folders and the folders pinned by hand.
    Pinned,
    Devices,
    /// The seven colours and the user's own tags: each opens the list of what carries it.
    Tags,
    /// The trash, alone at the bottom.
    Trash,
}

impl Section {
    /// The heading over the section, if it has one.
    pub fn title(self) -> Option<&'static str> {
        use rada_core::i18n::Key;
        match self {
            Section::Home | Section::Trash => None,
            Section::Pinned => Some(Key::SecPinned.tr()),
            Section::Devices => Some(Key::SecDevices.tr()),
            Section::Tags => Some(Key::SecTags.tr()),
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
    /// A stick or a phone that is plugged in and not mounted: opening it mounts it.
    Unmounted(VolumeKind),
    /// A tag, in its colour if it has one.
    Tag(Option<Hue>),
    /// The files used lately, under Home, where the system has such a list.
    Recent,
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
    /// The system has refused this place until the person allows it.
    pub locked: bool,
}

impl Item {
    /// A few words after the name, dimmed: what is the case of this item, when it is not what
    /// the others are (a device that is plugged in and not mounted).
    pub fn note(&self) -> Option<&'static str> {
        matches!(self.origin, Origin::Unmounted(_))
            .then(|| rada_core::i18n::Key::DevNotMounted.tr())
    }

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
            Origin::Place(PlaceKind::ICloud) => ("\u{f0c2}", "☁"),
            Origin::Recent => ("\u{f017}", "○"),
            Origin::Bookmark { .. } => ("\u{f02e}", "★"),
            Origin::Device(VolumeKind::Removable) => ("\u{f287}", "⏏"),
            Origin::Device(VolumeKind::Network) => ("\u{f0ac}", "≈"),
            Origin::Device(VolumeKind::Device) | Origin::Unmounted(VolumeKind::Device) => {
                ("\u{f10b}", "▥")
            }
            Origin::Unmounted(VolumeKind::Removable) => ("\u{f287}", "⏏"),
            Origin::Device(_) | Origin::Unmounted(_) => ("\u{f0a0}", "▥"),
            // A dot in the tag's colour, in every set: it is the colour that says what it is.
            Origin::Tag(_) => ("●", "●"),
        };
        match set {
            IconSet::Nerd => nerd,
            IconSet::UnicodeIcons | IconSet::UnicodeStd => plain,
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

/// Everything the navigation pane lists, in order: Home, the pinned folders, the disks and
/// the trash. `places_only` is the narrow mode: pinned-by-hand folders and disks are left
/// out (they stay reachable with the jump palette).
pub fn items(app: &App, places_only: bool) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let place = |p: &rada_core::places::Place, section: Section| Item {
        section,
        origin: Origin::Place(p.kind),
        // The Finder's and Explorer's names ("Scrivania"), except for the two this pane names
        // itself.
        name: match p.kind {
            PlaceKind::Home | PlaceKind::Trash => p.name(),
            _ => app.shown_name(&p.path, p.name()),
        },
        locked: app.denied_for(&p.path).is_some(),
        path: p.path.clone(),
        usage: None,
    };
    for p in app
        .paths
        .places
        .iter()
        .filter(|p| p.kind == PlaceKind::Home)
    {
        out.push(place(p, Section::Home));
    }
    // The files used lately, right under Home, when the system has a list of them.
    if app.paths.recent_files {
        out.push(Item {
            section: Section::Home,
            origin: Origin::Recent,
            name: rada_core::i18n::Key::PlaceRecent.tr().to_string(),
            path: rada_core::recents::view_path(),
            usage: None,
            locked: false,
        });
    }
    for p in app
        .paths
        .places
        .iter()
        .filter(|p| !matches!(p.kind, PlaceKind::Home | PlaceKind::Trash))
    {
        out.push(place(p, Section::Pinned));
    }
    let trash = |out: &mut Vec<Item>| {
        for p in app
            .paths
            .places
            .iter()
            .filter(|p| p.kind == PlaceKind::Trash)
        {
            out.push(place(p, Section::Trash));
        }
    };
    if places_only {
        trash(&mut out);
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
        // A bookmark of a folder the pane already lists as a standard place (Movies, say) is
        // not listed a second time.
        if seen.contains(&path)
            || app
                .paths
                .places
                .iter()
                .any(|p| rada_core::places::same_folder(&p.path, path))
        {
            continue;
        }
        seen.push(path);
        // A bookmark in both places can be removed from the state, but would stay.
        let removable = removable && !app.config_bookmarks().contains(path);
        out.push(Item {
            section: Section::Pinned,
            origin: Origin::Bookmark { removable },
            name: app.shown_name(path, base_name(path)),
            locked: app.denied_for(path).is_some(),
            path: path.clone(),
            usage: None,
        });
    }

    for v in app.volumes.iter().filter(|v| v.kind != VolumeKind::Virtual) {
        let name = app.shown_name(&v.mount_point, device_name(v));
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
            locked: app.denied_for(&v.mount_point).is_some(),
            path: v.mount_point.clone(),
            usage,
        });
    }
    for u in app.pending_devices() {
        out.push(Item {
            section: Section::Devices,
            origin: Origin::Unmounted(u.kind),
            name: u.name.clone(),
            locked: false,
            path: PathBuf::from(&u.id),
            usage: None,
        });
    }
    if app.tags_supported() {
        // The seven colours always, in the Finder's order and called what the system calls
        // them; then the user's own tags seen in the folders opened so far.
        let tag = |name: String, color: Option<Hue>| Item {
            section: Section::Tags,
            origin: Origin::Tag(color),
            path: rada_core::tagsearch::view_path(&name),
            name,
            usage: None,
            locked: false,
        };
        for c in Hue::ALL {
            out.push(tag(c.label(&app.tag_lang).to_string(), Some(c)));
        }
        for t in &app.known_tags {
            out.push(tag(t.name.clone(), t.color));
        }
    }
    trash(&mut out);
    out
}

/// A disk's name in the sidebar: its label, "System" for the root, else its mount folder.
pub fn device_name(v: &rada_core::platform::Volume) -> String {
    v.label.clone().unwrap_or_else(|| {
        if v.mount_point == Path::new(std::path::MAIN_SEPARATOR_STR) {
            rada_core::i18n::Key::DiskSystem.tr().to_string()
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
    fn the_navigation_pane_shrinks_to_icons_and_then_goes() {
        assert_eq!(Mode::for_width(200, true), Mode::Full);
        assert_eq!(Mode::for_width(100, true), Mode::Full);
        assert_eq!(Mode::for_width(99, true), Mode::Rail);
        assert_eq!(Mode::for_width(60, true), Mode::Rail);
        assert_eq!(Mode::for_width(59, true), Mode::Hidden);
        assert_eq!(Mode::for_width(200, false), Mode::Hidden);
        // Whatever the pane takes, a usable list is left.
        assert!(100 - Mode::Full.columns() >= 60);
        assert!(60 - Mode::Rail.columns() >= 40);
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
