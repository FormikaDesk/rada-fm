//! Recipes: small programs, written by the person or built into rada, that look at the files
//! chosen and ask for a plan.
//!
//! The interface never runs a recipe's language itself. It speaks to a [`PluginHost`], which
//! knows which recipes there are, which of them fit the files chosen, and turns one of them,
//! run with its arguments, into a [`RecipePlan`]: a request like any other, that goes through
//! the same plan window, the same journal and the same undo. A recipe cannot change a file by
//! itself; it can only ask.
//!
//! The one host is the Lua one (the `rada-lua` crate); rada built without it, or started with
//! recipes off, has no host and no recipes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::events::CoreEvent;
pub use crate::ops::Cancel;
use crate::ops::RecipePlan;
use crate::ops::programs::Os;

/// The version of what recipes are given and may call. A recipe written for it keeps working
/// in later versions of rada that have the same one.
pub const API_VERSION: u32 = 1;

/// What a recipe is run on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeContext {
    /// The items the recipe is for: the marked ones, or the one under the cursor.
    pub selection: Vec<PathBuf>,
    /// The items marked, which may be none.
    pub marked: Vec<PathBuf>,
    /// The folder shown.
    pub cwd: PathBuf,
    pub os: Os,
}

impl RecipeContext {
    /// The name the recipes see the system by: `linux`, `macos` or `windows`.
    pub fn platform(&self) -> &'static str {
        match self.os {
            Os::Linux => "linux",
            Os::MacOs => "macos",
            Os::Windows => "windows",
        }
    }
}

/// Where a recipe comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipeOrigin {
    /// Built into rada.
    Builtin,
    /// One of the person's files.
    User(PathBuf),
}

/// Whether a recipe can run on the files chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// It fits these files, but cannot run now: the sentence says why.
    Disabled(String),
}

/// The kind of value an argument takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgKind {
    Text,
    /// One of these words.
    Choice(Vec<String>),
    /// A whole number between the two, both included.
    Int {
        min: i64,
        max: i64,
    },
    Flag,
}

/// A value given to an argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgValue {
    Text(String),
    Int(i64),
    Flag(bool),
}

/// The arguments a recipe is run with, by name.
pub type ArgValues = BTreeMap<String, ArgValue>;

/// One argument a recipe asks for before it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeArg {
    pub name: String,
    /// What the window asking for it calls it.
    pub label: String,
    pub kind: ArgKind,
    pub default: Option<ArgValue>,
}

impl RecipeArg {
    /// Whether `value` is one this argument takes.
    pub fn accepts(&self, value: &ArgValue) -> bool {
        match (&self.kind, value) {
            (ArgKind::Text, ArgValue::Text(_)) | (ArgKind::Flag, ArgValue::Flag(_)) => true,
            (ArgKind::Choice(words), ArgValue::Text(t)) => words.contains(t),
            (ArgKind::Int { min, max }, ArgValue::Int(n)) => (min..=max).contains(&n),
            _ => false,
        }
    }
}

/// A recipe as a menu shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeInfo {
    /// The name it is called by (`:recipe <name>`), unique among the recipes.
    pub name: String,
    /// The words of a menu entry, in the language of the interface.
    pub title: String,
    pub description: String,
    pub args: Vec<RecipeArg>,
    pub origin: RecipeOrigin,
    pub state: Availability,
}

/// A file of recipes that could not be read, or one of its recipes that is not right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadProblem {
    pub file: PathBuf,
    /// The line, when the problem is at one.
    pub line: Option<u32>,
    pub message: String,
}

impl std::fmt::Display for LoadProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = crate::display::path(&self.file);
        match self.line {
            Some(line) => write!(f, "{file}:{line}: {}", self.message),
            None => write!(f, "{file}: {}", self.message),
        }
    }
}

/// Which of its limits stopped a recipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Instructions,
    Memory,
    Time,
}

/// Why a recipe gave no plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipeError {
    /// No recipe of that name.
    Unknown(String),
    /// The recipe does not fit the files chosen, or cannot run now.
    NotAvailable(String),
    /// An argument that is missing or of the wrong kind.
    BadArgument { name: String, message: String },
    /// The recipe went wrong, at this line of this file.
    Script {
        file: PathBuf,
        line: Option<u32>,
        message: String,
    },
    /// The recipe was stopped at one of its limits.
    Limit(Limit),
    /// Stopped by the person.
    Cancelled,
}

impl std::fmt::Display for RecipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::i18n::{Key, tf};
        match self {
            RecipeError::Unknown(n) => f.write_str(&tf(Key::RecipeUnknown, &[n])),
            RecipeError::NotAvailable(why) => f.write_str(why),
            RecipeError::BadArgument { name, message } => write!(f, "{name}: {message}"),
            // A recipe built in says what is wrong in words: its file and line would mean
            // nothing to the person.
            RecipeError::Script {
                file,
                line: None,
                message,
            } if file.to_string_lossy().starts_with("rada:") => f.write_str(message),
            RecipeError::Script {
                file,
                line,
                message,
            } => LoadProblem {
                file: file.clone(),
                line: *line,
                message: message.clone(),
            }
            .fmt(f),
            RecipeError::Limit(limit) => f.write_str(match limit {
                Limit::Instructions => Key::RecipeTooLong.tr(),
                Limit::Memory => Key::RecipeTooBig.tr(),
                Limit::Time => Key::RecipeTooSlow.tr(),
            }),
            RecipeError::Cancelled => f.write_str(Key::RecipeStopped.tr()),
        }
    }
}

/// Knows the recipes and runs them. Running one never changes a file: it gives a plan, or why
/// not.
pub trait PluginHost: Send + Sync {
    /// Read the recipes again: those built in and the person's files. What could not be read
    /// is left out and said; the others are there.
    fn load(&self) -> Vec<LoadProblem>;

    /// The recipes that fit `ctx`, sorted by title. Those that do not fit are not there.
    fn recipes(&self, ctx: &RecipeContext) -> Vec<RecipeInfo>;

    /// Run the recipe `name` on `ctx` and give the plan it asks for. Blocks until the recipe is
    /// done, stopped at a limit or `cancel` says stop: call it away from the interface (see
    /// [`run_in_background`]).
    fn run(
        &self,
        name: &str,
        ctx: &RecipeContext,
        args: &ArgValues,
        cancel: &Cancel,
    ) -> Result<RecipePlan, RecipeError>;
}

/// The end of a recipe run in the background.
#[derive(Clone, Debug)]
pub struct RecipeEvent {
    /// The number [`run_in_background`] gave the run.
    pub id: u64,
    pub recipe: String,
    pub result: Result<RecipePlan, RecipeError>,
}

/// Run a recipe on a thread of its own. Its end arrives as [`CoreEvent::Recipe`]; the
/// [`Cancel`] given back stops it.
pub fn run_in_background(
    host: Arc<dyn PluginHost>,
    id: u64,
    name: String,
    ctx: RecipeContext,
    args: ArgValues,
    out: Sender<CoreEvent>,
) -> Cancel {
    let cancel = Cancel::new();
    let flag = cancel.clone();
    let lang = crate::i18n::lang();
    let spawned = std::thread::Builder::new()
        .name("rada-recipe".into())
        .spawn(move || {
            let result = crate::i18n::with_lang(lang, || host.run(&name, &ctx, &args, &flag));
            let _ = out.send(CoreEvent::Recipe(RecipeEvent {
                id,
                recipe: name,
                result,
            }));
        });
    if let Err(e) = spawned {
        tracing::error!("could not start a recipe: {e}");
    }
    cancel
}

/// The `*.lua` files of the person's recipes, sorted by name: only regular files, and only
/// the first `max` of them.
pub fn recipe_files(dir: &Path, max: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "lua"))
        .collect();
    files.sort();
    files.truncate(max);
    files
}
