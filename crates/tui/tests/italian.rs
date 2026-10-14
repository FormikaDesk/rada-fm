//! The interface in Italian: the words of the reference screens are there, and the layout
//! holds the longest Italian words at the widths the design is made for.

mod common;

use common::*;
use rada_core::i18n::{Lang, with_lang};
use unicode_width::UnicodeWidthStr;

/// What the design shows at each width: (columns, rows).
const SIZES: [(u16, u16); 4] = [(170, 42), (140, 35), (120, 30), (80, 24)];

fn lines(h: &mut H) -> Vec<String> {
    h.screen().lines().map(str::to_string).collect()
}

#[test]
fn the_words_of_the_reference_screens() {
    with_lang(Lang::It, || {
        let mut h = demo_scene(170, 42);
        let s = h.screen();
        for word in [
            "Nuovo",
            "Taglia",
            "Copia",
            "Incolla",
            "Rinomina",
            "Comprimi",
            "Elimina",
            "Ordina",
            "Tag",
            "Annulla",
            "PREFERITI",
            "DISPOSITIVI",
            "Nome",
            "Modificato",
            "Tipo",
            "Dimensione",
            "Immagine JPEG",
            "Cartella",
            "Archivio (tar.gz)",
            "Documento di testo",
            "Sorgente Rust",
            "Script shell",
            "Video MP4",
            "Audio MP3",
            "Dati JSON",
            "Impostazioni",
            "3 giorni fa",
            "Ieri",
            "Oggi 14:10",
            "70,7 KB",
            "elementi",
            "liberi",
            "Aiuto",
            "Dettagli",
            "Cerca file o digita un comando…",
            "apri",
            "seleziona",
            "incolla",
            "cestino",
            "rinomina",
            "annulla",
            "nuova scheda",
            "comandi",
        ] {
            assert!(s.contains(word), "{word:?} is not on the screen:\n{s}");
        }
        // And no English is left in what the interface owns.
        for word in [
            "Date modified",
            "Cut",
            "Paste",
            "PINNED",
            "DEVICES",
            "free",
            "ready to",
        ] {
            assert!(!s.contains(word), "{word:?} should be Italian:\n{s}");
        }
    });
}

#[test]
fn a_selection_and_the_clipboard_are_in_italian() {
    with_lang(Lang::It, || {
        let mut h = demo_scene(170, 42);
        h.keys("g");
        for _ in 0..3 {
            h.press("space");
        }
        h.app.clipboard = Some(rada_tui::app::Clip {
            mode: rada_core::ops::TransferMode::Copy,
            paths: vec![h.app.current().unwrap().path.clone(); 2],
        });
        let s = h.screen();
        assert!(s.contains("3 selezionati"), "{s}");
        assert!(s.contains("2 elementi da incollare"), "{s}");
        assert!(
            !s.contains("selected") && !s.contains("ready to paste"),
            "{s}"
        );
    });
}

#[test]
fn the_layout_holds_the_longest_italian_words_at_170_140_120_and_80_columns() {
    with_lang(Lang::It, || {
        for (w, hh) in SIZES {
            let mut h = demo_scene(w, hh);
            h.app.marked.clear();
            let rows = lines(&mut h);
            // Nothing is wider than the screen.
            for l in &rows {
                assert!(
                    l.trim_end().width() <= w as usize,
                    "{w}: a line is wider than the screen: {l:?}"
                );
            }
            let s = rows.join("\n");
            // The command bar keeps its buttons, with words down to 120 columns and then as
            // icons, never overlapping: every word that is there is whole.
            if w >= 120 {
                for word in [
                    "Taglia", "Copia", "Incolla", "Rinomina", "Comprimi", "Elimina", "Ordina",
                ] {
                    assert!(s.contains(word), "{w}: {word}\n{s}");
                }
                assert!(s.contains("Annulla"), "{w}\n{s}");
            }
            // The column titles are whole (with the arrow of the sort for the first).
            assert!(s.contains("Nome ↑"), "{w}: Nome ↑\n{s}");
            if w >= 120 {
                assert!(s.contains("Modificato"), "{w}\n{s}");
            }
            if w >= 140 {
                assert!(s.contains("Tipo") && s.contains("Dimensione"), "{w}\n{s}");
            }
            // The headings of the sidebar and the hints are whole.
            if w >= 100 {
                assert!(
                    s.contains("PREFERITI") && s.contains("DISPOSITIVI"),
                    "{w}\n{s}"
                );
            }
            // The cards keep their corners: every card's top and bottom edge is whole.
            let tops = rows
                .iter()
                .filter(|l| l.trim_start().starts_with('╭'))
                .count();
            assert_eq!(tops, 1, "{w}: one row of card tops");
            let first = rows
                .iter()
                .position(|l| l.trim_start().starts_with('╭'))
                .unwrap();
            let last = rows
                .iter()
                .rposition(|l| l.trim_start().starts_with('╰'))
                .unwrap();
            for l in &rows[first..=last] {
                let t = l.trim_end();
                assert!(
                    t.starts_with(['╭', '│', '╰']) && t.ends_with(['╮', '│', '╯']),
                    "{w}: a card row loses its border: {t:?}"
                );
            }
        }
    });
}

#[test]
fn the_size_column_is_wide_enough_for_its_title_and_the_arrow_in_italian() {
    with_lang(Lang::It, || {
        let cols = rada_tui::ui::list_columns(
            120,
            rada_tui::ui::ListOpts::for_terminal(170, false),
            2,
            rada_tui::fmt::DateStyle::Relative,
        );
        assert!(cols.size >= "Dimensione ↑".width(), "{}", cols.size);
        assert!(cols.date >= "Modificato ↑".width(), "{}", cols.date);
    });
}

#[test]
fn english_is_unchanged_and_italian_follows_the_thread_only() {
    let mut h = demo_scene(170, 42);
    let s = h.screen();
    assert!(s.contains("Date modified") && s.contains("PINNED"), "{s}");
    assert!(!s.contains("Modificato"));
    let italian = std::thread::spawn(|| {
        with_lang(Lang::It, || {
            let mut h = demo_scene(170, 42);
            h.screen().contains("Modificato")
        })
    })
    .join()
    .unwrap();
    assert!(italian);
}
