//! The command line: the bar when what is typed starts with a colon.
//!
//! The line is a command by its name with its arguments (`:mkdir notes`, `:sort size
//! --reverse`, `:cd ~/src`). Enter runs it when it reads as one; when it does not name a
//! command the line is words, and Enter runs the best suggestion as the bar always did. Tab
//! completes the name, a value or a path; the arrows go through the lines run before, which
//! are kept between sessions.
//!
//! A command that changes files makes a request and the plan window opens: nothing here
//! touches a file.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use rada_core::i18n::{Key, tf};
use rada_core::model::SortKey;

use super::address::{expand, is_separator};
use super::{App, BarHit, BarItem};
use crate::registry::{self, Args, Avail, CommandSpec, Completion, Ctx, Facts, Surface};

/// Lines kept in the history.
const HISTORY: usize = 200;

/// The path being typed as an argument of the command line `text`, if its last word is one:
/// the folder it is in, that folder as it was typed, and the start of the name.
pub(super) fn typed_path(text: &str, cwd: &Path, home: &Path) -> Option<(PathBuf, String, String)> {
    let Completion::Path { typed, .. } = registry::complete(line_of(text)) else {
        return None;
    };
    let (dir_text, prefix) = match typed.rfind(is_separator) {
        Some(i) => (typed[..=i].to_string(), typed[i + 1..].to_string()),
        None => (String::new(), typed),
    };
    let dir = if dir_text.is_empty() {
        cwd.to_path_buf()
    } else {
        expand(&dir_text, cwd, home)
    };
    Some((dir, dir_text, prefix))
}

/// What is written after the colon.
fn line_of(text: &str) -> &str {
    let text = text.trim_start();
    text.strip_prefix(':').unwrap_or(text)
}

/// A word as it must be typed to be read as one: its spaces after a backslash.
fn escaped(word: &str) -> String {
    word.replace('\\', "\\\\").replace(' ', "\\ ")
}

impl App {
    /// What a command needs to know about where it is asked for from `surface`.
    pub fn facts(&self, surface: Surface) -> Facts {
        let cur = self.tab.current();
        Facts {
            surface,
            read_only: self.tab.archive.is_some(),
            virtual_view: self.tab.virtual_view().is_some(),
            clipboard: self.ops.clipboard.is_some(),
            has_target: !self.tab.sel.marked.is_empty() || cur.is_some(),
            has_cursor: cur.is_some(),
            cursor_dir: cur.is_some_and(|e| e.is_navigable_dir()),
            cursor_package: cur.is_some_and(|e| e.package),
            on_archive_item: self.on_archive_item(),
            quick_look: self.svc.platform.opener().can_quick_look(),
            chooser: self.session.chooser.as_ref().map(|c| c.facts()),
        }
    }

    /// The bar as a command line: open, with the colon already written.
    pub fn open_command_line(&mut self) {
        self.start_bar(false);
        if let Some(a) = &mut self.windows.address {
            a.text = ":".into();
            a.cursor = 1;
        }
        self.bar_changed();
    }

    /// A key for the command line. `false` when the bar is not one, and for a key that only
    /// edits the text.
    pub(super) fn on_command_line_key(&mut self, key: KeyEvent) -> bool {
        let line = self.windows.address.as_ref().map(|a| a.mode());
        if line != Some(super::BarMode::Command) {
            return false;
        }
        match key.code {
            KeyCode::Enter => self.command_enter(),
            KeyCode::Tab | KeyCode::BackTab => self.complete_command_line(),
            KeyCode::Up => self.recall(true),
            KeyCode::Down => self.recall(false),
            _ => return false,
        }
        true
    }

    /// Enter: run the line if it is a command; say what is wrong with it if it is one that
    /// cannot be read; otherwise run the best suggestion, as the bar does with words.
    fn command_enter(&mut self) {
        let Some(a) = &self.windows.address else {
            return;
        };
        let line = line_of(&a.text).trim().to_string();
        let parsed = match registry::parse(&line, &self.svc.home) {
            Err(e) => return self.line_problem(e.at, &e.message),
            Ok(None) => return self.run_best_hit(),
            Ok(Some(parsed)) => parsed,
        };
        // The bar closes first: what the command opens (a window, another bar) comes after.
        let bar = self.windows.address.take();
        match self.run_command(parsed.spec, &parsed.args) {
            Ok(()) => self.remember_line(line),
            Err(why) => {
                self.windows.address = bar;
                self.line_problem(0, &why);
            }
        }
        self.redraw.dirty = true;
    }

    /// Run a command with its arguments. With none, a command that is an action does what
    /// its key does; with some, it makes its request, which goes to the plan window, or does
    /// what it does in the interface.
    pub fn run_command(&mut self, spec: &CommandSpec, args: &Args) -> Result<(), String> {
        match spec.availability(&self.facts(Surface::Line)) {
            Avail::Enabled => {}
            Avail::Disabled(why) => return Err(why.tr().to_string()),
            Avail::Hidden => return Err(Key::WhyNotHere.tr().to_string()),
        }
        match (spec.action, spec.request, spec.run_ui) {
            (Some(action), _, _) if args.is_empty() => self.dispatch(action),
            (_, Some(request), _) => {
                let req = request(args, &self.line_ctx())?;
                let handle = self.svc.jobs.plan_request(req);
                self.begin_plan(Key::PlanningGeneric.tr(), handle);
            }
            (_, _, Some(run)) => run(self, args)?,
            (Some(action), _, _) => self.dispatch(action),
            (None, None, None) => {}
        }
        Ok(())
    }

    fn line_ctx(&self) -> Ctx {
        Ctx {
            cwd: self.tab.cwd.clone(),
            home: self.svc.home.clone(),
            targets: self.tab.targets(),
        }
    }

    /// Say under the bar why the line was not run, and where in it the problem is.
    fn line_problem(&mut self, at: usize, message: &str) {
        let Some(a) = &mut self.windows.address else {
            return;
        };
        // Counted from 1, and the colon is the first column.
        let text = tf(Key::LineAt, &[&(at + 2), &message]);
        a.hits.retain(|h| !matches!(h.item, BarItem::Problem(_)));
        a.hits.insert(
            0,
            BarHit {
                item: BarItem::Problem(text),
                label_pos: Vec::new(),
            },
        );
        a.hit_selected = None;
    }

    /// The line is not a command: its words found commands and places, and the first one runs.
    fn run_best_hit(&mut self) {
        let best = self.windows.address.as_ref().and_then(|a| {
            a.hits
                .iter()
                .position(|h| !matches!(h.item, BarItem::Problem(_) | BarItem::Search(_)))
        });
        match best {
            Some(i) => self.run_bar_hit(i),
            None => self.cancel_address_edit(),
        }
    }

    /// Keep a line that was run: once, the latest last, for the next time and the next start.
    fn remember_line(&mut self, line: String) {
        let lines = &mut self.tabs.saved_ui.commands;
        lines.retain(|l| *l != line);
        lines.push(line);
        let extra = lines.len().saturating_sub(HISTORY);
        lines.drain(..extra);
        self.save_ui_state();
    }

    /// The lines run before, oldest first.
    pub fn command_history(&self) -> &[String] {
        &self.tabs.saved_ui.commands
    }

    /// The arrows: the line run before the one shown, or the one after it; past the latest,
    /// the empty line again.
    fn recall(&mut self, older: bool) {
        let lines = self.tabs.saved_ui.commands.clone();
        let Some(a) = &mut self.windows.address else {
            return;
        };
        let at = match (a.recalled, older) {
            (None, true) if !lines.is_empty() => Some(lines.len() - 1),
            (None, _) => return,
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < lines.len() => Some(i + 1),
            (Some(_), false) => None,
        };
        a.text = format!(":{}", at.map_or("", |i| lines[i].as_str()));
        a.cursor = a.text.chars().count();
        self.bar_changed();
        if let Some(a) = &mut self.windows.address {
            a.recalled = at;
        }
    }

    /// Tab: put the next thing the last word can become in its place; with nothing to
    /// complete, go to the next suggestion as the bar does.
    fn complete_command_line(&mut self) {
        let Some(a) = &self.windows.address else {
            return;
        };
        let (start, words, at) = match a.cycle.clone() {
            Some((start, words, at)) => (start, words.clone(), (at + 1) % words.len()),
            None => match self.completions() {
                Some((start, words)) => (start, words, 0),
                None => {
                    if let Some(a) = &mut self.windows.address {
                        a.next_hit();
                    }
                    return;
                }
            },
        };
        if let Some(a) = &mut self.windows.address {
            let kept: String = line_of(&a.text).chars().take(start).collect();
            a.text = format!(":{kept}{}", words[at]);
            a.cursor = a.text.chars().count();
        }
        self.bar_changed();
        if let Some(a) = &mut self.windows.address {
            a.cycle = Some((start, words, at));
        }
    }

    /// What the last word of the line can become, and where it starts; `None` for nothing.
    fn completions(&self) -> Option<(usize, Vec<String>)> {
        let a = self.windows.address.as_ref()?;
        let found = match registry::complete(line_of(&a.text)) {
            Completion::Words { start, words } => (start, words),
            Completion::Path { start, .. } => (start, self.path_completions()),
            Completion::Nothing => return None,
        };
        (!found.1.is_empty()).then_some(found)
    }

    /// The names in the folder of the path being typed that start as it does: folders with
    /// their separator, then files. From the last list the worker gave; none if it is of
    /// another folder (the next Tab finds it).
    fn path_completions(&self) -> Vec<String> {
        let (Some(a), cwd, home) = (&self.windows.address, &self.tab.cwd, &self.svc.home) else {
            return Vec::new();
        };
        let Some((dir, dir_text, prefix)) = typed_path(&a.text, cwd, home) else {
            return Vec::new();
        };
        if a.listed.as_deref() != Some(dir.as_path()) {
            return Vec::new();
        }
        let lower = prefix.to_lowercase();
        let starts = |n: &&String| n.to_lowercase().starts_with(&lower);
        let dirs = a.names.iter().filter(starts).map(|n| format!("{n}/"));
        let files = a.files.iter().filter(starts).cloned();
        dirs.chain(files)
            .map(|n| format!("{}{}", escaped(&dir_text), escaped(&n)))
            .collect()
    }

    /// `:sort size --reverse`: the order of the list, by words.
    pub fn sort_by_words(&mut self, args: &Args) -> Result<(), String> {
        let key = match args.text("by") {
            Some("name") => Some(SortKey::Name),
            Some("size") => Some(SortKey::Size),
            Some("date") => Some(SortKey::Modified),
            Some("type") => Some(SortKey::Type),
            _ => None,
        };
        let mut spec = self.tab.sort;
        if let Some(key) = key {
            spec = spec.with_key(key);
        }
        spec.reverse = args.flag("reverse");
        self.set_sort(spec);
        Ok(())
    }

    /// `:find words`: the search in this folder and the ones below.
    pub fn find_below(&mut self, args: &Args) -> Result<(), String> {
        let facts = self.facts(Surface::Line);
        if facts.read_only {
            return Err(Key::WhyReadOnly.tr().to_string());
        }
        if facts.virtual_view {
            return Err(Key::WhyNotAFolder.tr().to_string());
        }
        self.search_below(args.text("text").unwrap_or_default());
        Ok(())
    }

    /// `:cd folder`: go there.
    pub fn go_to_typed(&mut self, args: &Args) -> Result<(), String> {
        let typed = args.text("folder").unwrap_or_default();
        let target = expand(typed, &self.tab.cwd, &self.svc.home);
        if target != self.tab.cwd {
            self.open_dir(target);
        }
        Ok(())
    }
}
