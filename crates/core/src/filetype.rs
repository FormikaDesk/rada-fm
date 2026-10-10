//! What kind of thing a file is, in words: "JPEG image", "Archive (tar.gz)", "Rust source".
//!
//! The words come from the name alone (extension, a few well-known file names), so choosing
//! them never touches the disk. A table maps extensions to labels; anything it does not know
//! becomes "<EXT> file" (or just "File" when there is no extension).

use std::borrow::Cow;

use crate::fs::FileKind;

/// Extension (lower case, no dot) to label.
const BY_EXTENSION: &[(&str, &str)] = &[
    // pictures
    ("jpg", "JPEG image"),
    ("jpeg", "JPEG image"),
    ("jpe", "JPEG image"),
    ("png", "PNG image"),
    ("gif", "GIF image"),
    ("webp", "WebP image"),
    ("bmp", "BMP image"),
    ("svg", "SVG image"),
    ("ico", "Icon"),
    ("tif", "TIFF image"),
    ("tiff", "TIFF image"),
    ("avif", "AVIF image"),
    ("heic", "HEIC image"),
    ("psd", "Photoshop image"),
    ("ai", "Illustrator drawing"),
    ("eps", "EPS drawing"),
    // video
    ("mp4", "MP4 video"),
    ("m4v", "MP4 video"),
    ("mkv", "Matroska video"),
    ("webm", "WebM video"),
    ("mov", "QuickTime video"),
    ("avi", "AVI video"),
    ("wmv", "Windows Media video"),
    ("flv", "Flash video"),
    // audio
    ("mp3", "MP3 audio"),
    ("flac", "FLAC audio"),
    ("ogg", "Ogg audio"),
    ("opus", "Opus audio"),
    ("wav", "WAV audio"),
    ("m4a", "MPEG-4 audio"),
    ("aac", "AAC audio"),
    ("wma", "Windows Media audio"),
    ("mid", "MIDI sequence"),
    // documents
    ("pdf", "PDF document"),
    ("txt", "Text document"),
    ("md", "Markdown"),
    ("markdown", "Markdown"),
    ("mdx", "Markdown"),
    ("rst", "reStructuredText"),
    ("tex", "LaTeX document"),
    ("log", "Log file"),
    ("rtf", "Rich text document"),
    ("doc", "Word document"),
    ("docx", "Word document"),
    ("odt", "OpenDocument text"),
    ("xls", "Excel spreadsheet"),
    ("xlsx", "Excel spreadsheet"),
    ("ods", "OpenDocument spreadsheet"),
    ("ppt", "PowerPoint presentation"),
    ("pptx", "PowerPoint presentation"),
    ("odp", "OpenDocument presentation"),
    ("epub", "E-book"),
    ("html", "HTML document"),
    ("htm", "HTML document"),
    // data and settings
    ("json", "JSON data"),
    ("jsonc", "JSON data"),
    ("ndjson", "JSON data"),
    ("yaml", "YAML data"),
    ("yml", "YAML data"),
    ("xml", "XML data"),
    ("csv", "CSV data"),
    ("tsv", "TSV data"),
    ("parquet", "Parquet data"),
    ("db", "Database"),
    ("sqlite", "SQLite database"),
    ("sqlite3", "SQLite database"),
    ("sql", "SQL script"),
    ("toml", "Settings file"),
    ("ini", "Settings file"),
    ("conf", "Settings file"),
    ("cfg", "Settings file"),
    ("env", "Settings file"),
    ("properties", "Settings file"),
    ("lock", "Lock file"),
    // source code
    ("rs", "Rust source"),
    ("py", "Python source"),
    ("pyw", "Python source"),
    ("js", "JavaScript source"),
    ("mjs", "JavaScript source"),
    ("cjs", "JavaScript source"),
    ("jsx", "JavaScript source"),
    ("ts", "TypeScript source"),
    ("tsx", "TypeScript source"),
    ("go", "Go source"),
    ("c", "C source"),
    ("h", "C header"),
    ("cpp", "C++ source"),
    ("cc", "C++ source"),
    ("cxx", "C++ source"),
    ("hpp", "C++ header"),
    ("java", "Java source"),
    ("kt", "Kotlin source"),
    ("swift", "Swift source"),
    ("rb", "Ruby source"),
    ("php", "PHP source"),
    ("lua", "Lua source"),
    ("zig", "Zig source"),
    ("cs", "C# source"),
    ("scala", "Scala source"),
    ("hs", "Haskell source"),
    ("ex", "Elixir source"),
    ("exs", "Elixir source"),
    ("dart", "Dart source"),
    ("nix", "Nix source"),
    ("css", "Stylesheet"),
    ("scss", "Stylesheet"),
    ("vue", "Vue component"),
    ("svelte", "Svelte component"),
    ("sh", "Shell script"),
    ("bash", "Shell script"),
    ("zsh", "Shell script"),
    ("fish", "Shell script"),
    ("ps1", "PowerShell script"),
    ("bat", "Batch file"),
    ("cmd", "Batch file"),
    // archives and images of disks
    ("zip", "Archive (ZIP)"),
    ("tar", "Archive (tar)"),
    ("gz", "Archive (gzip)"),
    ("bz2", "Archive (bzip2)"),
    ("xz", "Archive (xz)"),
    ("zst", "Archive (zstd)"),
    ("7z", "Archive (7z)"),
    ("rar", "Archive (RAR)"),
    ("tgz", "Archive (tar.gz)"),
    ("tbz2", "Archive (tar.bz2)"),
    ("txz", "Archive (tar.xz)"),
    ("iso", "Disk image (ISO)"),
    ("img", "Disk image"),
    ("deb", "Debian package"),
    ("rpm", "RPM package"),
    ("apk", "Android package"),
    ("jar", "Java archive"),
    // programs
    ("exe", "Application"),
    ("msi", "Windows installer"),
    ("dll", "Library"),
    ("so", "Shared library"),
    ("dylib", "Shared library"),
    ("o", "Object file"),
    ("a", "Static library"),
    ("appimage", "AppImage"),
    ("wasm", "WebAssembly module"),
    ("bin", "Binary file"),
    ("desktop", "Launcher"),
    ("ttf", "TrueType font"),
    ("otf", "OpenType font"),
    ("woff", "Web font"),
    ("woff2", "Web font"),
];

/// Names that mean something whatever their extension.
const BY_NAME: &[(&str, &str)] = &[
    ("makefile", "Makefile"),
    ("dockerfile", "Dockerfile"),
    ("justfile", "Justfile"),
    ("cmakelists.txt", "CMake script"),
    ("license", "License"),
    ("copying", "License"),
    ("readme", "Text document"),
    ("authors", "Text document"),
    ("changelog", "Text document"),
    (".gitignore", "Git ignore list"),
    (".gitattributes", "Git settings"),
    (".gitmodules", "Git settings"),
    (".bashrc", "Shell settings"),
    (".zshrc", "Shell settings"),
    (".profile", "Shell settings"),
];

/// Double extensions that name a kind of their own, longest first.
const COMPOUND: &[(&str, &str)] = &[
    (".tar.gz", "Archive (tar.gz)"),
    (".tar.bz2", "Archive (tar.bz2)"),
    (".tar.xz", "Archive (tar.xz)"),
    (".tar.zst", "Archive (tar.zst)"),
    (".tar.zstd", "Archive (tar.zst)"),
    (".tar.lz", "Archive (tar.lz)"),
    (".tar.z", "Archive (tar.Z)"),
];

/// The label for a name. `kind` and `executable` only matter for things that are not plain
/// files or have no extension.
pub fn label(name: &str, kind: FileKind, is_dir: bool, executable: bool) -> Cow<'static, str> {
    if is_dir {
        return Cow::Borrowed("Folder");
    }
    match kind {
        FileKind::Dir => return Cow::Borrowed("Folder"),
        FileKind::Symlink => return Cow::Borrowed("Link"),
        FileKind::Other => return Cow::Borrowed("Special file"),
        FileKind::File => {}
    }
    let lower = name.to_lowercase();
    if let Some((_, l)) = BY_NAME.iter().find(|(n, _)| *n == lower) {
        return Cow::Borrowed(l);
    }
    if let Some((_, l)) = COMPOUND.iter().find(|(s, _)| lower.ends_with(s)) {
        return Cow::Borrowed(l);
    }
    match lower.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            if let Some((_, l)) = BY_EXTENSION.iter().find(|(e, _)| *e == ext) {
                Cow::Borrowed(l)
            } else if ext.chars().count() <= 8 && ext.chars().all(|c| c.is_alphanumeric()) {
                Cow::Owned(format!("{} file", ext.to_uppercase()))
            } else {
                Cow::Borrowed("File")
            }
        }
        _ if executable => Cow::Borrowed("Program"),
        _ => Cow::Borrowed("File"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> String {
        label(name, FileKind::File, false, false).into_owned()
    }

    #[test]
    fn the_words_people_expect() {
        for (name, want) in [
            ("photo.jpg", "JPEG image"),
            ("PHOTO.JPEG", "JPEG image"),
            ("a.png", "PNG image"),
            ("backup.zip", "Archive (ZIP)"),
            ("archive.tar.gz", "Archive (tar.gz)"),
            ("archive.tgz", "Archive (tar.gz)"),
            ("data.tar.zst", "Archive (tar.zst)"),
            ("old.tar.bz2", "Archive (tar.bz2)"),
            ("main.rs", "Rust source"),
            ("setup.sh", "Shell script"),
            ("report.pdf", "PDF document"),
            ("demo.mp4", "MP4 video"),
            ("song.mp3", "MP3 audio"),
            ("notes.txt", "Text document"),
            ("README.md", "Markdown"),
            ("data.json", "JSON data"),
            ("Cargo.toml", "Settings file"),
            ("app.ini", "Settings file"),
            ("nginx.conf", "Settings file"),
            ("logo.svg", "SVG image"),
            ("Makefile", "Makefile"),
            ("LICENSE", "License"),
        ] {
            assert_eq!(file(name), want, "{name}");
        }
    }

    #[test]
    fn what_the_table_does_not_know_falls_back_to_the_extension() {
        assert_eq!(file("model.xyz"), "XYZ file");
        assert_eq!(file("scene.blend"), "BLEND file");
        assert_eq!(file("noextension"), "File");
        // A leading dot is part of the name, not an extension.
        assert_eq!(file(".hidden"), "File");
        // Absurd extensions do not become labels.
        assert_eq!(file("x.not-an-extension-at-all"), "File");
    }

    #[test]
    fn folders_links_and_programs() {
        assert_eq!(label("src", FileKind::Dir, false, false), "Folder");
        assert_eq!(
            label("link-to-dir", FileKind::Symlink, true, false),
            "Folder"
        );
        assert_eq!(label("l", FileKind::Symlink, false, false), "Link");
        assert_eq!(label("run", FileKind::File, false, true), "Program");
        assert_eq!(label("sock", FileKind::Other, false, false), "Special file");
    }
}
