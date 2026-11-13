//! Paths between rada and Lua. A Lua string is bytes: on Unix a path is its bytes as they are;
//! on Windows it must be Unicode, and one that is not is refused rather than guessed.

use std::path::{Path, PathBuf};

use mlua::{Lua, Value};

#[cfg(unix)]
pub(crate) fn to_lua(lua: &Lua, path: &Path) -> mlua::Result<mlua::LuaString> {
    use std::os::unix::ffi::OsStrExt;
    lua.create_string(path.as_os_str().as_bytes())
}

#[cfg(not(unix))]
pub(crate) fn to_lua(lua: &Lua, path: &Path) -> mlua::Result<mlua::LuaString> {
    match path.to_str() {
        Some(s) => lua.create_string(s),
        None => Err(mlua::Error::runtime(format!(
            "{} is not a name recipes can read",
            rada_core::display::path(path)
        ))),
    }
}

#[cfg(unix)]
pub(crate) fn from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
pub(crate) fn from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

/// A path given by the script, or why it is not one.
pub(crate) fn from_lua(value: &Value) -> Result<PathBuf, String> {
    match value {
        Value::String(s) if !s.as_bytes().is_empty() => {
            from_bytes(&s.as_bytes()).ok_or_else(|| "a path that is not Unicode".to_string())
        }
        Value::String(_) => Err("an empty path".into()),
        other => Err(format!("a path is a string, not {}", other.type_name())),
    }
}

/// The same, and it must be a whole path: a recipe never works relative to some folder.
pub(crate) fn absolute(value: &Value) -> Result<PathBuf, String> {
    let path = from_lua(value)?;
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!(
            "{} is not a whole path (rada.join(ctx.cwd, name) makes one)",
            rada_core::display::path(&path)
        ))
    }
}

/// One path or a list of them.
pub(crate) fn absolute_list(value: &Value) -> Result<Vec<PathBuf>, String> {
    match value {
        Value::Table(t) => {
            let mut out = Vec::new();
            for v in t.sequence_values::<Value>() {
                out.push(absolute(&v.map_err(|e| e.to_string())?)?);
            }
            if out.is_empty() {
                return Err("an empty list of paths".into());
            }
            Ok(out)
        }
        other => Ok(vec![absolute(other)?]),
    }
}
