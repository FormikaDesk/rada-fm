//! Actions and the keys that trigger them.
//!
//! Every thing the browser can do is an [`Action`]. A [`Keymap`] maps key chords to
//! actions. Two schemes are built in and active together by default: vim-style keys and
//! the usual desktop shortcuts. The configuration can choose a preset
//! (`vim+classic`, `vim`, `classic`) and rebind individual actions.

use std::collections::HashMap;
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a key (or a click on a hint, or a menu entry) asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    // moving
    Up,
    Down,
    Parent,
    Open,
    First,
    Last,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    // selecting
    ToggleMark,
    SelectAll,
    SelectUp,
    SelectDown,
    SelectToFirst,
    SelectToLast,
    ClearSelection,
    // working with files
    Copy,
    Cut,
    Paste,
    Trash,
    DeletePermanently,
    Rename,
    BulkRename,
    NewFolder,
    Undo,
    Redo,
    History,
    // looking
    Filter,
    Palette,
    Sort,
    SortReverse,
    ToggleHidden,
    ToggleHex,
    PreviewDown,
    PreviewUp,
    Bookmark,
    GoHome,
    // the program
    Help,
    Quit,
}

/// Where an action appears in the help, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Move,
    Select,
    Files,
    View,
    Program,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::Move,
        Group::Select,
        Group::Files,
        Group::View,
        Group::Program,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Move => "Moving around",
            Group::Select => "Selecting",
            Group::Files => "Working with files",
            Group::View => "Looking",
            Group::Program => "The program",
        }
    }
}

impl Action {
    pub const ALL: [Action; 40] = [
        Action::Up,
        Action::Down,
        Action::Parent,
        Action::Open,
        Action::First,
        Action::Last,
        Action::PageUp,
        Action::PageDown,
        Action::HalfPageUp,
        Action::HalfPageDown,
        Action::ToggleMark,
        Action::SelectAll,
        Action::SelectUp,
        Action::SelectDown,
        Action::SelectToFirst,
        Action::SelectToLast,
        Action::ClearSelection,
        Action::Copy,
        Action::Cut,
        Action::Paste,
        Action::Trash,
        Action::DeletePermanently,
        Action::Rename,
        Action::BulkRename,
        Action::NewFolder,
        Action::Undo,
        Action::Redo,
        Action::History,
        Action::Filter,
        Action::Palette,
        Action::Sort,
        Action::SortReverse,
        Action::ToggleHidden,
        Action::ToggleHex,
        Action::PreviewDown,
        Action::PreviewUp,
        Action::Bookmark,
        Action::GoHome,
        Action::Help,
        Action::Quit,
    ];

    /// The name used in the configuration file.
    pub fn id(self) -> &'static str {
        match self {
            Action::Up => "up",
            Action::Down => "down",
            Action::Parent => "parent",
            Action::Open => "open",
            Action::First => "first",
            Action::Last => "last",
            Action::PageUp => "page_up",
            Action::PageDown => "page_down",
            Action::HalfPageUp => "half_page_up",
            Action::HalfPageDown => "half_page_down",
            Action::ToggleMark => "toggle_mark",
            Action::SelectAll => "select_all",
            Action::SelectUp => "select_up",
            Action::SelectDown => "select_down",
            Action::SelectToFirst => "select_to_first",
            Action::SelectToLast => "select_to_last",
            Action::ClearSelection => "clear_selection",
            Action::Copy => "copy",
            Action::Cut => "cut",
            Action::Paste => "paste",
            Action::Trash => "trash",
            Action::DeletePermanently => "delete_permanently",
            Action::Rename => "rename",
            Action::BulkRename => "bulk_rename",
            Action::NewFolder => "new_folder",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::History => "history",
            Action::Filter => "filter",
            Action::Palette => "palette",
            Action::Sort => "sort",
            Action::SortReverse => "sort_reverse",
            Action::ToggleHidden => "toggle_hidden",
            Action::ToggleHex => "toggle_hex",
            Action::PreviewDown => "preview_down",
            Action::PreviewUp => "preview_up",
            Action::Bookmark => "bookmark",
            Action::GoHome => "go_home",
            Action::Help => "help",
            Action::Quit => "quit",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }

    /// Short human description (help, context menu, palette).
    pub fn label(self) -> &'static str {
        match self {
            Action::Up => "Move up",
            Action::Down => "Move down",
            Action::Parent => "Parent folder",
            Action::Open => "Open",
            Action::First => "First item",
            Action::Last => "Last item",
            Action::PageUp => "Page up",
            Action::PageDown => "Page down",
            Action::HalfPageUp => "Half page up",
            Action::HalfPageDown => "Half page down",
            Action::ToggleMark => "Select / unselect item",
            Action::SelectAll => "Select all",
            Action::SelectUp => "Extend selection up",
            Action::SelectDown => "Extend selection down",
            Action::SelectToFirst => "Extend selection to start",
            Action::SelectToLast => "Extend selection to end",
            Action::ClearSelection => "Clear selection / cancel",
            Action::Copy => "Copy",
            Action::Cut => "Cut",
            Action::Paste => "Paste",
            Action::Trash => "Move to trash",
            Action::DeletePermanently => "Delete permanently",
            Action::Rename => "Rename",
            Action::BulkRename => "Rename many",
            Action::NewFolder => "New folder",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::History => "Operation history",
            Action::Filter => "Filter this folder",
            Action::Palette => "Jump to…",
            Action::Sort => "Change sort order",
            Action::SortReverse => "Reverse sort",
            Action::ToggleHidden => "Show / hide hidden files",
            Action::ToggleHex => "Hex dump of a binary file",
            Action::PreviewDown => "Scroll preview down",
            Action::PreviewUp => "Scroll preview up",
            Action::Bookmark => "Bookmark this folder",
            Action::GoHome => "Go to home",
            Action::Help => "Help",
            Action::Quit => "Quit",
        }
    }

    pub fn group(self) -> Group {
        use Action::*;
        match self {
            Up | Down | Parent | Open | First | Last | PageUp | PageDown | HalfPageUp
            | HalfPageDown | GoHome => Group::Move,
            ToggleMark | SelectAll | SelectUp | SelectDown | SelectToFirst | SelectToLast
            | ClearSelection => Group::Select,
            Copy | Cut | Paste | Trash | DeletePermanently | Rename | BulkRename | NewFolder
            | Undo | Redo | History => Group::Files,
            Filter | Palette | Sort | SortReverse | ToggleHidden | ToggleHex | PreviewDown
            | PreviewUp | Bookmark => Group::View,
            Help | Quit => Group::Program,
        }
    }
}

/// A key with its modifiers, normalised so that `G` and `shift+g` are the same thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Chord {
    pub fn new(code: KeyCode, mods: KeyModifiers) -> Chord {
        let mut mods = mods & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let code = match code {
            KeyCode::Char(c) if c.is_alphabetic() => {
                // Terminals disagree on shifted letters: `G`, `G`+SHIFT and `g`+SHIFT all
                // mean the same key. With Ctrl held the letter stays lower case and
                // Shift stays a modifier (Ctrl+Shift+Z).
                let shifted = mods.contains(KeyModifiers::SHIFT) || c.is_uppercase();
                let lower = c.to_lowercase().next().unwrap_or(c);
                let upper = c.to_uppercase().next().unwrap_or(c);
                if mods.contains(KeyModifiers::CONTROL) {
                    mods.set(KeyModifiers::SHIFT, shifted);
                    KeyCode::Char(lower)
                } else if shifted {
                    mods.remove(KeyModifiers::SHIFT);
                    KeyCode::Char(upper)
                } else {
                    KeyCode::Char(c)
                }
            }
            KeyCode::Char(c) => {
                // Symbols already carry their shift (`?`, `~`): SHIFT adds nothing.
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::Char(c)
            }
            // BackTab is Shift+Tab.
            KeyCode::BackTab => {
                mods.insert(KeyModifiers::SHIFT);
                KeyCode::Tab
            }
            other => other,
        };
        Chord { code, mods }
    }

    pub fn from_event(k: &KeyEvent) -> Chord {
        Chord::new(k.code, k.modifiers)
    }

    /// Parse `ctrl+shift+delete`, `F2`, `G`, `space`, `ctrl+a`…
    pub fn parse(text: &str) -> Result<Chord, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty key".into());
        }
        // The key is whatever follows the last `+` (or a lone `+`).
        let (mods_part, key_part) = match text.rfind('+') {
            Some(0) | None => ("", text),
            Some(i) if i + 1 == text.len() => (text[..i].trim_end_matches('+'), "+"),
            Some(i) => (&text[..i], &text[i + 1..]),
        };
        let mut mods = KeyModifiers::NONE;
        for m in mods_part.split('+').filter(|m| !m.is_empty()) {
            mods |= match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "c" => KeyModifiers::CONTROL,
                "alt" | "meta" | "opt" | "option" => KeyModifiers::ALT,
                "shift" | "s" => KeyModifiers::SHIFT,
                other => return Err(format!("unknown modifier `{other}` in `{text}`")),
            };
        }
        let lower = key_part.to_ascii_lowercase();
        let code = match lower.as_str() {
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "space" => KeyCode::Char(' '),
            "backspace" | "bs" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "insert" | "ins" => KeyCode::Insert,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" | "pgdown" => KeyCode::PageDown,
            f if f.starts_with('f')
                && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) =>
            {
                KeyCode::F(f[1..].parse().unwrap())
            }
            _ => {
                let mut cs = key_part.chars();
                match (cs.next(), cs.next()) {
                    (Some(c), None) => KeyCode::Char(c),
                    _ => return Err(format!("unknown key `{key_part}` in `{text}`")),
                }
            }
        };
        Ok(Chord::new(code, mods))
    }
}

impl fmt::Display for Chord {
    /// The form shown in help and hints: `Ctrl+C`, `Shift+Del`, `G`, `F2`, `↑`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.contains(KeyModifiers::CONTROL) {
            f.write_str("Ctrl+")?;
        }
        if self.mods.contains(KeyModifiers::ALT) {
            f.write_str("Alt+")?;
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            f.write_str("Shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) if self.mods.contains(KeyModifiers::CONTROL) => {
                write!(f, "{}", c.to_ascii_uppercase())
            }
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Esc => f.write_str("Esc"),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::Backspace => f.write_str("Backspace"),
            KeyCode::Delete => f.write_str("Del"),
            KeyCode::Insert => f.write_str("Ins"),
            KeyCode::Up => f.write_str("↑"),
            KeyCode::Down => f.write_str("↓"),
            KeyCode::Left => f.write_str("←"),
            KeyCode::Right => f.write_str("→"),
            KeyCode::Home => f.write_str("Home"),
            KeyCode::End => f.write_str("End"),
            KeyCode::PageUp => f.write_str("PgUp"),
            KeyCode::PageDown => f.write_str("PgDn"),
            KeyCode::F(n) => write!(f, "F{n}"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// Which family a binding belongs to (the help shows them side by side).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Vim,
    Classic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    VimClassic,
    Vim,
    Classic,
}

impl Preset {
    pub const NAMES: [&'static str; 3] = ["vim+classic", "vim", "classic"];

    pub fn parse(s: &str) -> Option<Preset> {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '_'], "")
            .as_str()
        {
            "vim+classic" | "classic+vim" | "both" | "default" => Some(Preset::VimClassic),
            "vim" | "vimonly" | "vim-only" => Some(Preset::Vim),
            "classic" | "classiconly" | "classic-only" => Some(Preset::Classic),
            _ => None,
        }
    }

    fn has(self, s: Scheme) -> bool {
        matches!(
            (self, s),
            (Preset::VimClassic, _)
                | (Preset::Vim, Scheme::Vim)
                | (Preset::Classic, Scheme::Classic)
        )
    }
}

/// Vim-style bindings.
const VIM: &[(Action, &[&str])] = &[
    (Action::Up, &["k", "up"]),
    (Action::Down, &["j", "down"]),
    (Action::Parent, &["h", "left", "backspace"]),
    (Action::Open, &["l", "right", "enter"]),
    (Action::First, &["g", "home"]),
    (Action::Last, &["G", "end"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::HalfPageUp, &["ctrl+u"]),
    (Action::HalfPageDown, &["ctrl+d"]),
    (Action::ToggleMark, &["space"]),
    (Action::ClearSelection, &["esc"]),
    (Action::Copy, &["y"]),
    (Action::Cut, &["x"]),
    (Action::Paste, &["p"]),
    (Action::Trash, &["d"]),
    (Action::DeletePermanently, &["D"]),
    (Action::Rename, &["r"]),
    (Action::BulkRename, &["R"]),
    (Action::NewFolder, &["n"]),
    (Action::Undo, &["u"]),
    (Action::Redo, &["ctrl+r"]),
    (Action::History, &["U"]),
    (Action::Filter, &["/"]),
    (Action::Palette, &["m"]),
    (Action::Sort, &["s"]),
    (Action::SortReverse, &["S"]),
    (Action::ToggleHidden, &["."]),
    (Action::ToggleHex, &["H"]),
    (Action::PreviewDown, &["J"]),
    (Action::PreviewUp, &["K"]),
    (Action::Bookmark, &["B"]),
    (Action::GoHome, &["~"]),
    (Action::Help, &["?"]),
    (Action::Quit, &["q"]),
];

/// The usual desktop shortcuts.
const CLASSIC: &[(Action, &[&str])] = &[
    (Action::Up, &["up"]),
    (Action::Down, &["down"]),
    (Action::Parent, &["backspace", "left", "alt+up"]),
    (Action::Open, &["enter", "right"]),
    (Action::First, &["home"]),
    (Action::Last, &["end"]),
    (Action::PageUp, &["pageup"]),
    (Action::PageDown, &["pagedown"]),
    (Action::ToggleMark, &["space", "ctrl+space", "insert"]),
    (Action::SelectAll, &["ctrl+a"]),
    (Action::SelectUp, &["shift+up"]),
    (Action::SelectDown, &["shift+down"]),
    (Action::SelectToFirst, &["shift+home", "ctrl+shift+home"]),
    (Action::SelectToLast, &["shift+end", "ctrl+shift+end"]),
    (Action::ClearSelection, &["esc"]),
    (Action::Copy, &["ctrl+c"]),
    (Action::Cut, &["ctrl+x"]),
    (Action::Paste, &["ctrl+v"]),
    (Action::Trash, &["delete"]),
    (Action::DeletePermanently, &["shift+delete"]),
    (Action::Rename, &["f2"]),
    (Action::NewFolder, &["ctrl+n", "f7"]),
    (Action::Undo, &["ctrl+z"]),
    (Action::Redo, &["ctrl+y", "ctrl+shift+z"]),
    (Action::History, &["f3"]),
    (Action::Filter, &["ctrl+f"]),
    (Action::Palette, &["ctrl+p", "ctrl+l"]),
    (Action::Help, &["f1"]),
    (Action::Quit, &["ctrl+q"]),
];

/// Keys that mean the same in both schemes and are bound whatever the preset.
const ALWAYS: &[(Action, &[&str])] = &[(Action::Palette, &["ctrl+p"]), (Action::Help, &["f1"])];

#[derive(Clone, Debug)]
pub struct Binding {
    pub chord: Chord,
    /// Belongs to the vim scheme / the classic scheme (arrows and Enter belong to both).
    pub vim: bool,
    pub classic: bool,
    /// Set by the user's configuration.
    pub custom: bool,
}

impl Binding {
    fn in_scheme(&self, s: Scheme) -> bool {
        match s {
            Scheme::Vim => self.vim,
            Scheme::Classic => self.classic,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Keymap {
    by_chord: HashMap<Chord, Action>,
    by_action: HashMap<Action, Vec<Binding>>,
    pub preset: Preset,
    /// Problems found in the configuration (shown once at start).
    pub warnings: Vec<String>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::new(Preset::VimClassic, &HashMap::new())
    }
}

impl Keymap {
    /// Build a keymap from a preset and per-action overrides (`action id -> keys`).
    /// An override replaces *all* the keys of that action; an empty list unbinds it.
    /// A key given to an action is taken away from any other action.
    pub fn new(preset: Preset, overrides: &HashMap<String, Vec<String>>) -> Keymap {
        let mut km = Keymap {
            by_chord: HashMap::new(),
            by_action: HashMap::new(),
            preset,
            warnings: Vec::new(),
        };
        for (scheme, table) in [(Scheme::Vim, VIM), (Scheme::Classic, CLASSIC)] {
            if !preset.has(scheme) {
                continue;
            }
            for (action, keys) in table {
                for k in *keys {
                    km.bind(
                        *action,
                        Chord::parse(k).expect("built-in key"),
                        scheme,
                        false,
                    );
                }
            }
        }
        for (action, keys) in ALWAYS {
            for k in *keys {
                let c = Chord::parse(k).expect("built-in key");
                if !km.by_chord.contains_key(&c) {
                    km.bind(*action, c, Scheme::Classic, false);
                }
            }
        }
        let mut ids: Vec<&String> = overrides.keys().collect();
        ids.sort();
        for id in ids {
            let Some(action) = Action::from_id(id) else {
                km.warnings
                    .push(format!("[keys]: unknown action `{id}` (ignored)"));
                continue;
            };
            let mut chords = Vec::new();
            for k in &overrides[id] {
                match Chord::parse(k) {
                    Ok(c) => chords.push(c),
                    Err(e) => km.warnings.push(format!("[keys] {id}: {e}")),
                }
            }
            km.unbind_action(action);
            for c in chords {
                // Taking a key from another action is allowed, and said out loud.
                if let Some(prev) = km.by_chord.get(&c).copied()
                    && prev != action
                {
                    km.warnings.push(format!(
                        "[keys]: {c} now means “{}” (it was “{}”)",
                        action.label(),
                        prev.label()
                    ));
                    km.unbind_chord(c);
                }
                let scheme = if is_vim_like(c) {
                    Scheme::Vim
                } else {
                    Scheme::Classic
                };
                km.bind(action, c, scheme, true);
            }
        }
        km
    }

    fn bind(&mut self, action: Action, chord: Chord, scheme: Scheme, custom: bool) {
        // A key in both tables (arrows, Enter, Esc) keeps its first owner and is marked
        // as belonging to both schemes.
        let (vim, classic) = (scheme == Scheme::Vim, scheme == Scheme::Classic);
        if let Some(existing) = self.by_chord.get(&chord) {
            if *existing == action
                && let Some(b) = self
                    .by_action
                    .get_mut(&action)
                    .and_then(|l| l.iter_mut().find(|b| b.chord == chord))
            {
                b.vim |= vim;
                b.classic |= classic;
            }
            return;
        }
        self.by_chord.insert(chord, action);
        self.by_action.entry(action).or_default().push(Binding {
            chord,
            vim,
            classic,
            custom,
        });
    }

    fn unbind_action(&mut self, action: Action) {
        if let Some(list) = self.by_action.remove(&action) {
            for b in list {
                self.by_chord.remove(&b.chord);
            }
        }
    }

    fn unbind_chord(&mut self, chord: Chord) {
        if let Some(action) = self.by_chord.remove(&chord)
            && let Some(list) = self.by_action.get_mut(&action)
        {
            list.retain(|b| b.chord != chord);
        }
    }

    pub fn action_for(&self, key: &KeyEvent) -> Option<Action> {
        self.by_chord.get(&Chord::from_event(key)).copied()
    }

    pub fn bindings(&self, action: Action) -> &[Binding] {
        self.by_action
            .get(&action)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The key to show next to an action in menus and hints, e.g. `Ctrl+C`.
    /// Prefers the classic one when both exist (it is the more readable).
    pub fn hint(&self, action: Action) -> Option<String> {
        let list = self.bindings(action);
        list.iter()
            .find(|b| b.custom)
            .or_else(|| {
                list.iter()
                    .filter(|b| b.classic)
                    .min_by_key(|b| nav_rank(b.chord))
            })
            .or_else(|| list.first())
            .map(|b| b.chord.to_string())
    }

    /// Keys of one scheme for the help, `/`-joined.
    pub fn keys_text(&self, action: Action, scheme: Scheme) -> String {
        let mut keys: Vec<String> = self
            .bindings(action)
            .iter()
            .filter(|b| b.in_scheme(scheme))
            .map(|b| b.chord.to_string())
            .collect();
        keys.dedup();
        keys.join(" ")
    }
}

/// A bare printable key or a capital: what vim users type.
fn is_vim_like(c: Chord) -> bool {
    matches!(c.code, KeyCode::Char(_))
        && !c.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Which of several shortcuts reads best in a hint: ordinary keys first, then Enter,
/// Backspace and the arrows (an arrow alone says little about what it does).
fn nav_rank(c: Chord) -> u8 {
    if !c.mods.is_empty() {
        return 0;
    }
    match c.code {
        KeyCode::Enter => 1,
        KeyCode::Backspace => 2,
        KeyCode::Right => 3,
        KeyCode::Left | KeyCode::Up | KeyCode::Down => 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn chords_parse_and_print() {
        assert_eq!(Chord::parse("ctrl+c").unwrap().to_string(), "Ctrl+C");
        assert_eq!(
            Chord::parse("Shift+Delete").unwrap().to_string(),
            "Shift+Del"
        );
        assert_eq!(Chord::parse("F2").unwrap().to_string(), "F2");
        assert_eq!(
            Chord::parse("G").unwrap(),
            Chord::new(KeyCode::Char('G'), KeyModifiers::NONE)
        );
        assert_eq!(Chord::parse("shift+g").unwrap(), Chord::parse("G").unwrap());
        assert_eq!(Chord::parse("+").unwrap().code, KeyCode::Char('+'));
        assert_eq!(Chord::parse("ctrl++").unwrap().mods, KeyModifiers::CONTROL);
        assert!(Chord::parse("hyper+x").is_err());
        assert!(Chord::parse("").is_err());
        assert!(Chord::parse("f99").is_err());
    }

    #[test]
    fn terminals_report_shifted_letters_differently_and_all_work() {
        let km = Keymap::default();
        // Kitty-protocol style: Char('G') + SHIFT. Legacy: Char('G') alone. Odd: Char('g') + SHIFT.
        for e in [
            ev(KeyCode::Char('G'), KeyModifiers::SHIFT),
            ev(KeyCode::Char('G'), KeyModifiers::NONE),
            ev(KeyCode::Char('g'), KeyModifiers::SHIFT),
        ] {
            assert_eq!(km.action_for(&e), Some(Action::Last), "{e:?}");
        }
        assert_eq!(
            km.action_for(&ev(KeyCode::Char('?'), KeyModifiers::SHIFT)),
            Some(Action::Help)
        );
        assert_eq!(
            km.action_for(&ev(KeyCode::BackTab, KeyModifiers::SHIFT)),
            None
        );
    }

    #[test]
    fn ctrl_shift_letters_are_recognised_however_the_terminal_reports_them() {
        let km = Keymap::default();
        for e in [
            ev(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            ev(
                KeyCode::Char('Z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            ev(KeyCode::Char('Z'), KeyModifiers::CONTROL),
        ] {
            assert_eq!(km.action_for(&e), Some(Action::Redo), "{e:?}");
        }
        // Plain Ctrl+Z stays undo.
        assert_eq!(
            km.action_for(&ev(KeyCode::Char('z'), KeyModifiers::CONTROL)),
            Some(Action::Undo)
        );
    }

    #[test]
    fn default_keeps_both_schemes_alive_together() {
        let km = Keymap::default();
        let act = |c: &str| {
            km.action_for(&KeyEvent::new(
                Chord::parse(c).unwrap().code,
                Chord::parse(c).unwrap().mods,
            ))
        };
        assert_eq!(act("y"), Some(Action::Copy));
        assert_eq!(act("ctrl+c"), Some(Action::Copy));
        assert_eq!(act("ctrl+x"), Some(Action::Cut));
        assert_eq!(act("ctrl+v"), Some(Action::Paste));
        assert_eq!(act("ctrl+z"), Some(Action::Undo));
        assert_eq!(act("ctrl+y"), Some(Action::Redo));
        assert_eq!(act("ctrl+r"), Some(Action::Redo));
        assert_eq!(act("delete"), Some(Action::Trash));
        assert_eq!(act("shift+delete"), Some(Action::DeletePermanently));
        assert_eq!(act("f2"), Some(Action::Rename));
        assert_eq!(act("ctrl+f"), Some(Action::Filter));
        assert_eq!(act("/"), Some(Action::Filter));
        assert_eq!(act("ctrl+q"), Some(Action::Quit));
        assert_eq!(act("q"), Some(Action::Quit));
        assert_eq!(act("ctrl+a"), Some(Action::SelectAll));
        assert_eq!(act("ctrl+l"), Some(Action::Palette));
        assert_eq!(act("f1"), Some(Action::Help));
    }

    #[test]
    fn ctrl_c_is_never_quit_in_any_preset() {
        for p in [Preset::VimClassic, Preset::Vim, Preset::Classic] {
            let km = Keymap::new(p, &HashMap::new());
            assert_ne!(
                km.action_for(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)),
                Some(Action::Quit),
                "{p:?}"
            );
        }
    }

    #[test]
    fn presets_restrict_the_schemes() {
        let vim = Keymap::new(Preset::Vim, &HashMap::new());
        assert_eq!(
            vim.action_for(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            vim.action_for(&ev(KeyCode::Char('y'), KeyModifiers::NONE)),
            Some(Action::Copy)
        );
        // the two keys that are bound whatever the preset
        assert_eq!(
            vim.action_for(&ev(KeyCode::F(1), KeyModifiers::NONE)),
            Some(Action::Help)
        );
        let classic = Keymap::new(Preset::Classic, &HashMap::new());
        assert_eq!(
            classic.action_for(&ev(KeyCode::Char('y'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            classic.action_for(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Copy)
        );
        assert_eq!(
            classic.action_for(&ev(KeyCode::Char('q'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn every_action_is_reachable_in_every_preset_except_the_scheme_specific_ones() {
        for p in [Preset::VimClassic, Preset::Vim, Preset::Classic] {
            let km = Keymap::new(p, &HashMap::new());
            for a in Action::ALL {
                let reachable = !km.bindings(a).is_empty();
                // Selection extension is a classic-only idea (vim selects with space).
                let classic_only = matches!(
                    a,
                    Action::SelectAll
                        | Action::SelectUp
                        | Action::SelectDown
                        | Action::SelectToFirst
                        | Action::SelectToLast
                );
                // Vim has no bulk-rename in classic, etc.: those stay reachable through the palette/menu.
                let vim_only = matches!(
                    a,
                    Action::BulkRename
                        | Action::Sort
                        | Action::SortReverse
                        | Action::ToggleHidden
                        | Action::ToggleHex
                        | Action::PreviewDown
                        | Action::PreviewUp
                        | Action::Bookmark
                        | Action::GoHome
                        | Action::HalfPageUp
                        | Action::HalfPageDown
                );
                if p == Preset::VimClassic {
                    assert!(reachable, "{a:?} has no key in the default preset");
                } else if (p == Preset::Vim && classic_only) || (p == Preset::Classic && vim_only) {
                    // allowed to be unbound; still available via the palette
                } else {
                    assert!(reachable, "{a:?} has no key in {p:?}");
                }
            }
        }
    }

    #[test]
    fn ids_round_trip() {
        for a in Action::ALL {
            assert_eq!(Action::from_id(a.id()), Some(a));
        }
        assert_eq!(Action::ALL.len(), 40);
        let mut ids: Vec<_> = Action::ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Action::ALL.len());
    }

    #[test]
    fn overrides_replace_unbind_and_steal_keys() {
        let mut o: HashMap<String, Vec<String>> = HashMap::new();
        o.insert("copy".into(), vec!["c".into()]);
        o.insert("quit".into(), vec![]);
        o.insert(
            "undo".into(),
            vec!["ctrl+z".into(), "bogus+key".into(), "f5".into()],
        );
        o.insert("fly".into(), vec!["f".into()]);
        let km = Keymap::new(Preset::VimClassic, &o);
        let k = |c: &str| {
            km.action_for(&KeyEvent::new(
                Chord::parse(c).unwrap().code,
                Chord::parse(c).unwrap().mods,
            ))
        };
        assert_eq!(k("c"), Some(Action::Copy));
        assert_eq!(
            k("y"),
            None,
            "the old keys of an overridden action are gone"
        );
        assert_eq!(k("ctrl+c"), None);
        assert_eq!(k("q"), None);
        assert_eq!(k("ctrl+q"), None);
        assert_eq!(k("f5"), Some(Action::Undo));
        assert_eq!(k("u"), None);
        assert!(km.warnings.iter().any(|w| w.contains("fly")));
        assert!(km.warnings.iter().any(|w| w.contains("bogus")));
        // stealing a key is reported
        let mut o = HashMap::new();
        o.insert("copy".into(), vec!["d".into()]);
        let km = Keymap::new(Preset::VimClassic, &o);
        assert_eq!(
            km.action_for(&ev(KeyCode::Char('d'), KeyModifiers::NONE)),
            Some(Action::Copy)
        );
        assert_eq!(
            km.bindings(Action::Trash)
                .iter()
                .filter(|b| b.chord.code == KeyCode::Char('d'))
                .count(),
            0
        );
        assert!(km.warnings.iter().any(|w| w.contains("now means")));
    }

    #[test]
    fn hints_prefer_readable_shortcuts() {
        let km = Keymap::default();
        assert_eq!(km.hint(Action::Copy).as_deref(), Some("Ctrl+C"));
        assert_eq!(km.hint(Action::Trash).as_deref(), Some("Del"));
        let vim = Keymap::new(Preset::Vim, &HashMap::new());
        assert_eq!(vim.hint(Action::Copy).as_deref(), Some("y"));
        assert_eq!(km.keys_text(Action::Copy, Scheme::Vim), "y");
        assert_eq!(km.keys_text(Action::Copy, Scheme::Classic), "Ctrl+C");
    }
}
