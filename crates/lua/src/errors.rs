//! What went wrong in a script, as a sentence with its file and line.

use rada_core::plugin::{Limit, LoadProblem, RecipeError};

use crate::session::{FileName, Stop};

/// The message of an error, without what mlua adds around it (the traceback of a Rust call,
/// "runtime error: ").
pub(crate) fn message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::RuntimeError(m) => without_traceback(m),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::MemoryError(_) => "not enough memory".into(),
        mlua::Error::CallbackError { cause, .. } => message(cause),
        mlua::Error::WithContext { cause, .. } => message(cause),
        other => other.to_string(),
    }
}

/// Lua's traceback names C functions and frames a recipe's author does not know: the line
/// of the script is what helps.
fn without_traceback(m: &str) -> String {
    m.split("\nstack traceback:")
        .next()
        .unwrap_or(m)
        .to_string()
}

/// The line and the rest of a Lua message that starts with `label:line:`.
pub(crate) fn split_line(label: &str, text: &str) -> (Option<u32>, String) {
    let Some(rest) = text.strip_prefix(label).and_then(|r| r.strip_prefix(':')) else {
        return (None, text.trim().to_string());
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    match (
        digits.parse::<u32>(),
        rest[digits.len()..].strip_prefix(':'),
    ) {
        (Ok(line), Some(after)) => (Some(line), after.trim().to_string()),
        _ => (None, rest.trim().to_string()),
    }
}

/// The error a recipe ended with, as rada tells it.
pub(crate) fn recipe_error(e: &mlua::Error, stopped: Option<Stop>, name: &FileName) -> RecipeError {
    match stopped {
        Some(Stop::Cancelled) => return RecipeError::Cancelled,
        Some(Stop::Limit(limit)) => return RecipeError::Limit(limit),
        None => {}
    }
    if is_memory(e) {
        return RecipeError::Limit(Limit::Memory);
    }
    let (line, message) = split_line(&name.label, &message(e));
    RecipeError::Script {
        file: name.file.clone(),
        line,
        message,
    }
}

/// The same, for a file that is being read.
pub(crate) fn load_problem(e: &mlua::Error, stopped: Option<Stop>, name: &FileName) -> LoadProblem {
    match recipe_error(e, stopped, name) {
        RecipeError::Script {
            file,
            line,
            message,
        } => LoadProblem {
            file,
            line,
            message,
        },
        other => LoadProblem {
            file: name.file.clone(),
            line: None,
            message: other.to_string(),
        },
    }
}

pub(crate) fn is_memory(e: &mlua::Error) -> bool {
    match e {
        mlua::Error::MemoryError(_) => true,
        mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => {
            is_memory(cause)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_is_taken_from_the_front_of_the_message() {
        assert_eq!(
            split_line("a.lua", "a.lua:12: attempt to call a nil value"),
            (Some(12), "attempt to call a nil value".to_string())
        );
        assert_eq!(
            split_line("a.lua", "b.lua:3: x"),
            (None, "b.lua:3: x".to_string())
        );
        assert_eq!(split_line("a.lua", "a.lua: x"), (None, "x".to_string()));
    }
}
