//! Windows: rada as the program that opens a folder when another program asks for one
//! (`rada setup desktop --default-file-manager`), the way Total Commander and Directory Opus can
//! be set up to be.
//!
//! What a program does to open a folder is to ask the shell for the `open` verb of the
//! `Directory` class. Explorer answers it unless the user's own `HKEY_CURRENT_USER\Software\
//! Classes\Directory\shell\open\command` says otherwise; that is all this writes, and **only
//! there**: nothing in `HKEY_LOCAL_MACHINE`, nothing for the machine, no administrator rights.
//!
//! * Only `Directory` (real folders). Not `Folder` (which includes This PC, the Control Panel
//!   and the rest of the shell's virtual folders, which a terminal program cannot show) and not
//!   `opennewwindow`: *Open in new window* in the right-click menu of a folder stays Explorer's,
//!   so that there is always a way to it.
//! * Explorer itself is not touched: it still opens its own folders when you click in it, and
//!   `Win+E` and what programs ask of Explorer by name (`explorer /select,…`, "show in folder"
//!   of browsers) stay as they are.
//! * What was there before (a value, a key) is read first and written down in the state folder,
//!   so that `--remove` puts exactly that back and takes away only the keys this made, and only
//!   if nothing else has been put in them since. A value that was changed after the setup is left
//!   as it is.
//! * Nothing is written before the person has seen it and said yes.
//!
//! The program is `rada.exe` itself: it is a console program, so the system opens it in a console
//! window (in Windows Terminal, if it is the default terminal). The folder is its argument.
//!
//! The registry is reached through the [`Hive`] trait; the real one runs `reg.exe`, which every
//! Windows has, and tests use one in memory. **What this does to a real Windows is not tested
//! by the tests**: they cover the logic with the in-memory registry only.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use rada_core::i18n::{Key, tf};
use serde::{Deserialize, Serialize};

/// Where the person's own classes are, under `HKEY_CURRENT_USER`.
pub const CLASSES: &str = r"HKEY_CURRENT_USER\Software\Classes";
const CLASS: &str = "Directory";
const SHELL: &str = r"Directory\shell";
const OPEN: &str = r"Directory\shell\open";
const COMMAND: &str = r"Directory\shell\open\command";
const DELEGATE: &str = "DelegateExecute";
const MANIFEST: &str = "windows-folder-handler.json";

/// The registry of the person, by key relative to [`CLASSES`]. A value `name` of `None` is the
/// key's default value.
pub trait Hive {
    fn key_exists(&self, key: &str) -> Result<bool>;
    fn get(&self, key: &str, name: Option<&str>) -> Result<Option<String>>;
    fn set(&self, key: &str, name: Option<&str>, value: &str) -> Result<()>;
    fn delete_value(&self, key: &str, name: Option<&str>) -> Result<()>;
    /// The names of the keys directly under `key`.
    fn subkeys(&self, key: &str) -> Result<Vec<String>>;
    /// The key and all it holds.
    fn delete_key(&self, key: &str) -> Result<()>;
}

/// One value that is written (or taken away), with what it was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub key: String,
    pub name: Option<String>,
    /// What was there before: `None` when nothing was.
    pub previous: Option<String>,
    /// What is written; `None` when the value is taken away.
    pub written: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Record {
    pub command: String,
    pub changes: Vec<Change>,
    /// The keys this made, in the order it made them.
    pub created_keys: Vec<String>,
}

/// The command line of the handler: `rada.exe` and the folder as one word (`%1` is how the
/// shell passes the folder; a folder that is not a drive's root never ends in a backslash).
pub fn command_line(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

fn shown_key(key: &str) -> String {
    format!(r"{CLASSES}\{key}")
}

fn shown_name(name: &Option<String>) -> String {
    name.clone().unwrap_or_else(|| "(Default)".into())
}

/// What would be written, from what is in the registry now, and what is already as wanted.
pub fn plan(hive: &dyn Hive, exe: &Path) -> Result<(Record, Vec<Change>)> {
    let command = command_line(exe);
    let mut record = Record {
        command: command.clone(),
        ..Default::default()
    };
    for key in [CLASS, SHELL, OPEN, COMMAND] {
        if !hive.key_exists(key)? {
            record.created_keys.push(key.to_string());
        }
    }
    let mut wanted = vec![
        (SHELL, None, Some("open".to_string())),
        (COMMAND, None, Some(command)),
    ];
    // Another program's own replacement leaves this in the key, and it would win over the command.
    if hive.get(OPEN, Some(DELEGATE))?.is_some() {
        wanted.push((OPEN, Some(DELEGATE.to_string()), None));
    }
    let mut todo = Vec::new();
    for (key, name, written) in wanted {
        let previous = hive.get(key, name.as_deref())?;
        let change = Change {
            key: key.to_string(),
            name,
            previous,
            written,
        };
        if change.previous != change.written {
            todo.push(change.clone());
        }
        record.changes.push(change);
    }
    Ok((record, todo))
}

/// The lines that say what would be written.
pub fn describe(todo: &[Change], exe: &Path) -> Vec<String> {
    let mut out = vec![Key::WfIntro.tr().to_string(), String::new()];
    for c in todo {
        let key = shown_key(&c.key);
        let name = shown_name(&c.name);
        out.push(match (&c.written, &c.previous) {
            (Some(w), Some(p)) => tf(Key::WfSetWas, &[&key, &name, &w, &p]),
            (Some(w), None) => tf(Key::WfSetNew, &[&key, &name, &w]),
            (None, p) => tf(
                Key::WfDelete,
                &[&key, &name, &p.clone().unwrap_or_default()],
            ),
        });
    }
    out.push(String::new());
    out.push(tf(Key::WfProgram, &[&exe.display()]));
    out.push(Key::WfExplorerStays.tr().to_string());
    out
}

/// Write what was planned and the record that lets `--remove` undo it.
pub fn apply(hive: &dyn Hive, todo: &[Change], record: &Record, state_dir: &Path) -> Result<()> {
    let mut saved = read_record(state_dir);
    // Setting up again keeps the first "before": that is what `--remove` goes back to.
    for c in &record.changes {
        match saved
            .changes
            .iter_mut()
            .find(|s| s.key == c.key && s.name == c.name)
        {
            Some(s) => s.written = c.written.clone(),
            None => saved.changes.push(c.clone()),
        }
    }
    for k in &record.created_keys {
        if !saved.created_keys.contains(k) {
            saved.created_keys.push(k.clone());
        }
    }
    saved.command = record.command.clone();
    // The record first: a write that stops halfway can still be undone.
    write_record(state_dir, &saved)?;
    for c in todo {
        match &c.written {
            Some(v) => hive.set(&c.key, c.name.as_deref(), v)?,
            None => hive.delete_value(&c.key, c.name.as_deref())?,
        }
    }
    Ok(())
}

/// Put back what was there: the values as they were, and the keys that were made taken away
/// (deepest first, and only when nothing else is in them). A value that is no longer what was
/// written is left alone.
pub fn undo(hive: &dyn Hive, state_dir: &Path) -> Result<Vec<String>> {
    let record = read_record(state_dir);
    if record.changes.is_empty() && record.created_keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut say = Vec::new();
    // Keys that hold a value someone changed since: they stay, and so do the keys above them.
    let mut kept: Vec<String> = Vec::new();
    for c in record.changes.iter().rev() {
        if !hive.key_exists(&c.key)? {
            continue;
        }
        let now = hive.get(&c.key, c.name.as_deref())?;
        let (key, name) = (shown_key(&c.key), shown_name(&c.name));
        if now != c.written {
            say.push(tf(Key::WfChangedSince, &[&key, &name]));
            kept.push(c.key.clone());
            continue;
        }
        match &c.previous {
            Some(p) => {
                hive.set(&c.key, c.name.as_deref(), p)?;
                say.push(tf(Key::WfRestored, &[&key, &name, p]));
            }
            None => {
                hive.delete_value(&c.key, c.name.as_deref())?;
                say.push(tf(Key::WfValueRemoved, &[&key, &name]));
            }
        }
    }
    for key in record.created_keys.iter().rev() {
        let holds_a_kept_value = kept
            .iter()
            .any(|k| k == key || k.starts_with(&format!("{key}\\")));
        if !holds_a_kept_value && hive.key_exists(key)? && hive.subkeys(key)?.is_empty() {
            hive.delete_key(key)?;
            say.push(tf(Key::SayRemoved, &[&shown_key(key)]));
        }
    }
    let _ = std::fs::remove_file(state_dir.join(MANIFEST));
    Ok(say)
}

fn read_record(state_dir: &Path) -> Record {
    std::fs::read_to_string(state_dir.join(MANIFEST))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_record(state_dir: &Path, r: &Record) -> Result<()> {
    let path = state_dir.join(MANIFEST);
    std::fs::create_dir_all(state_dir)
        .with_context(|| tf(Key::MCannotCreate, &[&state_dir.display()]))?;
    std::fs::write(&path, serde_json::to_string_pretty(r)?)
        .with_context(|| tf(Key::MCannotWrite, &[&path.display()]))
}

/// `rada setup desktop --default-file-manager` on Windows: show, ask, write.
pub fn install(
    hive: &dyn Hive,
    exe: &Path,
    state_dir: &Path,
    yes: bool,
    input: &mut dyn BufRead,
) -> Result<Vec<String>> {
    if !exe.is_file() {
        bail!("{}", tf(Key::WfNoProgram, &[&exe.display()]));
    }
    let (record, todo) = plan(hive, exe)?;
    if todo.is_empty() {
        return Ok(vec![Key::WfAlready.tr().to_string()]);
    }
    for line in describe(&todo, exe) {
        println!("{line}");
    }
    if !yes {
        print!("{}", Key::WfAsk.tr());
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut answer = String::new();
        input.read_line(&mut answer).ok();
        if !matches!(
            answer.trim().to_lowercase().as_str(),
            "y" | "yes" | "s" | "si" | "sì"
        ) {
            return Ok(vec![String::new(), Key::WfDeclined.tr().to_string()]);
        }
    }
    apply(hive, &todo, &record, state_dir)?;
    Ok(vec![
        tf(Key::WfDone, &[&record.command]),
        tf(Key::WfByHand, &[&shown_key(r"Directory\shell\open")]),
    ])
}

// ------------------------------------------------------------------ the real registry

/// `reg.exe`: the registry of the person, no privileges. Values are read by exporting the key to
/// a file of ours and reading that (what `reg query` prints is in the words of the system's
/// language, and says "(value not set)" for a default that is not there).
pub struct RegExe {
    /// A folder to export into (the state folder).
    pub scratch: PathBuf,
}

impl RegExe {
    fn path(key: &str) -> String {
        shown_key(key)
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output> {
        Command::new("reg")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| tf(Key::MNotAvailable, &[&"reg"]))
    }

    fn run_ok(&self, args: &[&str]) -> Result<()> {
        let out = self.run(args)?;
        if !out.status.success() {
            bail!(
                "{}",
                tf(
                    Key::MProgramFailed,
                    &[&"reg", &String::from_utf8_lossy(&out.stderr).trim()]
                )
            );
        }
        Ok(())
    }
}

impl Hive for RegExe {
    fn key_exists(&self, key: &str) -> Result<bool> {
        Ok(self.run(&["query", &Self::path(key)])?.status.success())
    }

    fn get(&self, key: &str, name: Option<&str>) -> Result<Option<String>> {
        if !self.key_exists(key)? {
            return Ok(None);
        }
        std::fs::create_dir_all(&self.scratch)
            .with_context(|| tf(Key::MCannotCreate, &[&self.scratch.display()]))?;
        let file = self.scratch.join("registry-export.reg");
        let _ = std::fs::remove_file(&file);
        self.run_ok(&["export", &Self::path(key), &file.to_string_lossy(), "/y"])?;
        let bytes =
            std::fs::read(&file).with_context(|| tf(Key::MCannotRead, &[&file.display()]))?;
        let _ = std::fs::remove_file(&file);
        Ok(read_value(&decode_reg(&bytes), &Self::path(key), name))
    }

    fn set(&self, key: &str, name: Option<&str>, value: &str) -> Result<()> {
        let path = Self::path(key);
        match name {
            Some(n) => self.run_ok(&["add", &path, "/v", n, "/t", "REG_SZ", "/d", value, "/f"]),
            None => self.run_ok(&["add", &path, "/ve", "/t", "REG_SZ", "/d", value, "/f"]),
        }
    }

    fn delete_value(&self, key: &str, name: Option<&str>) -> Result<()> {
        let path = Self::path(key);
        match name {
            Some(n) => self.run_ok(&["delete", &path, "/v", n, "/f"]),
            None => self.run_ok(&["delete", &path, "/ve", "/f"]),
        }
    }

    fn subkeys(&self, key: &str) -> Result<Vec<String>> {
        let path = Self::path(key);
        let out = self.run(&["query", &path])?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let own = path.to_lowercase();
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|l| l.to_lowercase().starts_with(&own) && l.to_lowercase() != own)
            .map(|l| l.rsplit('\\').next().unwrap_or(l).to_string())
            .collect())
    }

    fn delete_key(&self, key: &str) -> Result<()> {
        self.run_ok(&["delete", &Self::path(key), "/f"])
    }
}

/// The text of a `.reg` file that `reg export` wrote: UTF-16 with a byte order mark.
pub fn decode_reg(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    let units: Vec<u16> = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
}

/// A text value of the key `full_key` in the text of a `.reg` file: `@="…"` is the default,
/// `"name"="…"` is a named one. Only the key's own values count, not those of its subkeys.
/// A value of another kind (`dword:…`, `hex:…`) is not a text and gives `None`.
pub fn read_value(reg: &str, full_key: &str, name: Option<&str>) -> Option<String> {
    let header = format!("[{}]", full_key.to_lowercase());
    let mut inside = false;
    for line in reg.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('[') {
            inside = line.to_lowercase() == header;
            continue;
        }
        if !inside {
            continue;
        }
        let (left, right) = match line.split_once('=') {
            Some(p) if p.0 == "@" => (None, p.1),
            Some(p) if p.0.starts_with('"') && p.0.ends_with('"') && p.0.len() >= 2 => {
                (Some(unescape(&p.0[1..p.0.len() - 1])), p.1)
            }
            _ => continue,
        };
        if left.as_deref().map(str::to_lowercase) != name.map(str::to_lowercase) {
            continue;
        }
        let text = right.strip_prefix('"')?.strip_suffix('"')?;
        return Some(unescape(text));
    }
    None
}

/// What `reg export` escapes in a text: `\\` and `\"`.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(n @ ('\\' | '"')) => out.push(n),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A registry in memory. Keys are compared without regard to case, as Windows does.
    #[derive(Default)]
    struct Memory {
        keys: RefCell<BTreeMap<String, BTreeMap<Option<String>, String>>>,
    }

    impl Memory {
        fn k(key: &str) -> String {
            key.to_lowercase()
        }

        fn with(self, key: &str, name: Option<&str>, value: &str) -> Memory {
            self.make(key);
            self.keys
                .borrow_mut()
                .get_mut(&Self::k(key))
                .unwrap()
                .insert(name.map(String::from), value.into());
            self
        }

        fn make(&self, key: &str) {
            let mut at = String::new();
            for part in key.split('\\') {
                if !at.is_empty() {
                    at.push('\\');
                }
                at.push_str(part);
                self.keys.borrow_mut().entry(Self::k(&at)).or_default();
            }
        }

        fn names(&self) -> Vec<String> {
            self.keys.borrow().keys().cloned().collect()
        }

        fn value(&self, key: &str, name: Option<&str>) -> Option<String> {
            self.keys
                .borrow()
                .get(&Self::k(key))
                .and_then(|v| v.get(&name.map(String::from)).cloned())
        }
    }

    impl Hive for Memory {
        fn key_exists(&self, key: &str) -> Result<bool> {
            Ok(self.keys.borrow().contains_key(&Self::k(key)))
        }
        fn get(&self, key: &str, name: Option<&str>) -> Result<Option<String>> {
            Ok(self.value(key, name))
        }
        fn set(&self, key: &str, name: Option<&str>, value: &str) -> Result<()> {
            self.make(key);
            self.keys
                .borrow_mut()
                .get_mut(&Self::k(key))
                .unwrap()
                .insert(name.map(String::from), value.into());
            Ok(())
        }
        fn delete_value(&self, key: &str, name: Option<&str>) -> Result<()> {
            if let Some(v) = self.keys.borrow_mut().get_mut(&Self::k(key)) {
                v.remove(&name.map(String::from));
            }
            Ok(())
        }
        fn subkeys(&self, key: &str) -> Result<Vec<String>> {
            let prefix = format!("{}\\", Self::k(key));
            Ok(self
                .keys
                .borrow()
                .keys()
                .filter_map(|k| k.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('\\'))
                .map(String::from)
                .collect())
        }
        fn delete_key(&self, key: &str) -> Result<()> {
            let k = Self::k(key);
            self.keys
                .borrow_mut()
                .retain(|name, _| *name != k && !name.starts_with(&format!("{k}\\")));
            Ok(())
        }
    }

    fn tmp() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("rada-wf-")
            .tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .unwrap()
    }

    fn exe(dir: &Path) -> PathBuf {
        let p = dir.join("rada.exe");
        std::fs::write(&p, b"x").unwrap();
        p
    }

    fn run_install(h: &Memory, exe: &Path, state: &Path, answer: &str) -> Vec<String> {
        install(h, exe, state, false, &mut answer.as_bytes()).unwrap()
    }

    #[test]
    fn the_command_is_the_program_and_the_folder_as_one_word() {
        assert_eq!(
            command_line(Path::new(r"C:\Users\Anna Rossi\rada.exe")),
            r#""C:\Users\Anna Rossi\rada.exe" "%1""#
        );
    }

    #[test]
    fn nothing_is_written_without_a_yes() {
        let t = tmp();
        let h = Memory::default();
        let e = exe(t.path());
        for answer in ["", "n\n", "\n", "no\n", "maybe\n"] {
            let say = run_install(&h, &e, t.path(), answer);
            assert!(say.iter().any(|l| l.contains("not changed")), "{say:?}");
            assert!(h.names().is_empty(), "{:?}", h.names());
            assert!(!t.path().join(MANIFEST).exists());
        }
    }

    #[test]
    fn a_yes_writes_the_three_things_in_the_users_classes_and_nothing_else() {
        let t = tmp();
        let h = Memory::default();
        let e = exe(t.path());
        let say = run_install(&h, &e, t.path(), "y\n");
        assert!(say[0].contains("opens folders"), "{say:?}");
        assert!(
            say[1].contains(r"HKEY_CURRENT_USER\Software\Classes\Directory\shell\open"),
            "{say:?}"
        );
        assert_eq!(h.value(SHELL, None).as_deref(), Some("open"));
        assert_eq!(h.value(COMMAND, None), Some(command_line(&e)));
        // The right-click "Open in new window", the virtual folders and the drives are not touched.
        assert_eq!(
            h.names(),
            [
                "directory",
                r"directory\shell",
                r"directory\shell\open",
                r"directory\shell\open\command"
            ]
        );
    }

    #[test]
    fn what_is_described_names_every_key_and_value() {
        let t = tmp();
        let h = Memory::default();
        let e = exe(t.path());
        let (_, todo) = plan(&h, &e).unwrap();
        let text = describe(&todo, &e).join("\n");
        for want in [
            r"HKEY_CURRENT_USER\Software\Classes\Directory\shell",
            r"HKEY_CURRENT_USER\Software\Classes\Directory\shell\open\command",
            "(Default) = open",
            "Explorer",
        ] {
            assert!(text.contains(want), "{want}:\n{text}");
        }
        assert!(!text.contains("HKEY_LOCAL_MACHINE") && !text.contains("HKEY_CLASSES_ROOT"));
    }

    #[test]
    fn the_registry_is_read_before_it_is_written_and_comes_back_as_it_was() {
        let t = tmp();
        // Another handler was there already, with its own DelegateExecute, and the key
        // `Directory\shell` had a verb of its own.
        let h = Memory::default()
            .with(SHELL, None, "none")
            .with(r"Directory\shell\other\command", None, "other.exe")
            .with(OPEN, Some(DELEGATE), "{abc}")
            .with(COMMAND, None, "old.exe %1");
        let e = exe(t.path());
        run_install(&h, &e, t.path(), "y\n");
        assert_eq!(h.value(SHELL, None).as_deref(), Some("open"));
        assert_eq!(
            h.value(OPEN, Some(DELEGATE)),
            None,
            "it would win over the command"
        );
        assert_eq!(h.value(COMMAND, None), Some(command_line(&e)));

        let say = undo(&h, t.path()).unwrap();
        assert!(say.iter().any(|l| l.contains("old.exe")), "{say:?}");
        assert_eq!(h.value(SHELL, None).as_deref(), Some("none"));
        assert_eq!(h.value(OPEN, Some(DELEGATE)).as_deref(), Some("{abc}"));
        assert_eq!(h.value(COMMAND, None).as_deref(), Some("old.exe %1"));
        // Everything that was there is still there, and nothing was made that was not.
        assert!(
            h.names()
                .contains(&r"directory\shell\other\command".to_string())
        );
        assert!(!t.path().join(MANIFEST).exists());
    }

    #[test]
    fn the_keys_that_were_made_go_and_the_ones_that_were_not_stay() {
        let t = tmp();
        let h = Memory::default();
        let e = exe(t.path());
        run_install(&h, &e, t.path(), "y\n");
        undo(&h, t.path()).unwrap();
        assert!(h.names().is_empty(), "{:?}", h.names());
        // A key made by someone else beside ours is never taken away.
        let h = Memory::default().with(r"Directory\shell\other\command", None, "other.exe");
        run_install(&h, &e, t.path(), "y\n");
        undo(&h, t.path()).unwrap();
        assert!(
            h.names()
                .contains(&r"directory\shell\other\command".to_string())
        );
        assert!(
            h.names().contains(&r"directory\shell".to_string()),
            "it was there before"
        );
        assert!(!h.names().contains(&r"directory\shell\open".to_string()));
    }

    #[test]
    fn a_key_that_was_made_is_kept_if_something_else_has_been_put_in_it_since() {
        let t = tmp();
        let h = Memory::default();
        let e = exe(t.path());
        run_install(&h, &e, t.path(), "y\n");
        h.make(r"Directory\shell\open\mine");
        undo(&h, t.path()).unwrap();
        assert!(
            h.names()
                .contains(&r"directory\shell\open\mine".to_string())
        );
        assert!(h.names().contains(&r"directory\shell\open".to_string()));
    }

    #[test]
    fn a_value_changed_after_the_setup_is_left_alone() {
        let t = tmp();
        let h = Memory::default();
        let e = exe(t.path());
        run_install(&h, &e, t.path(), "y\n");
        h.set(COMMAND, None, "mine.exe %1").unwrap();
        let say = undo(&h, t.path()).unwrap();
        assert!(say.iter().any(|l| l.contains("changed after")), "{say:?}");
        assert_eq!(h.value(COMMAND, None).as_deref(), Some("mine.exe %1"));
    }

    #[test]
    fn setting_up_again_changes_nothing_and_keeps_the_first_before() {
        let t = tmp();
        let h = Memory::default().with(COMMAND, None, "old.exe %1");
        let e = exe(t.path());
        run_install(&h, &e, t.path(), "y\n");
        let again = run_install(&h, &e, t.path(), "");
        assert!(again[0].contains("already"), "{again:?}");
        // A different program path: written again, and the "before" is still the first one.
        let e2 = t.path().join("moved.exe");
        std::fs::write(&e2, b"x").unwrap();
        run_install(&h, &e2, t.path(), "y\n");
        assert_eq!(h.value(COMMAND, None), Some(command_line(&e2)));
        undo(&h, t.path()).unwrap();
        assert_eq!(h.value(COMMAND, None).as_deref(), Some("old.exe %1"));
    }

    #[test]
    fn a_program_that_is_not_there_is_refused_before_anything_is_read() {
        let t = tmp();
        let h = Memory::default();
        let err = install(
            &h,
            &t.path().join("gone.exe"),
            t.path(),
            true,
            &mut "".as_bytes(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("gone.exe"), "{err}");
        assert!(h.names().is_empty());
    }

    #[test]
    fn undoing_what_was_never_done_says_nothing() {
        let t = tmp();
        assert!(undo(&Memory::default(), t.path()).unwrap().is_empty());
    }

    #[test]
    fn the_values_of_an_exported_key_are_read_whatever_the_language_and_the_escapes() {
        let reg = "Windows Registry Editor Version 5.00\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Classes\\Directory\\shell]\r\n\
            @=\"open\"\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Classes\\Directory\\shell\\open]\r\n\
            \"DelegateExecute\"=\"{11dbb47c-a525-400b-9e80-a54615a090c0}\"\r\n\
            \"Other\"=dword:00000001\r\n\r\n\
            [HKEY_CURRENT_USER\\Software\\Classes\\Directory\\shell\\open\\command]\r\n\
            @=\"\\\"C:\\\\Program Files\\\\x\\\\rada.exe\\\" \\\"%1\\\"\"\r\n";
        let key = |k: &str| format!(r"HKEY_CURRENT_USER\Software\Classes\{k}");
        assert_eq!(read_value(reg, &key(SHELL), None).as_deref(), Some("open"));
        // The default of a key is not read from its subkeys, and the other way round.
        assert_eq!(read_value(reg, &key(OPEN), None), None);
        assert_eq!(
            read_value(reg, &key(OPEN), Some("delegateexecute")).as_deref(),
            Some("{11dbb47c-a525-400b-9e80-a54615a090c0}")
        );
        assert_eq!(
            read_value(reg, &key(OPEN), Some("Other")),
            None,
            "not a text"
        );
        assert_eq!(
            read_value(reg, &key(COMMAND), None).as_deref(),
            Some(r#""C:\Program Files\x\rada.exe" "%1""#)
        );
    }

    #[test]
    fn the_export_is_utf16_with_a_mark() {
        let mut bytes = vec![0xff, 0xfe];
        for u in "[K]\r\n@=\"é\"\r\n".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_reg(&bytes), "[K]\r\n@=\"é\"\r\n");
    }
}
