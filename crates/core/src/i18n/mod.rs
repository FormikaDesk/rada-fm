//! The words of the interface, in English and Italian.
//!
//! Every sentence the person reads comes from the table in [`table`]: a [`Key`] stands for it,
//! and [`t`] gives it in the language in use. A key that has no Italian (or no English) does
//! not compile, so the two languages cannot drift apart; the tests check what the compiler
//! cannot (the same placeholders on both sides, no forgotten copies of the English).
//!
//! The language is chosen once, at start: from the configuration (`language = "it"`) or, with
//! `auto`, from the system ([`Lang::system`]). It is a process-wide setting; tests that need
//! another language use [`with_lang`], which only changes the calling thread.

use std::cell::Cell;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

mod table;
mod types;

pub use table::Key;
pub use types::{type_label, type_label_in};

/// A language the interface speaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Lang {
    #[default]
    En,
    It,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::It];

    /// The two-letter code used in the configuration.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::It => "it",
        }
    }

    /// What the `language` setting says: `en` or `it`. `auto` (and anything else) is `None`.
    pub fn parse(s: &str) -> Option<Lang> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" | "english" => Some(Lang::En),
            "it" | "italiano" | "italian" => Some(Lang::It),
            _ => None,
        }
    }

    /// The language a locale name stands for (`it_IT.UTF-8`, `it-IT`, `en_US`): Italian when
    /// the language part is `it`, English for everything else (the interface has no more).
    pub fn from_locale(name: &str) -> Lang {
        let lang = name
            .split(['.', '@', '_', '-'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if lang == "it" { Lang::It } else { Lang::En }
    }

    /// From environment variables the way programs find the locale: `LC_ALL` over
    /// `LC_MESSAGES` over `LANG`, the first that is set to something. `None` when none is.
    pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Option<Lang> {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| var(k))
            .find(|v| !v.trim().is_empty())
            .map(|v| Lang::from_locale(v.trim()))
    }

    /// The first language of a list of preferences, as macOS prints `AppleLanguages`:
    /// `( "it-IT", "en-IT" )`. Italian when the person's first choice is Italian.
    pub fn from_apple_languages(text: &str) -> Option<Lang> {
        let first = text
            .split(['"', '\n', ','])
            .map(|s| s.trim().trim_matches(['(', ')', ' ', ';']))
            .find(|s| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))?;
        Some(Lang::from_locale(first))
    }

    /// From the primary language of a Windows language identifier (`LANGID`): `0x10` is
    /// Italian.
    pub fn from_langid(id: u16) -> Lang {
        if id & 0x3ff == 0x10 {
            Lang::It
        } else {
            Lang::En
        }
    }

    /// The language of the system: `LANG`, `LC_ALL` and `LC_MESSAGES` on Linux, the language
    /// preferences on macOS, the language of the interface on Windows.
    pub fn system() -> Lang {
        #[cfg(target_os = "macos")]
        {
            let out = std::process::Command::new("defaults")
                .args(["read", "-g", "AppleLanguages"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok());
            if let Some(lang) = out.as_deref().and_then(Lang::from_apple_languages) {
                return lang;
            }
        }
        #[cfg(windows)]
        {
            // SAFETY: no arguments, no pointers: a plain query of the user's settings.
            let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
            if id != 0 {
                return Lang::from_langid(id);
            }
        }
        Lang::from_env(&|k| std::env::var(k).ok()).unwrap_or_default()
    }

    /// The setting of the configuration (`auto`, `it`, `en`) turned into a language.
    pub fn resolve(setting: &str) -> Lang {
        Lang::parse(setting).unwrap_or_else(Lang::system)
    }
}

static GLOBAL: AtomicU8 = AtomicU8::new(0);
/// Set when the person chose the language (`language = "it"`) instead of leaving it to the
/// system: 0 not chosen, 1 English, 2 Italian.
static CHOSEN: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static LOCAL: Cell<Option<Lang>> = const { Cell::new(None) };
}

fn to_u8(l: Lang) -> u8 {
    match l {
        Lang::En => 0,
        Lang::It => 1,
    }
}

/// Set the language of the whole process.
pub fn set_lang(l: Lang) {
    GLOBAL.store(to_u8(l), Ordering::Relaxed);
}

/// Set the language of the whole process to the one the person asked for by name (as
/// opposed to the system's): names that depend on the language on the disk (the tags of
/// the Finder excepted) follow it too.
pub fn set_chosen_lang(l: Lang) {
    set_lang(l);
    CHOSEN.store(to_u8(l) + 1, Ordering::Relaxed);
}

/// The language the person chose by name, if they did.
pub fn chosen_lang() -> Option<Lang> {
    match CHOSEN.load(Ordering::Relaxed) {
        1 => Some(Lang::En),
        2 => Some(Lang::It),
        _ => None,
    }
}

/// The language in use on this thread.
pub fn lang() -> Lang {
    LOCAL.with(Cell::get).unwrap_or_else(|| {
        if GLOBAL.load(Ordering::Relaxed) == 1 {
            Lang::It
        } else {
            Lang::En
        }
    })
}

/// Run `f` with `l` as the language of this thread only (for tests: the others keep theirs).
pub fn with_lang<R>(l: Lang, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Lang>);
    impl Drop for Restore {
        fn drop(&mut self) {
            LOCAL.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(LOCAL.with(|c| c.replace(Some(l))));
    f()
}

impl Key {
    /// The text in a given language.
    pub fn in_lang(self, l: Lang) -> &'static str {
        match l {
            Lang::En => self.en(),
            Lang::It => self.it(),
        }
    }

    /// The text in the language in use.
    pub fn tr(self) -> &'static str {
        self.in_lang(lang())
    }
}

/// The text of `key` in the language in use.
pub fn t(key: Key) -> &'static str {
    key.tr()
}

/// The text of `key` with `{0}`, `{1}`… replaced by the arguments (the order of the words
/// differs between languages, hence the numbers).
pub fn tf(key: Key, args: &[&dyn Display]) -> String {
    fill(key.tr(), args)
}

/// `template` with `{0}`, `{1}`… replaced by the arguments.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        match tail.find('}') {
            Some(close) => match tail[..close].parse::<usize>() {
                Ok(i) if i < args.len() => {
                    out.push_str(&args[i].to_string());
                    rest = &tail[close + 1..];
                }
                _ => {
                    out.push('{');
                    rest = tail;
                }
            },
            None => {
                out.push('{');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The placeholders (`{0}`, `{1}`…) a text uses, sorted and without repeats.
pub fn placeholders(template: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let tail = &rest[open + 1..];
        if let Some(close) = tail.find('}')
            && let Ok(i) = tail[..close].parse::<usize>()
        {
            found.push(i);
        }
        rest = tail;
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// A whole number with the separator of thousands the language uses: `1,234` or `1.234`.
pub fn thousands(n: u64) -> String {
    let sep = match lang() {
        Lang::En => ',',
        Lang::It => '.',
    };
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// A number written with a decimal point (`70.7`) as the language writes it (`70,7`).
pub fn decimal(number: &str) -> String {
    match lang() {
        Lang::En => number.to_string(),
        Lang::It => number.replace('.', ","),
    }
}

/// "1 item", "3 items" / "1 elemento", "3 elementi": the count with the right form of the
/// word.
pub fn count(n: u64, one: Key, many: Key) -> String {
    if n == 1 {
        format!("1 {}", one.tr())
    } else {
        format!("{} {}", thousands(n), many.tr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_follows_the_locale_variables() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            Lang::from_env(&move |k| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            })
        };
        assert_eq!(env(&[("LANG", "it_IT.UTF-8")]), Some(Lang::It));
        assert_eq!(env(&[("LANG", "en_US.UTF-8")]), Some(Lang::En));
        assert_eq!(env(&[("LANG", "de_DE.UTF-8")]), Some(Lang::En));
        // LC_ALL wins over LC_MESSAGES, which wins over LANG; an empty one does not count.
        assert_eq!(
            env(&[("LANG", "en_US"), ("LC_MESSAGES", "it_IT")]),
            Some(Lang::It)
        );
        assert_eq!(
            env(&[("LC_ALL", "C"), ("LC_MESSAGES", "it_IT"), ("LANG", "it_IT")]),
            Some(Lang::En)
        );
        assert_eq!(env(&[("LC_ALL", ""), ("LANG", "it_IT")]), Some(Lang::It));
        assert_eq!(env(&[]), None);
        assert_eq!(Lang::from_locale("it"), Lang::It);
        assert_eq!(Lang::from_locale("it-IT"), Lang::It);
        assert_eq!(Lang::from_locale("it_CH@euro"), Lang::It);
        assert_eq!(Lang::from_locale("C"), Lang::En);
        assert_eq!(Lang::from_locale("POSIX"), Lang::En);
        assert_eq!(Lang::from_locale("ita"), Lang::En);
    }

    #[test]
    fn macos_reports_its_preferences_as_a_list() {
        let list = "(\n    \"it-IT\",\n    \"en-IT\"\n)\n";
        assert_eq!(Lang::from_apple_languages(list), Some(Lang::It));
        let list = "(\n    \"en-GB\",\n    \"it-IT\"\n)\n";
        assert_eq!(Lang::from_apple_languages(list), Some(Lang::En));
        assert_eq!(Lang::from_apple_languages("(\n)\n"), None);
        assert_eq!(Lang::from_apple_languages(""), None);
    }

    #[test]
    fn windows_names_languages_with_a_number() {
        assert_eq!(Lang::from_langid(0x0410), Lang::It, "Italian (Italy)");
        assert_eq!(Lang::from_langid(0x0810), Lang::It, "Italian (Switzerland)");
        assert_eq!(Lang::from_langid(0x0409), Lang::En);
        assert_eq!(Lang::from_langid(0x0407), Lang::En);
    }

    #[test]
    fn the_setting_chooses_a_language_or_leaves_it_to_the_system() {
        assert_eq!(Lang::parse("it"), Some(Lang::It));
        assert_eq!(Lang::parse(" EN "), Some(Lang::En));
        assert_eq!(Lang::parse("auto"), None);
        assert_eq!(Lang::parse("klingon"), None);
        assert_eq!(Lang::resolve("it"), Lang::It);
        assert_eq!(Lang::resolve("en"), Lang::En);
    }

    #[test]
    fn a_thread_can_speak_another_language_without_disturbing_the_rest() {
        assert_eq!(lang(), Lang::En);
        with_lang(Lang::It, || {
            assert_eq!(lang(), Lang::It);
            with_lang(Lang::En, || assert_eq!(lang(), Lang::En));
            assert_eq!(lang(), Lang::It, "the outer setting comes back");
            assert_eq!(std::thread::spawn(lang).join().unwrap(), Lang::En);
        });
        assert_eq!(lang(), Lang::En);
    }

    #[test]
    fn placeholders_are_filled_by_number() {
        assert_eq!(fill("{0} of {1}", &[&3, &"x"]), "3 of x");
        assert_eq!(fill("{1} before {0}", &[&"a", &"b"]), "b before a");
        assert_eq!(fill("no holes", &[]), "no holes");
        assert_eq!(fill("{9} stays", &[&1]), "{9} stays");
        assert_eq!(fill("{ open", &[]), "{ open");
        assert_eq!(placeholders("{1} and {0} and {1}"), vec![0, 1]);
        assert!(placeholders("plain").is_empty());
    }

    #[test]
    fn numbers_follow_the_language() {
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(12), "12");
        with_lang(Lang::It, || {
            assert_eq!(thousands(1_234_567), "1.234.567");
            assert_eq!(decimal("70.7"), "70,7");
        });
        assert_eq!(decimal("70.7"), "70.7");
    }

    #[test]
    fn every_key_is_there_in_both_languages() {
        // The compiler already insists on both texts; this checks what it cannot.
        let mut problems = Vec::new();
        for &k in Key::ALL {
            let (en, it) = (k.en(), k.it());
            if en.trim().is_empty() || it.trim().is_empty() {
                problems.push(format!("{k:?}: an empty text"));
            }
            if placeholders(en) != placeholders(it) {
                problems.push(format!("{k:?}: the placeholders differ ({en:?} / {it:?})"));
            }
            if en == it && !table::SAME_IN_BOTH.contains(&k) {
                problems.push(format!("{k:?}: Italian is a copy of the English ({en:?})"));
            }
            if table::SAME_IN_BOTH.contains(&k) && en != it {
                problems.push(format!("{k:?}: listed as the same in both, but it is not"));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn counts_use_the_right_form_of_the_word() {
        assert_eq!(count(1, Key::ItemOne, Key::ItemMany), "1 item");
        assert_eq!(count(3, Key::ItemOne, Key::ItemMany), "3 items");
        assert_eq!(count(1_200, Key::ItemOne, Key::ItemMany), "1,200 items");
        with_lang(Lang::It, || {
            assert_eq!(count(1, Key::ItemOne, Key::ItemMany), "1 elemento");
            assert_eq!(count(3, Key::ItemOne, Key::ItemMany), "3 elementi");
            assert_eq!(count(0, Key::ItemOne, Key::ItemMany), "0 elementi");
        });
    }
}
