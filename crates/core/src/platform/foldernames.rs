//! The names the Finder gives to the folders every Mac has: `Desktop` is "Scrivania" on an
//! Italian system, `Movies` is "Filmati". On disk the folders keep their English names; a
//! `.localized` marker inside tells the Finder to show the name of its language.
//!
//! Only the *shown* name changes: paths, search, copy, move, rename and the command line
//! keep the real name. The language is the one of the interface: English shows the names
//! as they are.
//!
//! Where the name comes from, in order: the system's own table
//! (`SystemFolderLocalizations/<lang>.lproj/SystemFolderLocalizations.strings`), then a short
//! fixed table for Italian as a reserve for when that file is missing or unreadable.
//! The module has no system calls of its own, so it is tested on any machine.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::i18n::Lang;

/// Where macOS keeps the tables of folder names.
pub const SYSTEM_TABLES: &str = "/System/Library/CoreServices/SystemFolderLocalizations";

/// The folders that have a localized name, by their name on disk.
pub const FOLDERS: [&str; 9] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Pictures",
    "Music",
    "Movies",
    "Public",
    "Applications",
    "Library",
];

/// The reserve: what the Italian Finder shows. Used for a folder the system table does not
/// name (or when it cannot be read).
const ITALIAN: [(&str, &str); 9] = [
    ("Desktop", "Scrivania"),
    ("Documents", "Documenti"),
    ("Downloads", "Download"),
    ("Pictures", "Immagini"),
    ("Music", "Musica"),
    ("Movies", "Filmati"),
    ("Public", "Pubblica"),
    ("Applications", "Applicazioni"),
    ("Library", "Libreria"),
];

/// The names of one language, by the name on disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderNames {
    names: HashMap<String, String>,
}

impl FolderNames {
    /// The names for `lang`: the system's table in `tables` first, the fixed one for what it
    /// leaves out. English has none.
    pub fn for_lang(lang: Lang, tables: &Path) -> FolderNames {
        let mut names = HashMap::new();
        if lang != Lang::En {
            let file = tables
                .join(format!("{}.lproj", lang.code()))
                .join("SystemFolderLocalizations.strings");
            if let Some(table) = std::fs::read(&file).ok().and_then(|b| parse_strings(&b)) {
                for f in FOLDERS {
                    if let Some(v) = table.get(f).filter(|v| usable(v)) {
                        names.insert(f.to_string(), v.clone());
                    }
                }
            }
            if lang == Lang::It {
                for (disk, shown) in ITALIAN {
                    names
                        .entry(disk.to_string())
                        .or_insert_with(|| shown.to_string());
                }
            }
        }
        FolderNames { names }
    }

    /// The names for the language the interface speaks now, the system's tables being where
    /// macOS keeps them. Read once per language.
    pub fn current() -> Arc<FolderNames> {
        Self::current_with(Path::new(SYSTEM_TABLES))
    }

    /// [`current`](Self::current) with the tables somewhere else (for tests, which must not
    /// depend on the machine). The answer is kept per language *and* folder of tables.
    pub fn current_with(tables: &Path) -> Arc<FolderNames> {
        type Cache = Mutex<HashMap<(Lang, std::path::PathBuf), Arc<FolderNames>>>;
        static CACHE: OnceLock<Cache> = OnceLock::new();
        let lang = crate::i18n::lang();
        let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
        cache
            .entry((lang, tables.to_path_buf()))
            .or_insert_with(|| Arc::new(FolderNames::for_lang(lang, tables)))
            .clone()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The name of `disk` in this language, if it is one of the folders.
    pub fn name_of(&self, disk: &str) -> Option<&str> {
        self.names.get(disk).map(String::as_str)
    }

    /// What the Finder shows for `path`: only for a folder that stands where the system puts
    /// them (in the home folder, or at the root for `Applications` and `Library`) and that
    /// carries the `.localized` marker, as the real ones do. `None`: the name on disk.
    pub fn shown(&self, path: &Path, home: &Path) -> Option<String> {
        if self.names.is_empty() {
            return None;
        }
        let disk = path.file_name()?.to_str()?;
        let shown = self.names.get(disk)?;
        let parent = path.parent()?;
        let at_root = parent == Path::new("/") && matches!(disk, "Applications" | "Library");
        if parent != home && !at_root {
            return None;
        }
        // A folder made by hand with the same name is not the system's: it keeps its name.
        path.join(".localized").is_file().then(|| shown.clone())
    }
}

/// Whether `path` stands where the system puts one of the folders with a localized name,
/// whether or not it has a localized name in this language.
pub fn is_system_folder(path: &Path, home: &Path) -> bool {
    let (Some(disk), Some(parent)) = (path.file_name().and_then(|n| n.to_str()), path.parent())
    else {
        return false;
    };
    FOLDERS.contains(&disk)
        && (parent == home
            || (parent == Path::new("/") && matches!(disk, "Applications" | "Library")))
}

/// A name worth showing: not empty and not the control characters the table might hold.
fn usable(v: &str) -> bool {
    !v.trim().is_empty() && !v.chars().any(char::is_control)
}

/// A `.strings` file: a binary or XML property list, or the old text form
/// `"key" = "value";` (UTF-8 or UTF-16).
pub fn parse_strings(bytes: &[u8]) -> Option<HashMap<String, String>> {
    if let Some(d) = super::bplist::decode_string_dict(bytes) {
        return Some(d);
    }
    let text = decode_text(bytes)?;
    let t = text.trim_start();
    if t.starts_with("<?xml") || t.starts_with("<plist") {
        return None;
    }
    parse_text(&text)
}

fn decode_text(b: &[u8]) -> Option<String> {
    let utf16 = |raw: &[u8], be: bool| {
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if be {
                    u16::from_be_bytes(*c)
                } else {
                    u16::from_le_bytes(*c)
                }
            })
            .collect();
        String::from_utf16(&units).ok()
    };
    match b {
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).ok(),
        _ => String::from_utf8(b.to_vec()).ok(),
    }
}

fn parse_text(text: &str) -> Option<HashMap<String, String>> {
    let mut out = HashMap::new();
    let mut chars = text.chars().peekable();
    // Reads one token: a quoted string, or a bare word.
    fn token(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<Option<String>> {
        loop {
            match it.peek().copied() {
                None => return Some(None),
                Some(c) if c.is_whitespace() => {
                    it.next();
                }
                Some('/') => {
                    it.next();
                    match it.next()? {
                        '/' => {
                            for c in it.by_ref() {
                                if c == '\n' {
                                    break;
                                }
                            }
                        }
                        '*' => {
                            let mut prev = ' ';
                            loop {
                                let c = it.next()?;
                                if prev == '*' && c == '/' {
                                    break;
                                }
                                prev = c;
                            }
                        }
                        _ => return None,
                    }
                }
                Some('"') => {
                    it.next();
                    let mut s = String::new();
                    loop {
                        match it.next()? {
                            '"' => return Some(Some(s)),
                            '\\' => match it.next()? {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                c => s.push(c),
                            },
                            c => s.push(c),
                        }
                    }
                }
                Some(c) if c.is_alphanumeric() || c == '_' || c == '.' => {
                    let mut s = String::new();
                    while let Some(&c) = it.peek() {
                        if c.is_alphanumeric() || c == '_' || c == '.' {
                            s.push(c);
                            it.next();
                        } else {
                            break;
                        }
                    }
                    return Some(Some(s));
                }
                Some(_) => return None,
            }
        }
    }
    fn skip_space(it: &mut std::iter::Peekable<std::str::Chars<'_>>) {
        while it.next_if(|c| c.is_whitespace()).is_some() {}
    }
    while let Some(key) = token(&mut chars)? {
        // `=`, the value, `;`
        skip_space(&mut chars);
        if chars.next()? != '=' {
            return None;
        }
        let value = token(&mut chars)??;
        skip_space(&mut chars);
        if chars.next()? != ';' {
            return None;
        }
        out.insert(key, value);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::with_lang;

    fn tables_with(content: &[u8]) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("it.lproj");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SystemFolderLocalizations.strings"), content).unwrap();
        t
    }

    /// A home folder with the folders of a Mac, each with its `.localized` marker.
    fn home() -> tempfile::TempDir {
        let h = tempfile::tempdir().unwrap();
        for f in FOLDERS {
            std::fs::create_dir_all(h.path().join(f)).unwrap();
            std::fs::write(h.path().join(f).join(".localized"), b"").unwrap();
        }
        h
    }

    fn shown_in(n: &FolderNames, h: &tempfile::TempDir, name: &str) -> Option<String> {
        n.shown(&h.path().join(name), h.path())
    }

    #[test]
    fn the_table_of_the_system_comes_before_the_fixed_one() {
        let t =
            tables_with(b"\"Desktop\" = \"Scrivania di sistema\";\n\"Movies\" = \"Filmati\";\n");
        let n = FolderNames::for_lang(Lang::It, t.path());
        assert_eq!(n.name_of("Desktop"), Some("Scrivania di sistema"));
        // What the system table leaves out comes from the reserve.
        assert_eq!(n.name_of("Documents"), Some("Documenti"));
    }

    #[test]
    fn without_a_table_the_italian_reserve_names_the_nine_folders() {
        let t = tempfile::tempdir().unwrap();
        let n = FolderNames::for_lang(Lang::It, t.path());
        for (disk, shown) in [
            ("Desktop", "Scrivania"),
            ("Documents", "Documenti"),
            ("Downloads", "Download"),
            ("Pictures", "Immagini"),
            ("Music", "Musica"),
            ("Movies", "Filmati"),
            ("Public", "Pubblica"),
            ("Applications", "Applicazioni"),
            ("Library", "Libreria"),
        ] {
            assert_eq!(n.name_of(disk), Some(shown), "{disk}");
        }
    }

    #[test]
    fn english_keeps_the_names_whatever_the_system_has() {
        let t = tables_with(b"\"Desktop\" = \"Scrivania\";");
        let n = FolderNames::for_lang(Lang::En, t.path());
        assert!(n.is_empty());
        let h = home();
        assert_eq!(shown_in(&n, &h, "Desktop"), None);
    }

    #[test]
    fn a_folder_is_renamed_only_where_the_system_has_it_and_with_its_marker() {
        let t = tempfile::tempdir().unwrap();
        let n = FolderNames::for_lang(Lang::It, t.path());
        let h = home();
        assert_eq!(shown_in(&n, &h, "Desktop").as_deref(), Some("Scrivania"));
        assert_eq!(shown_in(&n, &h, "Library").as_deref(), Some("Libreria"));
        // Not one of the nine.
        std::fs::create_dir(h.path().join("projects")).unwrap();
        assert_eq!(shown_in(&n, &h, "projects"), None);
        // The same name deeper down, or in someone else's folder, is an ordinary folder.
        std::fs::create_dir_all(h.path().join("work/Desktop")).unwrap();
        std::fs::write(h.path().join("work/Desktop/.localized"), b"").unwrap();
        assert_eq!(shown_in(&n, &h, "work/Desktop"), None);
        // One made by hand, with no marker, keeps its name.
        std::fs::remove_file(h.path().join("Music/.localized")).unwrap();
        assert_eq!(shown_in(&n, &h, "Music"), None);
        // A file with that name is no folder (and has no marker inside).
        std::fs::write(h.path().join("Public.txt"), b"x").unwrap();
        assert_eq!(shown_in(&n, &h, "Public.txt"), None);
    }

    #[test]
    fn the_current_names_follow_the_language_of_the_interface() {
        let t = tempfile::tempdir().unwrap();
        let en = with_lang(Lang::En, || FolderNames::current_with(t.path()));
        let it = with_lang(Lang::It, || FolderNames::current_with(t.path()));
        assert!(en.is_empty());
        assert_eq!(it.name_of("Movies"), Some("Filmati"));
    }

    #[test]
    fn the_three_forms_of_a_strings_file_are_read() {
        let text = b"/* names */\n\"Desktop\" = \"Scrivania\"; // a\n\"Public\"=\"Pubblica\";\n";
        let t = parse_strings(text).unwrap();
        assert_eq!(t["Desktop"], "Scrivania");
        assert_eq!(t["Public"], "Pubblica");
        // UTF-16 with a byte order mark, as the system writes them.
        let mut u: Vec<u8> = vec![0xFF, 0xFE];
        for unit in "\"Library\" = \"Libreria\";".encode_utf16() {
            u.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(parse_strings(&u).unwrap()["Library"], "Libreria");
        let xml = b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Music</key><string>Musica</string><key>A &amp; B</key><string>x</string></dict></plist>";
        let x = parse_strings(xml).unwrap();
        assert_eq!(x["Music"], "Musica");
        assert_eq!(x["A & B"], "x");
        // Junk is no table.
        assert_eq!(parse_strings(b"\x00\x01\x02 garbage"), None);
        assert_eq!(parse_strings(b"\"a\" = ;"), None);
    }

    #[test]
    fn a_binary_table_is_read_and_a_damaged_one_does_not_harm() {
        // bplist00: a dictionary of two strings: Desktop -> Scrivania, Music -> Musica.
        let mut b = b"bplist00".to_vec();
        let mut offs = Vec::new();
        offs.push(b.len());
        b.extend_from_slice(&[0xD2, 1, 2, 3, 4]);
        for s in ["Desktop", "Music", "Scrivania", "Musica"] {
            offs.push(b.len());
            b.push(0x50 | s.len() as u8);
            b.extend_from_slice(s.as_bytes());
        }
        let table_at = b.len();
        for o in &offs {
            b.push(*o as u8);
        }
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&[1, 1]);
        b.extend_from_slice(&(offs.len() as u64).to_be_bytes());
        b.extend_from_slice(&0u64.to_be_bytes());
        b.extend_from_slice(&(table_at as u64).to_be_bytes());
        let t = parse_strings(&b).unwrap();
        assert_eq!(t["Desktop"], "Scrivania");
        assert_eq!(t["Music"], "Musica");
        // Cut short, or with a reference out of range: nothing, and no panic.
        for n in 0..b.len() {
            let _ = parse_strings(&b[..n]);
        }
        let mut bad = b.clone();
        bad[9] = 200;
        assert_eq!(parse_strings(&bad), None);
        // The reserve still names the folders when the table is unreadable.
        let tables = tables_with(&b[..20]);
        let n = FolderNames::for_lang(Lang::It, tables.path());
        assert_eq!(n.name_of("Desktop"), Some("Scrivania"));
    }
}
