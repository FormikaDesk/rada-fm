//! The mark of a program in "Open with…": a Nerd Font glyph and a colour for the programs
//! people know, picked from what names the program on this system — the desktop file id on
//! Linux (`org.mozilla.firefox.desktop`), the bundle on macOS, the program on Windows — and
//! failing that from its name and what it runs. A program this does not know gets the mark of
//! the kind of file it is being offered for.
//!
//! The colours are the theme's own: each program takes the token that is closest to the
//! colour people know it by (Firefox the orange of documents, VLC too, a PDF reader the red of
//! errors, and so on), so that every theme stays whole.

use rada_core::platform::OpenApp;
use ratatui::style::Color;

use crate::icons::IconSet;
use crate::theme::Theme;

/// The programs that are known by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prog {
    Firefox,
    Browser,
    Writer,
    Calc,
    Impress,
    Office,
    Gimp,
    Krita,
    Inkscape,
    VideoPlayer,
    AudioPlayer,
    Pdf,
    Code,
    TextEditor,
    Files,
    Terminal,
    ImageViewer,
    Archive,
    Mail,
}

/// Which known program, if any, `hay` (lower case: its id, name and command) names.
fn known(hay: &str) -> Option<Prog> {
    // The order matters: the specific before the general.
    const TABLE: &[(&[&str], Prog)] = &[
        (&["firefox"], Prog::Firefox),
        (
            &[
                "chromium",
                "chrome",
                "brave",
                "vivaldi",
                "microsoft-edge",
                "msedge",
                "opera",
                "epiphany",
                "safari",
                "falkon",
                "librewolf",
                "zen-browser",
            ],
            Prog::Browser,
        ),
        (
            &[
                "libreoffice-writer",
                "lowriter",
                "writer",
                "winword",
                "pages",
            ],
            Prog::Writer,
        ),
        (
            &["libreoffice-calc", "localc", "calc", "excel", "numbers"],
            Prog::Calc,
        ),
        (
            &[
                "libreoffice-impress",
                "loimpress",
                "impress",
                "powerpnt",
                "keynote",
            ],
            Prog::Impress,
        ),
        (
            &["libreoffice", "onlyoffice", "openoffice", "wps"],
            Prog::Office,
        ),
        (&["gimp"], Prog::Gimp),
        (&["krita"], Prog::Krita),
        (&["inkscape"], Prog::Inkscape),
        (
            &[
                "vlc",
                "mpv",
                "totem",
                "celluloid",
                "smplayer",
                "haruna",
                "parole",
                "dragon",
                "quicktime",
                "movies",
                "videos",
                "kodi",
            ],
            Prog::VideoPlayer,
        ),
        (
            &[
                "rhythmbox",
                "audacious",
                "lollypop",
                "clementine",
                "amarok",
                "strawberry",
                "elisa",
                "music",
                "audacity",
                "deadbeef",
            ],
            Prog::AudioPlayer,
        ),
        (
            &[
                "okular", "evince", "zathura", "atril", "mupdf", "xreader", "papers", "foliate",
                "acroread", "acrobat", "xpdf", "qpdfview", "sioyek",
            ],
            Prog::Pdf,
        ),
        (
            &[
                "vscode",
                "code-oss",
                "codium",
                "visual-studio-code",
                "code.desktop",
                "zed",
                "jetbrains",
                "intellij",
                "pycharm",
                "clion",
                "android-studio",
                "xcode",
            ],
            Prog::Code,
        ),
        (
            &[
                "kate",
                "gedit",
                "mousepad",
                "pluma",
                "xed",
                "gnome-text-editor",
                "kwrite",
                "textedit",
                "notepad",
                "nano",
                "vim",
                "nvim",
                "neovim",
                "emacs",
                "micro",
                "helix",
                "sublime",
                "geany",
                "leafpad",
                "text-editor",
                "gnome-builder",
                "lite-xl",
            ],
            Prog::TextEditor,
        ),
        (
            &[
                "nautilus", "dolphin", "thunar", "pcmanfm", "nemo", "caja", "finder", "explorer",
                "krusader", "files", "spacefm",
            ],
            Prog::Files,
        ),
        (
            &[
                "ghostty",
                "kitty",
                "alacritty",
                "wezterm",
                "foot",
                "konsole",
                "gnome-terminal",
                "xterm",
                "terminator",
                "tilix",
                "iterm",
                "terminal",
                "windowsterminal",
                "cmd.exe",
                "powershell",
            ],
            Prog::Terminal,
        ),
        (
            &[
                "loupe",
                "eog",
                "eye-of-gnome",
                "gwenview",
                "feh",
                "imv",
                "ristretto",
                "viewnior",
                "shotwell",
                "image-viewer",
                "imageviewer",
                "preview",
                "photos",
                "nomacs",
                "geeqie",
                "sxiv",
                "oculante",
                "gthumb",
                "mirage",
                "pinta",
                "mspaint",
                "paint",
            ],
            Prog::ImageViewer,
        ),
        (
            &[
                "file-roller",
                "ark",
                "engrampa",
                "xarchiver",
                "7zip",
                "7-zip",
                "peazip",
                "archive-utility",
                "keka",
                "winrar",
                "bsdtar",
            ],
            Prog::Archive,
        ),
        (
            &["thunderbird", "evolution", "geary", "kmail", "mail"],
            Prog::Mail,
        ),
    ];
    // Whole words first (so that "paint" is not found in "paint-shop"), then fragments.
    let words: Vec<&str> = hay
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty())
        .collect();
    for (names, prog) in TABLE {
        if names.iter().any(|n| words.contains(n)) {
            return Some(*prog);
        }
    }
    for (names, prog) in TABLE {
        if names.iter().any(|n| n.len() >= 4 && hay.contains(n)) {
            return Some(*prog);
        }
    }
    None
}

/// The program a description stands for.
pub fn recognise(app: &OpenApp) -> Option<Prog> {
    let id = app.id.to_lowercase();
    let name = app.name.to_lowercase();
    let exec = app
        .exec
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_lowercase();
    // The id is the most exact, then the command, then the name people read.
    known(&id).or_else(|| known(&exec)).or_else(|| known(&name))
}

/// The glyph of the kind of file a program is offered for, when the program is not known.
fn by_kind(kind: Option<&str>) -> Prog {
    match kind.unwrap_or("") {
        k if k.starts_with("image/") => Prog::ImageViewer,
        k if k.starts_with("video/") => Prog::VideoPlayer,
        k if k.starts_with("audio/") => Prog::AudioPlayer,
        "application/pdf" => Prog::Pdf,
        k if k.starts_with("text/") => Prog::TextEditor,
        k if k.contains("zip") || k.contains("tar") || k.contains("compress") => Prog::Archive,
        "inode/directory" => Prog::Files,
        _ => Prog::Office,
    }
}

/// The glyph of a program in the icon set in use (empty where there is none) and its colour.
pub fn look(app: &OpenApp, kind: Option<&str>, set: IconSet, th: &Theme) -> (&'static str, Color) {
    let prog = recognise(app).unwrap_or_else(|| by_kind(kind));
    let (nerd, color) = match prog {
        Prog::Firefox => ("\u{f269}", th.kinds.doc),
        Prog::Browser => ("\u{f268}", th.accent),
        Prog::Writer => ("\u{f1c2}", th.accent),
        Prog::Calc => ("\u{f1c3}", th.success),
        Prog::Impress => ("\u{f1c4}", th.kinds.doc),
        Prog::Office => ("\u{f15c}", th.kinds.text),
        Prog::Gimp => ("\u{f1fc}", th.kinds.code),
        Prog::Krita => ("\u{f1fc}", th.kinds.markup),
        Prog::Inkscape => ("\u{f040}", th.kinds.vector),
        Prog::VideoPlayer => ("\u{f144}", th.kinds.video),
        Prog::AudioPlayer => ("\u{f001}", th.kinds.audio),
        Prog::Pdf => ("\u{f1c1}", th.error),
        Prog::Code => ("\u{f121}", th.accent),
        Prog::TextEditor => ("\u{f15c}", th.kinds.text),
        Prog::Files => ("\u{f07b}", th.kinds.folder),
        Prog::Terminal => ("\u{f120}", th.success),
        Prog::ImageViewer => ("\u{f03e}", th.kinds.image),
        Prog::Archive => ("\u{f1c6}", th.kinds.archive),
        Prog::Mail => ("\u{f0e0}", th.kinds.link),
    };
    match set {
        IconSet::Nerd => (nerd, color),
        // The plain sets have one mark that every font draws: a dot in the colour.
        IconSet::UnicodeIcons | IconSet::UnicodeStd => ("●", color),
        IconSet::None => ("", color),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ColorDepth;

    fn app(id: &str, name: &str, exec: &str) -> OpenApp {
        OpenApp {
            id: id.into(),
            name: name.into(),
            exec: exec.into(),
            terminal: false,
            is_default: false,
            source: None,
            icon: None,
        }
    }

    #[test]
    fn the_programs_people_know_are_recognised_by_id_command_or_name() {
        let cases = [
            (
                "org.mozilla.firefox.desktop",
                "Firefox",
                "firefox %u",
                Prog::Firefox,
            ),
            ("chromium.desktop", "Chromium", "chromium %U", Prog::Browser),
            (
                "libreoffice-writer.desktop",
                "LibreOffice Writer",
                "libreoffice --writer %U",
                Prog::Writer,
            ),
            (
                "libreoffice-calc.desktop",
                "LibreOffice Calc",
                "libreoffice --calc %U",
                Prog::Calc,
            ),
            (
                "libreoffice-impress.desktop",
                "LibreOffice Impress",
                "libreoffice --impress %U",
                Prog::Impress,
            ),
            ("gimp.desktop", "GIMP", "gimp-2.10 %U", Prog::Gimp),
            ("org.kde.krita.desktop", "Krita", "krita %F", Prog::Krita),
            (
                "org.inkscape.Inkscape.desktop",
                "Inkscape",
                "inkscape %F",
                Prog::Inkscape,
            ),
            (
                "vlc.desktop",
                "VLC media player",
                "/usr/bin/vlc --started-from-file %U",
                Prog::VideoPlayer,
            ),
            (
                "mpv.desktop",
                "mpv Media Player",
                "mpv -- %U",
                Prog::VideoPlayer,
            ),
            ("org.kde.okular.desktop", "Okular", "okular %U", Prog::Pdf),
            (
                "org.gnome.Evince.desktop",
                "Document Viewer",
                "evince %U",
                Prog::Pdf,
            ),
            ("zathura.desktop", "Zathura", "zathura %f", Prog::Pdf),
            (
                "code.desktop",
                "Visual Studio Code",
                "/usr/share/code/code %F",
                Prog::Code,
            ),
            ("org.kde.kate.desktop", "Kate", "kate %U", Prog::TextEditor),
            (
                "org.gnome.gedit.desktop",
                "gedit",
                "gedit %U",
                Prog::TextEditor,
            ),
            (
                "org.gnome.Nautilus.desktop",
                "Files",
                "nautilus %U",
                Prog::Files,
            ),
            (
                "org.kde.dolphin.desktop",
                "Dolphin",
                "dolphin %u",
                Prog::Files,
            ),
            (
                "thunar.desktop",
                "Thunar File Manager",
                "thunar %F",
                Prog::Files,
            ),
            (
                "com.mitchellh.ghostty.desktop",
                "Ghostty",
                "ghostty",
                Prog::Terminal,
            ),
            ("kitty.desktop", "kitty", "kitty", Prog::Terminal),
            (
                "org.gnome.Loupe.desktop",
                "Image Viewer",
                "loupe %U",
                Prog::ImageViewer,
            ),
            (
                "org.gnome.eog.desktop",
                "Eye of GNOME",
                "eog %U",
                Prog::ImageViewer,
            ),
            (
                "com.apple.Preview",
                "Preview",
                "Preview.app",
                Prog::ImageViewer,
            ),
            (
                "com.apple.TextEdit",
                "TextEdit",
                "TextEdit.app",
                Prog::TextEditor,
            ),
            ("Notepad", "Notepad", "notepad.exe", Prog::TextEditor),
            ("Paint", "Paint", "mspaint.exe", Prog::ImageViewer),
            ("Photos", "Photos", "Microsoft.Photos", Prog::ImageViewer),
        ];
        for (id, name, exec, want) in cases {
            assert_eq!(recognise(&app(id, name, exec)), Some(want), "{id}");
        }
        assert_eq!(recognise(&app("x.desktop", "Frobnicator", "frob %f")), None);
    }

    #[test]
    fn an_unknown_program_takes_the_mark_of_the_kind_of_file() {
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        let a = app("x.desktop", "Frobnicator", "frob %f");
        let (g, c) = look(&a, Some("image/jpeg"), IconSet::Nerd, &th);
        assert_eq!(g, "\u{f03e}");
        assert_eq!(c, th.kinds.image);
        let (g, _) = look(&a, Some("video/mp4"), IconSet::Nerd, &th);
        assert_eq!(g, "\u{f144}");
    }

    #[test]
    fn every_set_gets_one_cell_or_nothing() {
        use unicode_width::UnicodeWidthStr;
        let th = Theme::named("rada", ColorDepth::True).unwrap();
        for set in [
            IconSet::Nerd,
            IconSet::UnicodeIcons,
            IconSet::UnicodeStd,
            IconSet::None,
        ] {
            let (g, _) = look(
                &app("firefox.desktop", "Firefox", "firefox"),
                None,
                set,
                &th,
            );
            assert!(g.width() <= 1, "{set:?}: {g:?}");
        }
        let (g, _) = look(
            &app("firefox.desktop", "Firefox", "firefox"),
            None,
            IconSet::None,
            &th,
        );
        assert_eq!(g, "");
    }
}
