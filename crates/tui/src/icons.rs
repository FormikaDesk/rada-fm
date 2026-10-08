//! File icons. Nerd Font glyphs are optional: without a patched font they render as
//! boxes, so the default is a small set of plain Unicode markers.

use ratatui::style::Color;
use vela_core::fs::FileKind;
use vela_core::model::Entry;
use vela_core::ops::LinkState;

use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSet {
    Nerd,
    Unicode,
    None,
}

impl IconSet {
    pub fn parse(s: &str) -> Option<IconSet> {
        match s.to_ascii_lowercase().as_str() {
            "nerd" | "nerdfont" | "nerd-font" | "on" | "true" | "1" => Some(IconSet::Nerd),
            "unicode" | "plain" | "auto" => Some(IconSet::Unicode),
            "none" | "off" | "ascii" | "false" | "0" => Some(IconSet::None),
            _ => None,
        }
    }

    /// Width in cells reserved for the icon column (including the trailing space).
    pub fn width(self) -> usize {
        match self {
            IconSet::None => 0,
            _ => 2,
        }
    }
}

const FOLDER: &str = "\u{f07b}";
const FILE: &str = "\u{f15b}";
const TEXT: &str = "\u{f15c}";
const LINK: &str = "\u{f0c1}";
const IMAGE: &str = "\u{f1c5}";
const ARCHIVE: &str = "\u{f1c6}";
const PDF: &str = "\u{f1c1}";
const AUDIO: &str = "\u{f1c7}";
const VIDEO: &str = "\u{f1c8}";
const LOCK: &str = "\u{f023}";
const GEAR: &str = "\u{f013}";
const TERM: &str = "\u{f489}";
const GIT: &str = "\u{e702}";
const CONFIG: &str = "\u{e615}";
const RUST: &str = "\u{e7a8}";
const PYTHON: &str = "\u{e73c}";
const JS: &str = "\u{e74e}";
const TS: &str = "\u{e628}";
const JSON: &str = "\u{e60b}";
const MD: &str = "\u{e73e}";
const HTML: &str = "\u{e736}";
const CSS: &str = "\u{e749}";
const GO: &str = "\u{e627}";
const C: &str = "\u{e61e}";
const CPP: &str = "\u{e61d}";
const JAVA: &str = "\u{e738}";

fn by_extension(ext: &str) -> Option<(&'static str, Color)> {
    let c = |r, g, b| Color::Rgb(r, g, b);
    Some(match ext {
        "rs" => (RUST, c(250, 179, 135)),
        "py" => (PYTHON, c(249, 226, 175)),
        "js" | "mjs" | "cjs" => (JS, c(249, 226, 175)),
        "ts" | "tsx" => (TS, c(137, 180, 250)),
        "json" | "jsonc" => (JSON, c(249, 226, 175)),
        "md" | "markdown" => (MD, c(116, 199, 236)),
        "html" | "htm" => (HTML, c(250, 179, 135)),
        "css" | "scss" => (CSS, c(137, 180, 250)),
        "go" => (GO, c(116, 199, 236)),
        "c" | "h" => (C, c(137, 180, 250)),
        "cpp" | "cc" | "hpp" => (CPP, c(137, 180, 250)),
        "java" => (JAVA, c(243, 139, 168)),
        "sh" | "bash" | "zsh" | "fish" => (TERM, c(166, 227, 161)),
        "toml" | "yaml" | "yml" | "ini" | "conf" | "cfg" => (CONFIG, c(166, 173, 200)),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => {
            (IMAGE, c(203, 166, 247))
        }
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "tgz" => {
            (ARCHIVE, c(249, 226, 175))
        }
        "pdf" => (PDF, c(243, 139, 168)),
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" => (AUDIO, c(245, 194, 231)),
        "mp4" | "mkv" | "webm" | "mov" | "avi" => (VIDEO, c(245, 194, 231)),
        "txt" | "log" | "rtf" => (TEXT, c(166, 173, 200)),
        "lock" => (LOCK, c(108, 112, 134)),
        _ => return None,
    })
}

/// Glyph and color for an entry.
pub fn icon(e: &Entry, set: IconSet, th: &Theme) -> (&'static str, Color) {
    let rgb = th.truecolor;
    let tint = |c: Color| if rgb { c } else { th.subtle };
    if e.error.is_some() {
        return match set {
            IconSet::Nerd => (FILE, th.danger),
            _ => ("!", th.danger),
        };
    }
    if let Some(l) = &e.link {
        let color = if l.state == LinkState::Broken || l.state == LinkState::Circular {
            th.danger
        } else {
            th.link
        };
        return match set {
            IconSet::Nerd => (LINK, color),
            _ => ("↪", color),
        };
    }
    match (e.kind, set) {
        (FileKind::Dir, IconSet::Nerd) => (FOLDER, th.dir),
        (FileKind::Dir, _) => ("▸", th.dir),
        (FileKind::Other, IconSet::Nerd) => (GEAR, th.warn),
        (FileKind::Other, _) => ("◆", th.warn),
        (_, IconSet::Nerd) => {
            let lower = e.display.to_lowercase();
            if lower == ".gitignore" || lower == ".git" || lower == ".gitattributes" {
                return (GIT, tint(Color::Rgb(250, 179, 135)));
            }
            if e.executable {
                return (TERM, th.exec);
            }
            match lower
                .rsplit_once('.')
                .and_then(|(_, ext)| by_extension(ext))
            {
                Some((g, c)) => (g, tint(c)),
                None => (FILE, th.subtle),
            }
        }
        (_, _) => {
            if e.executable {
                ("*", th.exec)
            } else {
                ("·", th.muted)
            }
        }
    }
}
