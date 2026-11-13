//! rada-lua: recipes written in Lua 5.4, run in a closed state that can only look at files and
//! ask rada for a plan.
//!
//! What a recipe has: a few safe functions of the base library, `string` without patterns and
//! `dump`, `math` without `random`, `table`, `utf8`, and the table `rada` (see [`api`]). What it
//! does not have: `io`, `os`, `package`, `require`, `debug`, `load`, `loadfile`, `dofile`,
//! `collectgarbage`, `print`, coroutines, bytecode. Each task runs in a fresh state with a limit
//! on instructions, memory and time, and can be stopped from outside.
//!
//! A recipe never changes a file. Its `run` adds requests to a plan; the plan is shown like any
//! other, and only what the person confirms is done, by rada, with the journal and the undo.

mod api;
mod ask;
mod defs;
mod errors;
mod look;
mod paths;
mod session;
mod text;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mlua::{Lua, Table, Value};
use rada_core::meta::MetaReader;
use rada_core::ops::{FailurePolicy, Programs, RecipePlan};
use rada_core::plugin::{
    ArgValues, Availability, Cancel, LoadProblem, PluginHost, RecipeContext, RecipeError,
    RecipeInfo, RecipeOrigin,
};

pub use session::Budget;

use defs::RecipeDef;
use session::{FileName, Mode, Session};

/// What the scripts may consult.
#[derive(Clone, Debug, Default)]
pub struct Tools {
    /// For `rada.has`, the same table the plans use.
    pub programs: Programs,
    /// For `rada.meta`; without it every field is empty.
    pub meta: Option<MetaReader>,
}

/// How far a recipe may go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Memory of one state, over what the libraries take.
    pub memory: usize,
    /// A run of `run`.
    pub run: Budget,
    /// Reading a file and asking `available`.
    pub check: Budget,
    /// The biggest file of recipes.
    pub file_bytes: u64,
    /// The most files read from the person's folder.
    pub files: usize,
    /// The most entries `rada.list` gives.
    pub list_entries: usize,
    /// The most requests one run may make.
    pub requests: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            memory: 32 << 20,
            run: Budget {
                instructions: 100_000_000,
                time: Duration::from_secs(30),
            },
            check: Budget {
                instructions: 5_000_000,
                time: Duration::from_secs(2),
            },
            file_bytes: 64 << 10,
            files: 100,
            list_entries: 10_000,
            requests: 10_000,
        }
    }
}

macro_rules! builtin {
    ($file:literal) => {
        Builtin {
            file: $file,
            text: include_str!(concat!("../recipes/", $file)),
        }
    };
}

/// A recipe file built into rada.
#[derive(Clone, Copy, Debug)]
pub struct Builtin {
    /// Its name, such as `merge-pdf.lua`.
    pub file: &'static str,
    pub text: &'static str,
}

/// The recipes built into rada, written with the same API as the person's.
pub const BUILTINS: &[Builtin] = &[
    builtin!("merge-pdf.lua"),
    builtin!("extract-pages.lua"),
    builtin!("rotate-pages.lua"),
    builtin!("compress-pdf.lua"),
    builtin!("convert-image.lua"),
    builtin!("resize-image.lua"),
    builtin!("extract-audio.lua"),
    builtin!("document-to-pdf.lua"),
    builtin!("sha256.lua"),
];

/// The text of a file, as it was read when the recipes were loaded: a run uses that text,
/// not what the file says by then.
struct Source {
    name: FileName,
    text: Arc<str>,
    origin: RecipeOrigin,
}

struct Loaded {
    source: Arc<Source>,
    defs: Vec<RecipeDef>,
}

/// The Lua host of recipes.
pub struct LuaHost {
    builtins: Vec<Builtin>,
    user_dir: Option<PathBuf>,
    tools: Arc<Tools>,
    limits: Limits,
    loaded: Mutex<Vec<Loaded>>,
}

impl LuaHost {
    /// A host with no recipes until [`load`](PluginHost::load).
    pub fn new(tools: Tools) -> LuaHost {
        LuaHost {
            builtins: Vec::new(),
            user_dir: None,
            tools: Arc::new(tools),
            limits: Limits::default(),
            loaded: Mutex::new(Vec::new()),
        }
    }

    pub fn with_builtins(mut self, builtins: &[Builtin]) -> LuaHost {
        self.builtins = builtins.to_vec();
        self
    }

    /// The folder of the person's `*.lua` files.
    pub fn with_user_dir(mut self, dir: PathBuf) -> LuaHost {
        self.user_dir = Some(dir);
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> LuaHost {
        self.limits = limits;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Loaded>> {
        self.loaded.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every file to read: those built in first, so a file of the person's cannot take the
    /// name of one of them.
    fn sources(&self) -> (Vec<Source>, Vec<LoadProblem>) {
        let mut sources: Vec<Source> = self
            .builtins
            .iter()
            .map(|b| Source {
                name: FileName {
                    file: PathBuf::from(format!("rada:{}", b.file)),
                    label: b.file.to_string(),
                },
                text: Arc::from(b.text),
                origin: RecipeOrigin::Builtin,
            })
            .collect();
        let mut problems = Vec::new();
        let files = match &self.user_dir {
            Some(dir) => rada_core::plugin::recipe_files(dir, self.limits.files),
            None => Vec::new(),
        };
        for file in files {
            match read_text(&file, self.limits.file_bytes) {
                Ok(text) => sources.push(Source {
                    name: FileName {
                        label: label_of(&file),
                        file: file.clone(),
                    },
                    text: Arc::from(text),
                    origin: RecipeOrigin::User(file),
                }),
                Err(message) => problems.push(LoadProblem {
                    file,
                    line: None,
                    message,
                }),
            }
        }
        (sources, problems)
    }

    /// A closed state with `rada`, the file run in it.
    fn session(
        &self,
        source: &Source,
        mode: Mode,
        cancel: &Cancel,
    ) -> Result<Session, (mlua::Error, Option<session::Stop>)> {
        let budget = if mode == Mode::Run {
            self.limits.run
        } else {
            self.limits.check
        };
        let s = Session::new(&self.limits, budget, cancel.clone()).map_err(|e| (e, None))?;
        let shared = api::Shared {
            mode,
            name: source.name.clone(),
            guard: s.guard.clone(),
            tools: self.tools.clone(),
            limits: self.limits,
            recipes: Default::default(),
            requests: Default::default(),
            patterns: Default::default(),
            has: Default::default(),
            logs: Default::default(),
        };
        api::install(&s.lua, &s.env, shared).map_err(|e| (e, None))?;
        match s.exec(&source.name.label, &source.text) {
            Ok(()) => Ok(s),
            Err(e) => {
                let stopped = s.guard.stopped();
                Err((e, stopped))
            }
        }
    }

    /// The recipes of one file, and what is wrong with it.
    fn load_one(&self, source: &Source) -> (Vec<RecipeDef>, Vec<LoadProblem>) {
        let s = match self.session(source, Mode::Load, &Cancel::new()) {
            Ok(s) => s,
            Err((e, stopped)) => {
                return (
                    Vec::new(),
                    vec![errors::load_problem(&e, stopped, &source.name)],
                );
            }
        };
        let declared = declared(&s.lua);
        let mut defs = Vec::new();
        let mut problems = Vec::new();
        for (_, table) in declared {
            match defs::read_def(&table) {
                Ok(def) => defs.push(def),
                Err(message) => problems.push(LoadProblem {
                    file: source.name.file.clone(),
                    line: None,
                    message,
                }),
            }
        }
        (defs, problems)
    }

    fn find(&self, name: &str) -> Option<(Arc<Source>, RecipeDef)> {
        self.lock().iter().find_map(|l| {
            let def = l.defs.iter().find(|d| d.name == name)?;
            Some((l.source.clone(), def.clone()))
        })
    }

    /// Whether `def` can run on `ctx`; `None` when it is not for these files.
    fn availability(
        &self,
        source: &Source,
        def: &RecipeDef,
        ctx: &RecipeContext,
    ) -> Option<Availability> {
        let fits = defs::fits(&def.when, ctx)?;
        if !def.asks {
            return Some(fits);
        }
        // Asked even when too few files are chosen: a recipe whose program is missing must
        // not be offered as one that only needs another file.
        match (self.ask_available(source, def, ctx)?, fits) {
            (Availability::Available, fits) => Some(fits),
            (disabled, _) => Some(disabled),
        }
    }

    /// What `available` answers; `None` hides the recipe.
    fn ask_available(
        &self,
        source: &Source,
        def: &RecipeDef,
        ctx: &RecipeContext,
    ) -> Option<Availability> {
        let s = match self.session(source, Mode::Check, &Cancel::new()) {
            Ok(s) => s,
            Err((e, stopped)) => {
                let why = errors::recipe_error(&e, stopped, &source.name).to_string();
                return Some(Availability::Disabled(why));
            }
        };
        match call_available(&s, def, ctx) {
            Ok(a) => a,
            Err(e) => {
                let why = errors::recipe_error(&e, s.guard.stopped(), &source.name).to_string();
                Some(Availability::Disabled(why))
            }
        }
    }

    fn info(&self, source: &Source, def: &RecipeDef, state: Availability) -> RecipeInfo {
        RecipeInfo {
            name: def.name.clone(),
            title: def.title.get(),
            description: def.description.get(),
            args: def.args.iter().map(|a| a.public()).collect(),
            origin: source.origin.clone(),
            state,
        }
    }
}

/// The recipes the file declared, once it ran.
fn declared(lua: &Lua) -> Vec<(String, Table)> {
    lua.app_data_ref::<api::Shared>()
        .map(|s| s.recipes.borrow().clone())
        .unwrap_or_default()
}

/// The declaration of `name` in a state where its file ran.
fn table_of(lua: &Lua, name: &str) -> mlua::Result<Table> {
    declared(lua)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, t)| t)
        .ok_or_else(|| mlua::Error::runtime(format!("the file no longer declares {name}")))
}

/// Ask `available(ctx)`: `true` shows the recipe, `false` hides it, `false, "why"` (or
/// `nil, "why"`) shows it disabled with the reason.
fn call_available(
    s: &Session,
    def: &RecipeDef,
    ctx: &RecipeContext,
) -> mlua::Result<Option<Availability>> {
    let table = table_of(&s.lua, &def.name)?;
    let f: mlua::Function = table.get("available")?;
    let (ok, why): (Value, Value) = f.call(ctx_table(&s.lua, ctx)?)?;
    Ok(match (ok, why) {
        (Value::Boolean(true), _) => Some(Availability::Available),
        (_, Value::String(why)) => Some(Availability::Disabled(defs_clean(&why.to_string_lossy()))),
        _ => None,
    })
}

fn defs_clean(text: &str) -> String {
    rada_core::display::text(text).chars().take(200).collect()
}

/// `ctx` as a script sees it.
fn ctx_table(lua: &Lua, ctx: &RecipeContext) -> mlua::Result<Table> {
    let list = |paths: &[PathBuf]| -> mlua::Result<Table> {
        let t = lua.create_table()?;
        for p in paths {
            t.raw_push(paths::to_lua(lua, p)?)?;
        }
        Ok(t)
    };
    let t = lua.create_table()?;
    t.raw_set("selection", list(&ctx.selection)?)?;
    t.raw_set("marked", list(&ctx.marked)?)?;
    t.raw_set("cwd", paths::to_lua(lua, &ctx.cwd)?)?;
    t.raw_set("platform", ctx.platform())?;
    t.raw_set("lang", rada_core::i18n::lang().code())?;
    Ok(t)
}

/// The arguments of a run: those given, the defaults for the rest. Each must be there and be
/// a value the argument takes.
fn arg_table(lua: &Lua, def: &RecipeDef, given: &ArgValues) -> Result<Table, RecipeError> {
    let bad = |name: &str, message: &str| RecipeError::BadArgument {
        name: name.to_string(),
        message: message.to_string(),
    };
    let t = lua.create_table().map_err(|e| bad("", &e.to_string()))?;
    for arg in &def.args {
        let value = given.get(&arg.name).or(arg.default.as_ref());
        let Some(value) = value else {
            return Err(bad(&arg.name, "missing"));
        };
        if !arg.public().accepts(value) {
            return Err(bad(&arg.name, "not a value it takes"));
        }
        let lua_value = match value {
            rada_core::plugin::ArgValue::Text(s) => lua.create_string(s).map(Value::String),
            rada_core::plugin::ArgValue::Int(n) => Ok(Value::Integer(*n)),
            rada_core::plugin::ArgValue::Flag(b) => Ok(Value::Boolean(*b)),
        };
        let lua_value = lua_value.map_err(|e| bad(&arg.name, &e.to_string()))?;
        t.raw_set(arg.name.as_str(), lua_value)
            .map_err(|e| bad(&arg.name, &e.to_string()))?;
    }
    if let Some(unknown) = given
        .keys()
        .find(|k| !def.args.iter().any(|a| &a.name == *k))
    {
        return Err(bad(unknown, "the recipe has no such argument"));
    }
    Ok(t)
}

/// The text of one of the person's files: a regular file, small, in UTF-8, never bytecode.
fn read_text(path: &Path, max: u64) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > max {
        return Err(format!("bigger than {} KiB", max >> 10));
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!("bigger than {} KiB", max >> 10));
    }
    if bytes.starts_with(b"\x1bLua") {
        return Err("compiled Lua is not read: recipes are text".into());
    }
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".into())
}

fn label_of(file: &Path) -> String {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.chars().take(48).collect()
}

impl PluginHost for LuaHost {
    fn load(&self) -> Vec<LoadProblem> {
        let (sources, mut problems) = self.sources();
        let mut loaded: Vec<Loaded> = Vec::new();
        for source in sources {
            let (mut defs, found) = self.load_one(&source);
            problems.extend(found);
            defs.retain(|d| {
                let taken = loaded
                    .iter()
                    .any(|l| l.defs.iter().any(|o| o.name == d.name));
                if taken {
                    problems.push(LoadProblem {
                        file: source.name.file.clone(),
                        line: None,
                        message: format!("there is already a recipe called {}", d.name),
                    });
                }
                !taken
            });
            loaded.push(Loaded {
                source: Arc::new(source),
                defs,
            });
        }
        *self.lock() = loaded;
        problems
    }

    fn recipes(&self, ctx: &RecipeContext) -> Vec<RecipeInfo> {
        let all: Vec<(Arc<Source>, RecipeDef)> = self
            .lock()
            .iter()
            .flat_map(|l| l.defs.iter().map(|d| (l.source.clone(), d.clone())))
            .collect();
        let mut out: Vec<RecipeInfo> = all
            .iter()
            .filter_map(|(source, def)| {
                let state = self.availability(source, def, ctx)?;
                Some(self.info(source, def, state))
            })
            .collect();
        out.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(a.name.cmp(&b.name))
        });
        out
    }

    fn run(
        &self,
        name: &str,
        ctx: &RecipeContext,
        args: &ArgValues,
        cancel: &Cancel,
    ) -> Result<RecipePlan, RecipeError> {
        let (source, def) = self
            .find(name)
            .ok_or_else(|| RecipeError::Unknown(name.to_string()))?;
        match self.availability(&source, &def, ctx) {
            Some(Availability::Available) => {}
            Some(Availability::Disabled(why)) => return Err(RecipeError::NotAvailable(why)),
            None => {
                return Err(RecipeError::NotAvailable(format!(
                    "{} is not for the files chosen",
                    def.title.get()
                )));
            }
        }
        let s = self
            .session(&source, Mode::Run, cancel)
            .map_err(|(e, stopped)| errors::recipe_error(&e, stopped, &source.name))?;
        let args = arg_table(&s.lua, &def, args)?;
        let returned = call_run(&s, &def, ctx, args)
            .map_err(|e| errors::recipe_error(&e, s.guard.stopped(), &source.name))?;
        finish(&s, &def, &source, returned)
    }
}

fn call_run(s: &Session, def: &RecipeDef, ctx: &RecipeContext, args: Table) -> mlua::Result<Value> {
    let table = table_of(&s.lua, &def.name)?;
    let run: mlua::Function = table.get("run")?;
    let value = run.call::<Value>((ctx_table(&s.lua, ctx)?, args))?;
    api::checkpoint(&s.lua)?;
    Ok(value)
}

/// The plan of a run: what it asked for, with the title and notes it may have returned.
fn finish(
    s: &Session,
    def: &RecipeDef,
    source: &Source,
    returned: Value,
) -> Result<RecipePlan, RecipeError> {
    let script = |message: String| RecipeError::Script {
        file: source.name.file.clone(),
        line: None,
        message,
    };
    let requests = s
        .lua
        .app_data_ref::<api::Shared>()
        .map(|sh| sh.requests.take())
        .unwrap_or_default();
    let (title, notes) = returned_words(returned).map_err(script)?;
    let plan = RecipePlan {
        recipe: def.name.clone(),
        requests,
        title: title
            .map(|t| defs_clean(&t))
            .or_else(|| Some(def.title.get())),
        notes: notes.iter().take(20).map(|n| defs_clean(n)).collect(),
        failure: FailurePolicy::Keep,
    };
    plan.validate().map_err(|e| script(e.to_string()))?;
    Ok(plan)
}

/// The title and the notes `run` may return.
fn returned_words(returned: Value) -> Result<(Option<String>, Vec<String>), String> {
    match returned {
        Value::Nil => Ok((None, Vec::new())),
        Value::Table(t) => {
            let title: Option<String> = t.get("title").map_err(|_| "title is a string")?;
            let notes: Option<Vec<String>> = t.get("notes").map_err(|_| "notes lists strings")?;
            Ok((title, notes.unwrap_or_default()))
        }
        _ => Err("run returns nothing, or { title = …, notes = { … } }".into()),
    }
}
