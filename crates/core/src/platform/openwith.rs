//! "Open with": the applications installed on the system that can open a file, and what rada
//! remembers about the choices made (recently used ones, and "always use this for this kind of
//! file", which lives in rada's own state and never touches the system's defaults).
//!
//! * **Linux** follows the freedesktop.org specifications: the `.desktop` files of the
//!   `applications` folders of `$XDG_DATA_HOME` and `$XDG_DATA_DIRS` whose `MimeType` fits the
//!   file's type (and that `mimeinfo.cache` names for it), the system's default from
//!   `mimeapps.list`, and the `Exec` line with its field codes, run in a terminal when
//!   `Terminal=true`. Everything here that is not about the machine is a pure function of text,
//!   so it is tested with `.desktop` files of its own in a folder of its own.
//! * **macOS** asks Launch Services (through `osascript`) and opens with `open -a`.
//! * **Windows** asks the shell for the programs associated with the extension.

#[cfg_attr(not(unix), allow(unused_imports))]
use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One program that can open a file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenApp {
    /// What names it on this system: the desktop file id (`org.gnome.gedit.desktop`) on Linux,
    /// the application's path on macOS, the handler's name on Windows. Stable between runs.
    pub id: String,
    /// What the person calls it ("Text Editor").
    pub name: String,
    /// What to run: the `Exec` line (field codes still in it) on Linux, the application bundle
    /// on macOS, the program or handler on Windows.
    pub exec: String,
    /// It needs a terminal (`Terminal=true`).
    #[serde(default)]
    pub terminal: bool,
    /// The system's own choice for this kind of file.
    #[serde(default)]
    pub is_default: bool,
    /// The file the description came from (Linux: for `%k`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// What can open a file: its kind (the key of "always use this for this kind of file": a MIME
/// type on Linux, the extension elsewhere) and the programs, the system's choice first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenChoices {
    pub kind: Option<String>,
    pub apps: Vec<OpenApp>,
}

// ------------------------------------------------------------------------------------- macOS

/// The script `osascript` runs to ask Launch Services which applications open a file (its one
/// argument): the default one first, written with a `*`, then the others with a `-`, a path
/// each.
pub const LAUNCH_SERVICES_SCRIPT: &str = r#"ObjC.import('AppKit');
function run(argv) {
  var ws = $.NSWorkspace.sharedWorkspace;
  var url = $.NSURL.fileURLWithPath(argv[0]);
  var seen = {};
  var out = [];
  function add(u, star) {
    if (!u || !u.path) return;
    var p = ObjC.unwrap(u.path);
    if (!p || seen[p]) return;
    seen[p] = true;
    out.push((star ? '*' : '-') + p);
  }
  try { add(ws.URLForApplicationToOpenURL(url), true); } catch (e) {}
  try {
    var all = ws.URLsForApplicationsToOpenURL(url);
    for (var i = 0; i < all.count; i++) add(all.objectAtIndex(i), false);
  } catch (e) {}
  return out.join('\n');
}"#;

/// The applications in what [`LAUNCH_SERVICES_SCRIPT`] printed: the name of an application is
/// its bundle's, without `.app`; the system's choice is the one marked `*`.
pub fn parse_launch_services(output: &str) -> Vec<OpenApp> {
    let mut apps: Vec<OpenApp> = Vec::new();
    for line in output.lines() {
        let line = line.trim_end();
        let (is_default, path) = match line.split_at_checked(1) {
            Some(("*", p)) => (true, p),
            Some(("-", p)) => (false, p),
            _ => continue,
        };
        let p = Path::new(path);
        let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        if path.is_empty() || apps.iter().any(|a| a.id == path) {
            continue;
        }
        apps.push(OpenApp {
            id: path.to_string(),
            name: stem,
            exec: path.to_string(),
            terminal: false,
            is_default,
            source: None,
            icon: None,
        });
    }
    apps
}

// ------------------------------------------------------------------------------ desktop files

/// A `.desktop` file, the parts that matter here.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: String,
    pub try_exec: Option<String>,
    pub icon: Option<String>,
    pub terminal: bool,
    pub mime_types: Vec<String>,
    /// `NoDisplay` or `Hidden`: not meant to be offered.
    pub hidden: bool,
}

/// The value of a key with the escapes of the specification undone (`\s`, `\n`, `\t`, `\r`,
/// `\\`).
fn unescape_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The entries of a list value (`a;b;c;`, with `\;` for a semicolon in an item).
fn split_list(v: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&';') => {
                cur.push(';');
                chars.next();
            }
            ';' => items.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        items.push(cur);
    }
    items.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Read the `[Desktop Entry]` group of a `.desktop` file. `lang` (`it`) picks `Name[it]` over
/// `Name`. `None` when it is not an application, or has nothing to run.
pub fn parse_desktop(text: &str, lang: &str) -> Option<DesktopEntry> {
    let mut e = DesktopEntry::default();
    let mut in_group = false;
    let mut seen_group = false;
    let (mut kind, mut localized, mut plain_name) = (String::new(), None::<String>, None);
    let mut hidden = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            seen_group |= in_group;
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Type" => kind = value.to_string(),
            "Name" => plain_name = Some(unescape_value(value)),
            "Exec" => e.exec = unescape_value(value),
            "TryExec" => e.try_exec = Some(unescape_value(value)),
            "Icon" => e.icon = Some(unescape_value(value)),
            "Terminal" => e.terminal = value == "true",
            "MimeType" => e.mime_types = split_list(value),
            "NoDisplay" | "Hidden" if value == "true" => hidden = true,
            k if k.starts_with("Name[") && k.ends_with(']') => {
                let tag = &k[5..k.len() - 1];
                let base = tag.split(['_', '@', '.']).next().unwrap_or(tag);
                if base.eq_ignore_ascii_case(lang) && (localized.is_none() || tag == base) {
                    localized = Some(unescape_value(value));
                }
            }
            _ => {}
        }
    }
    e.hidden = hidden;
    e.name = localized.or(plain_name)?;
    if !seen_group || kind != "Application" || e.exec.trim().is_empty() || e.name.is_empty() {
        return None;
    }
    Some(e)
}

/// The words of an `Exec` line, with double quotes keeping blanks together and the backslash
/// escaping inside them, as the specification says.
fn exec_words(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut started = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match (in_quote, c) {
            (true, '"') => in_quote = false,
            (true, '\\') => match chars.next() {
                Some(n @ ('"' | '`' | '$' | '\\')) => cur.push(n),
                Some(n) => {
                    cur.push('\\');
                    cur.push(n);
                }
                None => cur.push('\\'),
            },
            (true, c) => cur.push(c),
            (false, '"') => {
                in_quote = true;
                started = true;
            }
            (false, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (false, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// `file:///…` for a path, with what a URL cannot hold written as `%XX`.
pub fn file_url(path: &Path) -> String {
    let mut out = String::from("file://");
    let s = path.to_string_lossy();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// What to run for an `Exec` line, the files given, and the field codes replaced:
/// `%f %F` the path(s), `%u %U` as URLs, `%i` `--icon <icon>`, `%c` the name, `%k` the
/// desktop file, `%%` a percent sign; the codes that are gone from the specification are
/// dropped. A line with no code for files gets the path at the end (the person chose this
/// program for this file).
pub fn expand_exec(
    exec: &str,
    files: &[&Path],
    app: &DesktopEntry,
    source: Option<&Path>,
) -> Vec<String> {
    let words = exec_words(exec);
    if words.is_empty() {
        return Vec::new();
    }
    let has_file_code = words.iter().any(|w| {
        let mut it = w.chars();
        while let Some(c) = it.next() {
            if c == '%'
                && let Some('f' | 'F' | 'u' | 'U') = it.next()
            {
                return true;
            }
        }
        false
    });
    let mut out: Vec<String> = Vec::new();
    for w in &words {
        if w == "%F" {
            out.extend(files.iter().map(|f| operand_text(f)));
            continue;
        }
        if w == "%f"
            && let Some(f) = files.first()
        {
            out.push(operand_text(f));
            continue;
        }
        if w == "%U" {
            out.extend(files.iter().map(|f| file_url(f)));
            continue;
        }
        if w == "%i" {
            if let Some(icon) = &app.icon {
                out.push("--icon".into());
                out.push(icon.clone());
            }
            continue;
        }
        let mut word = String::new();
        let mut chars = w.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                word.push(c);
                continue;
            }
            match chars.next() {
                Some('f' | 'F') => {
                    if let Some(f) = files.first() {
                        word.push_str(&f.to_string_lossy());
                    }
                }
                Some('u' | 'U') => {
                    if let Some(f) = files.first() {
                        word.push_str(&file_url(f));
                    }
                }
                Some('c') => word.push_str(&app.name),
                Some('k') => {
                    if let Some(s) = source {
                        word.push_str(&s.to_string_lossy());
                    }
                }
                Some('i') => {
                    if let Some(icon) = &app.icon {
                        word.push_str(icon);
                    }
                }
                Some('%') => word.push('%'),
                Some(_) | None => {}
            }
        }
        // A word that was only a code for something absent is not an argument.
        if !word.is_empty() || !w.contains('%') {
            out.push(word);
        }
    }
    if !has_file_code {
        out.extend(files.iter().map(|f| operand_text(f)));
    }
    out
}

/// A file as a word of its own in a command line.
fn operand_text(file: &Path) -> String {
    super::spawn::operand(file).to_string_lossy().into_owned()
}

// ------------------------------------------------------------------------------ where things are

/// The folders the specifications name. Passed in, never read from the environment deep inside,
/// so that a test can give it a folder of its own.
#[derive(Clone, Debug, Default)]
pub struct AppDirs {
    /// `$XDG_DATA_HOME` first, then `$XDG_DATA_DIRS`: each has an `applications` and a `mime`
    /// folder.
    pub data: Vec<PathBuf>,
    /// `$XDG_CONFIG_HOME` first, then `$XDG_CONFIG_DIRS`: `mimeapps.list` may be in each.
    pub config: Vec<PathBuf>,
}

impl AppDirs {
    /// From rada's own folders (`data_home`, `config_home`) and the environment's lists.
    pub fn from_env(data_home: &Path, config_home: &Path) -> AppDirs {
        let list = |key: &str, default: &str| -> Vec<PathBuf> {
            let v = std::env::var(key).ok().filter(|v| !v.trim().is_empty());
            std::env::split_paths(&v.unwrap_or_else(|| default.to_string()))
                .filter(|p| p.is_absolute())
                .collect()
        };
        let mut data = vec![data_home.to_path_buf()];
        data.extend(list("XDG_DATA_DIRS", "/usr/local/share:/usr/share"));
        let mut config = vec![config_home.to_path_buf()];
        config.extend(list("XDG_CONFIG_DIRS", "/etc/xdg"));
        AppDirs { data, config }
    }
}

/// The text of a file, or nothing.
fn read_text(p: &Path) -> Option<String> {
    std::fs::read(p)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

// ----------------------------------------------------------------------------------------- MIME

/// One line of `globs2`: weight, type, pattern, and whether the case matters.
#[derive(Clone, Debug)]
struct Glob {
    weight: u32,
    mime: String,
    pattern: String,
    case_sensitive: bool,
}

/// The shared-mime-info database as far as it is needed to name a file's type and its parents.
#[derive(Clone, Debug, Default)]
pub struct MimeDb {
    globs: Vec<Glob>,
    aliases: HashMap<String, String>,
    /// child → parents
    parents: HashMap<String, Vec<String>>,
}

/// `*` and `?` wildcards (and nothing else) against a whole name.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

impl MimeDb {
    /// Read `mime/globs2` (or the older `globs`), `mime/aliases` and `mime/subclasses` of each
    /// data folder; the first folder's lines win on equal weight.
    pub fn load(data_dirs: &[PathBuf]) -> MimeDb {
        let mut db = MimeDb::default();
        for dir in data_dirs {
            let mime = dir.join("mime");
            if let Some(t) = read_text(&mime.join("globs2")) {
                for line in t.lines() {
                    if line.starts_with('#') {
                        continue;
                    }
                    let mut f = line.split(':');
                    let (Some(w), Some(m), Some(p)) = (f.next(), f.next(), f.next()) else {
                        continue;
                    };
                    let flags = f.next().unwrap_or("");
                    db.globs.push(Glob {
                        weight: w.parse().unwrap_or(50),
                        mime: m.to_string(),
                        pattern: p.to_string(),
                        case_sensitive: flags.split(',').any(|x| x == "cs"),
                    });
                }
            } else if let Some(t) = read_text(&mime.join("globs")) {
                for line in t.lines() {
                    if let Some((m, p)) = line.split_once(':')
                        && !line.starts_with('#')
                    {
                        db.globs.push(Glob {
                            weight: 50,
                            mime: m.to_string(),
                            pattern: p.to_string(),
                            case_sensitive: false,
                        });
                    }
                }
            }
            if let Some(t) = read_text(&mime.join("aliases")) {
                for line in t.lines() {
                    if let Some((a, c)) = line.split_once(' ')
                        && !line.starts_with('#')
                    {
                        db.aliases
                            .entry(a.into())
                            .or_insert_with(|| c.trim().into());
                    }
                }
            }
            if let Some(t) = read_text(&mime.join("subclasses")) {
                for line in t.lines() {
                    if let Some((c, p)) = line.split_once(' ')
                        && !line.starts_with('#')
                    {
                        let v = db.parents.entry(c.into()).or_default();
                        if !v.iter().any(|x| x == p.trim()) {
                            v.push(p.trim().into());
                        }
                    }
                }
            }
        }
        db
    }

    /// The type a file name has, by the patterns of the database: the heaviest weight, then the
    /// longest pattern.
    pub fn by_name(&self, name: &str) -> Option<String> {
        let lower = name.to_lowercase();
        self.globs
            .iter()
            .filter(|g| {
                if g.case_sensitive {
                    glob_matches(&g.pattern, name)
                } else {
                    glob_matches(&g.pattern.to_lowercase(), &lower)
                }
            })
            .max_by_key(|g| (g.weight, g.pattern.len()))
            .map(|g| g.mime.clone())
    }

    /// `mime` and every type it is a kind of, nearest first: its canonical name, its parents
    /// (`text/x-rust` → `text/plain`), and `application/octet-stream` for anything that is
    /// not a folder.
    pub fn lineage(&self, mime: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut queue = vec![self.aliases.get(mime).cloned().unwrap_or(mime.to_string())];
        while let Some(m) = queue.first().cloned() {
            queue.remove(0);
            if out.contains(&m) {
                continue;
            }
            if let Some(ps) = self.parents.get(&m) {
                queue.extend(
                    ps.iter()
                        .map(|p| self.aliases.get(p).cloned().unwrap_or(p.clone())),
                );
            }
            if m.starts_with("text/") && m != "text/plain" {
                queue.push("text/plain".into());
            }
            out.push(m);
        }
        if mime != "inode/directory" && !out.iter().any(|m| m == "application/octet-stream") {
            out.push("application/octet-stream".into());
        }
        out
    }
}

/// The type of a file: from its name, or else from its first bytes.
pub fn mime_of(db: &MimeDb, path: &Path) -> String {
    if std::fs::metadata(path).is_ok_and(|m| m.is_dir()) {
        return "inode/directory".into();
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(m) = db.by_name(name)
    {
        return m;
    }
    sniff(path)
}

/// A few signatures, then text or not.
fn sniff(path: &Path) -> String {
    use std::io::Read;
    let mut buf = [0u8; 4096];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    let head = &buf[..n];
    let starts = |s: &[u8]| head.starts_with(s);
    if n == 0 {
        return "text/plain".into();
    }
    if starts(b"%PDF-") {
        return "application/pdf".into();
    }
    if starts(b"\x89PNG\r\n\x1a\n") {
        return "image/png".into();
    }
    if starts(b"\xff\xd8\xff") {
        return "image/jpeg".into();
    }
    if starts(b"GIF87a") || starts(b"GIF89a") {
        return "image/gif".into();
    }
    if starts(b"PK\x03\x04") {
        return "application/zip".into();
    }
    if starts(b"\x1f\x8b") {
        return "application/gzip".into();
    }
    if starts(b"\x7fELF") {
        return "application/x-executable".into();
    }
    if !head.contains(&0) && std::str::from_utf8(head).is_ok() {
        return "text/plain".into();
    }
    "application/octet-stream".into()
}

// ------------------------------------------------------------------------------ mimeapps.list

/// What `mimeapps.list` files say, merged: the default applications for a type, those added to
/// it, and those taken away.
#[derive(Clone, Debug, Default)]
pub struct MimeApps {
    defaults: HashMap<String, Vec<String>>,
    added: HashMap<String, Vec<String>>,
    removed: HashMap<String, Vec<String>>,
}

impl MimeApps {
    /// The files in the order of the specification: `$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS`,
    /// then `applications/` in each data folder. The first to say something about a type wins
    /// for the defaults.
    pub fn load(dirs: &AppDirs) -> MimeApps {
        let mut files: Vec<PathBuf> = dirs
            .config
            .iter()
            .map(|d| d.join("mimeapps.list"))
            .collect();
        files.extend(
            dirs.data
                .iter()
                .map(|d| d.join("applications/mimeapps.list")),
        );
        let mut m = MimeApps::default();
        for f in files {
            if let Some(text) = read_text(&f) {
                m.merge(&text);
            }
        }
        m
    }

    pub fn merge(&mut self, text: &str) {
        #[derive(PartialEq)]
        enum Group {
            Default,
            Added,
            Removed,
            Other,
        }
        let mut group = Group::Other;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                group = match line {
                    "[Default Applications]" => Group::Default,
                    "[Added Associations]" => Group::Added,
                    "[Removed Associations]" => Group::Removed,
                    _ => Group::Other,
                };
                continue;
            }
            let Some((mime, ids)) = line.split_once('=') else {
                continue;
            };
            let ids = split_list(ids);
            let target = match group {
                Group::Default => &mut self.defaults,
                Group::Added => &mut self.added,
                Group::Removed => &mut self.removed,
                Group::Other => continue,
            };
            let v = target.entry(mime.trim().to_string()).or_default();
            for id in ids {
                if !v.contains(&id) {
                    v.push(id);
                }
            }
        }
    }

    /// The default for the first type of `lineage` that has one.
    pub fn default_for(&self, lineage: &[String]) -> Vec<String> {
        lineage
            .iter()
            .find_map(|m| self.defaults.get(m).filter(|v| !v.is_empty()))
            .cloned()
            .unwrap_or_default()
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    fn added_for(&self, lineage: &[String]) -> Vec<String> {
        lineage
            .iter()
            .filter_map(|m| self.added.get(m))
            .flatten()
            .cloned()
            .collect()
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    fn removed_for(&self, lineage: &[String]) -> HashSet<String> {
        lineage
            .iter()
            .filter_map(|m| self.removed.get(m))
            .flatten()
            .cloned()
            .collect()
    }
}

// ----------------------------------------------------------------------------------- discovery

/// The id of a desktop file: its path under `applications/`, with `/` written `-`.
#[cfg(unix)]
fn desktop_id(applications: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(applications).ok()?;
    Some(rel.to_string_lossy().replace(['/', '\\'], "-"))
}

/// Every `.desktop` file under an `applications` folder, with its id (a folder inside is part
/// of the id).
#[cfg(unix)]
fn desktop_files(applications: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![applications.to_path_buf()];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            visited += 1;
            if visited > 20_000 {
                return out;
            }
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "desktop")
                && let Some(id) = desktop_id(applications, &p)
            {
                out.push((id, p));
            }
        }
    }
    out
}

/// Whether the program a desktop file names (`TryExec`, else the first word of `Exec`) exists.
#[cfg(unix)]
fn program_exists(program: &str, path_var: &std::ffi::OsStr) -> bool {
    if program.contains('/') {
        return super::which::is_executable(Path::new(program));
    }
    super::which::which_in(path_var, program).is_some()
}

/// The installed applications that can open `path`, the system's default first.
#[cfg(unix)]
pub fn find_apps(dirs: &AppDirs, db: &MimeDb, path: &Path, lang: &str) -> (String, Vec<OpenApp>) {
    let mime = mime_of(db, path);
    let lineage = db.lineage(&mime);
    let wanted: HashSet<&str> = lineage.iter().map(String::as_str).collect();
    let mimeapps = MimeApps::load(dirs);
    let removed = mimeapps.removed_for(&lineage);

    // What `mimeinfo.cache` names for these types, and what `mimeapps.list` adds.
    let mut named: Vec<String> = mimeapps.added_for(&lineage);
    for d in &dirs.data {
        if let Some(t) = read_text(&d.join("applications/mimeinfo.cache")) {
            for line in t.lines() {
                if let Some((m, ids)) = line.split_once('=')
                    && wanted.contains(m.trim())
                {
                    named.extend(split_list(ids));
                }
            }
        }
    }

    // Each desktop file once, the first folder's winning.
    let mut by_id: BTreeMap<String, (PathBuf, DesktopEntry)> = BTreeMap::new();
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for d in &dirs.data {
        for (id, file) in desktop_files(&d.join("applications")) {
            if by_id.contains_key(&id) {
                continue;
            }
            if let Some(text) = read_text(&file)
                && let Some(entry) = parse_desktop(&text, lang)
            {
                by_id.insert(id, (file, entry));
            }
        }
    }

    let defaults = mimeapps.default_for(&lineage);
    let mut found: Vec<OpenApp> = Vec::new();
    for (id, (file, entry)) in &by_id {
        if entry.hidden || removed.contains(id) {
            continue;
        }
        let fits =
            entry.mime_types.iter().any(|m| wanted.contains(m.as_str())) || named.contains(id);
        if !fits {
            continue;
        }
        let program = entry
            .try_exec
            .clone()
            .or_else(|| exec_words(&entry.exec).into_iter().next())
            .unwrap_or_default();
        if !program.is_empty() && !program_exists(&program, &path_var) {
            continue;
        }
        found.push(OpenApp {
            id: id.clone(),
            name: entry.name.clone(),
            exec: entry.exec.clone(),
            terminal: entry.terminal,
            is_default: false,
            source: Some(file.clone()),
            icon: entry.icon.clone(),
        });
    }
    // The system's default: the first of its list that is installed.
    if let Some(i) = defaults
        .iter()
        .find_map(|id| found.iter().position(|a| &a.id == id))
    {
        found[i].is_default = true;
        let d = found.remove(i);
        found.insert(0, d);
    }
    (mime, found)
}

// -------------------------------------------------------------------------------- what rada keeps

/// The choices rada remembers, in `open_with.json` of its state folder: programs used lately
/// (most recent first) and, per kind of file, the program to always use. Never the system's
/// own defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default)]
    pub recent: Vec<String>,
    #[serde(default)]
    pub always: BTreeMap<String, String>,
}

const PREFS_FILE: &str = "open_with.json";
const RECENT_KEPT: usize = 20;

impl Prefs {
    pub fn load(state_dir: &Path) -> Prefs {
        std::fs::read(state_dir.join(PREFS_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, state_dir: &Path) -> std::io::Result<()> {
        crate::statefile::write_atomic(
            &state_dir.join(PREFS_FILE),
            &serde_json::to_vec_pretty(self).unwrap_or_default(),
        )
    }

    /// `id` was used: first among the recent.
    pub fn used(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_string());
        self.recent.truncate(RECENT_KEPT);
    }
}

/// The apps in the order of the list: the one chosen to be used always for this kind of file,
/// the system's default, those used lately (the most recent first), then the rest by name.
pub fn ordered(mut apps: Vec<OpenApp>, prefs: &Prefs, kind: &str) -> Vec<OpenApp> {
    let always = prefs.always.get(kind);
    let rank = |a: &OpenApp| -> (u8, usize, String) {
        if always == Some(&a.id) {
            (0, 0, String::new())
        } else if a.is_default {
            (1, 0, String::new())
        } else if let Some(i) = prefs.recent.iter().position(|r| r == &a.id) {
            (2, i, String::new())
        } else {
            (3, 0, a.name.to_lowercase())
        }
    };
    apps.sort_by_key(rank);
    apps
}

/// Keep the apps whose name contains every word of `filter` (case and accents aside from
/// ASCII are ignored), in the order they came.
pub fn filtered<'a>(apps: &'a [OpenApp], filter: &str) -> Vec<&'a OpenApp> {
    let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    apps.iter()
        .filter(|a| {
            let n = a.name.to_lowercase();
            words.iter().all(|w| n.contains(w.as_str()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEDIT: &str = "[Desktop Entry]\nType=Application\nName=Text Editor\nName[it]=Editor di testo\nExec=gedit %U\nIcon=gedit\nMimeType=text/plain;text/x-tex;\nCategories=Utility;\n";

    #[test]
    fn a_desktop_file_gives_its_name_in_the_language_of_the_user() {
        let e = parse_desktop(GEDIT, "en").unwrap();
        assert_eq!(e.name, "Text Editor");
        assert_eq!(e.exec, "gedit %U");
        assert_eq!(e.mime_types, ["text/plain", "text/x-tex"]);
        assert!(!e.terminal && !e.hidden);
        assert_eq!(parse_desktop(GEDIT, "it").unwrap().name, "Editor di testo");
        assert_eq!(parse_desktop(GEDIT, "de").unwrap().name, "Text Editor");
    }

    #[test]
    fn what_is_not_an_application_is_not_one() {
        assert!(
            parse_desktop("[Desktop Entry]\nType=Link\nName=x\nURL=http://x\n", "en").is_none()
        );
        assert!(parse_desktop("[Desktop Entry]\nType=Application\nName=x\n", "en").is_none());
        assert!(parse_desktop("Name=x\nExec=y\nType=Application\n", "en").is_none());
        let hidden = parse_desktop(
            "[Desktop Entry]\nType=Application\nName=x\nExec=y\nNoDisplay=true\n",
            "en",
        )
        .unwrap();
        assert!(hidden.hidden);
        // Another group does not leak into the entry.
        let t = "[Desktop Entry]\nType=Application\nName=A\nExec=a\n[Desktop Action new]\nName=B\nExec=b\n";
        let e = parse_desktop(t, "en").unwrap();
        assert_eq!((e.name.as_str(), e.exec.as_str()), ("A", "a"));
    }

    #[test]
    fn terminal_and_semicolons_in_lists() {
        let e = parse_desktop(
            "[Desktop Entry]\nType=Application\nName=Vim\nExec=vim %F\nTerminal=true\nMimeType=text/plain;a\\;b/c;\n",
            "en",
        )
        .unwrap();
        assert!(e.terminal);
        assert_eq!(e.mime_types, ["text/plain", "a;b/c"]);
    }

    fn entry(exec: &str) -> DesktopEntry {
        DesktopEntry {
            name: "Viewer".into(),
            exec: exec.into(),
            icon: Some("viewer-icon".into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_relative_file_that_starts_with_a_dash_never_stands_where_an_option_could() {
        let f = Path::new("--help");
        let safe = Path::new(".").join(f).to_string_lossy().into_owned();
        let go = |exec: &str| expand_exec(exec, &[f], &entry(exec), None);
        assert_eq!(go("app %f"), ["app", safe.as_str()]);
        assert_eq!(go("app %F"), ["app", safe.as_str()]);
        assert_eq!(go("app --new"), ["app", "--new", safe.as_str()]);
        // Inside a word it follows something that makes it a value.
        assert_eq!(go("app --file=%f"), ["app", "--file=--help"]);
    }

    #[test]
    fn field_codes_become_the_files() {
        let f = Path::new("/home/u/My Docs/a b.txt");
        let g = Path::new("/home/u/c.txt");
        let run = |exec: &str, files: &[&Path]| {
            expand_exec(exec, files, &entry(exec), Some(Path::new("/x/v.desktop")))
        };
        assert_eq!(
            run("viewer %f", &[f]),
            ["viewer", "/home/u/My Docs/a b.txt"]
        );
        assert_eq!(
            run("viewer %F", &[f, g]),
            ["viewer", "/home/u/My Docs/a b.txt", "/home/u/c.txt"]
        );
        assert_eq!(
            run("viewer %u", &[f]),
            ["viewer", "file:///home/u/My%20Docs/a%20b.txt"]
        );
        assert_eq!(
            run("viewer --open=%f --name %c", &[f]),
            [
                "viewer",
                "--open=/home/u/My Docs/a b.txt",
                "--name",
                "Viewer"
            ]
        );
        assert_eq!(run("viewer %i %f", &[f])[1..3], ["--icon", "viewer-icon"]);
        assert_eq!(run("viewer 100%% %f", &[f])[1], "100%");
        // Codes the specification dropped, and codes for something absent, leave nothing.
        assert_eq!(
            run("viewer %d %n %m %f", &[f]),
            ["viewer", "/home/u/My Docs/a b.txt"]
        );
        assert_eq!(
            run("viewer %k", &[f]),
            ["viewer", "/x/v.desktop", "/home/u/My Docs/a b.txt"]
        );
    }

    #[test]
    fn quotes_keep_a_program_with_blanks_together() {
        let f = Path::new("/a.txt");
        let e = entry("");
        assert_eq!(
            expand_exec(
                r#""/opt/My App/bin/app" --flag "a \"quoted\" arg" %f"#,
                &[f],
                &e,
                None
            ),
            [
                "/opt/My App/bin/app",
                "--flag",
                r#"a "quoted" arg"#,
                "/a.txt"
            ]
        );
        // The general escapes of the file come first: \s is a blank, \\ a backslash.
        assert_eq!(unescape_value(r"a\sb\\c\n"), "a b\\c\n");
        // No code for files: the path is added, because the person chose this program for it.
        assert_eq!(
            expand_exec("app --new", &[f], &e, None),
            ["app", "--new", "/a.txt"]
        );
    }

    #[test]
    fn globs_name_the_type_of_a_file_and_the_heaviest_wins() {
        let mut db = MimeDb::default();
        for (w, m, p) in [
            (50, "text/plain", "*.txt"),
            (50, "application/x-compressed-tar", "*.tar.gz"),
            (50, "application/gzip", "*.gz"),
            (50, "text/x-makefile", "Makefile"),
            (50, "image/jpeg", "*.jpg"),
        ] {
            db.globs.push(Glob {
                weight: w,
                mime: m.into(),
                pattern: p.into(),
                case_sensitive: false,
            });
        }
        assert_eq!(db.by_name("notes.TXT").as_deref(), Some("text/plain"));
        assert_eq!(
            db.by_name("a.tar.gz").as_deref(),
            Some("application/x-compressed-tar")
        );
        assert_eq!(db.by_name("a.gz").as_deref(), Some("application/gzip"));
        assert_eq!(db.by_name("Makefile").as_deref(), Some("text/x-makefile"));
        assert_eq!(db.by_name("makefile.txt").as_deref(), Some("text/plain"));
        assert_eq!(db.by_name("photo.JPG").as_deref(), Some("image/jpeg"));
        assert_eq!(db.by_name("unknown.xyz"), None);
        assert!(glob_matches("a?c*", "abcdef"));
        assert!(!glob_matches("a?c", "ac"));
    }

    #[test]
    fn a_type_is_also_the_types_it_is_a_kind_of() {
        let mut db = MimeDb::default();
        db.parents
            .insert("text/x-rust".into(), vec!["text/plain".into()]);
        db.parents
            .insert("image/svg+xml".into(), vec!["application/xml".into()]);
        db.parents
            .insert("application/xml".into(), vec!["text/plain".into()]);
        db.aliases.insert("text/rust".into(), "text/x-rust".into());
        assert_eq!(
            db.lineage("text/x-rust"),
            ["text/x-rust", "text/plain", "application/octet-stream"]
        );
        assert_eq!(db.lineage("text/rust")[0], "text/x-rust");
        assert_eq!(
            db.lineage("image/svg+xml"),
            [
                "image/svg+xml",
                "application/xml",
                "text/plain",
                "application/octet-stream"
            ]
        );
        // Any text type is a text/plain, known or not.
        assert!(
            db.lineage("text/x-weird")
                .contains(&"text/plain".to_string())
        );
        assert_eq!(db.lineage("inode/directory"), ["inode/directory"]);
    }

    #[test]
    fn mimeapps_lists_say_who_is_the_default_who_is_added_and_who_is_gone() {
        let mut m = MimeApps::default();
        m.merge("[Default Applications]\ntext/plain=kate.desktop;gedit.desktop;\n[Added Associations]\ntext/plain=vim.desktop;\n[Removed Associations]\ntext/plain=nano.desktop;\n");
        // A second file: the first's default is not replaced, but a type it lacks is given.
        m.merge("[Default Applications]\ntext/plain=other.desktop;\nimage/png=viewer.desktop;\n");
        let lineage = vec!["text/x-rust".to_string(), "text/plain".to_string()];
        assert_eq!(
            m.default_for(&lineage),
            ["kate.desktop", "gedit.desktop", "other.desktop"]
        );
        assert_eq!(m.added_for(&lineage), ["vim.desktop"]);
        assert!(m.removed_for(&lineage).contains("nano.desktop"));
        assert_eq!(
            m.default_for(&["image/png".to_string()]),
            ["viewer.desktop"]
        );
        assert!(m.default_for(&["video/mp4".to_string()]).is_empty());
    }

    fn app(id: &str, name: &str, default: bool) -> OpenApp {
        OpenApp {
            id: id.into(),
            name: name.into(),
            exec: format!("{id} %f"),
            terminal: false,
            is_default: default,
            source: None,
            icon: None,
        }
    }

    #[test]
    fn the_list_is_the_chosen_one_the_default_the_recent_then_the_rest_by_name() {
        let apps = vec![
            app("zed", "Zed", false),
            app("kate", "Kate", true),
            app("emacs", "Emacs", false),
            app("vim", "Vim", false),
            app("gedit", "Text Editor", false),
        ];
        let mut p = Prefs::default();
        let names = |v: &[OpenApp]| v.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(&ordered(apps.clone(), &p, "text/plain")),
            ["Kate", "Emacs", "Text Editor", "Vim", "Zed"]
        );
        p.used("vim");
        p.used("zed");
        assert_eq!(
            names(&ordered(apps.clone(), &p, "text/plain")),
            ["Kate", "Zed", "Vim", "Emacs", "Text Editor"]
        );
        p.always.insert("text/plain".into(), "emacs".into());
        assert_eq!(
            names(&ordered(apps.clone(), &p, "text/plain"))[..2],
            ["Emacs", "Kate"]
        );
        // For another kind of file the choice does not apply.
        assert_eq!(names(&ordered(apps, &p, "image/png"))[0], "Kate");
        // Used again: first of the recent, and no more than a few are kept.
        for i in 0..30 {
            p.used(&format!("app{i}"));
        }
        p.used("vim");
        assert_eq!(p.recent[0], "vim");
        assert_eq!(p.recent.len(), RECENT_KEPT);
    }

    #[test]
    fn writing_a_name_filters_by_its_words() {
        let apps = vec![
            app("a", "Visual Studio Code", false),
            app("b", "Text Editor", false),
            app("c", "Image Viewer", false),
        ];
        let ids = |f: &str| {
            filtered(&apps, f)
                .iter()
                .map(|a| a.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(""), ["a", "b", "c"]);
        assert_eq!(ids("ed"), ["b"]);
        assert_eq!(ids("studio code"), ["a"]);
        assert_eq!(ids("CODE"), ["a"]);
        assert_eq!(ids("e v"), ["a", "c"]);
        assert!(ids("zzz").is_empty());
    }

    #[test]
    fn prefs_are_kept_between_runs_and_a_broken_file_is_nothing() {
        let t = tempfile::tempdir().unwrap();
        let mut p = Prefs::default();
        p.used("kate.desktop");
        p.always.insert("text/plain".into(), "kate.desktop".into());
        p.save(t.path()).unwrap();
        assert_eq!(Prefs::load(t.path()), p);
        std::fs::write(t.path().join(PREFS_FILE), "not json").unwrap();
        assert_eq!(Prefs::load(t.path()), Prefs::default());
        assert_eq!(Prefs::load(&t.path().join("none")), Prefs::default());
    }

    #[test]
    fn launch_services_answers_become_applications_with_the_default_marked() {
        let out = "*/Applications/TextEdit.app\n-/Applications/Visual Studio Code.app\n-/Applications/TextEdit.app\nnoise\n-/System/Applications/Notes.app\n";
        let apps = parse_launch_services(out);
        let got: Vec<(&str, bool)> = apps
            .iter()
            .map(|a| (a.name.as_str(), a.is_default))
            .collect();
        assert_eq!(
            got,
            [
                ("TextEdit", true),
                ("Visual Studio Code", false),
                ("Notes", false)
            ]
        );
        assert_eq!(apps[1].exec, "/Applications/Visual Studio Code.app");
        assert_eq!(apps[1].id, apps[1].exec);
        assert!(parse_launch_services("").is_empty());
        assert!(LAUNCH_SERVICES_SCRIPT.contains("URLsForApplicationsToOpenURL"));
    }

    #[test]
    fn urls_escape_what_a_url_cannot_hold() {
        assert_eq!(
            file_url(Path::new("/a/b c/é#.txt")),
            "file:///a/b%20c/%C3%A9%23.txt"
        );
    }
}
