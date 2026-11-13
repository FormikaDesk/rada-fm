//! One Lua state, closed, for one file and one task, thrown away at the end.
//!
//! A fresh state each time means a recipe cannot keep anything from one run to the next, two
//! runs of the same recipe on the same files give the same plan, and nothing a script did can
//! reach the next one.
//!
//! The state has only what is listed in [`SAFE_GLOBALS`] and the libraries below, minus what
//! could run for ever in one call (Lua patterns), write bytecode or reach outside: the
//! environment is built by hand, then the global table is emptied.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::chunk::ChunkMode;
use mlua::{Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Value};
use rada_core::plugin::{Cancel, Limit};

use crate::Limits;

/// How much one task may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Lua instructions, counted a thousand at a time.
    pub instructions: u64,
    pub time: Duration,
}

/// What a state is for: only a run may ask for a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Reading the file to learn its recipes.
    Load,
    /// Asking a recipe whether it can run.
    Check,
    Run,
}

/// Why the state was stopped from outside the script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    Limit(Limit),
    Cancelled,
}

/// Watches the limits: the hook asks it, and so does every function of `rada` that may wait.
pub(crate) struct Guard {
    deadline: Instant,
    instructions: u64,
    used: Cell<u64>,
    cancel: Cancel,
    stopped: Cell<Option<Stop>>,
}

/// How often the hook runs, in instructions.
const HOOK_EVERY: u32 = 1000;

impl Guard {
    fn new(budget: Budget, cancel: Cancel) -> Guard {
        Guard {
            deadline: Instant::now() + budget.time,
            instructions: budget.instructions,
            used: Cell::new(0),
            cancel,
            stopped: Cell::new(None),
        }
    }

    /// Whether to stop now. Once stopped, always stopped: a script that caught the error in a
    /// `pcall` meets it again at once.
    pub(crate) fn check(&self) -> Option<Stop> {
        if self.stopped.get().is_none() {
            let stop = if self.cancel.is_cancelled() {
                Some(Stop::Cancelled)
            } else if Instant::now() >= self.deadline {
                Some(Stop::Limit(Limit::Time))
            } else if self.used.get() > self.instructions {
                Some(Stop::Limit(Limit::Instructions))
            } else {
                None
            };
            self.stopped.set(stop);
        }
        self.stopped.get()
    }

    pub(crate) fn stop(&self, why: Stop) {
        if self.stopped.get().is_none() {
            self.stopped.set(Some(why));
        }
    }

    pub(crate) fn stopped(&self) -> Option<Stop> {
        self.stopped.get()
    }

    /// Whether a wait inside a function of `rada` should give up.
    pub(crate) fn should_stop(&self) -> bool {
        self.check().is_some()
    }

    fn tick(&self) -> Option<Stop> {
        self.used.set(self.used.get() + u64::from(HOOK_EVERY));
        self.check()
    }
}

/// The error a stop raises inside the script.
pub(crate) fn stop_error(stop: Stop) -> mlua::Error {
    mlua::Error::runtime(match stop {
        Stop::Cancelled => "stopped",
        Stop::Limit(Limit::Time) => "time limit reached",
        Stop::Limit(Limit::Instructions) => "instruction limit reached",
        Stop::Limit(Limit::Memory) => "memory limit reached",
    })
}

/// The functions of the base library a script may use; `pairs`, `pcall` and `setmetatable`
/// are rada's own versions (see [`install_base`]).
const SAFE_GLOBALS: &[&str] = &[
    "assert",
    "error",
    "ipairs",
    "next",
    "select",
    "tonumber",
    "tostring",
    "type",
    "rawequal",
    "rawget",
    "rawlen",
    "rawset",
    "getmetatable",
];

/// Builds the environment out of the libraries Lua opened. It runs before the hook, with the
/// full global table, and is rada's own code.
const BOOTSTRAP: &str = r#"
local names, drop_string, drop_math = ...
local function copy(src, drop)
  local t = {}
  for k, v in next, src do
    if not drop[k] then t[k] = v end
  end
  return t
end
local env = {}
for _, n in ipairs(names) do env[n] = _G[n] end
env.string = copy(string, drop_string)
env.math = copy(math, drop_math)
env.table = copy(table, {})
env.utf8 = copy(utf8, {})
local mt = getmetatable("")
mt.__index = env.string
mt.__metatable = false
return env, setmetatable
"#;

/// `string` without its patterns (one call to `find` can take hours, and the hook cannot
/// interrupt a C function) and without `dump` (bytecode).
const DROP_STRING: &[&str] = &["find", "match", "gmatch", "gsub", "dump"];
const DROP_MATH: &[&str] = &["random", "randomseed"];

/// A closed state, ready for one file.
pub(crate) struct Session {
    pub lua: Lua,
    pub env: Table,
    pub guard: Rc<Guard>,
}

impl Session {
    pub(crate) fn new(limits: &Limits, budget: Budget, cancel: Cancel) -> mlua::Result<Session> {
        let libs = StdLib::STRING | StdLib::MATH | StdLib::TABLE | StdLib::UTF8;
        let lua = Lua::new_with(libs, LuaOptions::new())?;
        let env = build_env(&lua)?;
        lua.set_memory_limit(lua.used_memory() + limits.memory)?;
        let guard = Rc::new(Guard::new(budget, cancel));
        let watch = guard.clone();
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_EVERY),
            move |_, _| match watch.tick() {
                Some(stop) => Err(stop_error(stop)),
                None => Ok(mlua::VmState::Continue),
            },
        )?;
        Ok(Session { lua, env, guard })
    }

    /// Run the text of a file (never bytecode) in the closed environment.
    pub(crate) fn exec(&self, label: &str, text: &str) -> mlua::Result<()> {
        self.lua
            .load(text)
            .set_name(format!("={label}"))
            .set_mode(ChunkMode::Text)
            .set_environment(self.env.clone())
            .exec()
    }
}

/// The environment, and a global table left empty.
fn build_env(lua: &Lua) -> mlua::Result<Table> {
    let names = lua.create_sequence_from(SAFE_GLOBALS.iter().copied())?;
    let set = |list: &[&str]| -> mlua::Result<Table> {
        let t = lua.create_table()?;
        for n in list {
            t.raw_set(*n, true)?;
        }
        Ok(t)
    };
    let (env, base_setmetatable): (Table, Function) = lua
        .load(BOOTSTRAP)
        .set_name("=rada")
        .set_mode(ChunkMode::Text)
        .call((names, set(DROP_STRING)?, set(DROP_MATH)?))?;
    install_base(lua, &env, base_setmetatable)?;
    let globals = lua.globals();
    let keys: Vec<Value> = globals
        .pairs::<Value, Value>()
        .filter_map(|kv| kv.ok().map(|(k, _)| k))
        .collect();
    for key in keys {
        globals.raw_set(key, Value::Nil)?;
    }
    Ok(env)
}

/// rada's own `pairs`, `pcall` and `setmetatable`.
fn install_base(lua: &Lua, env: &Table, base_setmetatable: Function) -> mlua::Result<()> {
    env.raw_set("pairs", lua.create_function(sorted_pairs)?)?;
    env.raw_set("pcall", lua.create_function(guarded_pcall)?)?;
    let setmt = lua.create_function(move |_, (t, mt): (Value, Value)| {
        if let Value::Table(mt) = &mt
            && !mt.raw_get::<Value>("__gc")?.is_nil()
        {
            return Err(mlua::Error::runtime("__gc is not available to recipes"));
        }
        base_setmetatable.call::<Value>((t, mt))
    })?;
    env.raw_set("setmetatable", setmt)
}

/// `pcall` that catches the script's own errors, never a stop: a recipe out of time or memory
/// cannot carry on by catching it.
fn guarded_pcall(lua: &Lua, (f, args): (Value, MultiValue)) -> mlua::Result<MultiValue> {
    let result = match &f {
        Value::Function(f) => f.call::<MultiValue>(args),
        other => Err(mlua::Error::runtime(format!(
            "attempt to call a {} value",
            other.type_name()
        ))),
    };
    let guard = lua
        .app_data_ref::<crate::api::Shared>()
        .map(|s| s.guard.clone());
    if let (Err(e), Some(g)) = (&result, &guard)
        && crate::errors::is_memory(e)
    {
        g.stop(Stop::Limit(Limit::Memory));
    }
    match result {
        Ok(mut values) => {
            values.push_front(Value::Boolean(true));
            Ok(values)
        }
        Err(e) if guard.is_some_and(|g| g.stopped().is_some()) => Err(e),
        Err(e) => {
            let text = lua.create_string(crate::errors::message(&e))?;
            Ok(MultiValue::from_vec(vec![
                Value::Boolean(false),
                Value::String(text),
            ]))
        }
    }
}

/// `pairs` in a fixed order (numbers, then strings, then the rest), because Lua's own order
/// changes from one state to the next and a recipe would give a different plan each time.
fn sorted_pairs(lua: &Lua, t: Value) -> mlua::Result<MultiValue> {
    let Value::Table(t) = t else {
        return Err(mlua::Error::runtime(format!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            t.type_name()
        )));
    };
    if let Some(mt) = t.metatable()
        && let Value::Function(f) = mt.raw_get::<Value>("__pairs")?
    {
        return f.call::<MultiValue>(t);
    }
    let mut keys: Vec<Value> = Vec::new();
    for kv in t.clone().pairs::<Value, Value>() {
        keys.push(kv?.0);
    }
    keys.sort_by(order);
    let at = Cell::new(0usize);
    let table = t.clone();
    let next = lua.create_function(move |_, _: MultiValue| {
        while let Some(key) = keys.get(at.get()) {
            at.set(at.get() + 1);
            let value: Value = table.raw_get(key.clone())?;
            if !value.is_nil() {
                return Ok(MultiValue::from_vec(vec![key.clone(), value]));
            }
        }
        Ok(MultiValue::from_vec(vec![Value::Nil]))
    })?;
    Ok(MultiValue::from_vec(vec![
        Value::Function(next),
        Value::Table(t),
        Value::Nil,
    ]))
}

fn rank(v: &Value) -> u8 {
    match v {
        Value::Integer(_) | Value::Number(_) => 0,
        Value::String(_) => 1,
        Value::Boolean(_) => 2,
        _ => 3,
    }
}

fn order(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let number = |v: &Value| match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Number(n) => Some(*n),
        _ => None,
    };
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.as_bytes().cmp(&y.as_bytes()),
        (Value::Boolean(x), Value::Boolean(y)) => x.cmp(y),
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => rank(a).cmp(&rank(b)),
        },
    }
}

/// The file a state was made for, as messages name it.
#[derive(Clone, Debug)]
pub(crate) struct FileName {
    /// The whole path, or `rada:<name>` for one built in.
    pub file: PathBuf,
    /// The name Lua puts before the line in its messages.
    pub label: String,
}
