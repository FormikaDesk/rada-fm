//! Bulk rename: a small pattern language, a pure preview, and a plan that handles
//! swaps and chains (`a->b, b->a`) through temporary names.
//!
//! Patterns
//!   `{name}` stem            `{name:lower}` / `{name:upper}`
//!   `{ext}`  extension (no dot)
//!   `{n}`    counter         `{n:3}` zero-padded to 3 digits
//!   `{parent}` name of the containing folder
//!   `{{` `}}` literal braces
//!   `s/find/replace/`  literal search-and-replace on the whole name

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::engine::Engine;
use super::names;
use super::plan::*;
use crate::i18n::{Key, tf};
use crate::platform::PathRules;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Lit(String),
    Name(CaseMode),
    Ext,
    Counter { width: usize },
    Parent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaseMode {
    Keep,
    Lower,
    Upper,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Template(Vec<Part>),
    Replace { find: String, with: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    kind: Kind,
    pub start: u64,
    pub step: u64,
}

#[derive(Clone, Debug)]
pub struct Preview {
    pub from: PathBuf,
    pub to: Result<OsString, String>,
}

/// The widest zero-padded counter: a name never needs more, and the preview is recomputed on
/// every key, so a huge width must be refused before anything is formatted with it.
const MAX_WIDTH: usize = 255;

fn parse_token(tok: &str) -> Result<Part, String> {
    let (head, arg) = match tok.split_once(':') {
        Some((h, a)) => (h, Some(a)),
        None => (tok, None),
    };
    Ok(match (head, arg) {
        ("name", None) => Part::Name(CaseMode::Keep),
        ("name", Some("lower")) => Part::Name(CaseMode::Lower),
        ("name", Some("upper")) => Part::Name(CaseMode::Upper),
        ("ext", None) => Part::Ext,
        ("parent", None) => Part::Parent,
        ("n", None) => Part::Counter { width: 1 },
        ("n", Some(w)) => Part::Counter {
            width: parse_width(w)?,
        },
        _ => return Err(format!("unknown token {{{tok}}}")),
    })
}

fn parse_width(w: &str) -> Result<usize, String> {
    if w.is_empty() || !w.bytes().all(|b| b.is_ascii_digit()) {
        return Err(tf(Key::RenBadWidth, &[&format!("{w:?}")]));
    }
    // Digits that do not even fit a number are out of range too.
    match w.parse::<usize>() {
        Ok(n) if (1..=MAX_WIDTH).contains(&n) => Ok(n),
        _ => Err(tf(Key::RenWidthRange, &[&MAX_WIDTH, &w])),
    }
}

impl Pattern {
    pub fn parse(src: &str) -> Result<Pattern, String> {
        if let Some(rest) = src.strip_prefix("s/") {
            return Self::parse_replace(rest);
        }
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut chars = src.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    lit.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    lit.push('}');
                }
                '{' => {
                    let mut tok = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(ch) => tok.push(ch),
                            None => return Err("unclosed '{'".into()),
                        }
                    }
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(parse_token(&tok)?);
                }
                '}' => return Err("unmatched '}'".into()),
                c => lit.push(c),
            }
        }
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        if parts.is_empty() {
            return Err("the pattern is empty".into());
        }
        Ok(Pattern {
            kind: Kind::Template(parts),
            start: 1,
            step: 1,
        })
    }

    fn parse_replace(rest: &str) -> Result<Pattern, String> {
        let mut it = rest.splitn(3, '/');
        let (find, with) = (
            it.next().unwrap_or(""),
            it.next().ok_or("use s/find/replace/")?,
        );
        if find.is_empty() {
            return Err("nothing to search for".into());
        }
        Ok(Pattern {
            kind: Kind::Replace {
                find: find.into(),
                with: with.into(),
            },
            start: 1,
            step: 1,
        })
    }

    pub fn with_counter(mut self, start: u64, step: u64) -> Self {
        self.start = start;
        self.step = step.max(1);
        self
    }

    /// `start + step * index`, or an error if that is more than a counter can hold.
    fn counter_at(&self, index: usize) -> Result<u64, String> {
        (index as u64)
            .checked_mul(self.step)
            .and_then(|d| self.start.checked_add(d))
            .ok_or_else(|| Key::RenCounterTooLarge.tr().to_string())
    }

    /// The new name for the `index`-th item.
    pub fn render(&self, index: usize, path: &Path) -> Result<OsString, String> {
        let name = path.file_name().ok_or("no file name")?;
        match &self.kind {
            Kind::Replace { find, with } => {
                let text = name.to_string_lossy();
                if name.to_str().is_none() {
                    return Err("name is not valid UTF-8; use a {name} pattern instead".into());
                }
                Ok(OsString::from(text.replace(find.as_str(), with)))
            }
            Kind::Template(parts) => {
                let p = Path::new(name);
                let stem = p.file_stem().unwrap_or(name);
                let ext = p.extension().unwrap_or(OsStr::new(""));
                let mut out = OsString::new();
                for part in parts {
                    match part {
                        Part::Lit(s) => out.push(s),
                        Part::Name(CaseMode::Keep) => out.push(stem),
                        Part::Name(mode) => match stem.to_str() {
                            Some(t) => out.push(if *mode == CaseMode::Lower {
                                t.to_lowercase()
                            } else {
                                t.to_uppercase()
                            }),
                            None => out.push(stem),
                        },
                        Part::Ext => out.push(ext),
                        Part::Counter { width } => {
                            let n = self.counter_at(index)?;
                            out.push(format!("{n:0width$}", width = *width));
                        }
                        Part::Parent => {
                            out.push(
                                path.parent()
                                    .and_then(Path::file_name)
                                    .unwrap_or(OsStr::new("")),
                            );
                        }
                    }
                }
                Ok(out)
            }
        }
    }

    /// Pure preview (no I/O): safe to call on every keystroke.
    /// `rules` are the platform's, the ones the plan will check the names against.
    pub fn preview(&self, items: &[PathBuf], rules: &PathRules) -> Vec<Preview> {
        items
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let to = self
                    .render(i, p)
                    .and_then(|n| names::validate(rules, &n).map(|_| n));
                Preview {
                    from: p.clone(),
                    to,
                }
            })
            .collect()
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Engine {
    pub fn plan_bulk_rename(&self, items: &[PathBuf], pattern: &Pattern) -> Plan {
        let mut plan = Plan::empty(
            OpKind::BulkRename,
            crate::i18n::tf(
                crate::i18n::Key::TitleBulkRename,
                &[&crate::i18n::count(
                    items.len() as u64,
                    crate::i18n::Key::ItemOne,
                    crate::i18n::Key::ItemMany,
                )],
            ),
        );
        let mut ws = WarningSet::default();
        let rules = self.platform.path_rules();
        // Compared component by component: `d/2` and `d\2` are the same place on Windows.
        let key = |p: &Path| -> Vec<OsString> {
            p.components()
                .map(|c| {
                    if rules.case_insensitive {
                        OsString::from(c.as_os_str().to_string_lossy().to_lowercase())
                    } else {
                        c.as_os_str().to_os_string()
                    }
                })
                .collect()
        };

        let mut pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
        for (i, from) in items.iter().enumerate() {
            if self.fs.lstat(from).is_err() {
                ws.add(WarningKind::Missing, Severity::Blocking, Some(from));
                continue;
            }
            match pattern
                .render(i, from)
                .and_then(|n| names::validate(&rules, &n).map(|_| n))
            {
                Ok(new) => {
                    let to = from.with_file_name(new);
                    if to != *from {
                        pairs.push((from.clone(), to));
                    }
                }
                Err(why) => ws.add_with(
                    WarningKind::InvalidName,
                    Severity::Blocking,
                    Some(from),
                    Some(why),
                ),
            }
        }

        let sources: HashSet<Vec<OsString>> = pairs.iter().map(|(f, _)| key(f)).collect();
        let mut seen: HashMap<Vec<OsString>, &PathBuf> = HashMap::new();
        let mut staging = false;
        for (from, to) in &pairs {
            if seen.insert(key(to), from).is_some() {
                ws.add_with(
                    WarningKind::Conflict,
                    Severity::Blocking,
                    Some(to),
                    Some("two items would get the same name".into()),
                );
            }
            if sources.contains(&key(to)) {
                staging = true;
            } else if self.fs.lstat(to).is_ok() {
                // On a case-insensitive filesystem "a" -> "A" is the same entry.
                let same_entry = key(from) == key(to);
                if !same_entry {
                    ws.add(WarningKind::Conflict, Severity::Blocking, Some(to));
                } else {
                    staging = true;
                }
            }
        }

        if pairs.is_empty() && !ws.has_blocking() {
            ws.add_with(
                WarningKind::InvalidName,
                Severity::Blocking,
                None,
                Some("no name would change".into()),
            );
        }

        if !ws.has_blocking() {
            if staging {
                // Swaps and chains: everything goes through a unique temporary name first.
                let mut temps = Vec::new();
                for (from, _) in &pairs {
                    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
                    let tmp =
                        from.with_file_name(format!(".rada-rename-{}-{n}", std::process::id()));
                    plan.steps.push(Step::Rename {
                        from: from.clone(),
                        to: tmp.clone(),
                    });
                    temps.push(tmp);
                }
                for (tmp, (_, to)) in temps.into_iter().zip(&pairs) {
                    plan.steps.push(Step::Rename {
                        from: tmp,
                        to: to.clone(),
                    });
                }
            } else {
                for (from, to) in &pairs {
                    plan.steps.push(Step::Rename {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
            }
        }
        plan.totals.items = pairs.len() as u64;
        plan.title = format!(
            "Rename {} item{}",
            pairs.len(),
            if pairs.len() == 1 { "" } else { "s" }
        );
        plan.items = pairs
            .iter()
            .map(|(from, to)| ItemSummary {
                path: from.clone(),
                kind: crate::fs::FileKind::File,
                action: ItemAction::Rename,
                files: 0,
                dirs: 0,
                symlinks: 0,
                bytes: 0,
                target: Some(to.clone()),
            })
            .collect();
        plan.renames = pairs;
        plan.warnings = ws.finish();
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(p: &str, i: usize, path: &str) -> String {
        Pattern::parse(p)
            .unwrap()
            .render(i, Path::new(path))
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn templates() {
        assert_eq!(r("{name}_{n:3}.{ext}", 4, "/x/photo.jpg"), "photo_005.jpg");
        assert_eq!(r("{name:upper}.{ext}", 0, "/x/a.b.txt"), "A.B.txt");
        assert_eq!(r("{parent}-{n}", 1, "/x/v1.2/file.txt"), "v1.2-2");
        assert_eq!(r("{{literal}}", 0, "/x/f"), "{literal}");
        assert_eq!(r("s/IMG_/Photo_/", 0, "/x/IMG_001.jpg"), "Photo_001.jpg");
    }

    #[test]
    fn bad_patterns() {
        assert!(Pattern::parse("{nope}").is_err());
        assert!(Pattern::parse("{name").is_err());
        assert!(Pattern::parse("a}").is_err());
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("s/x").is_err());
    }

    #[test]
    fn a_counter_width_is_limited_before_anything_is_formatted() {
        for w in ["0", "256", "1000000000", "99999999999999999999999"] {
            let e = Pattern::parse(&format!("{{n:{w}}}")).unwrap_err();
            assert!(e.contains("between 1 and 255"), "{w}: {e}");
        }
        assert!(Pattern::parse("{n:1}").is_ok() && Pattern::parse("{n:255}").is_ok());
        let it = crate::i18n::with_lang(crate::i18n::Lang::It, || {
            Pattern::parse("{n:1000000000}").unwrap_err()
        });
        assert!(it.contains("tra 1 e 255"), "{it}");
    }

    #[test]
    fn a_counter_that_passes_u64_is_an_error_for_that_item() {
        let p = Pattern::parse("{n}").unwrap().with_counter(u64::MAX - 1, 1);
        assert_eq!(
            p.render(1, Path::new("/x/a")).unwrap(),
            "18446744073709551615"
        );
        let e = p.render(2, Path::new("/x/a")).unwrap_err();
        assert!(e.contains("too large"), "{e}");
        // The multiplication alone can pass the limit.
        let p = Pattern::parse("{n}").unwrap().with_counter(1, u64::MAX);
        assert!(p.render(2, Path::new("/x/a")).is_err());
        let it = crate::i18n::with_lang(crate::i18n::Lang::It, || {
            p.render(2, Path::new("/x/a")).unwrap_err()
        });
        assert!(it.contains("troppo grande"), "{it}");
    }
}
