//! `rada.match`, `rada.replace` and `rada.split`: regular expressions of the `regex` crate in
//! place of Lua patterns. They run in time proportional to the text, whatever the pattern, so
//! no pattern can hang a recipe; their size and what they make are bounded too.

use std::collections::HashMap;

use mlua::{Lua, MultiValue, Value};
use regex::bytes::{Regex, RegexBuilder};

/// The longest pattern.
const MAX_PATTERN: usize = 4096;
/// How big a compiled pattern may grow.
const MAX_COMPILED: usize = 1 << 20;
/// The longest text `replace` may make, and the most pieces `split` may give.
const MAX_OUTPUT: usize = 4 << 20;
const MAX_PIECES: usize = 100_000;
/// Patterns kept compiled during one run.
const MAX_CACHED: usize = 64;

#[derive(Default)]
pub(crate) struct Patterns {
    compiled: HashMap<Vec<u8>, Regex>,
}

impl Patterns {
    pub(crate) fn get(&mut self, pattern: &[u8]) -> Result<Regex, String> {
        if let Some(re) = self.compiled.get(pattern) {
            return Ok(re.clone());
        }
        if pattern.len() > MAX_PATTERN {
            return Err(format!("a pattern is at most {MAX_PATTERN} bytes"));
        }
        let text = std::str::from_utf8(pattern).map_err(|_| "a pattern must be UTF-8")?;
        let re = RegexBuilder::new(text)
            .size_limit(MAX_COMPILED)
            .dfa_size_limit(MAX_COMPILED)
            .nest_limit(64)
            .build()
            .map_err(|e| format!("bad pattern: {e}"))?;
        if self.compiled.len() >= MAX_CACHED {
            self.compiled.clear();
        }
        self.compiled.insert(pattern.to_vec(), re.clone());
        Ok(re)
    }
}

/// The groups of the first match (the whole match when there are none), or nil.
pub(crate) fn find(lua: &Lua, re: &Regex, text: &[u8]) -> mlua::Result<MultiValue> {
    let Some(caps) = re.captures(text) else {
        return Ok(MultiValue::from_vec(vec![Value::Nil]));
    };
    let groups: Vec<Option<regex::bytes::Match>> = if caps.len() > 1 {
        caps.iter().skip(1).collect()
    } else {
        vec![caps.get(0)]
    };
    let mut out = Vec::new();
    for g in groups {
        out.push(match g {
            Some(m) => Value::String(lua.create_string(m.as_bytes())?),
            None => Value::Nil,
        });
    }
    Ok(MultiValue::from_vec(out))
}

/// `text` with every match replaced (`$1`, `${name}` stand for the groups), and how many.
pub(crate) fn replace(re: &Regex, text: &[u8], with: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut out = Vec::new();
    let mut last = 0;
    let mut count = 0;
    for caps in re.captures_iter(text) {
        let whole = caps.get(0).expect("group 0 is always there");
        out.extend_from_slice(&text[last..whole.start()]);
        caps.expand(with, &mut out);
        last = whole.end();
        count += 1;
        if out.len() > MAX_OUTPUT {
            return Err(format!("the text made is over {} MiB", MAX_OUTPUT >> 20));
        }
    }
    out.extend_from_slice(&text[last..]);
    if out.len() > MAX_OUTPUT {
        return Err(format!("the text made is over {} MiB", MAX_OUTPUT >> 20));
    }
    Ok((out, count))
}

/// The pieces of `text` between the matches.
pub(crate) fn split<'t>(re: &Regex, text: &'t [u8]) -> Result<Vec<&'t [u8]>, String> {
    let mut pieces = Vec::new();
    for piece in re.split(text) {
        if pieces.len() == MAX_PIECES {
            return Err(format!("more than {MAX_PIECES} pieces"));
        }
        pieces.push(piece);
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_that_would_backtrack_for_ever_answers_at_once() {
        let mut p = Patterns::default();
        let re = p
            .get(b"(a*)*(a*)*(a*)*(a*)*(a*)*(a*)*(a*)*(a*)*(a*)*(a*)*c")
            .unwrap();
        let text = vec![b'a'; 10_000];
        let start = std::time::Instant::now();
        assert!(!re.is_match(&text));
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn replace_and_split_are_bounded() {
        let mut p = Patterns::default();
        let re = p.get(b"(o)").unwrap();
        assert_eq!(
            replace(&re, b"foo", b"[$1]").unwrap(),
            (b"f[o][o]".to_vec(), 2)
        );
        let every = p.get(b"").unwrap();
        let big = vec![b'x'; 1 << 20];
        assert!(replace(&every, &big, b"yyyyyyyy").is_err());
        let comma = p.get(b",").unwrap();
        assert_eq!(split(&comma, b"a,b,,c").unwrap().len(), 4);
        assert!(p.get(&vec![b'a'; MAX_PATTERN + 1]).is_err());
        assert!(p.get(b"(").is_err());
    }
}
