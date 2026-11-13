//! The functions that ask: each adds one request to the plan of the run, and returns. Nothing
//! is done until the person confirms the plan, and then by rada, never by the script.

use std::path::PathBuf;

use mlua::{Lua, Table, Value};
use rada_core::ops::{
    Arg, ChecksumAlgo, Collect, ConflictPolicy, OpRequest, ProduceSpec, ProducerSpec, ProgramCall,
    SubRequest,
};

use crate::api::{Shared, checkpoint, fail};
use crate::paths;
use crate::session::Mode;

pub(crate) fn install(lua: &Lua, rada: &Table) -> mlua::Result<()> {
    rada.raw_set(
        "copy",
        lua.create_function(|lua, a: (Value, Value, Value)| transfer(lua, a, false))?,
    )?;
    rada.raw_set(
        "move",
        lua.create_function(|lua, a: (Value, Value, Value)| transfer(lua, a, true))?,
    )?;
    rada.raw_set("trash", lua.create_function(trash)?)?;
    rada.raw_set("mkdir", lua.create_function(mkdir)?)?;
    rada.raw_set("rename", lua.create_function(rename)?)?;
    rada.raw_set("run", lua.create_function(run)?)?;
    rada.raw_set("checksum", lua.create_function(checksum)?)?;
    tokens(lua, rada)
}

/// The words a program's arguments can hold besides plain text.
fn tokens(lua: &Lua, rada: &Table) -> mlua::Result<()> {
    let token = |word: &str| -> mlua::Result<Table> {
        let t = lua.create_table()?;
        t.raw_set("rada_arg", word)?;
        Ok(t)
    };
    rada.raw_set("inputs", token("inputs")?)?;
    rada.raw_set("out", token("out")?)?;
    rada.raw_set("outdir", token("outdir")?)?;
    rada.raw_set("prev", token("prev")?)?;
    rada.raw_set("profile", token("profile")?)?;
    let input = lua.create_function(|lua, n: i64| {
        if n < 1 {
            return Err(fail(lua, "rada.input counts from 1"));
        }
        let t = lua.create_table()?;
        t.raw_set("rada_arg", "input")?;
        t.raw_set("n", n)?;
        Ok(t)
    })?;
    rada.raw_set("input", input)
}

/// Add `request` to the plan, if this is a run and there is room.
fn push(lua: &Lua, request: SubRequest) -> mlua::Result<()> {
    checkpoint(lua)?;
    let shared = lua
        .app_data_ref::<Shared>()
        .expect("the state was made with it");
    if shared.mode != Mode::Run {
        return Err(fail(
            lua,
            "plans are asked for in run, not while the recipe is read or checked",
        ));
    }
    let mut requests = shared.requests.borrow_mut();
    if requests.len() >= shared.limits.requests {
        return Err(fail(
            lua,
            format!(
                "a recipe asks for at most {} things",
                shared.limits.requests
            ),
        ));
    }
    requests.push(request);
    Ok(())
}

fn path(lua: &Lua, v: &Value) -> mlua::Result<PathBuf> {
    paths::absolute(v).map_err(|e| fail(lua, e))
}

fn path_list(lua: &Lua, v: &Value) -> mlua::Result<Vec<PathBuf>> {
    paths::absolute_list(v).map_err(|e| fail(lua, e))
}

/// `conflict = "skip"` or `"keep_both"`. A recipe never replaces what is there.
fn conflict(lua: &Lua, opts: &Value) -> mlua::Result<(ConflictPolicy, bool)> {
    let Value::Table(t) = opts else {
        return Ok((ConflictPolicy::Skip, false));
    };
    let policy = match t.get::<Value>("conflict")? {
        Value::Nil => ConflictPolicy::Skip,
        Value::String(s) if s.as_bytes().as_ref() == b"skip" => ConflictPolicy::Skip,
        Value::String(s) if s.as_bytes().as_ref() == b"keep_both" => ConflictPolicy::KeepBoth,
        _ => {
            return Err(fail(
                lua,
                "conflict is \"skip\" or \"keep_both\": a recipe never replaces a file",
            ));
        }
    };
    Ok((
        policy,
        matches!(t.get::<Value>("verify")?, Value::Boolean(true)),
    ))
}

fn transfer(
    lua: &Lua,
    (sources, dest, opts): (Value, Value, Value),
    moving: bool,
) -> mlua::Result<()> {
    let sources = path_list(lua, &sources)?;
    let destination = path(lua, &dest)?;
    let (conflict, verify) = conflict(lua, &opts)?;
    let request = if moving {
        OpRequest::Move {
            sources,
            destination,
            conflict,
            verify,
        }
    } else {
        OpRequest::Copy {
            sources,
            destination,
            conflict,
            verify,
        }
    };
    push(lua, SubRequest::Op(request))
}

fn trash(lua: &Lua, sources: Value) -> mlua::Result<()> {
    let sources = path_list(lua, &sources)?;
    push(lua, SubRequest::Op(OpRequest::Trash { sources }))
}

/// A name inside a folder, never a path.
fn name_arg(lua: &Lua, v: &Value) -> mlua::Result<String> {
    match v {
        Value::String(s) => match s.to_str() {
            Ok(n) if !n.is_empty() && !n.contains(['/', '\\']) => Ok(n.to_string()),
            _ => Err(fail(
                lua,
                "a new name is one name, in Unicode, without / or \\",
            )),
        },
        other => Err(fail(
            lua,
            format!("a name is a string, not {}", other.type_name()),
        )),
    }
}

fn mkdir(lua: &Lua, at: Value) -> mlua::Result<()> {
    let at = path(lua, &at)?;
    let (Some(parent), Some(name)) = (at.parent(), at.file_name()) else {
        return Err(fail(
            lua,
            "rada.mkdir takes the whole path of the new folder",
        ));
    };
    let name = name
        .to_str()
        .ok_or_else(|| fail(lua, "a new name must be Unicode"))?
        .to_string();
    let request = OpRequest::MakeDir {
        parent: parent.to_path_buf(),
        name,
    };
    push(lua, SubRequest::Op(request))
}

fn rename(lua: &Lua, (at, new_name, opts): (Value, Value, Value)) -> mlua::Result<()> {
    let request = OpRequest::Rename {
        path: path(lua, &at)?,
        new_name: name_arg(lua, &new_name)?,
        conflict: conflict(lua, &opts)?.0,
    };
    push(lua, SubRequest::Op(request))
}

fn checksum(lua: &Lua, t: Table) -> mlua::Result<()> {
    let spec = ProduceSpec {
        inputs: vec![path(lua, &t.get::<Value>("input")?)?],
        output: path(lua, &t.get::<Value>("output")?)?,
        producer: ProducerSpec::Checksum {
            algo: ChecksumAlgo::Sha256,
        },
        size_hint: None,
    };
    push(lua, SubRequest::Produce(spec))
}

fn run(lua: &Lua, t: Table) -> mlua::Result<()> {
    let chain = match t.get::<Value>("chain")? {
        Value::Nil => vec![call(lua, &t)?],
        Value::Table(list) => {
            let mut chain = Vec::new();
            for c in list.sequence_values::<Table>() {
                chain.push(call(lua, &c.map_err(|_| fail(lua, "chain lists tables"))?)?);
            }
            chain
        }
        _ => return Err(fail(lua, "chain is a list of { program = …, args = … }")),
    };
    let producer = ProducerSpec::Program {
        chain,
        timeout_secs: whole(lua, &t, "timeout")?.unwrap_or(600),
        ok_exit: exits(lua, &t)?,
        collect: collect(lua, &t.get::<Value>("collect")?)?,
    };
    let spec = ProduceSpec {
        inputs: path_list(lua, &t.get::<Value>("inputs")?)?,
        output: path(lua, &t.get::<Value>("output")?)?,
        producer,
        size_hint: whole(lua, &t, "size_hint")?,
    };
    spec.validate().map_err(|e| fail(lua, e))?;
    push(lua, SubRequest::Produce(spec))
}

fn whole(lua: &Lua, t: &Table, key: &str) -> mlua::Result<Option<u64>> {
    match t.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::Integer(n) if n > 0 => Ok(Some(n as u64)),
        _ => Err(fail(lua, format!("{key} is a whole number above 0"))),
    }
}

fn exits(lua: &Lua, t: &Table) -> mlua::Result<Vec<i32>> {
    match t.get::<Value>("ok_exit")? {
        Value::Nil => Ok(vec![0]),
        Value::Table(list) => list
            .sequence_values::<i32>()
            .collect::<mlua::Result<Vec<i32>>>()
            .map_err(|_| fail(lua, "ok_exit lists whole numbers")),
        _ => Err(fail(lua, "ok_exit lists whole numbers")),
    }
}

/// `collect = { single = "pdf" }`: the program names its file by itself.
fn collect(lua: &Lua, v: &Value) -> mlua::Result<Collect> {
    match v {
        Value::Nil => Ok(Collect::Exact),
        Value::Table(t) => match t.get::<Value>("single")? {
            Value::String(s) => Ok(Collect::Single {
                ext: s.to_string_lossy(),
            }),
            _ => Err(fail(lua, "collect is { single = \"ext\" }")),
        },
        _ => Err(fail(lua, "collect is { single = \"ext\" }")),
    }
}

fn call(lua: &Lua, t: &Table) -> mlua::Result<ProgramCall> {
    let program = match t.get::<Value>("program")? {
        Value::String(s) => s
            .to_str()
            .map(|s| s.to_string())
            .map_err(|_| fail(lua, "program is a name in Unicode"))?,
        _ => {
            return Err(fail(
                lua,
                "program is the name of a program, such as \"qpdf\"",
            ));
        }
    };
    let mut args = Vec::new();
    if let Value::Table(list) = t.get::<Value>("args")? {
        for v in list.sequence_values::<Value>() {
            args.push(arg(lua, &v?)?);
        }
    }
    Ok(ProgramCall { program, args })
}

fn arg(lua: &Lua, v: &Value) -> mlua::Result<Arg> {
    match v {
        Value::String(s) => match s.to_str() {
            Ok(text) => Ok(Arg::Lit(text.to_string())),
            Err(_) => Err(fail(lua, "an argument must be Unicode text")),
        },
        Value::Integer(n) => Ok(Arg::Lit(n.to_string())),
        Value::Table(t) => token(lua, t),
        other => Err(fail(
            lua,
            format!(
                "an argument is text or a rada word, not {}",
                other.type_name()
            ),
        )),
    }
}

fn token(lua: &Lua, t: &Table) -> mlua::Result<Arg> {
    let word: Option<String> = t.raw_get("rada_arg")?;
    Ok(match word.as_deref() {
        Some("inputs") => Arg::Inputs,
        Some("out") => Arg::Out,
        Some("outdir") => Arg::OutDir,
        Some("prev") => Arg::Prev,
        Some("profile") => Arg::ProfileUrl,
        Some("input") => {
            let n: i64 = t.raw_get("n")?;
            Arg::Input(usize::try_from(n - 1).map_err(|_| fail(lua, "rada.input counts from 1"))?)
        }
        _ => {
            return Err(fail(
                lua,
                "a table in args must be one of rada.out, rada.inputs, rada.input(n)…",
            ));
        }
    })
}
