//! The words of the Type column: "JPEG image" in English, "Immagine JPEG" in Italian.
//!
//! The core names a kind of file in English (it is also what the column sorts by); the
//! interface shows it through [`type_label`]. A handful of labels are translated one by one,
//! the many that follow a pattern ("X image", "X source", "Archive (X)") by their last word.

use std::borrow::Cow;

use super::{Lang, lang};

/// Labels that do not follow a pattern.
const EXACT: &[(&str, &str)] = &[
    ("Folder", "Cartella"),
    ("Link", "Collegamento"),
    ("Broken link", "Collegamento interrotto"),
    ("Special file", "File speciale"),
    ("Unreadable", "Non leggibile"),
    ("File", "File"),
    ("Program", "Programma"),
    ("Application", "Applicazione"),
    ("Package", "Pacchetto"),
    ("Bundle", "Bundle"),
    ("Framework", "Framework"),
    ("Icon", "Icona"),
    ("Text document", "Documento di testo"),
    ("Rich text document", "Documento RTF"),
    ("Settings file", "Impostazioni"),
    ("Shell settings", "Impostazioni shell"),
    ("Git settings", "Impostazioni Git"),
    ("Git ignore list", "Elenco ignorati di Git"),
    ("Lock file", "File di blocco"),
    ("Log file", "File di log"),
    ("License", "Licenza"),
    ("Launcher", "Avviatore"),
    ("Library", "Libreria"),
    ("Shared library", "Libreria condivisa"),
    ("Static library", "Libreria statica"),
    ("Photos library", "Libreria Foto"),
    ("Object file", "File oggetto"),
    ("Binary file", "File binario"),
    ("Batch file", "File batch"),
    ("Shell script", "Script shell"),
    ("Java archive", "Archivio Java"),
    ("Disk image", "Immagine disco"),
    ("Disk image (ISO)", "Immagine disco (ISO)"),
    ("MIDI sequence", "Sequenza MIDI"),
    ("Windows installer", "Programma di installazione Windows"),
    ("Web font", "Font web"),
    ("Stylesheet", "Foglio di stile"),
    ("OpenDocument text", "Testo OpenDocument"),
    ("AppImage", "AppImage"),
    ("Database", "Database"),
    ("Dockerfile", "Dockerfile"),
    ("Makefile", "Makefile"),
    ("Justfile", "Justfile"),
    ("Markdown", "Markdown"),
    ("E-book", "E-book"),
    ("reStructuredText", "reStructuredText"),
];

/// Patterns: a label ending in the English word is the Italian word followed by the rest.
const SUFFIXES: &[(&str, &str)] = &[
    (" source", "Sorgente"),
    (" image", "Immagine"),
    (" video", "Video"),
    (" audio", "Audio"),
    (" document", "Documento"),
    (" data", "Dati"),
    (" file", "File"),
    (" header", "Header"),
    (" component", "Componente"),
    (" package", "Pacchetto"),
    (" script", "Script"),
    (" database", "Database"),
    (" font", "Font"),
    (" spreadsheet", "Foglio"),
    (" presentation", "Presentazione"),
    (" drawing", "Disegno"),
    (" module", "Modulo"),
];

/// A type label as the language in use writes it.
pub fn type_label(en: &str) -> Cow<'_, str> {
    type_label_in(en, lang())
}

/// A type label in a given language.
pub fn type_label_in(en: &str, l: Lang) -> Cow<'_, str> {
    if l == Lang::En {
        return Cow::Borrowed(en);
    }
    if let Some((_, it)) = EXACT.iter().find(|(e, _)| *e == en) {
        return Cow::Borrowed(it);
    }
    if let Some(inner) = en
        .strip_prefix("Archive (")
        .and_then(|r| r.strip_suffix(')'))
    {
        return Cow::Owned(format!("Archivio ({inner})"));
    }
    if let Some(inner) = en.strip_prefix("Archive") {
        return Cow::Owned(format!("Archivio{inner}"));
    }
    for (word, it) in SUFFIXES {
        if let Some(rest) = en.strip_suffix(word)
            && !rest.is_empty()
        {
            return Cow::Owned(format!("{it} {rest}"));
        }
    }
    Cow::Borrowed(en)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(en: &str) -> String {
        type_label_in(en, Lang::It).into_owned()
    }

    #[test]
    fn the_words_of_the_reference_screens() {
        for (en, want) in [
            ("JPEG image", "Immagine JPEG"),
            ("Folder", "Cartella"),
            ("Archive (tar.gz)", "Archivio (tar.gz)"),
            ("Archive (ZIP)", "Archivio (ZIP)"),
            ("Settings file", "Impostazioni"),
            ("JSON data", "Dati JSON"),
            ("MP4 video", "Video MP4"),
            ("Rust source", "Sorgente Rust"),
            ("SVG image", "Immagine SVG"),
            ("Text document", "Documento di testo"),
            ("PDF document", "Documento PDF"),
            ("Shell script", "Script shell"),
            ("MP3 audio", "Audio MP3"),
            ("Markdown", "Markdown"),
            ("PNG image", "Immagine PNG"),
        ] {
            assert_eq!(it(en), want, "{en}");
        }
        // The fallback of the core for an extension it does not know.
        assert_eq!(it("XYZ file"), "File XYZ");
        assert_eq!(type_label_in("JPEG image", Lang::En), "JPEG image");
    }

    /// Every label the core can produce reads in Italian: no English word of a pattern is
    /// left behind.
    #[test]
    fn every_label_of_the_core_has_an_italian_form() {
        let english_words = [
            "source",
            "image",
            "document",
            "data",
            "component",
            "package",
            "spreadsheet",
            "presentation",
            "drawing",
            "module",
            "library",
            "archive",
            "settings",
            "list",
            "installer",
        ];
        let mut labels = crate::filetype::all_labels();
        labels.extend([
            "Broken link",
            "Unreadable",
            "Application",
            "Framework",
            "Bundle",
            "Pages document",
            "Numbers spreadsheet",
            "Keynote presentation",
            "Rich text document",
            "Photos library",
            "Disk image",
            "Package",
        ]);
        let mut left = Vec::new();
        for en in labels {
            let it = it(en);
            let lower = it.to_lowercase();
            let leftover = english_words
                .iter()
                .any(|w| lower.split(|c: char| !c.is_alphabetic()).any(|x| x == *w));
            if leftover {
                left.push(format!("{en} -> {it}"));
            }
            if it == en && !SAME.contains(&en) {
                left.push(format!("{en}: not translated"));
            }
        }
        assert!(left.is_empty(), "{}", left.join("\n"));
    }

    /// Labels that are the same word in both languages (a name, a format).
    const SAME: &[&str] = &[
        "AppImage",
        "Bundle",
        "Database",
        "Dockerfile",
        "E-book",
        "File",
        "Framework",
        "Justfile",
        "Makefile",
        "Markdown",
        "reStructuredText",
    ];
}
