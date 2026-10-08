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

impl Pattern {
    pub fn parse(src: &str) -> Result<Pattern, String> {
        if let Some(rest) = src.strip_prefix("s/") {
            let mut it = rest.splitn(3, '/');
            let (find, with) = (
                it.next().unwrap_or(""),
                it.next().ok_or("use s/find/replace/")?,
            );
            if find.is_empty() {
                return Err("nothing to search for".into());
            }
            return Ok(Pattern {
                kind: Kind::Replace {
                    find: find.into(),
                    with: with.into(),
                },
                start: 1,
                step: 1,
            });
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
                    let (head, arg) = match tok.split_once(':') {
                        Some((h, a)) => (h, Some(a)),
                        None => (tok.as_str(), None),
                    };
                    parts.push(match (head, arg) {
                        ("name", None) => Part::Name(CaseMode::Keep),
                        ("name", Some("lower")) => Part::Name(CaseMode::Lower),
                        ("name", Some("upper")) => Part::Name(CaseMode::Upper),
                        ("ext", None) => Part::Ext,
                        ("parent", None) => Part::Parent,
                        ("n", None) => Part::Counter { width: 1 },
                        ("n", Some(w)) => Part::Counter {
                            width: w.parse().map_err(|_| format!("bad width {w:?}"))?,
                        },
                        _ => return Err(format!("unknown token {{{tok}}}")),
                    });
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

    pub fn with_counter(mut self, start: u64, step: u64) -> Self {
        self.start = start;
        self.step = step.max(1);
        self
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
                            let n = self.start + self.step * index as u64;
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
    pub fn preview(&self, items: &[PathBuf]) -> Vec<Preview> {
        let rules = crate::platform::PathRules::POSIX;
        items
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let to = self
                    .render(i, p)
                    .and_then(|n| names::validate(&rules, &n).map(|_| n));
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
        let mut plan = Plan::empty(OpKind::BulkRename, format!("Rename {} items", items.len()));
        let mut ws = WarningSet::default();
        let rules = self.platform.path_rules();
        let key = |p: &Path| -> OsString {
            if rules.case_insensitive {
                OsString::from(p.as_os_str().to_string_lossy().to_lowercase())
            } else {
                p.as_os_str().to_os_string()
            }
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

        let sources: HashSet<OsString> = pairs.iter().map(|(f, _)| key(f)).collect();
        let mut seen: HashMap<OsString, &PathBuf> = HashMap::new();
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
                        from.with_file_name(format!(".vela-rename-{}-{n}", std::process::id()));
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
}
