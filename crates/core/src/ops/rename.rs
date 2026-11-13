//! Bulk rename: a small pattern language, a pure preview, and a plan that handles
//! swaps and chains (`a->b, b->a`) through temporary names.
//!
//! Patterns
//!   `{name}` stem            `{name:lower}` / `{name:upper}`
//!   `{ext}`  extension (no dot)
//!   `{n}`    counter         `{n:3}` zero-padded to 3 digits
//!   `{parent}` name of the containing folder
//!   `{exif.date:%Y-%m-%d}`, `{audio.track:02}`, `{pdf.title}`… what the file says about
//!   itself, `{mtime}` when it was last changed, `{exif.date|mtime}` the first that is there
//!   (see [`fields`])
//!   `{{` `}}` literal braces
//!   `s/find/replace/`  literal search-and-replace on the whole name

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::engine::Engine;
use super::names;
use super::plan::*;
use super::planner::seen_by_plan;
use super::scan::ScanControl;
use crate::fs::Fingerprint;
use crate::i18n::{Key, tf};
use crate::platform::PathRules;

mod fields;
mod preview;

pub use fields::Facts;
pub use preview::{RenamePreview, RenamePreviewer};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Lit(String),
    Name(CaseMode),
    Ext,
    Counter { width: usize },
    Parent,
    Field(fields::FieldPart),
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
    if fields::is_field(head) {
        return fields::parse(head, arg).map(Part::Field);
    }
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

pub(super) fn parse_width(w: &str) -> Result<usize, String> {
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

    /// The new name for the `index`-th item, knowing nothing of it but its path: a field of
    /// its metadata is then missing.
    pub fn render(&self, index: usize, path: &Path) -> Result<OsString, String> {
        self.render_with(index, path, &Facts::default())
    }

    /// The new name for the `index`-th item, with what is known of it.
    pub fn render_with(
        &self,
        index: usize,
        path: &Path,
        facts: &Facts,
    ) -> Result<OsString, String> {
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
                let mut out = OsString::new();
                for part in parts {
                    self.push_part(&mut out, part, index, path, facts)?;
                }
                Ok(out)
            }
        }
    }

    fn push_part(
        &self,
        out: &mut OsString,
        part: &Part,
        index: usize,
        path: &Path,
        facts: &Facts,
    ) -> Result<(), String> {
        let name = Path::new(path.file_name().unwrap_or_default());
        let stem = name.file_stem().unwrap_or(name.as_os_str());
        match part {
            Part::Lit(s) => out.push(s),
            Part::Name(mode) => match (mode, stem.to_str()) {
                (CaseMode::Lower, Some(t)) => out.push(t.to_lowercase()),
                (CaseMode::Upper, Some(t)) => out.push(t.to_uppercase()),
                _ => out.push(stem),
            },
            Part::Ext => out.push(name.extension().unwrap_or(OsStr::new(""))),
            Part::Counter { width } => {
                let n = self.counter_at(index)?;
                out.push(format!("{n:0width$}", width = *width));
            }
            Part::Parent => out.push(
                path.parent()
                    .and_then(Path::file_name)
                    .unwrap_or(OsStr::new("")),
            ),
            Part::Field(f) => out.push(f.render(facts)?),
        }
        Ok(())
    }

    fn fields(&self) -> impl Iterator<Item = &fields::FieldPart> {
        let parts = match &self.kind {
            Kind::Template(parts) => parts.as_slice(),
            Kind::Replace { .. } => &[],
        };
        parts.iter().filter_map(|p| match p {
            Part::Field(f) => Some(f),
            _ => None,
        })
    }

    /// Whether the names need what the files say about themselves, which takes reading them.
    pub fn needs_meta(&self) -> bool {
        self.fields().any(fields::FieldPart::needs_meta)
    }

    /// Whether the names need the time each item was last changed.
    pub fn needs_mtime(&self) -> bool {
        self.fields().any(fields::FieldPart::needs_mtime)
    }

    /// Pure preview (no I/O), with nothing known of the items but their paths.
    /// `rules` are the platform's, the ones the plan will check the names against.
    pub fn preview(&self, items: &[PathBuf], rules: &PathRules) -> Vec<Preview> {
        let none = Facts::default();
        items
            .iter()
            .enumerate()
            .map(|(i, p)| self.preview_one(i, p, &none, rules))
            .collect()
    }

    /// What the `index`-th item would be called, or why it cannot be.
    pub fn preview_one(
        &self,
        index: usize,
        path: &Path,
        facts: &Facts,
        rules: &PathRules,
    ) -> Preview {
        let to = self
            .render_with(index, path, facts)
            .and_then(|n| names::validate(rules, &n).map(|_| n));
        Preview {
            from: path.to_path_buf(),
            to,
        }
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

type Pairs = (
    Vec<(PathBuf, PathBuf)>,
    HashMap<PathBuf, Option<Fingerprint>>,
);

impl Engine {
    /// The new name of each item, and each item as the plan saw it. The fingerprint is taken
    /// before the file is read for its metadata: a file changed after that is refused when
    /// the plan runs.
    fn rename_pairs(
        &self,
        items: &[PathBuf],
        pattern: &Pattern,
        rules: &PathRules,
        ws: &mut WarningSet,
        mut ctl: ScanControl<'_>,
    ) -> Pairs {
        let mut pairs = Vec::new();
        let mut looked = HashMap::new();
        for (i, from) in items.iter().enumerate() {
            if ctl.cancel.load(Ordering::Relaxed) {
                break;
            }
            let Ok(meta) = self.fs.lstat(from) else {
                ws.add(WarningKind::Missing, Severity::Blocking, Some(from));
                continue;
            };
            looked.insert(from.clone(), seen_by_plan(from, &meta, items));
            let facts = self.facts_of(i, from, meta.mtime, pattern, &mut ctl);
            match pattern.preview_one(i, from, &facts, rules).to {
                Ok(new) if from.with_file_name(&new) != *from => {
                    pairs.push((from.clone(), from.with_file_name(new)))
                }
                Ok(_) => {}
                Err(why) => ws.add_with(
                    WarningKind::InvalidName,
                    Severity::Blocking,
                    Some(from),
                    Some(why),
                ),
            }
        }
        (pairs, looked)
    }

    /// What the pattern needs to know of `path`, and no more: a file is only read when a field
    /// of its metadata is in the pattern.
    fn facts_of(
        &self,
        index: usize,
        path: &Path,
        mtime: Option<std::time::SystemTime>,
        pattern: &Pattern,
        ctl: &mut ScanControl<'_>,
    ) -> Facts {
        let cancel = ctl.cancel;
        let stop = || cancel.load(Ordering::Relaxed);
        let meta = pattern.needs_meta().then(|| {
            let read = self.meta.read(path, &stop).1;
            (ctl.progress)(&super::scan::ScanProgress {
                files: index as u64 + 1,
                dirs: 0,
                bytes: 0,
                current: path.to_path_buf(),
            });
            read
        });
        Facts {
            meta,
            mtime: mtime.filter(|_| pattern.needs_mtime()),
        }
    }
}

impl Engine {
    pub fn plan_bulk_rename(&self, items: &[PathBuf], pattern: &Pattern) -> Plan {
        let never = std::sync::atomic::AtomicBool::new(false);
        let ctl = ScanControl {
            cancel: &never,
            progress: &mut |_| {},
        };
        self.plan_bulk_rename_with(items, pattern, ctl)
    }

    /// [`plan_bulk_rename`](Self::plan_bulk_rename), saying how far the reading of the files'
    /// metadata is and giving it up when `ctl` says so (the plan is then not to be used).
    pub fn plan_bulk_rename_with(
        &self,
        items: &[PathBuf],
        pattern: &Pattern,
        ctl: ScanControl<'_>,
    ) -> Plan {
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

        let (pairs, looked) = self.rename_pairs(items, pattern, &rules, &mut ws, ctl);

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
                    // Only the first phase looks at the item: after it the temporary name
                    // holds a file that was moved by this plan itself.
                    plan.steps.push(Step::Rename {
                        from: from.clone(),
                        to: tmp.clone(),
                        expect_from: looked.get(from).cloned().flatten(),
                    });
                    temps.push(tmp);
                }
                for (tmp, (_, to)) in temps.into_iter().zip(&pairs) {
                    plan.steps.push(Step::Rename {
                        from: tmp,
                        to: to.clone(),
                        expect_from: None,
                    });
                }
            } else {
                for (from, to) in &pairs {
                    plan.steps.push(Step::Rename {
                        from: from.clone(),
                        to: to.clone(),
                        expect_from: looked.get(from).cloned().flatten(),
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
    fn a_pattern_says_whether_the_files_have_to_be_read() {
        let p = Pattern::parse("{exif.date|mtime:%Y-%m-%d}_{n:3}.{ext}").unwrap();
        assert!(p.needs_meta() && p.needs_mtime());
        let p = Pattern::parse("{mtime:%Y}_{name}").unwrap();
        assert!(!p.needs_meta() && p.needs_mtime());
        let p = Pattern::parse("{name}_{n}").unwrap();
        assert!(!p.needs_meta() && !p.needs_mtime());
        assert!(!Pattern::parse("s/{exif.date}/x/").unwrap().needs_meta());
        // Knowing nothing of the file, its field is missing.
        let p = Pattern::parse("{audio.title}.{ext}").unwrap();
        let e = p.render(0, Path::new("/x/a.mp3")).unwrap_err();
        assert!(e.contains("{audio.title}"), "{e}");
        assert!(Pattern::parse("{name|mtime}").is_err());
        assert!(Pattern::parse("{exif.date:%B}").is_err());
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
