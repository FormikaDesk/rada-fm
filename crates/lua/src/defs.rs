//! What a recipe says about itself in `rada.recipe { … }`, read once when its file is loaded,
//! and the check of `when` against the files chosen, made without running any Lua.

use std::collections::BTreeMap;
use std::path::Path;

use mlua::{Table, Value};
use rada_core::i18n::{self, Key, tf};
use rada_core::plugin::{ArgKind, ArgValue, Availability, RecipeArg, RecipeContext};

/// A text in several languages: `"Merge"` or `{ en = "Merge", it = "Unisci" }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Words(BTreeMap<String, String>);

impl Words {
    /// The text in the language of the interface, else the English one, else any.
    pub(crate) fn get(&self) -> String {
        let code = i18n::lang().code();
        self.0
            .get(code)
            .or_else(|| self.0.get("en"))
            .or_else(|| self.0.values().next())
            .cloned()
            .unwrap_or_default()
    }

    fn read(value: Value, what: &str) -> Result<Words, String> {
        let mut out = BTreeMap::new();
        match value {
            Value::Nil => {}
            Value::String(s) => {
                out.insert("en".into(), clean(&s.to_string_lossy()));
            }
            Value::Table(t) => {
                for kv in t.pairs::<String, String>() {
                    let (lang, text) =
                        kv.map_err(|_| format!("{what}: languages and texts are strings"))?;
                    out.insert(lang, clean(&text));
                }
            }
            other => return Err(format!("{what} is a string, not {}", other.type_name())),
        }
        Ok(Words(out))
    }
}

/// Text from a script goes on screen: the same cleaning as every outside text.
fn clean(text: &str) -> String {
    rada_core::display::text(text).chars().take(200).collect()
}

/// What each of the files chosen must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemKind {
    File,
    Folder,
    Any,
}

/// `when = { ext, kind, min, max, dir }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct When {
    /// Extensions without the dot, in small letters; empty: any.
    pub ext: Vec<String>,
    pub kind: ItemKind,
    pub min: usize,
    pub max: Option<usize>,
    /// The recipe is for the folder shown: it fits with nothing chosen.
    pub dir: bool,
}

impl Default for When {
    fn default() -> When {
        When {
            ext: Vec::new(),
            kind: ItemKind::File,
            min: 1,
            max: None,
            dir: false,
        }
    }
}

/// One recipe, as its file declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecipeDef {
    pub name: String,
    pub title: Words,
    pub description: Words,
    pub when: When,
    pub args: Vec<ArgDef>,
    /// It has an `available` function, asked when the menu is made.
    pub asks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArgDef {
    pub name: String,
    pub label: Words,
    pub kind: ArgKind,
    pub default: Option<ArgValue>,
}

impl ArgDef {
    pub(crate) fn public(&self) -> RecipeArg {
        RecipeArg {
            name: self.name.clone(),
            label: self.label.get(),
            kind: self.kind.clone(),
            default: self.default.clone(),
        }
    }
}

/// A name a recipe or an argument can have: small letters, digits, `-` and `_`.
fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Read the declaration; the `run` function must be there.
pub(crate) fn read_def(t: &Table) -> Result<RecipeDef, String> {
    let name: String = t.get("name").map_err(|_| "name must be a string")?;
    if !is_name(&name) {
        return Err(format!(
            "{name:?} is not a recipe name (small letters, digits, - and _)"
        ));
    }
    let field = |k: &str| t.get::<Value>(k).map_err(|e| e.to_string());
    if !matches!(field("run")?, Value::Function(_)) {
        return Err(format!("{name}: run must be a function"));
    }
    let asks = match field("available")? {
        Value::Nil => false,
        Value::Function(_) => true,
        _ => return Err(format!("{name}: available must be a function")),
    };
    let mut title = Words::read(field("title")?, "title")?;
    if title.0.is_empty() {
        title.0.insert("en".into(), name.clone());
    }
    Ok(RecipeDef {
        description: Words::read(field("description")?, "description")?,
        when: read_when(field("when")?).map_err(|e| format!("{name}: when: {e}"))?,
        args: read_args(field("args")?).map_err(|e| format!("{name}: args: {e}"))?,
        name,
        title,
        asks,
    })
}

fn read_when(value: Value) -> Result<When, String> {
    let mut when = When::default();
    let t = match value {
        Value::Nil => return Ok(when),
        Value::Table(t) => t,
        other => return Err(format!("a table, not {}", other.type_name())),
    };
    let get = |k: &str| t.get::<Value>(k).map_err(|e| e.to_string());
    when.ext = match get("ext")? {
        Value::Nil => Vec::new(),
        Value::String(s) => vec![ext_word(&s.to_string_lossy())],
        Value::Table(list) => list
            .sequence_values::<String>()
            .map(|e| {
                e.map(|s| ext_word(&s))
                    .map_err(|_| "ext lists strings".to_string())
            })
            .collect::<Result<_, _>>()?,
        _ => return Err("ext is a string or a list of strings".into()),
    };
    when.kind = match get("kind")? {
        Value::Nil => ItemKind::File,
        Value::String(s) => match s.to_string_lossy().as_str() {
            "file" => ItemKind::File,
            "folder" | "dir" => ItemKind::Folder,
            "any" => ItemKind::Any,
            other => return Err(format!("kind is file, folder or any, not {other:?}")),
        },
        _ => return Err("kind is a string".into()),
    };
    when.min = count(get("min")?, "min")?.unwrap_or(1);
    when.max = count(get("max")?, "max")?;
    when.dir = matches!(get("dir")?, Value::Boolean(true));
    if when.max.is_some_and(|m| m < when.min) {
        return Err("max is less than min".into());
    }
    Ok(when)
}

fn ext_word(s: &str) -> String {
    s.trim_start_matches('.').to_lowercase()
}

fn count(value: Value, what: &str) -> Result<Option<usize>, String> {
    match value {
        Value::Nil => Ok(None),
        Value::Integer(n) if n >= 0 => Ok(Some(n as usize)),
        _ => Err(format!("{what} is a whole number, 0 or more")),
    }
}

fn read_args(value: Value) -> Result<Vec<ArgDef>, String> {
    let list = match value {
        Value::Nil => return Ok(Vec::new()),
        Value::Table(t) => t,
        other => return Err(format!("a list, not {}", other.type_name())),
    };
    let mut out: Vec<ArgDef> = Vec::new();
    for item in list.sequence_values::<Table>() {
        let arg = read_arg(&item.map_err(|_| "each argument is a table".to_string())?)?;
        if out.iter().any(|a| a.name == arg.name) {
            return Err(format!("{} is there twice", arg.name));
        }
        out.push(arg);
    }
    Ok(out)
}

fn read_arg(t: &Table) -> Result<ArgDef, String> {
    let name: String = t.get("name").map_err(|_| "an argument needs a name")?;
    if !is_name(&name) {
        return Err(format!("{name:?} is not an argument name"));
    }
    let get = |k: &str| t.get::<Value>(k).map_err(|e| e.to_string());
    let kind = read_kind(&get("type")?, &get("choices")?, &get("min")?, &get("max")?)
        .map_err(|e| format!("{name}: {e}"))?;
    let default = match get("default")? {
        Value::Nil => None,
        v => Some(arg_value(&kind, &v).map_err(|e| format!("{name}: default: {e}"))?),
    };
    let mut label = Words::read(get("label")?, "label")?;
    if label.0.is_empty() {
        label.0.insert("en".into(), name.clone());
    }
    Ok(ArgDef {
        name,
        label,
        kind,
        default,
    })
}

fn read_kind(ty: &Value, choices: &Value, min: &Value, max: &Value) -> Result<ArgKind, String> {
    let ty = match ty {
        Value::Nil if !choices.is_nil() => "choice".to_string(),
        Value::Nil => "text".to_string(),
        Value::String(s) => s.to_string_lossy(),
        _ => return Err("type is a string".into()),
    };
    let int = |v: &Value, d: i64| match v {
        Value::Nil => Ok(d),
        Value::Integer(n) => Ok(*n),
        _ => Err("min and max are whole numbers".to_string()),
    };
    Ok(match ty.as_str() {
        "text" => ArgKind::Text,
        "flag" => ArgKind::Flag,
        "int" => ArgKind::Int {
            min: int(min, i64::MIN)?,
            max: int(max, i64::MAX)?,
        },
        "choice" => {
            let Value::Table(t) = choices else {
                return Err("a choice needs its choices".into());
            };
            let words: Vec<String> = t
                .sequence_values::<String>()
                .collect::<Result<_, _>>()
                .map_err(|_| "choices are strings")?;
            if words.is_empty() {
                return Err("a choice needs its choices".into());
            }
            ArgKind::Choice(words)
        }
        other => return Err(format!("type is text, choice, int or flag, not {other:?}")),
    })
}

/// A value from Lua as the argument takes it.
pub(crate) fn arg_value(kind: &ArgKind, v: &Value) -> Result<ArgValue, String> {
    let value = match (kind, v) {
        (ArgKind::Text | ArgKind::Choice(_), Value::String(s)) => {
            ArgValue::Text(s.to_string_lossy())
        }
        (ArgKind::Int { .. }, Value::Integer(n)) => ArgValue::Int(*n),
        (ArgKind::Flag, Value::Boolean(b)) => ArgValue::Flag(*b),
        _ => return Err(format!("a {} is not a value for it", v.type_name())),
    };
    let probe = RecipeArg {
        name: String::new(),
        label: String::new(),
        kind: kind.clone(),
        default: None,
    };
    if probe.accepts(&value) {
        Ok(value)
    } else {
        Err("not one of the values it takes".into())
    }
}

/// Whether the recipe is for these files: `None` when it is not (it is not shown), else
/// whether it can run.
pub(crate) fn fits(when: &When, ctx: &RecipeContext) -> Option<Availability> {
    if ctx.selection.is_empty() {
        return when.dir.then_some(Availability::Available);
    }
    if !ctx.selection.iter().all(|p| item_fits(when, p)) {
        return None;
    }
    let n = ctx.selection.len();
    if n < when.min {
        let need = tf(Key::RecipeNeedsAtLeast, &[&when.min.to_string()]);
        return Some(Availability::Disabled(need));
    }
    if let Some(max) = when.max.filter(|m| n > *m) {
        let why = if max == 1 {
            Key::RecipeNeedsOne.in_lang(i18n::lang()).to_string()
        } else {
            tf(Key::RecipeNeedsAtMost, &[&max.to_string()])
        };
        return Some(Availability::Disabled(why));
    }
    Some(Availability::Available)
}

fn item_fits(when: &When, path: &Path) -> bool {
    let Ok(m) = std::fs::metadata(path) else {
        return false;
    };
    let kind_ok = match when.kind {
        ItemKind::File => m.is_file(),
        ItemKind::Folder => m.is_dir(),
        ItemKind::Any => true,
    };
    let ext_ok = when.ext.is_empty()
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| when.ext.contains(&e.to_lowercase()));
    kind_ok && ext_ok
}
