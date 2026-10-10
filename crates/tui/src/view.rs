//! Small choices about how the main area looks: details or icons, the explorer layout or
//! the compact one.

/// How a folder is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    /// A table: name, date, type, size.
    #[default]
    Details,
    /// A grid of tiles: a big icon and the name under it.
    Icons,
}

impl ViewMode {
    pub fn parse(s: &str) -> Option<ViewMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "details" | "detail" | "list" => Some(ViewMode::Details),
            "icons" | "icon" | "grid" => Some(ViewMode::Icons),
            _ => None,
        }
    }

    /// The word used in the configuration and in the saved state.
    pub fn id(self) -> &'static str {
        match self {
            ViewMode::Details => "details",
            ViewMode::Icons => "icons",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Details => "Details",
            ViewMode::Icons => "Icons",
        }
    }

    pub fn other(self) -> ViewMode {
        match self {
            ViewMode::Details => ViewMode::Icons,
            ViewMode::Icons => ViewMode::Details,
        }
    }
}

/// The whole arrangement of the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutKind {
    /// Tabs, address bar, command bar, navigation pane, details pane, hints.
    #[default]
    Explorer,
    /// Only the list and the status line, for the most room.
    Compact,
}

impl LayoutKind {
    pub fn parse(s: &str) -> Option<LayoutKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "explorer" => Some(LayoutKind::Explorer),
            "compact" => Some(LayoutKind::Compact),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for v in [ViewMode::Details, ViewMode::Icons] {
            assert_eq!(ViewMode::parse(v.id()), Some(v));
            assert_ne!(v.other(), v);
        }
        assert_eq!(ViewMode::parse("nonsense"), None);
        assert_eq!(LayoutKind::parse("Compact"), Some(LayoutKind::Compact));
        assert_eq!(LayoutKind::parse("explorer"), Some(LayoutKind::Explorer));
        assert_eq!(LayoutKind::parse("x"), None);
    }
}
