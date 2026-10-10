//! The address bar when it is a text field: typing a path, completing it with the
//! subfolders of the folder being typed in, and going there.
//!
//! The subfolders come from a worker (`Services::complete_dirs`): this code never reads the
//! disk. What it keeps is the last list it was given and filters it as the text changes.

use std::path::Component;

use super::*;

/// Most suggestions shown under the field.
pub const MAX_SUGGESTIONS: usize = 8;

pub struct AddressEdit {
    pub text: String,
    /// Cursor position in chars.
    pub cursor: usize,
    /// Subfolders of `listed`, as last reported.
    names: Vec<String>,
    listed: Option<PathBuf>,
    /// The folder a listing was last asked for (so typing does not ask again and again).
    asked: Option<PathBuf>,
    /// The suggestion highlighted with the arrow keys.
    pub selected: Option<usize>,
}

impl AddressEdit {
    pub fn new(text: String) -> AddressEdit {
        let cursor = text.chars().count();
        AddressEdit {
            text,
            cursor,
            names: Vec::new(),
            listed: None,
            asked: None,
            selected: None,
        }
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
        self.selected = None;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            let at = self.byte_at(self.cursor - 1);
            self.text.remove(at);
            self.cursor -= 1;
            self.selected = None;
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
            self.selected = None;
        }
    }

    /// The folder whose subfolders are being completed, and the start of the name typed in it.
    fn target(&self, cwd: &Path, home: &Path) -> (PathBuf, String) {
        match self.text.rfind(is_separator) {
            Some(i) => {
                let dir_text = &self.text[..=i];
                (expand(dir_text, cwd, home), self.text[i + 1..].to_string())
            }
            None => (cwd.to_path_buf(), self.text.clone()),
        }
    }

    /// Names that continue what is typed, best first.
    pub fn suggestions(&self, cwd: &Path, home: &Path) -> Vec<String> {
        let (dir, prefix) = self.target(cwd, home);
        if self.listed.as_deref() != Some(dir.as_path()) {
            return Vec::new();
        }
        let lower = prefix.to_lowercase();
        let hidden_ok = prefix.starts_with('.');
        let mut out: Vec<&String> = self
            .names
            .iter()
            .filter(|n| (hidden_ok || !n.starts_with('.')) && n.to_lowercase().starts_with(&lower))
            .collect();
        // What matches with the case as typed comes first.
        out.sort_by_key(|n| !n.starts_with(&prefix));
        out.into_iter().take(MAX_SUGGESTIONS).cloned().collect()
    }

    /// The text with the name being typed replaced by `name` and a separator after it.
    fn with_name(&self, name: &str) -> String {
        let cut = self.text.rfind(is_separator).map_or(0, |i| i + 1);
        format!("{}{}{}", &self.text[..cut], name, SEP)
    }
}

const SEP: char = '/';

fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// Where typed text leads: `~` is the home folder, a relative path starts at the folder
/// being looked at, and `.` and `..` are resolved by the text alone (no link is followed).
pub fn expand(text: &str, cwd: &Path, home: &Path) -> PathBuf {
    let text = text.trim();
    let raw = if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text
        .strip_prefix("~/")
        .or_else(|| text.strip_prefix("~\\").filter(|_| cfg!(windows)))
    {
        home.join(rest)
    } else {
        let p = PathBuf::from(text);
        if p.is_absolute() { p } else { cwd.join(p) }
    };
    let mut out = PathBuf::new();
    for c in raw.components() {
        match c {
            Component::ParentDir => {
                // Never above the root.
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir) | None
                ) {
                    out.pop();
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(std::path::MAIN_SEPARATOR_STR);
    }
    out
}

impl App {
    /// Turn the address bar into a text field holding the current path.
    pub fn start_address_edit(&mut self) {
        let mut text = fmt::short_path(&self.address_base(), &self.svc.home);
        if !text.ends_with(is_separator) {
            text.push(SEP);
        }
        self.address = Some(AddressEdit::new(text));
        self.filter_editing_off();
        self.address_changed();
    }

    /// The folder shown in the address bar: inside an archive, the archive's own path.
    fn address_base(&self) -> PathBuf {
        self.cwd.clone()
    }

    fn filter_editing_off(&mut self) {
        if let Some(f) = &mut self.filter {
            f.editing = false;
        }
    }

    pub fn cancel_address_edit(&mut self) {
        self.address = None;
        self.dirty = true;
    }

    /// The text changed: ask for the subfolders of the folder it is in, if not asked yet.
    fn address_changed(&mut self) {
        let (home, cwd) = (self.svc.home.clone(), self.cwd.clone());
        let Some(a) = &mut self.address else { return };
        let (dir, _) = a.target(&cwd, &home);
        if a.asked.as_deref() != Some(dir.as_path()) {
            a.asked = Some(dir.clone());
            self.svc.complete_dirs(dir);
        }
    }

    /// A list of subfolders arrived from the worker.
    pub(super) fn on_complete(&mut self, dir: PathBuf, names: Vec<String>) {
        if let Some(a) = &mut self.address
            && a.asked.as_deref() == Some(dir.as_path())
        {
            a.names = names;
            a.listed = Some(dir);
            a.selected = None;
        }
    }

    /// What is offered under the field right now.
    pub fn address_suggestions(&self) -> Vec<String> {
        match &self.address {
            Some(a) => a.suggestions(&self.cwd, &self.svc.home),
            None => Vec::new(),
        }
    }

    /// A suggestion was clicked: go there.
    pub(super) fn pick_suggestion(&mut self, i: usize) {
        let sugg = self.address_suggestions();
        if let (Some(a), Some(name)) = (&mut self.address, sugg.get(i)) {
            a.text = a.with_name(name);
        }
        self.submit_address();
    }

    pub(super) fn on_address_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let sugg = self.address_suggestions();
        let Some(a) = &mut self.address else { return };
        match key.code {
            KeyCode::Esc => return self.cancel_address_edit(),
            KeyCode::Enter => {
                if let Some(name) = a.selected.and_then(|i| sugg.get(i)) {
                    let text = a.with_name(name);
                    a.text = text;
                }
                return self.submit_address();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let pick = a.selected.and_then(|i| sugg.get(i)).cloned().or_else(|| {
                    if sugg.len() == 1 {
                        sugg.first().cloned()
                    } else {
                        None
                    }
                });
                match pick {
                    Some(name) => {
                        a.text = a.with_name(&name);
                        a.cursor = a.text.chars().count();
                    }
                    None => {
                        // Several names: as much as they all share.
                        let common = common_prefix(&sugg);
                        let cut = a.text.rfind(is_separator).map_or(0, |i| i + 1);
                        if common.chars().count() > a.text[cut..].chars().count() {
                            a.text = format!("{}{}", &a.text[..cut], common);
                            a.cursor = a.text.chars().count();
                        }
                    }
                }
                a.selected = None;
            }
            KeyCode::Down => {
                if !sugg.is_empty() {
                    a.selected = Some(a.selected.map_or(0, |i| (i + 1) % sugg.len()));
                }
            }
            KeyCode::Up => {
                if !sugg.is_empty() {
                    a.selected = Some(
                        a.selected
                            .map_or(sugg.len() - 1, |i| (i + sugg.len() - 1) % sugg.len()),
                    );
                }
            }
            KeyCode::Left => a.cursor = a.cursor.saturating_sub(1),
            KeyCode::Right => a.cursor = (a.cursor + 1).min(a.text.chars().count()),
            KeyCode::Home => a.cursor = 0,
            KeyCode::End => a.cursor = a.text.chars().count(),
            KeyCode::Backspace => a.backspace(),
            KeyCode::Delete => a.delete(),
            KeyCode::Char('u') if ctrl => {
                a.text.clear();
                a.cursor = 0;
                a.selected = None;
            }
            KeyCode::Char(c) if !ctrl && !alt => a.insert(c),
            _ => {}
        }
        self.address_changed();
    }

    /// Go where the text says.
    pub fn submit_address(&mut self) {
        let Some(a) = self.address.take() else { return };
        if a.text.trim().is_empty() {
            return;
        }
        let target = expand(&a.text, &self.cwd, &self.svc.home);
        if target != self.cwd {
            self.open_dir(target);
        }
    }
}

fn common_prefix(names: &[String]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut len = first.chars().count();
    for n in &names[1..] {
        len = len.min(
            first
                .chars()
                .zip(n.chars())
                .take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
                .count(),
        );
    }
    first.chars().take(len).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_paths_are_resolved_by_their_text() {
        let (cwd, home) = (Path::new("/home/u/projects"), Path::new("/home/u"));
        assert_eq!(expand("~", cwd, home), PathBuf::from("/home/u"));
        assert_eq!(expand("~/docs", cwd, home), PathBuf::from("/home/u/docs"));
        assert_eq!(expand("/etc/", cwd, home), PathBuf::from("/etc"));
        assert_eq!(
            expand("demo", cwd, home),
            PathBuf::from("/home/u/projects/demo")
        );
        assert_eq!(expand("..", cwd, home), PathBuf::from("/home/u"));
        assert_eq!(expand("../../..", cwd, home), PathBuf::from("/"));
        assert_eq!(expand("../../../../..", cwd, home), PathBuf::from("/"));
        assert_eq!(
            expand("./a/./b/../c", cwd, home),
            PathBuf::from("/home/u/projects/a/c")
        );
        assert_eq!(expand("  /tmp  ", cwd, home), PathBuf::from("/tmp"));
    }

    #[test]
    fn suggestions_follow_the_name_being_typed() {
        let (cwd, home) = (Path::new("/home/u"), Path::new("/home/u"));
        let mut a = AddressEdit::new("~/pr".to_string());
        a.names = ["projects", "Pictures", ".private", "docs", "prints"]
            .map(String::from)
            .to_vec();
        a.listed = Some(PathBuf::from("/home/u"));
        // Hidden folders only when asked for with a dot; case is ignored but exact case first.
        assert_eq!(a.suggestions(cwd, home), ["projects", "prints"]);
        a.text = "~/P".into();
        assert_eq!(a.suggestions(cwd, home), ["Pictures", "projects", "prints"]);
        a.text = "~/.".into();
        assert_eq!(a.suggestions(cwd, home), [".private"]);
        // A list for another folder is no use for this text.
        a.text = "/etc/".into();
        assert!(a.suggestions(cwd, home).is_empty());
    }

    #[test]
    fn a_chosen_name_replaces_the_one_being_typed() {
        let a = AddressEdit::new("~/projects/de".to_string());
        assert_eq!(a.with_name("demo"), "~/projects/demo/");
        let b = AddressEdit::new("de".to_string());
        assert_eq!(b.with_name("demo"), "demo/");
    }

    #[test]
    fn what_the_names_share_is_what_tab_completes() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(common_prefix(&n(&["projects", "Prints", "prisma"])), "pr");
        assert_eq!(common_prefix(&n(&["alpha"])), "alpha");
        assert_eq!(common_prefix(&[]), "");
    }
}
