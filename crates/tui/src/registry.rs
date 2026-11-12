//! The registry of commands: one card for everything that can be asked of rada by name.
//!
//! A card ([`CommandSpec`]) says what the command is called, where it can be used and what it
//! takes. The menus, the buttons and the bar ask the registry whether a command is available
//! where they are ([`CommandSpec::availability`]) instead of each keeping its own rules, and
//! the command line (`:`) finds a command by its name, reads its arguments and runs it.
//!
//! Every action of the keymap has a card ([`Action::spec`]); a few commands exist only by
//! name (`:cd`, `:find`). A command that changes files never does it itself: it makes a
//! request, and the request goes through the plan window like any other.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use rada_core::i18n::Key;
use rada_core::ops::OpRequest;

use crate::app::App;
use crate::keymap::{Action, Group};

/// Where a command is being offered or asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A key was pressed.
    Keys,
    /// The suggestions under the bar.
    Bar,
    /// The command line.
    Line,
    /// The buttons of the command bar.
    Buttons,
    NewMenu,
    MoreMenu,
    /// The menu of the right button on an item.
    ItemMenu,
    /// The menu of the right button on the empty space of the list.
    SpaceMenu,
}

/// What a chooser of files lets do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChooserFacts {
    /// Several items can be chosen.
    pub multiple: bool,
    /// A folder is the answer, or a place to save: a new folder can be made.
    pub makes_folders: bool,
}

/// What a command needs to know about where it is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    pub surface: Surface,
    /// Inside an archive: nothing can be changed.
    pub read_only: bool,
    /// A list that is not a folder (a tag, a search, the recent files).
    pub virtual_view: bool,
    /// Something was copied or cut.
    pub clipboard: bool,
    /// Something is selected, or the cursor is on an item.
    pub has_target: bool,
    pub has_cursor: bool,
    /// The item under the cursor is a folder that can be entered.
    pub cursor_dir: bool,
    /// The item under the cursor is a package (a macOS `.app`).
    pub cursor_package: bool,
    /// The item under the cursor is an archive.
    pub on_archive_item: bool,
    /// The system has a quick look.
    pub quick_look: bool,
    /// Rada is choosing a file for another program.
    pub chooser: Option<ChooserFacts>,
}

impl Facts {
    /// Nothing can be made here: an archive, or a list that is not a folder.
    pub fn nowhere(&self) -> bool {
        self.read_only || self.virtual_view
    }

    /// The same place, looked at from another surface.
    pub fn on(mut self, surface: Surface) -> Facts {
        self.surface = surface;
        self
    }
}

/// Whether a command can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Avail {
    Enabled,
    /// Shown greyed, or refused with this reason.
    Disabled(Key),
    /// Not shown at all.
    Hidden,
}

/// What a chooser of files makes of a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChooserRule {
    /// Looking, moving around, showing: always allowed.
    Always,
    /// Selecting: only where several items can be chosen.
    Multiple,
    /// A new folder: only where a folder is the answer.
    MakesFolders,
    /// Whatever changes a file, starts a program or touches the settings.
    Never,
}

impl ChooserRule {
    pub fn allows(self, chooser: ChooserFacts) -> bool {
        match self {
            ChooserRule::Always => true,
            ChooserRule::Multiple => chooser.multiple,
            ChooserRule::MakesFolders => chooser.makes_folders,
            ChooserRule::Never => false,
        }
    }
}

/// What an argument of a command is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgType {
    /// One word, or several between quotes.
    Text,
    Int,
    /// One of the words the argument lists.
    Choice,
    /// A path: `~` is the home folder, a relative one starts at the folder shown.
    Path,
    /// `--name`: there or not.
    Flag,
    /// Everything up to the end of the line, as it is written.
    Rest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgType,
    pub required: bool,
    /// For a [`ArgType::Choice`]: the words it can be.
    pub choices: &'static [&'static str],
}

impl ArgSpec {
    const fn new(name: &'static str, kind: ArgType, required: bool) -> ArgSpec {
        ArgSpec {
            name,
            kind,
            required,
            choices: &[],
        }
    }
}

/// The arguments of a command, read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    values: HashMap<&'static str, String>,
}

impl Args {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        self.text(name)?.parse().ok()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }
}

/// Where a command that makes a request is asked for.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// The folder shown.
    pub cwd: PathBuf,
    pub home: PathBuf,
    /// What is selected, or the item under the cursor.
    pub targets: Vec<PathBuf>,
}

pub type AvailFn = fn(&Facts) -> Avail;
pub type RequestFn = fn(&Args, &Ctx) -> Result<OpRequest, String>;
pub type RunFn = fn(&mut App, &Args) -> Result<(), String>;

/// The card of a command.
pub struct CommandSpec {
    /// `file.copy`, `view.sort`: the group and the name.
    pub id: String,
    /// The names it answers to on the command line; the first is the one shown.
    pub aliases: Vec<&'static str>,
    pub title: Key,
    pub group: Group,
    /// The action of the keymap this is the card of; `None` for a command that exists only
    /// by name.
    pub action: Option<Action>,
    /// Whether it can be used in a menu or as a button (see [`CommandSpec::availability`]
    /// for the whole answer).
    pub avail: AvailFn,
    pub chooser: ChooserRule,
    pub args: &'static [ArgSpec],
    /// With arguments: the request it makes, which goes to the plan window.
    pub request: Option<RequestFn>,
    /// With arguments: what it does in the interface, when it changes no file.
    pub run_ui: Option<RunFn>,
}

impl CommandSpec {
    /// The name shown and completed on the command line.
    pub fn name(&self) -> &'static str {
        self.aliases.first().copied().unwrap_or_default()
    }

    /// Whether the bar offers it: what is done to files, to the view, to the tabs and to the
    /// program; not each step of the cursor.
    pub fn in_bar(&self) -> bool {
        matches!(
            self.group,
            Group::Files | Group::View | Group::Tabs | Group::Program
        ) || self.action == Some(Action::SelectAll)
    }

    /// Whether the command can be used where `facts` says.
    pub fn availability(&self, facts: &Facts) -> Avail {
        let refused = facts.chooser.is_some_and(|c| !self.chooser.allows(c));
        match facts.surface {
            Surface::Bar if refused || !self.in_bar() => Avail::Hidden,
            Surface::Keys | Surface::Line if refused => Avail::Disabled(Key::ChooseRefused),
            Surface::Bar | Surface::Keys => Avail::Enabled,
            _ => (self.avail)(facts),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Where each command can be used
// ---------------------------------------------------------------------------------------------

use Avail::{Disabled, Enabled, Hidden};

fn when(ok: bool, reason: Key) -> Avail {
    if ok { Enabled } else { Disabled(reason) }
}

fn shown(visible: bool) -> Avail {
    if visible { Enabled } else { Hidden }
}

fn always(_: &Facts) -> Avail {
    Enabled
}

/// Takes the selection away from where it is.
fn changes_target(f: &Facts) -> Avail {
    match f.surface {
        Surface::Buttons if !f.has_target => Disabled(Key::WhyNothingSelected),
        _ => when(!f.read_only, Key::WhyReadOnly),
    }
}

fn copy(f: &Facts) -> Avail {
    match f.surface {
        Surface::Buttons => when(f.has_target, Key::WhyNothingSelected),
        _ => Enabled,
    }
}

fn paste(f: &Facts) -> Avail {
    if !f.clipboard {
        return Disabled(Key::WhyNothingCopied);
    }
    when(!f.nowhere(), Key::WhyNotAFolder)
}

fn rename(f: &Facts) -> Avail {
    match f.surface {
        Surface::Buttons if !f.has_cursor => Disabled(Key::WhyNothingSelected),
        _ => when(!f.read_only, Key::WhyReadOnly),
    }
}

/// In the menu of an item it is left out where nothing can be changed; elsewhere it is grey.
fn left_out_in_archives(f: &Facts) -> Avail {
    match f.surface {
        Surface::ItemMenu if f.read_only => Hidden,
        _ => changes_target(f),
    }
}

fn makes(f: &Facts) -> Avail {
    if f.read_only {
        return Disabled(Key::WhyReadOnly);
    }
    when(!f.virtual_view, Key::WhyNotAFolder)
}

/// Only where there is an archive to take things out of.
fn extract(f: &Facts) -> Avail {
    match f.surface {
        Surface::SpaceMenu => shown(f.read_only),
        Surface::ItemMenu | Surface::MoreMenu => shown(f.read_only || f.on_archive_item),
        _ => Enabled,
    }
}

fn open_terminal(f: &Facts) -> Avail {
    match f.surface {
        Surface::SpaceMenu => shown(!f.read_only),
        _ => when(!f.read_only, Key::WhyReadOnly),
    }
}

fn show_in_folder(f: &Facts) -> Avail {
    match f.surface {
        Surface::ItemMenu => shown(f.virtual_view),
        _ => Enabled,
    }
}

fn show_package(f: &Facts) -> Avail {
    match f.surface {
        Surface::ItemMenu => shown(f.cursor_package),
        _ => Enabled,
    }
}

fn quick_look(f: &Facts) -> Avail {
    match f.surface {
        Surface::ItemMenu if !f.quick_look => Hidden,
        _ => when(!f.read_only, Key::WhyReadOnly),
    }
}

fn open_with(f: &Facts) -> Avail {
    if !f.has_cursor || f.cursor_dir {
        return Disabled(Key::WhyNotAFile);
    }
    when(!f.read_only, Key::WhyReadOnly)
}

/// The rule of a menu or a button for `action`.
fn avail_of(action: Action) -> AvailFn {
    use Action::*;
    match action {
        Cut | Trash | BulkRename => changes_target,
        Compress | DeletePermanently => left_out_in_archives,
        Copy => copy,
        Paste => paste,
        Rename => rename,
        NewFolder | NewFile => makes,
        ExtractHere | ExtractToFolder => extract,
        OpenTerminal => open_terminal,
        ShowInFolder => show_in_folder,
        ShowPackage => show_package,
        QuickLook => quick_look,
        OpenWith => open_with,
        _ => always,
    }
}

/// What a chooser of files makes of `action`: everything that looks, moves around or shows
/// is allowed; whatever changes a file, starts another program or touches the settings is
/// not. A new folder is the one thing made, and only where a folder is the answer.
fn chooser_rule(action: Action) -> ChooserRule {
    use Action::*;
    match action {
        ToggleMark | SelectAll | SelectUp | SelectDown | SelectToFirst | SelectToLast => {
            ChooserRule::Multiple
        }
        NewFolder => ChooserRule::MakesFolders,
        Copy | Cut | Paste | Trash | DeletePermanently | Rename | BulkRename | ExtractHere
        | ExtractToFolder | Compress | Undo | Redo | History | Bookmark | NewFile | OpenWith
        | OpenTerminal | ContextMenu | Update | QuickLook | ShowPackage => ChooserRule::Never,
        _ => ChooserRule::Always,
    }
}

// ---------------------------------------------------------------------------------------------
// The cards
// ---------------------------------------------------------------------------------------------

/// The words `:sort` takes.
pub const SORT_KEYS: [&str; 4] = ["name", "size", "date", "type"];

const RENAME_ARGS: [ArgSpec; 1] = [ArgSpec::new("pattern", ArgType::Rest, false)];
const MKDIR_ARGS: [ArgSpec; 1] = [ArgSpec::new("name", ArgType::Text, false)];
const SORT_ARGS: [ArgSpec; 2] = [
    ArgSpec {
        name: "by",
        kind: ArgType::Choice,
        required: false,
        choices: &SORT_KEYS,
    },
    ArgSpec::new("reverse", ArgType::Flag, false),
];
const FIND_ARGS: [ArgSpec; 1] = [ArgSpec::new("text", ArgType::Rest, true)];
const CD_ARGS: [ArgSpec; 1] = [ArgSpec::new("folder", ArgType::Path, true)];

fn group_word(group: Group) -> &'static str {
    match group {
        Group::Move => "go",
        Group::Select => "select",
        Group::Files => "file",
        Group::View => "view",
        Group::Tabs => "tab",
        Group::Program => "app",
    }
}

/// Other names an action answers to on the command line, after its own.
fn extra_names(action: Action) -> &'static [&'static str] {
    match action {
        Action::NewFolder => &["mkdir"],
        Action::NewFile => &["touch"],
        Action::Copy => &["cp"],
        Action::Cut => &["mv"],
        Action::Quit => &["q"],
        _ => &[],
    }
}

fn card_of(action: Action) -> CommandSpec {
    let (args, request, run_ui): (&'static [ArgSpec], Option<RequestFn>, Option<RunFn>) =
        match action {
            Action::Rename => (&RENAME_ARGS, Some(rename_request), None),
            Action::NewFolder => (&MKDIR_ARGS, Some(mkdir_request), None),
            Action::Sort => (&SORT_ARGS, None, Some(App::sort_by_words)),
            _ => (&[], None, None),
        };
    let mut aliases = vec![action.id()];
    aliases.extend(extra_names(action));
    CommandSpec {
        id: format!("{}.{}", group_word(action.group()), action.id()),
        aliases,
        title: action.label_key(),
        group: action.group(),
        action: Some(action),
        avail: avail_of(action),
        chooser: chooser_rule(action),
        args,
        request,
        run_ui,
    }
}

/// The commands that have no action: they exist by name only.
fn named_only() -> Vec<CommandSpec> {
    let named = |name: &'static str, title: Key, group: Group, args, run: RunFn| CommandSpec {
        id: format!("{}.{name}", group_word(group)),
        aliases: vec![name],
        title,
        group,
        action: None,
        avail: always,
        chooser: ChooserRule::Always,
        args,
        request: None,
        run_ui: Some(run),
    };
    vec![
        named(
            "find",
            Key::CmdFind,
            Group::View,
            &FIND_ARGS,
            App::find_below,
        ),
        named("cd", Key::CmdCd, Group::Move, &CD_ARGS, App::go_to_typed),
    ]
}

static REGISTRY: LazyLock<Vec<CommandSpec>> = LazyLock::new(|| {
    let mut all: Vec<CommandSpec> = Action::ALL.iter().map(|a| card_of(*a)).collect();
    all.extend(named_only());
    all
});

/// Every command, the actions first and in their order.
pub fn all() -> &'static [CommandSpec] {
    &REGISTRY
}

/// The command that answers to `name` on the command line.
pub fn by_name(name: &str) -> Option<&'static CommandSpec> {
    all().iter().find(|c| c.aliases.contains(&name))
}

impl Action {
    /// The card of this action.
    pub fn spec(self) -> &'static CommandSpec {
        let at = Action::ALL.iter().position(|a| *a == self).unwrap_or(0);
        &all()[at]
    }
}

fn rename_request(args: &Args, ctx: &Ctx) -> Result<OpRequest, String> {
    let pattern = args.text("pattern").unwrap_or_default().to_string();
    if ctx.targets.is_empty() {
        return Err(Key::WhyNothingSelected.tr().to_string());
    }
    Ok(OpRequest::BulkRename {
        items: ctx.targets.clone(),
        pattern,
        counter_start: None,
        counter_step: None,
    })
}

fn mkdir_request(args: &Args, ctx: &Ctx) -> Result<OpRequest, String> {
    Ok(OpRequest::MakeDir {
        parent: ctx.cwd.clone(),
        name: args.text("name").unwrap_or_default().to_string(),
    })
}

// ---------------------------------------------------------------------------------------------
// Reading a line
// ---------------------------------------------------------------------------------------------

/// A word of a line, with where it is (in characters from the start of the line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// What is wrong with a line, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// The character the problem is at, from the start of the line.
    pub at: usize,
    pub message: String,
}

fn error<T>(at: usize, message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError {
        at,
        message: message.into(),
    })
}

/// Split a line into words: spaces separate them, `"a b"` is one word, and `\` takes the
/// character after it as it is (`a\ b`).
pub fn tokens(line: &str) -> Result<Vec<Token>, ParseError> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(token) = next_word(&chars, i)? {
        i = token.end;
        out.push(token);
    }
    Ok(out)
}

/// The first word at or after `from`; `None` past the last one.
fn next_word(chars: &[char], from: usize) -> Result<Option<Token>, ParseError> {
    let start = (from..chars.len()).find(|i| !chars[*i].is_whitespace());
    start.map(|i| word_at(chars, i)).transpose()
}

/// The word that starts at `start`.
fn word_at(chars: &[char], start: usize) -> Result<Token, ParseError> {
    let (mut i, mut text, mut quote) = (start, String::new(), None);
    while i < chars.len() {
        match (chars[i], quote) {
            ('\\', _) => {
                let Some(next) = chars.get(i + 1) else {
                    return error(i, Key::LineLoneBackslash.tr());
                };
                text.push(*next);
                i += 1;
            }
            ('"', None) => quote = Some(i),
            ('"', Some(_)) => quote = None,
            (c, None) if c.is_whitespace() => break,
            (c, _) => text.push(c),
        }
        i += 1;
    }
    if let Some(open) = quote {
        return error(open, Key::LineOpenQuote.tr());
    }
    Ok(Token {
        text,
        start,
        end: i,
    })
}

/// A line read as a command with its arguments.
pub struct Parsed {
    pub spec: &'static CommandSpec,
    pub args: Args,
}

/// Read `line` (without its `:`). `Ok(None)` when its first word is not the name of a
/// command: the line is then not a command at all, and whoever asked treats it as words.
pub fn parse(line: &str, home: &std::path::Path) -> Result<Option<Parsed>, ParseError> {
    let chars: Vec<char> = line.chars().collect();
    let Some(first) = next_word(&chars, 0)? else {
        return Ok(None);
    };
    let Some(spec) = by_name(&first.text) else {
        return Ok(None);
    };
    let args = bind(spec.args, line, first.end, home)?;
    Ok(Some(Parsed { spec, args }))
}

/// Give the words of `line` from `from` on to the arguments `specs` describes. The words are
/// read one at a time: what an argument that takes the rest of the line gets is never read as
/// words, so its quotes and backslashes (a pattern, `/a\d"/`) stay as they are.
pub fn bind(
    specs: &[ArgSpec],
    line: &str,
    from: usize,
    home: &std::path::Path,
) -> Result<Args, ParseError> {
    let chars: Vec<char> = line.chars().collect();
    let mut args = Args::default();
    let mut places = specs.iter().filter(|s| s.kind != ArgType::Flag).peekable();
    let mut i = from;
    loop {
        if places.peek().is_some_and(|s| s.kind == ArgType::Rest) {
            let rest: String = chars[i.min(chars.len())..].iter().collect();
            if !rest.trim().is_empty() {
                args.values
                    .insert(places.next().unwrap().name, rest.trim().to_string());
            }
            break;
        }
        let Some(word) = next_word(&chars, i)? else {
            break;
        };
        i = word.end;
        give(specs, &mut places, &word, home, &mut args)?;
    }
    match specs
        .iter()
        .find(|s| s.required && !args.values.contains_key(s.name))
    {
        Some(missing) => error(
            chars.len(),
            rada_core::i18n::tf(Key::LineMissing, &[&missing.name]),
        ),
        None => Ok(args),
    }
}

/// Give one word to the argument it is for: an option, `name=value`, or the next in place.
fn give<'a>(
    specs: &[ArgSpec],
    places: &mut impl Iterator<Item = &'a ArgSpec>,
    word: &Token,
    home: &std::path::Path,
    args: &mut Args,
) -> Result<(), ParseError> {
    if let Some(flag) = word.text.strip_prefix("--") {
        let known = specs
            .iter()
            .find(|s| s.kind == ArgType::Flag && s.name == flag);
        let Some(spec) = known else {
            return error(
                word.start,
                rada_core::i18n::tf(Key::LineUnknownOption, &[&flag]),
            );
        };
        args.values.insert(spec.name, String::new());
        return Ok(());
    }
    let (spec, value) = match named(specs, word) {
        Some(found) => found,
        None => match places.next() {
            Some(spec) => (spec, word.text.as_str()),
            None => return error(word.start, Key::LineTooMany.tr()),
        },
    };
    args.values
        .insert(spec.name, checked(spec, value, word.start, home)?);
    Ok(())
}

/// `name=value`, when `name` is an argument that takes a value.
fn named<'a>(specs: &'a [ArgSpec], word: &'a Token) -> Option<(&'a ArgSpec, &'a str)> {
    let (name, value) = word.text.split_once('=')?;
    let spec = specs
        .iter()
        .find(|s| s.name == name && s.kind != ArgType::Flag)?;
    Some((spec, value))
}

/// The value of an argument, if it is one the argument can take.
fn checked(
    spec: &ArgSpec,
    value: &str,
    at: usize,
    home: &std::path::Path,
) -> Result<String, ParseError> {
    match spec.kind {
        ArgType::Int if value.parse::<i64>().is_err() => {
            error(at, rada_core::i18n::tf(Key::LineNotANumber, &[&value]))
        }
        ArgType::Choice if !spec.choices.contains(&value) => {
            let choices = spec.choices.join(", ");
            error(
                at,
                rada_core::i18n::tf(Key::LineNotAChoice, &[&value, &choices]),
            )
        }
        ArgType::Path => Ok(with_home(value, home)),
        _ => Ok(value.to_string()),
    }
}

/// `~` and `~/x` with the home folder in place of the tilde.
fn with_home(value: &str, home: &std::path::Path) -> String {
    if value == "~" {
        return home.display().to_string();
    }
    match value.strip_prefix("~/") {
        Some(rest) => home.join(rest).display().to_string(),
        None => value.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Completing a line
// ---------------------------------------------------------------------------------------------

/// What could continue the last word of a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completion {
    /// These words, in place of what is typed from `start` on.
    Words {
        start: usize,
        words: Vec<String>,
    },
    /// A path: the names in the folder it is in are asked for elsewhere (the registry never
    /// reads the disk). `start` is where the path begins.
    Path {
        start: usize,
        typed: String,
    },
    Nothing,
}

/// What can continue `line` at its end.
pub fn complete(line: &str) -> Completion {
    let chars = line.chars().count();
    let Ok(words) = tokens(line) else {
        return Completion::Nothing;
    };
    let open = line.chars().last().is_none_or(char::is_whitespace);
    let (typed, start) = match words.last() {
        Some(last) if !open => (last.text.clone(), last.start),
        _ => (String::new(), chars),
    };
    let before = if open { words.len() } else { words.len() - 1 };
    if before == 0 {
        return Completion::Words {
            start,
            words: names_starting(&typed),
        };
    }
    let Some(spec) = by_name(&words[0].text) else {
        return Completion::Nothing;
    };
    argument_completion(spec, &words[1..before], typed, start)
}

/// Every name a command answers to that starts with `typed`.
fn names_starting(typed: &str) -> Vec<String> {
    let mut names: Vec<String> = all()
        .iter()
        .flat_map(|c| c.aliases.iter())
        .filter(|n| n.starts_with(typed))
        .map(|n| n.to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The continuation of an argument: `given` are the words already there after the name.
fn argument_completion(
    spec: &CommandSpec,
    given: &[Token],
    typed: String,
    start: usize,
) -> Completion {
    if let Some(flag) = typed.strip_prefix("--") {
        let words = spec
            .args
            .iter()
            .filter(|a| a.kind == ArgType::Flag && a.name.starts_with(flag))
            .map(|a| format!("--{}", a.name))
            .collect();
        return Completion::Words { start, words };
    }
    let placed = given.iter().filter(|w| !w.text.starts_with("--")).count();
    let arg = spec
        .args
        .iter()
        .filter(|a| a.kind != ArgType::Flag)
        .nth(placed);
    match arg {
        Some(a) if a.kind == ArgType::Choice => Completion::Words {
            start,
            words: a
                .choices
                .iter()
                .filter(|c| c.starts_with(typed.as_str()))
                .map(|c| c.to_string())
                .collect(),
        },
        Some(a) if a.kind == ArgType::Path => Completion::Path { start, typed },
        _ => Completion::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const HOME: &str = "/home/u";

    fn read(line: &str) -> Result<Option<Parsed>, ParseError> {
        parse(line, Path::new(HOME))
    }

    fn args_of(line: &str) -> Args {
        read(line).unwrap().unwrap().args
    }

    fn at(line: &str) -> usize {
        read(line).err().unwrap().at
    }

    #[test]
    fn every_action_has_a_card_with_its_group_in_the_id() {
        for a in Action::ALL {
            let spec = a.spec();
            assert_eq!(spec.action, Some(a));
            assert_eq!(spec.title, a.label_key());
            assert!(
                spec.id.ends_with(a.id()) && spec.id.contains('.'),
                "{}",
                spec.id
            );
            assert_eq!(by_name(a.id()).map(|s| s.action), Some(Some(a)));
        }
        assert_eq!(Action::Copy.spec().id, "file.copy");
        assert_eq!(Action::Sort.spec().id, "view.sort");
        assert_eq!(by_name("cd").unwrap().id, "go.cd");
    }

    #[test]
    fn no_two_commands_answer_to_one_name() {
        let mut names: Vec<&str> = all().iter().flat_map(|c| c.aliases.clone()).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn words_are_split_at_spaces_unless_quoted_or_escaped() {
        let texts = |line: &str| -> Vec<String> {
            tokens(line).unwrap().into_iter().map(|t| t.text).collect()
        };
        assert_eq!(texts("mkdir  a b"), ["mkdir", "a", "b"]);
        assert_eq!(texts(r#"mkdir "a b" c"#), ["mkdir", "a b", "c"]);
        assert_eq!(texts(r"mkdir a\ b"), ["mkdir", "a b"]);
        assert_eq!(texts(r#"x "a \"q\" b""#), ["x", r#"a "q" b"#]);
        assert_eq!(texts(r#"x pre"mid dle"post"#), ["x", "premid dlepost"]);
        let t = tokens("ab  cd").unwrap();
        assert_eq!((t[1].start, t[1].end), (4, 6));
    }

    #[test]
    fn a_line_that_is_not_a_command_is_not_an_error() {
        assert!(read("").unwrap().is_none());
        assert!(read("no-such-command x").unwrap().is_none());
        assert!(read("Copy").unwrap().is_none(), "a label is not a name");
    }

    #[test]
    fn arguments_are_given_by_place_by_name_and_as_options() {
        assert!(args_of("undo").is_empty());
        assert_eq!(args_of("mkdir notes").text("name"), Some("notes"));
        assert_eq!(
            args_of(r#"mkdir "my notes""#).text("name"),
            Some("my notes")
        );
        assert_eq!(args_of("mkdir name=notes").text("name"), Some("notes"));
        let sort = args_of("sort size --reverse");
        assert_eq!(sort.text("by"), Some("size"));
        assert!(sort.flag("reverse"));
        assert!(!args_of("sort by=date").flag("reverse"));
        // The home folder with the rest joined the way the system writes a path.
        let src = Path::new(HOME).join("src").display().to_string();
        assert_eq!(args_of("cd ~/src").text("folder"), Some(src.as_str()));
        assert_eq!(args_of("cd ~").text("folder"), Some(HOME));
    }

    #[test]
    fn the_rest_of_the_line_is_taken_as_it_is_written() {
        assert_eq!(
            args_of("find  two  words ").text("text"),
            Some("two  words")
        );
        assert_eq!(
            args_of(r"find /^img_\d+ x/").text("text"),
            Some(r"/^img_\d+ x/")
        );
        assert_eq!(
            args_of("rename {name}_{n:3}.{ext}").text("pattern"),
            Some("{name}_{n:3}.{ext}")
        );
        // Never read as words: a backslash at the end, a quote left open, an option.
        assert_eq!(args_of(r"find /a\/").text("text"), Some(r"/a\/"));
        assert_eq!(
            args_of(r#"find /say "hi/"#).text("text"),
            Some(r#"/say "hi/"#)
        );
        assert_eq!(args_of("find --all").text("text"), Some("--all"));
        assert_eq!(args_of("find text=x y").text("text"), Some("text=x y"));
    }

    #[test]
    fn what_is_wrong_is_said_with_where_it_is() {
        assert_eq!(at(r#"mkdir "open"#), 6);
        assert_eq!(at("mkdir a\\"), 7);
        assert_eq!(at("mkdir a b"), 8, "the word that is one too many");
        assert_eq!(at("sort colour"), 5);
        assert_eq!(at("sort size --backwards"), 10);
        assert_eq!(at("find"), 4, "what is missing is at the end");
        assert_eq!(at("cd"), 2);
        assert!(
            read("sort colour")
                .err()
                .unwrap()
                .message
                .contains("name, size, date, type")
        );
    }

    #[test]
    fn a_number_must_be_a_number() {
        let specs = [ArgSpec::new("count", ArgType::Int, true)];
        let bound = |line: &str| bind(&specs, line, 0, Path::new(HOME));
        assert_eq!(bound("12").unwrap().int("count"), Some(12));
        assert_eq!(bound("-3").unwrap().int("count"), Some(-3));
        assert_eq!(bound("  x1").unwrap_err().at, 2);
    }

    #[test]
    fn names_values_and_options_are_completed() {
        let words = |line: &str| match complete(line) {
            Completion::Words { start, words } => (start, words),
            other => panic!("{other:?}"),
        };
        assert_eq!(words("und"), (0, vec!["undo".to_string()]));
        assert!(words("").1.len() > 60);
        assert_eq!(words("mk"), (0, vec!["mkdir".to_string()]));
        assert_eq!(words("sort "), (5, SORT_KEYS.map(String::from).to_vec()));
        assert_eq!(words("sort s"), (5, vec!["size".to_string()]));
        assert_eq!(words("sort size --r"), (10, vec!["--reverse".to_string()]));
        assert_eq!(
            complete("cd ~/s"),
            Completion::Path {
                start: 3,
                typed: "~/s".into()
            }
        );
        assert_eq!(complete("find x"), Completion::Nothing);
        assert_eq!(complete("nothing x"), Completion::Nothing);
    }

    fn folder() -> Facts {
        Facts {
            surface: Surface::ItemMenu,
            read_only: false,
            virtual_view: false,
            clipboard: false,
            has_target: true,
            has_cursor: true,
            cursor_dir: false,
            cursor_package: false,
            on_archive_item: false,
            quick_look: false,
            chooser: None,
        }
    }

    #[test]
    fn a_command_says_why_it_cannot_be_used() {
        let archive = Facts {
            read_only: true,
            ..folder()
        };
        let avail = |a: Action, f: &Facts| a.spec().availability(f);
        assert_eq!(avail(Action::Cut, &archive), Disabled(Key::WhyReadOnly));
        assert_eq!(avail(Action::Compress, &archive), Hidden);
        assert_eq!(avail(Action::Copy, &archive), Enabled);
        assert_eq!(
            avail(Action::Paste, &folder()),
            Disabled(Key::WhyNothingCopied)
        );
        let search = Facts {
            virtual_view: true,
            clipboard: true,
            ..folder()
        };
        assert_eq!(avail(Action::Paste, &search), Disabled(Key::WhyNotAFolder));
        assert_eq!(
            avail(Action::NewFolder, &search.on(Surface::Line)),
            Disabled(Key::WhyNotAFolder)
        );
    }

    #[test]
    fn a_chooser_hides_from_the_bar_what_it_refuses_at_the_keys() {
        let choosing = Facts {
            chooser: Some(ChooserFacts {
                multiple: false,
                makes_folders: false,
            }),
            ..folder()
        };
        let trash = Action::Trash.spec();
        assert_eq!(trash.availability(&choosing.on(Surface::Bar)), Hidden);
        assert_eq!(
            trash.availability(&choosing.on(Surface::Keys)),
            Disabled(Key::ChooseRefused)
        );
        assert_eq!(
            Action::Refresh
                .spec()
                .availability(&choosing.on(Surface::Bar)),
            Enabled
        );
        assert_eq!(
            Action::Up.spec().availability(&folder().on(Surface::Bar)),
            Hidden
        );
    }
}
