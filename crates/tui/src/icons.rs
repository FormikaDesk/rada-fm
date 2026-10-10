//! File categories, their icons and badges. Colours come from the theme's tokens.
//!
//! Nerd Font glyphs need a patched font: without one they render as boxes, so the
//! fallback is a small set of plain Unicode markers.

use rada_core::fs::FileKind;
use rada_core::model::Entry;
use rada_core::ops::LinkState;
use ratatui::style::Color;

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
            "unicode" | "plain" => Some(IconSet::Unicode),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Code,
    Markup,
    Text,
    Doc,
    Image,
    Vector,
    Audio,
    Video,
    Archive,
    Data,
    Config,
    Binary,
    Folder,
    Link,
    Other,
}

impl Category {
    /// Short word shown in the Type column.
    pub fn label(self) -> &'static str {
        match self {
            Category::Code => "code",
            Category::Markup => "md",
            Category::Text => "text",
            Category::Doc => "doc",
            Category::Image => "image",
            Category::Vector => "vector",
            Category::Audio => "audio",
            Category::Video => "video",
            Category::Archive => "archive",
            Category::Data => "data",
            Category::Config => "config",
            Category::Binary => "binary",
            Category::Folder => "Folder",
            Category::Link => "link",
            Category::Other => "file",
        }
    }

    pub fn color(self, th: &Theme) -> Color {
        let k = &th.kinds;
        match self {
            Category::Code => k.code,
            Category::Markup => k.markup,
            Category::Text => k.text,
            Category::Doc => k.doc,
            Category::Image => k.image,
            Category::Vector => k.vector,
            Category::Audio => k.audio,
            Category::Video => k.video,
            Category::Archive => k.archive,
            Category::Data => k.data,
            Category::Config => k.config,
            Category::Binary => k.binary,
            Category::Folder => k.folder,
            Category::Link => k.link,
            Category::Other => k.other,
        }
    }
}

fn by_extension(ext: &str) -> Option<Category> {
    Some(match ext {
        "rs" | "py" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "go" | "c" | "h" | "cpp"
        | "cc" | "hpp" | "java" | "kt" | "swift" | "rb" | "php" | "sh" | "bash" | "zsh"
        | "fish" | "lua" | "zig" | "cs" | "scala" | "hs" | "ex" | "exs" | "sql" | "css"
        | "scss" | "html" | "htm" | "vue" | "svelte" | "nix" | "pl" | "r" | "dart" => {
            Category::Code
        }
        "md" | "markdown" | "mdx" => Category::Markup,
        "txt" | "log" | "rst" | "org" | "tex" => Category::Text,
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "epub" | "ppt" | "pptx" | "xls" | "xlsx"
        | "ods" => Category::Doc,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "avif"
        | "heic" => Category::Image,
        "svg" | "ai" | "eps" => Category::Vector,
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" | "aac" => Category::Audio,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => Category::Video,
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "tgz" | "txz" | "iso"
        | "deb" | "rpm" => Category::Archive,
        "json" | "jsonc" | "csv" | "tsv" | "xml" | "yaml" | "yml" | "db" | "sqlite" | "ndjson"
        | "parquet" => Category::Data,
        "toml" | "ini" | "conf" | "cfg" | "env" | "lock" | "properties" => Category::Config,
        "exe" | "dll" | "so" | "o" | "a" | "bin" | "elf" | "appimage" | "wasm" => Category::Binary,
        _ => return None,
    })
}

pub fn categorize(e: &Entry) -> Category {
    if e.link.is_some() {
        return Category::Link;
    }
    match e.kind {
        FileKind::Dir => Category::Folder,
        FileKind::Other => Category::Other,
        _ => {
            let lower = e.display.to_lowercase();
            if matches!(
                lower.as_str(),
                "makefile" | "dockerfile" | "cmakelists.txt" | "justfile"
            ) {
                return Category::Code;
            }
            if matches!(
                lower.as_str(),
                "license" | "readme" | "authors" | "copying" | "changelog"
            ) {
                return Category::Text;
            }
            match lower.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => {
                    by_extension(ext).unwrap_or(if e.executable {
                        Category::Binary
                    } else {
                        Category::Other
                    })
                }
                _ if e.executable => Category::Binary,
                _ => Category::Other,
            }
        }
    }
}

// Nerd Font (v3) code points.
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
const DB: &str = "\u{f1c0}";
const CODE: &str = "\u{f121}";
const VECTOR: &str = "\u{f0e7}";
const BIN: &str = "\u{f471}";

fn nerd_glyph(e: &Entry, cat: Category) -> &'static str {
    let lower = e.display.to_lowercase();
    if matches!(
        lower.as_str(),
        ".gitignore" | ".git" | ".gitattributes" | ".gitmodules"
    ) {
        return GIT;
    }
    let ext = lower.rsplit_once('.').map(|(_, x)| x).unwrap_or("");
    match ext {
        "rs" => RUST,
        "py" => PYTHON,
        "js" | "mjs" | "cjs" | "jsx" => JS,
        "ts" | "tsx" => TS,
        "json" | "jsonc" => JSON,
        "md" | "markdown" | "mdx" => MD,
        "html" | "htm" => HTML,
        "css" | "scss" => CSS,
        "go" => GO,
        "c" | "h" => C,
        "cpp" | "cc" | "hpp" => CPP,
        "java" => JAVA,
        "sh" | "bash" | "zsh" | "fish" => TERM,
        "pdf" => PDF,
        "lock" => LOCK,
        _ => match cat {
            Category::Folder => FOLDER,
            Category::Link => LINK,
            Category::Code => CODE,
            Category::Text | Category::Markup => TEXT,
            Category::Doc => PDF,
            Category::Image => IMAGE,
            Category::Vector => VECTOR,
            Category::Audio => AUDIO,
            Category::Video => VIDEO,
            Category::Archive => ARCHIVE,
            Category::Data => DB,
            Category::Config => CONFIG,
            Category::Binary => BIN,
            Category::Other => {
                if e.kind == FileKind::Other {
                    GEAR
                } else {
                    FILE
                }
            }
        },
    }
}

/// Glyph and colour for an entry.
pub fn icon(e: &Entry, set: IconSet, th: &Theme) -> (&'static str, Color) {
    if e.error.is_some() {
        return (if set == IconSet::Nerd { FILE } else { "!" }, th.error);
    }
    let cat = categorize(e);
    let broken = e
        .link
        .as_ref()
        .is_some_and(|l| matches!(l.state, LinkState::Broken | LinkState::Circular));
    let color = if broken { th.error } else { cat.color(th) };
    let glyph = match set {
        IconSet::Nerd => nerd_glyph(e, cat),
        _ => match cat {
            Category::Folder => "▸",
            Category::Link => "↪",
            Category::Binary => "*",
            Category::Other if e.kind == FileKind::Other => "◆",
            _ => "·",
        },
    };
    (glyph, color)
}

// ------------------------------------------------------------------------- interface icons

/// The small pictures on buttons and bars (not the ones of files).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Back,
    Forward,
    Up,
    Refresh,
    Home,
    Search,
    New,
    Cut,
    Copy,
    Paste,
    Rename,
    Compress,
    Delete,
    Undo,
    Sort,
    View,
    More,
    Pin,
    Help,
    Close,
    Details,
    Icons,
    Eye,
    Open,
    Clipboard,
    Checked,
    Unchecked,
    TabFolder,
    Filter,
}

impl Glyph {
    pub const ALL: [Glyph; 29] = [
        Glyph::Back,
        Glyph::Forward,
        Glyph::Up,
        Glyph::Refresh,
        Glyph::Home,
        Glyph::Search,
        Glyph::New,
        Glyph::Cut,
        Glyph::Copy,
        Glyph::Paste,
        Glyph::Rename,
        Glyph::Compress,
        Glyph::Delete,
        Glyph::Undo,
        Glyph::Sort,
        Glyph::View,
        Glyph::More,
        Glyph::Pin,
        Glyph::Help,
        Glyph::Close,
        Glyph::Details,
        Glyph::Icons,
        Glyph::Eye,
        Glyph::Open,
        Glyph::Clipboard,
        Glyph::Checked,
        Glyph::Unchecked,
        Glyph::TabFolder,
        Glyph::Filter,
    ];
}

/// The glyph for a button, in the icon set in use: a Nerd Font symbol, a plain Unicode mark
/// that one cell holds in any font, or (without icons) a plain ASCII character where the
/// interface cannot do without one and nothing otherwise.
pub fn ui(set: IconSet, g: Glyph) -> &'static str {
    let (nerd, uni, ascii) = match g {
        Glyph::Back => ("\u{f060}", "←", "<"),
        Glyph::Forward => ("\u{f061}", "→", ">"),
        Glyph::Up => ("\u{f062}", "↑", "^"),
        Glyph::Refresh => ("\u{f021}", "⟳", "@"),
        Glyph::Home => ("\u{f015}", "⌂", ""),
        Glyph::Search => ("\u{f002}", "⌕", ""),
        Glyph::New => ("\u{f067}", "+", "+"),
        Glyph::Cut => ("\u{f0c4}", "✂", ""),
        Glyph::Copy => ("\u{f0c5}", "⧉", ""),
        Glyph::Paste => ("\u{f0ea}", "⎘", ""),
        Glyph::Rename => ("\u{f040}", "✎", ""),
        Glyph::Compress => ("\u{f1c6}", "▣", ""),
        Glyph::Delete => ("\u{f1f8}", "✗", ""),
        Glyph::Undo => ("\u{f0e2}", "↶", ""),
        Glyph::Sort => ("\u{f0dc}", "⇅", ""),
        Glyph::View => ("\u{f00b}", "≣", ""),
        Glyph::More => ("\u{f141}", "⋯", "…"),
        Glyph::Pin => ("\u{f08d}", "⚲", ""),
        Glyph::Help => ("\u{f128}", "?", "?"),
        Glyph::Close => ("\u{f00d}", "✕", "x"),
        Glyph::Details => ("\u{f03a}", "≣", "="),
        Glyph::Icons => ("\u{f009}", "▦", "#"),
        Glyph::Eye => ("\u{f06e}", "◉", "o"),
        Glyph::Open => ("\u{f07c}", "▸", ">"),
        Glyph::Clipboard => ("\u{f0ea}", "⎘", ""),
        Glyph::Checked => ("\u{f046}", "☑", "x"),
        Glyph::Unchecked => ("\u{f096}", "☐", "."),
        Glyph::TabFolder => ("\u{f07b}", "▸", ""),
        Glyph::Filter => ("\u{f0b0}", "▽", ""),
    };
    match set {
        IconSet::Nerd => nerd,
        IconSet::Unicode => uni,
        IconSet::None => ascii,
    }
}

#[cfg(test)]
mod ui_glyph_tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn every_interface_glyph_takes_one_cell_in_every_set() {
        for g in Glyph::ALL {
            for set in [IconSet::Nerd, IconSet::Unicode, IconSet::None] {
                let s = ui(set, g);
                assert!(
                    s.width() <= 1,
                    "{g:?} in {set:?} is {s:?}, {} cells wide",
                    s.width()
                );
            }
            // With a symbol font or Unicode there is always something to draw.
            assert!(!ui(IconSet::Nerd, g).is_empty() && !ui(IconSet::Unicode, g).is_empty());
        }
    }
}
