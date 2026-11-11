//! Several plans made into one: one window, one entry in the journal, one undo.
//!
//! The plans were made one by one, each from the disk as it is now. Run one after the other
//! they must not get in each other's way, so the merge refuses two that write the same name,
//! and one that reads what another removes or makes: such a plan would run on a disk that is
//! not the one it was made from.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::plan::*;
use crate::display;

/// The paths a plan writes, removes and reads.
#[derive(Default)]
struct Touches {
    writes: Vec<PathBuf>,
    removes: Vec<PathBuf>,
    reads: Vec<PathBuf>,
}

fn touches(plan: &Plan) -> Touches {
    let mut t = Touches::default();
    for step in &plan.steps {
        t.writes.extend(step.destination().map(Path::to_path_buf));
        note_sources(step, &mut t);
    }
    t
}

/// What `step` reads, and what it takes away.
fn note_sources(step: &Step, t: &mut Touches) {
    match step {
        Step::CopyFile {
            src, remove_source, ..
        }
        | Step::CopySymlink {
            src, remove_source, ..
        } => {
            t.reads.push(src.clone());
            if *remove_source {
                t.removes.push(src.clone());
            }
        }
        Step::HardLink { src_link, .. } => t.removes.extend(src_link.clone()),
        Step::ExtractFile { archive, .. } => t.reads.push(archive.clone()),
        Step::Compress { items, .. } => t.reads.extend(items.iter().map(|i| i.src.clone())),
        Step::ProduceFile { inputs, .. } => t.reads.extend(inputs.iter().map(|i| i.path.clone())),
        Step::Rename { from, .. } => {
            t.reads.push(from.clone());
            t.removes.push(from.clone());
        }
        Step::TrashItem { path, .. } | Step::RemoveFile { path, .. } | Step::RemoveDir { path } => {
            t.reads.push(path.clone());
            t.removes.push(path.clone());
        }
        Step::SetTags { path, .. } => t.reads.push(path.clone()),
        Step::MakeDir { .. }
        | Step::MakeFile { .. }
        | Step::FinishDir { .. }
        | Step::ExtractSymlink { .. }
        | Step::Restore { .. } => {}
    }
}

/// The first of `path` and the folders it is in that `owner` gives to another plan than
/// `part`.
fn taken_by_another<'a>(
    path: &'a Path,
    part: usize,
    owner: &HashMap<&Path, usize>,
) -> Option<&'a Path> {
    path.ancestors()
        .find(|a| owner.get(a).is_some_and(|p| *p != part))
}

/// Why the plans cannot be run one after the other, each said once.
fn clashes(parts: &[Plan]) -> Vec<String> {
    let all: Vec<Touches> = parts.iter().map(touches).collect();
    let mut out = Vec::new();
    let mut written: HashMap<&Path, usize> = HashMap::new();
    let mut removed: HashMap<&Path, usize> = HashMap::new();
    for (part, t) in all.iter().enumerate() {
        for w in &t.writes {
            if written.insert(w, part).is_some_and(|p| p != part) {
                out.push(format!("two requests write {}", display::path(w)));
            }
        }
        for r in &t.removes {
            removed.entry(r).or_insert(part);
        }
    }
    for (part, t) in all.iter().enumerate() {
        for r in &t.reads {
            if let Some(gone) = taken_by_another(r, part, &removed) {
                let (r, gone) = (display::path(r), display::path(gone));
                out.push(format!("one request reads {r}, another removes {gone}"));
            } else if let Some(made) = taken_by_another(r, part, &written) {
                let (r, made) = (display::path(r), display::path(made));
                out.push(format!("one request reads {r}, another makes {made}"));
            }
        }
    }
    out.dedup();
    out
}

/// The warnings of all the plans, the same one said once.
fn merged_warnings(parts: &[Plan], clashes: Vec<String>) -> Vec<Warning> {
    let mut out: Vec<Warning> = Vec::new();
    for w in parts.iter().flat_map(|p| &p.warnings) {
        match out
            .iter_mut()
            .find(|o| o.kind == w.kind && o.severity == w.severity && o.message == w.message)
        {
            Some(same) => same.examples.extend(w.examples.iter().cloned()),
            None => out.push(w.clone()),
        }
    }
    for message in clashes {
        out.push(Warning {
            kind: WarningKind::Other,
            severity: Severity::Blocking,
            message,
            count: 1,
            examples: Vec::new(),
        });
    }
    for w in &mut out {
        w.examples.dedup();
        w.examples.truncate(5);
    }
    out.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.kind.cmp(&b.kind)));
    out
}

fn add_totals(sum: &mut Totals, t: &Totals) {
    sum.items += t.items;
    sum.files += t.files;
    sum.dirs += t.dirs;
    sum.symlinks += t.symlinks;
    sum.bytes += t.bytes;
}

impl Plan {
    /// One plan that runs `parts` one after the other. It cannot be run if the parts get in
    /// each other's way (see the module documentation); the warnings say which.
    pub fn merge(parts: Vec<Plan>, title: impl Into<String>) -> Plan {
        let mut plan = Plan::empty(OpKind::Action, title);
        plan.warnings = merged_warnings(&parts, clashes(&parts));
        plan.reversible = parts.iter().all(|p| p.reversible);
        plan.destination = parts.iter().find_map(|p| p.destination.clone());
        for part in parts {
            add_totals(&mut plan.totals, &part.totals);
            plan.steps.extend(part.steps);
            plan.items.extend(part.items);
            plan.renames.extend(part.renames);
            plan.notes.extend(part.notes);
        }
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_of(steps: Vec<Step>) -> Plan {
        let mut p = Plan::empty(OpKind::Copy, "part");
        p.totals.items = steps.len() as u64;
        p.steps = steps;
        p
    }

    fn make_file(path: &str) -> Step {
        Step::MakeFile {
            path: PathBuf::from(path),
            mode: None,
        }
    }

    fn trash(path: &str) -> Step {
        Step::TrashItem {
            path: PathBuf::from(path),
            expect: None,
        }
    }

    fn copy(src: &str, dst: &str) -> Step {
        Step::CopySymlink {
            src: PathBuf::from(src),
            dst: PathBuf::from(dst),
            target: PathBuf::from("t"),
            remove_source: false,
        }
    }

    #[test]
    fn plans_that_do_not_meet_become_one_with_all_the_steps() {
        let merged = Plan::merge(
            vec![
                plan_of(vec![make_file("/a/x")]),
                plan_of(vec![make_file("/a/y"), trash("/b/z")]),
            ],
            "both",
        );
        assert_eq!(merged.steps.len(), 3);
        assert_eq!(merged.totals.items, 3);
        assert_eq!(merged.kind, OpKind::Action);
        assert!(merged.is_executable(), "{:?}", merged.warnings);
    }

    #[test]
    fn two_plans_that_write_one_name_are_refused() {
        let merged = Plan::merge(
            vec![
                plan_of(vec![make_file("/a/x")]),
                plan_of(vec![make_file("/a/x")]),
            ],
            "twice",
        );
        assert!(!merged.is_executable());
        assert!(merged.warnings[0].message.contains("two requests write"));
    }

    #[test]
    fn a_plan_that_reads_what_another_removes_or_makes_is_refused() {
        let removed = Plan::merge(
            vec![
                plan_of(vec![copy("/a/dir/f", "/b/f")]),
                plan_of(vec![trash("/a/dir")]),
            ],
            "gone",
        );
        assert!(!removed.is_executable());
        assert!(removed.warnings[0].message.contains("another removes"));

        let made = Plan::merge(
            vec![
                plan_of(vec![make_file("/a/new")]),
                plan_of(vec![copy("/a/new", "/b/new")]),
            ],
            "made",
        );
        assert!(!made.is_executable());
        assert!(made.warnings[0].message.contains("another makes"));
    }

    #[test]
    fn one_plan_may_read_what_it_removes_itself() {
        let merged = Plan::merge(vec![plan_of(vec![trash("/a/x"), trash("/a/y")])], "one");
        assert!(merged.is_executable(), "{:?}", merged.warnings);
    }
}
