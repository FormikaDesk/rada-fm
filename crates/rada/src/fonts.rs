//! Is the terminal using a Nerd Font? A terminal cannot be asked, but its configuration
//! can be read: if the font it is set to use is a Nerd Font, the icons will render.
//!
//! And failing that: is there a symbol font on the system? The Unicode icons (dingbats,
//! rare arrows, technical marks) are in no default monospace font; a terminal draws them
//! from another font of the system when there is one that has them.

use std::path::{Path, PathBuf};

/// Is this the name of a Nerd Font? The patched fonts are named in one of two ways:
/// `<Family> Nerd Font` (with `Mono` or `Propo` after it), or the short form `<Family> NF`,
/// `NFM`, `NFP`, which is also how the builds that come with a font are named
/// (`Cascadia Mono NF`). The short form is recognised as a word of its own, or stuck to
/// the family name (`CaskaydiaCoveNF`), but not inside a word (`Info`, `CONFIG`).
fn is_nerd_font_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("nerd font") || lower.contains("nerdfont") {
        return true;
    }
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|word| {
            if matches!(word.to_lowercase().as_str(), "nf" | "nfm" | "nfp") {
                return true;
            }
            // `JetBrainsMonoNF`: the capitals follow a small letter or a digit.
            ["NFM", "NFP", "NF"].iter().any(|tag| {
                word.strip_suffix(tag)
                    .and_then(|family| family.chars().last())
                    .is_some_and(|c| c.is_lowercase() || c.is_ascii_digit())
            })
        })
}

/// The first line of a terminal's configuration that sets a font and names a Nerd Font.
/// Good for the formats that put a setting on a line (`font-family = …`, `Font=…`,
/// `font_family …`, `"face": "…"`), comments left out.
fn nerd_font_line(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with("--") && !l.starts_with("//"))
        .find(|l| {
            let lower = l.to_lowercase();
            // What comes after the key is the name (or names) of the font.
            let value = l
                .split_once(['=', ':'])
                .map(|(_, v)| v)
                .or_else(|| l.split_once(char::is_whitespace).map(|(_, v)| v))
                .unwrap_or("");
            (lower.contains("font") || lower.contains("\"face\"")) && is_nerd_font_name(value)
        })
}

#[cfg(test)]
fn mentions_nerd_font(text: &str) -> bool {
    nerd_font_line(text).is_some()
}

/// Windows Terminal's settings are JSON with comments and, sometimes, a comma before a
/// closing bracket: both are taken out, so that a JSON parser reads the rest.
fn plain_json(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut in_string) = (0, false);
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            continue;
        } else if c == ','
            && chars[i + 1..]
                .iter()
                .find(|n| !n.is_whitespace())
                .is_some_and(|n| matches!(n, '}' | ']'))
        {
            // a trailing comma
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// What a Windows Terminal settings file says about fonts: the one of the profile in use
/// (`profile_id`, or the file's default profile), and the one set for all profiles. `Err`
/// when the file cannot be understood. A *fragment* (the file a program adds its profile
/// with) has the same shape, with `profiles` as a plain list.
fn windows_terminal_fonts(
    settings: &str,
    profile_id: Option<&str>,
) -> Result<(Option<String>, Option<String>), ()> {
    let json: serde_json::Value = serde_json::from_str(&plain_json(settings)).map_err(|_| ())?;
    // `"font": { "face": … }`, or the older `"fontFace": …`.
    let face = |profile: &serde_json::Value| {
        profile
            .get("font")
            .and_then(|f| f.get("face"))
            .or_else(|| profile.get("fontFace"))
            .and_then(|f| f.as_str())
            .map(str::to_string)
    };
    let profiles = json.get("profiles").ok_or(())?;
    // `profiles` is `{ "defaults": …, "list": […] }`, or (older, and in fragments) the list.
    let (defaults, list) = match profiles {
        serde_json::Value::Array(list) => (None, Some(list)),
        other => (
            other.get("defaults"),
            other.get("list").and_then(|l| l.as_array()),
        ),
    };
    let wanted = profile_id
        .map(str::to_string)
        .or_else(|| {
            json.get("defaultProfile")
                .and_then(|d| d.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let current = list.and_then(|l| {
        l.iter().find(|p| {
            p.get("guid")
                .and_then(|g| g.as_str())
                .is_some_and(|g| g.eq_ignore_ascii_case(&wanted))
        })
    });
    Ok((current.and_then(face), defaults.and_then(face)))
}

/// The font Windows Terminal draws the current tab with, from its `settings.json`:
/// the one of the profile in use, or else the one set for all profiles. `Ok(None)` when no
/// font is set anywhere (the terminal then uses its own, which is not a Nerd Font).
#[cfg(test)]
fn windows_terminal_font(settings: &str, profile_id: Option<&str>) -> Result<Option<String>, ()> {
    windows_terminal_fonts(settings, profile_id).map(|(own, common)| own.or(common))
}

/// The folders where programs put the profiles they add to Windows Terminal (one folder per
/// program, JSON files inside): for this user and for the whole machine.
fn windows_terminal_fragment_dirs() -> Vec<PathBuf> {
    ["LOCALAPPDATA", "ProgramData"]
        .into_iter()
        .filter_map(|k| std::env::var_os(k).map(PathBuf::from))
        .map(|base| base.join("Microsoft/Windows Terminal/Fragments"))
        .collect()
}

/// The font a profile added by a program asks for (`rada setup desktop` adds one, with the
/// Nerd Font it installs): such a profile is not written in the person's `settings.json`.
fn fragment_font(dirs: &[PathBuf], profile_id: &str) -> Option<String> {
    dirs.iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|programs| programs.flatten())
        .filter_map(|program| std::fs::read_dir(program.path()).ok())
        .flat_map(|files| files.flatten())
        .map(|f| f.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        })
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|text| {
            windows_terminal_fonts(&text, Some(profile_id))
                .ok()
                .and_then(|(own, _)| own)
        })
}

/// Where Windows Terminal keeps its settings: the Store package, the preview one, and the
/// unpackaged install.
fn windows_terminal_settings() -> Vec<PathBuf> {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    vec![
        local.join("Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json"),
        local.join(
            "Packages/Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe/LocalState/settings.json",
        ),
        local.join("Microsoft/Windows Terminal/settings.json"),
    ]
}

fn candidates(home: &Path) -> Vec<PathBuf> {
    let c = home.join(".config");
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (prog, term) = (env("TERM_PROGRAM").to_lowercase(), env("TERM"));
    let mut v = Vec::new();
    if prog == "ghostty" || term.contains("ghostty") {
        v.extend([c.join("ghostty/config"), c.join("ghostty/config.ghostty")]);
    }
    if std::env::var_os("KITTY_WINDOW_ID").is_some() || term.contains("kitty") {
        v.push(c.join("kitty/kitty.conf"));
    }
    if prog == "wezterm" || std::env::var_os("WEZTERM_EXECUTABLE").is_some() {
        v.extend([c.join("wezterm/wezterm.lua"), home.join(".wezterm.lua")]);
    }
    if std::env::var_os("ALACRITTY_LOG").is_some() || term.contains("alacritty") {
        v.extend([
            c.join("alacritty/alacritty.toml"),
            c.join("alacritty/alacritty.yml"),
        ]);
    }
    if term.starts_with("foot") {
        v.push(c.join("foot/foot.ini"));
    }
    if std::env::var_os("KONSOLE_VERSION").is_some() {
        // Konsole keeps one file per profile; any of them may be the one in use.
        let profiles = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("konsole");
        if let Ok(entries) = std::fs::read_dir(profiles) {
            v.extend(
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "profile")),
            );
        }
    }
    v
}

/// The Nerd Font the current terminal is set to draw with, as its configuration names it;
/// `None` when the terminal is not one whose configuration is known, or its font is not a
/// Nerd Font.
///
/// Windows Terminal is asked precisely: its settings say which font the profile of this
/// very tab uses. The other terminals are read line by line. `windows_terminal` is for a
/// Windows Terminal that the environment does not announce (a console handed over to it).
pub fn terminal_nerd_font(home: &Path, windows_terminal: bool) -> Option<String> {
    if windows_terminal || std::env::var_os("WT_SESSION").is_some() {
        let profile = std::env::var("WT_PROFILE_ID").ok();
        // The profile's font: what the person set for it wins, then what the program that
        // added the profile asked for, then the font for all profiles.
        let added = profile
            .as_deref()
            .and_then(|id| fragment_font(&windows_terminal_fragment_dirs(), id));
        let mut read_any = false;
        for path in windows_terminal_settings() {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match windows_terminal_fonts(&text, profile.as_deref()) {
                Ok((own, common)) => {
                    read_any = true;
                    if let Some(face) = own.or_else(|| added.clone()).or(common)
                        && is_nerd_font_name(&face)
                    {
                        return Some(face);
                    }
                }
                // Settings a parser cannot read are still searched for a font line.
                Err(()) => {
                    if let Some(line) = nerd_font_line(&text) {
                        return Some(line.to_string());
                    }
                }
            }
        }
        // No settings file at all (a terminal never configured): the added profile decides.
        if !read_any && let Some(face) = added.filter(|f| is_nerd_font_name(f)) {
            return Some(face);
        }
    }
    candidates(home)
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|t| nerd_font_line(&t).map(str::to_string))
}

/// True when the current terminal's own configuration selects a Nerd Font.
pub fn terminal_uses_nerd_font(home: &Path, windows_terminal: bool) -> bool {
    terminal_nerd_font(home, windows_terminal).is_some()
}

/// A font file known to carry the symbols of the `unicode-icons` set.
fn is_symbol_font(file_name: &str) -> bool {
    let name = file_name.to_lowercase();
    [
        "dejavusans",
        "notosanssymbols",
        "notosansmath",
        "symbola",
        "freesans",
        "freemono",
        "unifont",
    ]
    .iter()
    .any(|known| name.starts_with(known))
}

fn holds_symbol_font(dir: &Path, depth: u8) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let path = e.path();
        if path.is_dir() {
            depth > 0 && holds_symbol_font(&path, depth - 1)
        } else {
            path.file_name()
                .is_some_and(|n| is_symbol_font(&n.to_string_lossy()))
        }
    })
}

/// True when the system has a font for the terminal to draw the Unicode icons from:
/// Segoe UI Symbol on Windows, Apple Symbols on macOS, and on the other systems one of
/// the usual symbol fonts in the font folders (fontconfig finds it as a fallback).
pub fn system_has_symbol_font(home: &Path) -> bool {
    if cfg!(windows) {
        let windows = std::env::var_os("WINDIR")
            .or_else(|| std::env::var_os("SystemRoot"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:/Windows"));
        windows.join("Fonts/seguisym.ttf").is_file()
    } else if cfg!(target_os = "macos") {
        Path::new("/System/Library/Fonts/Apple Symbols.ttf").is_file()
    } else {
        let user = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("fonts");
        [
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
            user,
            home.join(".fonts"),
        ]
        .iter()
        .any(|dir| holds_symbol_font(dir, 4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_nerd_font_settings() {
        assert!(mentions_nerd_font(
            "font-size = 11\nfont-family = \"JetBrainsMono Nerd Font\"\n"
        ));
        assert!(mentions_nerd_font(
            "font_family      FiraCode Nerd Font Mono"
        ));
        assert!(!mentions_nerd_font("font-family = \"JetBrains Mono\"\n"));
        assert!(!mentions_nerd_font("# font-family = \"X Nerd Font\"\n"));
        assert!(!mentions_nerd_font("theme = nerd-dark"));
        // Windows Terminal's settings.json and a Konsole profile.
        assert!(mentions_nerd_font(
            "    \"font\": { \"face\": \"CaskaydiaCove Nerd Font\" },"
        ));
        assert!(mentions_nerd_font(
            "Font=JetBrainsMono Nerd Font,10,-1,5,50,0"
        ));
        assert!(!mentions_nerd_font("Font=Hack,10,-1,5,50,0,0,0,0,0"));
        // The Nerd Font builds that come with Windows Terminal are named "… NF", and its
        // settings keep the name on a line apart from the word "font".
        assert!(mentions_nerd_font(
            "\"font\":\n{\n    \"face\": \"Cascadia Mono NF\"\n}"
        ));
        assert!(!mentions_nerd_font(
            "\"font\":\n{\n    \"face\": \"Cascadia Mono\"\n}"
        ));
    }

    #[test]
    fn recognises_symbol_fonts_by_their_file_name() {
        for name in [
            "DejaVuSans.ttf",
            "DejaVuSansMono-Bold.ttf",
            "NotoSansSymbols2-Regular.ttf",
            "Symbola.otf",
        ] {
            assert!(is_symbol_font(name), "{name}");
        }
        for name in ["Hack-Regular.ttf", "AdwaitaMono-Regular.ttf", "fonts.dir"] {
            assert!(!is_symbol_font(name), "{name}");
        }
    }

    #[test]
    fn a_symbol_font_is_found_in_a_folder_of_fonts_and_not_in_an_empty_one() {
        let root = tempfile::tempdir().unwrap();
        assert!(!holds_symbol_font(root.path(), 4));
        let nested = root.path().join("truetype/dejavu");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("DejaVuSans.ttf"), b"").unwrap();
        assert!(holds_symbol_font(root.path(), 4));
        // Not looked for deeper than asked.
        assert!(!holds_symbol_font(root.path(), 1));
    }

    #[test]
    fn a_nerd_font_is_known_by_its_name() {
        for name in [
            "JetBrainsMono Nerd Font",
            "FiraCode Nerd Font Mono",
            "Symbols Nerd Font",
            "Cascadia Mono NF",
            "Cascadia Code NF",
            "JetBrainsMono NFM",
            "Hack NFP",
            "CaskaydiaCoveNF",
            "JetBrainsMonoNFM",
            "Cascadia Mono NF, Segoe UI Emoji",
            "Cascadia Mono NF SemiBold",
            "hack nf",
        ] {
            assert!(is_nerd_font_name(name), "{name}");
        }
        for name in [
            "Cascadia Mono",
            "JetBrains Mono",
            "Consolas",
            "Info",
            "CONF",
            "DINFNext",
            "Noto Sans Symbols",
            "",
        ] {
            assert!(!is_nerd_font_name(name), "{name}");
        }
    }

    const WT_SETTINGS: &str = r#"
    {
        // the profile a new tab opens
        "defaultProfile": "{aaaa}",
        "profiles":
        {
            "defaults":
            {
                "font":
                {
                    "face": "Cascadia Mono NF"
                }
            },
            "list":
            [
                { "guid": "{aaaa}", "name": "PowerShell" },
                { "guid": "{bbbb}", "name": "cmd", "font": { "face": "Consolas" } },
                { "guid": "{cccc}", "name": "old", "fontFace": "Hack Nerd Font", },
            ]
        }
    }"#;

    #[test]
    fn windows_terminal_says_which_font_the_current_profile_uses() {
        let font = |id: Option<&str>| windows_terminal_font(WT_SETTINGS, id).unwrap();
        // The profile's own font wins over the one for all profiles.
        assert_eq!(font(Some("{bbbb}")).as_deref(), Some("Consolas"));
        assert_eq!(font(Some("{CCCC}")).as_deref(), Some("Hack Nerd Font"));
        // A profile without a font, and no profile id at all (the default profile).
        assert_eq!(font(Some("{aaaa}")).as_deref(), Some("Cascadia Mono NF"));
        assert_eq!(font(None).as_deref(), Some("Cascadia Mono NF"));
        // A profile that is not in the file (a generated one) uses the common font.
        assert_eq!(font(Some("{zzzz}")).as_deref(), Some("Cascadia Mono NF"));
    }

    #[test]
    fn windows_terminal_without_a_font_setting_uses_no_nerd_font() {
        let none = r#"{ "profiles": { "defaults": {}, "list": [ { "guid": "{a}" } ] } }"#;
        assert_eq!(windows_terminal_font(none, Some("{a}")), Ok(None));
        // The older shape: `profiles` is the list.
        let old = r#"{ "profiles": [ { "guid": "{a}", "fontFace": "Hack NF" } ] }"#;
        assert_eq!(
            windows_terminal_font(old, Some("{a}")).unwrap().as_deref(),
            Some("Hack NF")
        );
        assert_eq!(windows_terminal_font("not json", None), Err(()));
        assert_eq!(windows_terminal_font("{}", None), Err(()));
    }

    #[test]
    fn comments_and_trailing_commas_do_not_stop_the_reading() {
        let text = "{ /* a */ \"a\": \"x // not a comment, ]\", // b\n \"b\": [1, 2, ], }";
        let json: serde_json::Value = serde_json::from_str(&plain_json(text)).unwrap();
        assert_eq!(json["a"], "x // not a comment, ]");
        assert_eq!(json["b"][1], 2);
    }

    #[test]
    fn a_profile_added_by_a_program_keeps_its_font_in_a_fragment() {
        // What `rada setup desktop` writes: a list of profiles, in a folder of its own.
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Fragments");
        std::fs::create_dir_all(dir.join("rada")).unwrap();
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(
            dir.join("rada/rada.json"),
            r#"{ "profiles": [ { "guid": "{rada}", "name": "rada",
                 "font": { "face": "JetBrainsMono Nerd Font Mono", "size": 12 } } ] }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("other/other.json"),
            r#"{ "profiles": [ { "guid": "{other}", "name": "other" } ] }"#,
        )
        .unwrap();
        std::fs::write(dir.join("other/notes.txt"), "not a fragment").unwrap();
        let dirs = [root.path().join("missing"), dir];
        assert_eq!(
            fragment_font(&dirs, "{RADA}").as_deref(),
            Some("JetBrainsMono Nerd Font Mono")
        );
        // A profile without a font of its own, and one that no fragment adds.
        assert_eq!(fragment_font(&dirs, "{other}"), None);
        assert_eq!(fragment_font(&dirs, "{nobody}"), None);
    }

    #[test]
    fn the_profiles_own_font_and_the_common_one_are_told_apart() {
        assert_eq!(
            windows_terminal_fonts(WT_SETTINGS, Some("{bbbb}")),
            Ok((Some("Consolas".into()), Some("Cascadia Mono NF".into())))
        );
        assert_eq!(
            windows_terminal_fonts(WT_SETTINGS, Some("{aaaa}")),
            Ok((None, Some("Cascadia Mono NF".into())))
        );
    }
}
