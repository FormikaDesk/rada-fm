//! The functions that look: `rada.list`, `rada.stat`, `rada.meta` and `rada.has`. None of them
//! changes anything, follows a link into a folder, or reads more of a file than its
//! metadata needs.

use std::path::Path;
use std::time::UNIX_EPOCH;

use mlua::{Lua, Table, Value};
use rada_core::meta::Meta;

use crate::api::{Shared, checkpoint, fail};
use crate::paths;

pub(crate) fn install(lua: &Lua, rada: &Table) -> mlua::Result<()> {
    rada.raw_set("list", lua.create_function(list)?)?;
    rada.raw_set("stat", lua.create_function(stat)?)?;
    rada.raw_set("meta", lua.create_function(meta)?)?;
    rada.raw_set("has", lua.create_function(has)?)
}

fn kind_word(m: &std::fs::Metadata) -> &'static str {
    let t = m.file_type();
    if t.is_symlink() {
        "link"
    } else if t.is_dir() {
        "folder"
    } else if t.is_file() {
        "file"
    } else {
        "other"
    }
}

/// `{ path, name, kind, size, mtime }` of what is at `path`, a link not followed.
fn entry(lua: &Lua, path: &Path, m: &std::fs::Metadata) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("path", paths::to_lua(lua, path)?)?;
    if let Some(name) = path.file_name() {
        t.raw_set("name", paths::to_lua(lua, Path::new(name))?)?;
    }
    t.raw_set("kind", kind_word(m))?;
    t.raw_set("size", m.len() as i64)?;
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    if let Some(d) = mtime {
        t.raw_set("mtime", d.as_secs() as i64)?;
    }
    Ok(t)
}

fn stat(lua: &Lua, path: Value) -> mlua::Result<(Value, Option<String>)> {
    checkpoint(lua)?;
    let path = paths::absolute(&path).map_err(|e| fail(lua, e))?;
    match std::fs::symlink_metadata(&path) {
        Ok(m) => Ok((Value::Table(entry(lua, &path, &m)?), None)),
        Err(e) => Ok((Value::Nil, Some(e.to_string()))),
    }
}

/// What is in a folder, sorted by name, at most `limits.list_entries` of it; the second value
/// says whether there was more.
fn list(lua: &Lua, dir: Value) -> mlua::Result<(Value, Value)> {
    checkpoint(lua)?;
    let dir = paths::absolute(&dir).map_err(|e| fail(lua, e))?;
    let max = lua
        .app_data_ref::<Shared>()
        .map_or(0, |s| s.limits.list_entries);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => return Ok((Value::Nil, Value::String(lua.create_string(e.to_string())?))),
    };
    let mut found = Vec::new();
    let mut more = false;
    for e in entries.flatten() {
        if found.len() == max {
            more = true;
            break;
        }
        found.push(e.path());
        if found.len() % 256 == 0 {
            checkpoint(lua)?;
        }
    }
    found.sort();
    let out = lua.create_table()?;
    for p in found {
        if let Ok(m) = std::fs::symlink_metadata(&p) {
            out.raw_push(entry(lua, &p, &m)?)?;
        }
    }
    Ok((Value::Table(out), Value::Boolean(more)))
}

fn has(lua: &Lua, name: String) -> mlua::Result<bool> {
    checkpoint(lua)?;
    let shared = lua
        .app_data_ref::<Shared>()
        .expect("the state was made with it");
    if let Some(known) = shared.has.borrow().get(&name) {
        return Ok(*known);
    }
    let found = shared.tools.programs.resolve(&name).is_ok();
    shared.has.borrow_mut().insert(name, found);
    Ok(found)
}

fn meta(lua: &Lua, path: Value) -> mlua::Result<Table> {
    checkpoint(lua)?;
    let path = paths::absolute(&path).map_err(|e| fail(lua, e))?;
    let (reader, guard) = {
        let shared = lua
            .app_data_ref::<Shared>()
            .expect("the state was made with it");
        (shared.tools.meta.clone(), shared.guard.clone())
    };
    let read = match reader {
        Some(r) => r.read(&path, &|| guard.should_stop()).1,
        None => std::sync::Arc::new(Meta::default()),
    };
    checkpoint(lua)?;
    meta_table(lua, &read)
}

fn meta_table(lua: &Lua, m: &Meta) -> mlua::Result<Table> {
    let exif = lua.create_table()?;
    exif.raw_set("date", m.exif.date.map(|d| d.to_string()))?;
    exif.raw_set(
        "time",
        m.exif.time.map(|t| t.strftime("%H:%M:%S").to_string()),
    )?;
    exif.raw_set("camera", m.exif.camera.clone())?;
    exif.raw_set("orientation", m.exif.orientation)?;
    exif.raw_set("place", m.exif.place.clone())?;
    if let Some(g) = m.exif.gps {
        let gps = lua.create_table()?;
        gps.raw_set("lat", g.lat)?;
        gps.raw_set("lon", g.lon)?;
        exif.raw_set("gps", gps)?;
    }
    let audio = lua.create_table()?;
    audio.raw_set("artist", m.audio.artist.clone())?;
    audio.raw_set("title", m.audio.title.clone())?;
    audio.raw_set("album", m.audio.album.clone())?;
    audio.raw_set("track", m.audio.track)?;
    audio.raw_set("year", m.audio.year)?;
    let pdf = lua.create_table()?;
    pdf.raw_set("title", m.pdf.title.clone())?;
    pdf.raw_set("author", m.pdf.author.clone())?;
    pdf.raw_set("pages", m.pdf.pages)?;
    let video = lua.create_table()?;
    video.raw_set("duration", m.video.duration.map(|d| d.as_secs_f64()))?;
    video.raw_set("width", m.video.width)?;
    video.raw_set("height", m.video.height)?;
    let t = lua.create_table()?;
    t.raw_set("exif", exif)?;
    t.raw_set("audio", audio)?;
    t.raw_set("pdf", pdf)?;
    t.raw_set("video", video)?;
    Ok(t)
}
