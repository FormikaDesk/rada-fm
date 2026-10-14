//! The command line speaks the language of the system, or the one the config file asks for:
//! `--help`, the errors, `rada doctor`, `rada setup` and `rada update`. The real program is run
//! with HOME and the data folders inside a temporary folder; nothing is installed or fetched.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct World {
    root: tempfile::TempDir,
    /// `settle` wrote the configuration (so it may write it again for another locale).
    settled: std::cell::Cell<bool>,
}

impl World {
    fn new() -> World {
        let root = tempfile::Builder::new()
            .prefix("rada-lang-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap();
        for d in ["home", "config/rada", "data", "state", "cache"] {
            fs::create_dir_all(root.path().join(d)).unwrap();
        }
        World {
            root,
            settled: std::cell::Cell::new(false),
        }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    /// Make the language of the system this locale. On Linux the program reads the locale
    /// variables; on macOS and Windows it reads the system's settings, which a test cannot
    /// set, so the same language is asked for in the configuration (unless a test wrote one).
    fn settle(&self, locale: &str) {
        let file = self.p("config/rada/config.toml");
        if cfg!(not(target_os = "linux")) && (!file.exists() || self.settled.get()) {
            self.settled.set(true);
            let lang = if locale.starts_with("it") { "it" } else { "en" };
            fs::write(file, format!("language = \"{lang}\"\n")).unwrap();
        }
    }

    /// Run rada with this locale (and nothing else of the language set).
    fn rada(&self, locale: &str, args: &[&str]) -> Output {
        self.settle(locale);
        self.rada_as_is(locale, args)
    }

    fn rada_as_is(&self, locale: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rada"))
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("LC_ALL", locale)
            .env("HOME", self.p("home"))
            .env("USERPROFILE", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("config"))
            .env("XDG_DATA_HOME", self.p("data"))
            .env("XDG_STATE_HOME", self.p("state"))
            .env("XDG_CACHE_HOME", self.p("cache"))
            .env("APPDATA", self.p("config"))
            .env("LOCALAPPDATA", self.p("data"))
            .env("TERM", "xterm-256color")
            .env("RADA_NO_TERM_QUERY", "1")
            .output()
            .unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

const IT: &str = "it_IT.UTF-8";
const EN: &str = "en_US.UTF-8";

#[test]
fn the_help_pages_follow_the_system_language() {
    let w = World::new();
    for args in [
        &["--help"][..],
        &["update", "--help"],
        &["setup", "--help"],
        &["setup", "desktop", "--help"],
        &["setup", "font", "--help"],
        &["doctor", "--help"],
    ] {
        let it = text(&w.rada(IT, args));
        assert!(it.contains("Uso:"), "{args:?}:\n{it}");
        for english in ["Usage:", "Options:", "Commands:", "Print help"] {
            assert!(!it.contains(english), "{args:?}: {english:?} left:\n{it}");
        }
        let en = text(&w.rada(EN, args));
        assert!(
            en.contains("Usage:") && !en.contains("Uso:"),
            "{args:?}:\n{en}"
        );
    }
    let it = text(&w.rada(IT, &["--help"]));
    for word in ["Comandi:", "Opzioni:", "Mostra i file nascosti all'avvio."] {
        assert!(it.contains(word), "{word:?}:\n{it}");
    }
}

#[test]
fn the_language_of_the_config_file_wins_over_the_system() {
    let w = World::new();
    fs::write(w.p("config/rada/config.toml"), "language = \"en\"\n").unwrap();
    let help = text(&w.rada(IT, &["--help"]));
    assert!(help.contains("Usage:") && !help.contains("Uso:"), "{help}");
    fs::write(w.p("config/rada/config.toml"), "language = \"it\"\n").unwrap();
    let help = text(&w.rada(EN, &["--help"]));
    assert!(help.contains("Uso:") && !help.contains("Usage:"), "{help}");
}

#[test]
fn an_error_in_a_choice_is_said_in_the_language() {
    let w = World::new();
    let out = w.rada(IT, &["--profile", "bogus", "."]);
    assert!(!out.status.success());
    let t = text(&out);
    assert!(t.contains("profilo sconosciuto \"bogus\""), "{t}");
    let t = text(&w.rada(EN, &["--profile", "bogus", "."]));
    assert!(t.contains("unknown profile \"bogus\""), "{t}");
    let t = text(&w.rada(IT, &["--theme", "bogus", "."]));
    assert!(t.contains("tema sconosciuto"), "{t}");
}

#[test]
fn rada_doctor_speaks_the_language() {
    let w = World::new();
    let it = text(&w.rada(IT, &["doctor"]));
    for word in ["profilo:", "terminale:", "colori:", "immagini:", "appunti:"] {
        assert!(it.contains(word), "{word}:\n{it}");
    }
    assert!(
        !it.contains("profile:") && !it.contains("clipboard:"),
        "{it}"
    );
    let en = text(&w.rada(EN, &["doctor"]));
    assert!(en.contains("profile:") && en.contains("clipboard:"), "{en}");
}

#[test]
fn setup_says_what_it_did_in_the_language() {
    let w = World::new();
    let out = w.rada(IT, &["setup", "font", "--remove"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("non c'è nulla da togliere"),
        "{}",
        text(&out)
    );
    let out = w.rada(
        IT,
        &["setup", "desktop", "--remove", "--default-file-manager"],
    );
    assert!(!out.status.success());
    assert!(
        text(&out).contains("non si possono usare insieme"),
        "{}",
        text(&out)
    );
}

#[test]
fn update_is_said_in_the_language() {
    let w = World::new();
    // A server that is not there: the error is the language's too.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/releases", dead.local_addr().unwrap());
    drop(dead);
    w.settle(IT);
    let out = Command::new(env!("CARGO_BIN_EXE_rada"))
        .args(["update", "--check"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LC_ALL", IT)
        .env("HOME", w.p("home"))
        .env("USERPROFILE", w.p("home"))
        .env("APPDATA", w.p("config"))
        .env("LOCALAPPDATA", w.p("data"))
        .env("XDG_CONFIG_HOME", w.p("config"))
        .env("XDG_STATE_HOME", w.p("state"))
        .env("XDG_DATA_HOME", w.p("data"))
        .env("XDG_CACHE_HOME", w.p("cache"))
        .env("RADA_UPDATE_API", url)
        .output()
        .unwrap();
    let t = text(&out);
    assert!(t.contains("cerco una versione più nuova"), "{t}");
    assert!(t.contains("impossibile raggiungere"), "{t}");
    assert!(!t.contains("looking for a newer"), "{t}");
}
