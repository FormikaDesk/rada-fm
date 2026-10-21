//! Syntax highlighting for the preview of a text file: which parts of each line are a comment,
//! a string, a number, a keyword… and nothing more. The colours are the interface's to choose
//! from the theme in use; this module only says what each part is.
//!
//! The language is found from the name of the file (or the first line, for a script). syntect
//! does the parsing, with its bundled definitions and its pure Rust regex engine; TOML, which
//! they do not include, has a small tokenizer of its own. Only the beginning of a file is read
//! ([`MAX_BYTES`], and the lines the preview shows), and a language that is not known gives
//! nothing, so the preview stays as plain text.

use std::path::Path;
use std::sync::OnceLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};

use crate::display;

/// How much of a file is coloured: the first 64 KB.
pub const MAX_BYTES: usize = 64 * 1024;

/// What a part of a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Comment,
    String,
    /// A number, or a constant such as `true` and `null`.
    Number,
    Keyword,
    /// The name of a type or class, of an HTML tag.
    Type,
    Function,
    /// A macro, an attribute, a key of a table.
    Attribute,
}

/// A coloured part of a displayed line, in characters (`start..end`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

fn set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

enum Language {
    Toml,
    Syntect(&'static SyntaxReference),
}

fn language(name: &str, first_line: &str) -> Option<Language> {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    if ext.as_deref() == Some("toml") {
        return Some(Language::Toml);
    }
    let set = set();
    ext.as_deref()
        .and_then(|e| set.find_syntax_by_extension(e))
        .or_else(|| set.find_syntax_by_extension(name))
        .or_else(|| set.find_syntax_by_first_line(first_line))
        .filter(|s| s.name != "Plain Text")
        .map(Language::Syntect)
}

/// The kind of the part of a line under the scopes `stack`, if it is coloured.
fn classify(stack: &[Scope]) -> Option<Kind> {
    let names: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
    // A comment or a string colours all it holds, whatever is inside.
    for n in &names {
        if n.starts_with("comment") {
            return Some(Kind::Comment);
        }
        if n.starts_with("string") && !n.starts_with("string.regexp.") {
            return Some(Kind::String);
        }
    }
    for n in names.iter().rev() {
        let n = n.as_str();
        let kind = if n.starts_with("constant.numeric")
            || n.starts_with("constant.language")
            || n.starts_with("constant.other")
        {
            Kind::Number
        } else if n.starts_with("constant.character.escape") || n.starts_with("string") {
            Kind::String
        } else if n.starts_with("keyword.operator") || n.starts_with("keyword.control.import") {
            continue;
        } else if n.starts_with("entity.name.function.macro")
            || n.starts_with("support.macro")
            || n.starts_with("entity.other.attribute-name")
            || n.starts_with("meta.annotation")
            || n.starts_with("variable.annotation")
        {
            Kind::Attribute
        } else if n.starts_with("entity.name.function")
            || n.starts_with("support.function")
            || n.starts_with("variable.function")
        {
            Kind::Function
        } else if n.starts_with("entity.name.type")
            || n.starts_with("entity.name.class")
            || n.starts_with("entity.name.struct")
            || n.starts_with("entity.name.enum")
            || n.starts_with("entity.name.tag")
            || n.starts_with("entity.other.inherited-class")
            || n.starts_with("support.type")
            || n.starts_with("support.class")
        {
            Kind::Type
        } else if n.starts_with("keyword")
            || n.starts_with("storage")
            || n.starts_with("variable.language")
        {
            Kind::Keyword
        } else {
            continue;
        };
        return Some(kind);
    }
    None
}

/// Cuts a text into the lines of a preview and keeps, for each, its coloured parts.
pub struct Highlighter {
    kind: HighlightState,
    /// Bytes of text read so far.
    used: usize,
}

enum HighlightState {
    Syntect {
        parse: ParseState,
        stack: ScopeStack,
    },
    Toml,
}

impl Highlighter {
    /// For the file `name`, whose text starts with `first_line`; `None` for a language that
    /// is not known.
    pub fn new(name: &str, first_line: &str) -> Option<Highlighter> {
        let kind = match language(name, first_line)? {
            Language::Toml => HighlightState::Toml,
            Language::Syntect(syntax) => HighlightState::Syntect {
                parse: ParseState::new(syntax),
                stack: ScopeStack::new(),
            },
        };
        Some(Highlighter { kind, used: 0 })
    }

    /// The coloured parts of the next line, `raw` (without its end of line), as they are in
    /// the line once `display::line` has prepared it. A line past [`MAX_BYTES`] of text has
    /// none.
    pub fn line(&mut self, raw: &str) -> Vec<Token> {
        if self.used >= MAX_BYTES {
            return Vec::new();
        }
        self.used += raw.len() + 1;
        let parts = match &mut self.kind {
            HighlightState::Toml => toml_parts(raw),
            HighlightState::Syntect { parse, stack } => {
                let mut text = String::with_capacity(raw.len() + 1);
                text.push_str(raw);
                text.push('\n');
                let Ok(ops) = parse.parse_line(&text, set()) else {
                    return Vec::new();
                };
                let mut parts = Vec::new();
                let mut at = 0;
                let mut push = |stack: &ScopeStack, from: usize, to: usize| {
                    let to = to.min(raw.len());
                    if from < to
                        && let Some(kind) = classify(stack.as_slice())
                    {
                        parts.push((from, to, kind));
                    }
                };
                for (pos, op) in &ops {
                    push(stack, at, *pos);
                    at = *pos;
                    if stack.apply(&ScopeStackOp::clone(op)).is_err() {
                        return Vec::new();
                    }
                }
                push(stack, at, text.len());
                parts
            }
        };
        in_displayed_chars(raw, &parts)
    }
}

/// `parts` (byte ranges of `raw`) as character ranges of `display::line(raw, 4)`, with the
/// neighbours of one kind joined.
fn in_displayed_chars(raw: &str, parts: &[(usize, usize, Kind)]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let mut at = 0; // byte of `raw` reached
    let mut shown = 0; // characters of the displayed line before `at`
    for &(from, to, kind) in parts {
        if !raw.is_char_boundary(from) || !raw.is_char_boundary(to) || from < at {
            continue;
        }
        shown += display::line(&raw[at..from], 4).chars().count();
        let len = display::line(&raw[from..to], 4).chars().count();
        at = to;
        let (start, end) = (shown, shown + len);
        shown = end;
        match out.last_mut() {
            Some(last) if last.end == start && last.kind == kind => last.end = end,
            _ => out.push(Token { start, end, kind }),
        }
    }
    out
}

/// TOML: comments, strings, numbers and constants, table names and keys.
fn toml_parts(raw: &str) -> Vec<(usize, usize, Kind)> {
    let mut parts = Vec::new();
    let b = raw.as_bytes();
    let trimmed = raw.trim_start();
    let indent = raw.len() - trimmed.len();
    if trimmed.starts_with('[') {
        let close = if trimmed.starts_with("[[") { "]]" } else { "]" };
        let end = trimmed
            .find(close)
            .map_or(raw.len(), |i| indent + i + close.len());
        parts.push((indent, end, Kind::Attribute));
        return toml_rest(raw, end, parts);
    }
    // `key =`: the key (bare, or quoted) up to the equal sign.
    if let Some(eq) = raw.find('=') {
        let key = raw[..eq].trim_end();
        if !key.is_empty() && !key.trim_start().starts_with('#') {
            parts.push((indent, key.len(), Kind::Attribute));
            return toml_rest(raw, eq + 1, parts);
        }
    }
    let _ = b;
    toml_rest(raw, 0, parts)
}

/// The value part of a line from byte `from`: strings, comments, numbers, constants.
fn toml_rest(
    raw: &str,
    from: usize,
    mut parts: Vec<(usize, usize, Kind)>,
) -> Vec<(usize, usize, Kind)> {
    let b = raw.as_bytes();
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'#' => {
                parts.push((i, raw.len(), Kind::Comment));
                break;
            }
            q @ (b'"' | b'\'') => {
                let mut j = i + 1;
                while j < b.len() && b[j] != q {
                    if q == b'"' && b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let end = (j + 1).min(raw.len());
                parts.push((i, end, Kind::String));
                i = end;
            }
            c if c.is_ascii_digit()
                || ((c == b'-' || c == b'+') && b.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                let mut j = i + 1;
                while j < b.len()
                    && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'.' | b'_' | b':' | b'-'))
                {
                    j += 1;
                }
                parts.push((i, j, Kind::Number));
                i = j;
            }
            c if c.is_ascii_alphabetic() => {
                let mut j = i + 1;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                if matches!(&raw[i..j], "true" | "false" | "inf" | "nan") {
                    parts.push((i, j, Kind::Number));
                }
                i = j;
            }
            _ => i += 1,
        }
    }
    parts.sort_by_key(|p| p.0);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(name: &str, text: &str) -> Vec<Vec<(String, Kind)>> {
        let mut h = Highlighter::new(name, text.lines().next().unwrap_or("")).expect("known");
        text.lines()
            .map(|l| {
                let shown = display::line(l, 4);
                h.line(l)
                    .iter()
                    .map(|t| {
                        (
                            shown.chars().skip(t.start).take(t.end - t.start).collect(),
                            t.kind,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn rust_is_read_as_rust() {
        let k = kinds(
            "main.rs",
            "fn main() { let x = 42; // go\n    println!(\"hi\");\n}\n",
        );
        assert!(k[0].contains(&("fn".into(), Kind::Keyword)), "{:?}", k[0]);
        assert!(k[0].contains(&("42".into(), Kind::Number)), "{:?}", k[0]);
        assert!(
            k[0].iter()
                .any(|(s, c)| *c == Kind::Comment && s.contains("go"))
        );
        assert!(
            k[1].iter()
                .any(|(s, c)| *c == Kind::String && s.contains("hi")),
            "{:?}",
            k[1]
        );
    }

    #[test]
    fn toml_has_keys_strings_numbers_and_comments() {
        let k = kinds(
            "Cargo.toml",
            "[package]\nname = \"rada\" # the name\nversion = 12\nok = true\n",
        );
        assert_eq!(k[0], [("[package]".into(), Kind::Attribute)]);
        assert!(k[1].contains(&("name".into(), Kind::Attribute)));
        assert!(k[1].contains(&("\"rada\"".into(), Kind::String)));
        assert!(k[1].contains(&("# the name".into(), Kind::Comment)));
        assert!(k[2].contains(&("12".into(), Kind::Number)));
        assert!(k[3].contains(&("true".into(), Kind::Number)));
    }

    #[test]
    fn unknown_languages_give_nothing() {
        assert!(Highlighter::new("notes.txt", "hello").is_none());
        assert!(Highlighter::new("data.zzzz", "hello").is_none());
        assert!(Highlighter::new("README", "just words").is_none());
    }

    #[test]
    fn a_script_is_known_by_its_first_line() {
        assert!(Highlighter::new("run", "#!/bin/sh").is_some());
    }

    #[test]
    fn tabs_move_the_coloured_parts() {
        let k = kinds("a.py", "\tx = 'a'\n");
        assert_eq!(
            k[0].last().unwrap(),
            &("'a'".to_string(), Kind::String),
            "{k:?}"
        );
    }

    #[test]
    fn only_the_start_of_a_big_file_is_read() {
        let line = "let value = 12345; // a comment to fill the line\n";
        let mut h = Highlighter::new("a.rs", line).unwrap();
        let mut coloured = 0;
        let n = 2 * MAX_BYTES / line.len();
        for _ in 0..n {
            if !h.line(line.trim_end()).is_empty() {
                coloured += 1;
            }
        }
        assert!(coloured > 0 && coloured < n, "{coloured} of {n}");
    }
}
