//! The table `rada`, the one way out of a script: version 1 of the API.
//!
//! * `rada.recipe { … }` declares a recipe;
//! * `rada.list`, `rada.stat`, `rada.meta`, `rada.has` look, and never change anything;
//! * `rada.match`, `rada.replace`, `rada.split` work on text;
//! * `rada.join`, `rada.parent`, `rada.name`, `rada.stem`, `rada.ext` work on paths as text;
//! * `rada.copy`, `rada.move`, `rada.trash`, `rada.mkdir`, `rada.rename`, `rada.run` and
//!   `rada.checksum` add a request to the plan of the run. They do nothing else: what is asked
//!   is shown in the plan window, and only happens if the person confirms it;
//! * `rada.log` writes to rada's log.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use mlua::{Lua, MultiValue, Table, Value};
use rada_core::ops::SubRequest;

use crate::session::{FileName, Guard, Mode, stop_error};
use crate::text::Patterns;
use crate::{Limits, Tools};

/// What the functions of `rada` share during one task.
pub(crate) struct Shared {
    pub mode: Mode,
    pub name: FileName,
    pub guard: Rc<Guard>,
    pub tools: Arc<Tools>,
    pub limits: Limits,
    /// The recipes the file declared, in order.
    pub recipes: RefCell<Vec<(String, Table)>>,
    /// What a run asked for.
    pub requests: RefCell<Vec<SubRequest>>,
    pub patterns: RefCell<Patterns>,
    pub has: RefCell<HashMap<String, bool>>,
    pub logs: Cell<u32>,
}

/// An error at the line of the script that called the function.
pub(crate) fn fail(lua: &Lua, message: impl std::fmt::Display) -> mlua::Error {
    let label = lua
        .app_data_ref::<Shared>()
        .map(|s| s.name.label.clone())
        .unwrap_or_default();
    let line = lua.inspect_stack(1, |d| d.current_line()).flatten();
    match line {
        Some(line) => mlua::Error::runtime(format!("{label}:{line}: {message}")),
        None => mlua::Error::runtime(format!("{label}: {message}")),
    }
}

/// Stop here if the run has been stopped; every function of `rada` asks first.
pub(crate) fn checkpoint(lua: &Lua) -> mlua::Result<()> {
    let stop = lua.app_data_ref::<Shared>().and_then(|s| s.guard.check());
    match stop {
        Some(stop) => Err(stop_error(stop)),
        None => Ok(()),
    }
}

/// Make the table `rada` in `env`.
pub(crate) fn install(lua: &Lua, env: &Table, shared: Shared) -> mlua::Result<()> {
    lua.set_app_data(shared);
    let rada = lua.create_table()?;
    rada.raw_set("api", rada_core::plugin::API_VERSION)?;
    declare(lua, &rada)?;
    crate::look::install(lua, &rada)?;
    crate::ask::install(lua, &rada)?;
    text_functions(lua, &rada)?;
    path_functions(lua, &rada)?;
    env.raw_set("rada", rada)
}

fn declare(lua: &Lua, rada: &Table) -> mlua::Result<()> {
    let recipe = lua.create_function(|lua, t: Value| {
        checkpoint(lua)?;
        let Value::Table(t) = t else {
            return Err(fail(
                lua,
                "rada.recipe takes a table: rada.recipe { name = …, run = … }",
            ));
        };
        let name = match t.get::<Value>("name")? {
            Value::String(s) => s.to_string_lossy(),
            _ => return Err(fail(lua, "a recipe needs a name")),
        };
        let shared = lua
            .app_data_ref::<Shared>()
            .expect("the state was made with it");
        let mut recipes = shared.recipes.borrow_mut();
        if recipes.iter().any(|(n, _)| *n == name) {
            return Err(fail(
                lua,
                format!("there is already a recipe called {name}"),
            ));
        }
        recipes.push((name, t));
        Ok(())
    })?;
    rada.raw_set("recipe", recipe)?;
    rada.raw_set("log", lua.create_function(log)?)
}

/// `rada.log(...)`: a line in rada's log, cleaned and cut short.
fn log(lua: &Lua, args: MultiValue) -> mlua::Result<()> {
    checkpoint(lua)?;
    let shared = lua
        .app_data_ref::<Shared>()
        .expect("the state was made with it");
    if shared.logs.get() >= MAX_LOGS {
        return Ok(());
    }
    shared.logs.set(shared.logs.get() + 1);
    let words: Vec<String> = args.iter().map(show).collect();
    let text: String = words.join(" ").chars().take(1000).collect();
    let text = rada_core::display::text(&text);
    tracing::info!(target: "rada::recipe", "{}: {text}", shared.name.label);
    Ok(())
}

/// Lines one task may write to the log.
const MAX_LOGS: u32 = 200;

fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.to_string_lossy(),
        Value::Integer(n) => n.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Nil => "nil".into(),
        other => other.type_name().to_string(),
    }
}

fn bytes_arg(lua: &Lua, v: &Value, what: &str) -> mlua::Result<Vec<u8>> {
    match v {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Integer(_) | Value::Number(_) => Ok(show(v).into_bytes()),
        other => Err(fail(
            lua,
            format!("{what} is a string, not {}", other.type_name()),
        )),
    }
}

fn pattern(lua: &Lua, v: &Value) -> mlua::Result<regex::bytes::Regex> {
    let p = bytes_arg(lua, v, "the pattern")?;
    let shared = lua
        .app_data_ref::<Shared>()
        .expect("the state was made with it");
    let re = shared.patterns.borrow_mut().get(&p);
    re.map_err(|e| fail(lua, e))
}

fn text_functions(lua: &Lua, rada: &Table) -> mlua::Result<()> {
    let find = lua.create_function(|lua, (text, pat): (Value, Value)| {
        checkpoint(lua)?;
        let text = bytes_arg(lua, &text, "the text")?;
        crate::text::find(lua, &pattern(lua, &pat)?, &text)
    })?;
    let replace = lua.create_function(|lua, (text, pat, with): (Value, Value, Value)| {
        checkpoint(lua)?;
        let text = bytes_arg(lua, &text, "the text")?;
        let with = bytes_arg(lua, &with, "the replacement")?;
        let (out, n) =
            crate::text::replace(&pattern(lua, &pat)?, &text, &with).map_err(|e| fail(lua, e))?;
        Ok((lua.create_string(out)?, n))
    })?;
    let split = lua.create_function(|lua, (text, pat): (Value, Value)| {
        checkpoint(lua)?;
        let text = bytes_arg(lua, &text, "the text")?;
        let re = pattern(lua, &pat)?;
        let pieces = crate::text::split(&re, &text).map_err(|e| fail(lua, e))?;
        let list = lua.create_table()?;
        for p in pieces {
            list.raw_push(lua.create_string(p)?)?;
        }
        Ok(list)
    })?;
    rada.raw_set("match", find)?;
    rada.raw_set("replace", replace)?;
    rada.raw_set("split", split)
}

fn path_arg(lua: &Lua, v: &Value) -> mlua::Result<PathBuf> {
    crate::paths::from_lua(v).map_err(|e| fail(lua, e))
}

fn path_functions(lua: &Lua, rada: &Table) -> mlua::Result<()> {
    let join = lua.create_function(|lua, parts: MultiValue| {
        let mut path = PathBuf::new();
        for part in parts.iter() {
            let piece = path_arg(lua, part)?;
            if path.as_os_str().is_empty() || !piece.is_absolute() {
                path.push(piece);
            } else {
                return Err(fail(
                    lua,
                    "only the first part of rada.join can be a whole path",
                ));
            }
        }
        crate::paths::to_lua(lua, &path)
    })?;
    rada.raw_set("join", join)?;
    type Part = fn(&std::path::Path) -> Option<PathBuf>;
    let parts: [(&str, Part); 4] = [
        ("parent", |p| p.parent().map(PathBuf::from)),
        ("name", |p| p.file_name().map(PathBuf::from)),
        ("stem", |p| p.file_stem().map(PathBuf::from)),
        ("ext", |p| p.extension().map(PathBuf::from)),
    ];
    for (name, part) in parts {
        let f = lua.create_function(move |lua, v: Value| match part(&path_arg(lua, &v)?) {
            Some(p) if !p.as_os_str().is_empty() => {
                Ok(Value::String(crate::paths::to_lua(lua, &p)?))
            }
            _ => Ok(Value::Nil),
        })?;
        rada.raw_set(name, f)?;
    }
    Ok(())
}
