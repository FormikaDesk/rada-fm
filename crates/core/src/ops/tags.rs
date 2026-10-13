//! Changing tags as an operation: a plan (what each item will carry afterwards), a journal
//! entry and an undo, like everything else rada does to files.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::engine::Engine;
use super::plan::{ItemAction, ItemSummary, OpKind, Plan, Severity, Step, WarningKind, WarningSet};
use crate::fs::FileKind;
use crate::platform::{Tag, tags};
use crate::{Error, Result, display};

/// What to do to the tags of the items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "how", rename_all = "snake_case")]
pub enum TagChange {
    /// Give every item this tag (those that have it are left alone).
    Add { tag: Tag },
    /// Take the tag called `name` off every item.
    Remove { name: String },
    /// Take the tag off if every item has it, give it to every item otherwise: what clicking
    /// a colour in the menu does.
    Toggle { tag: Tag },
    /// Remove all tags.
    Clear,
    /// Replace the tags of every item with exactly these.
    Set { tags: Vec<Tag> },
}

impl TagChange {
    /// Names are checked; nothing else needs the filesystem.
    pub fn validate(&self) -> Result<()> {
        let check = |name: &str| {
            tags::validate_name(name)
                .map(|_| ())
                .map_err(Error::Invalid)
        };
        match self {
            TagChange::Add { tag } | TagChange::Toggle { tag } => check(&tag.name),
            TagChange::Remove { name } => check(name),
            TagChange::Set { tags } => tags.iter().try_for_each(|t| check(&t.name)),
            TagChange::Clear => Ok(()),
        }
    }

    /// In a few words, for the title of the plan.
    pub fn describe(&self) -> String {
        match self {
            TagChange::Add { tag } | TagChange::Toggle { tag } => format!("tag {}", tag.name),
            TagChange::Remove { name } => format!("remove tag {name}"),
            TagChange::Clear => "remove all tags".to_string(),
            TagChange::Set { tags } => format!("set tags to {}", super::plan::tag_names(tags)),
        }
    }

    /// The tags an item ends with. `everyone_has` says whether every item of the operation
    /// has the tag a toggle is about (decided once, for all of them).
    fn apply(&self, current: &[Tag], everyone_has: bool) -> Vec<Tag> {
        match self {
            TagChange::Add { tag } => tags::with_tag(current, tag),
            TagChange::Remove { name } => tags::without_tag(current, name),
            TagChange::Toggle { tag } => {
                if everyone_has {
                    tags::without_tag(current, &tag.name)
                } else {
                    tags::with_tag(current, tag)
                }
            }
            TagChange::Clear => Vec::new(),
            TagChange::Set { tags } => tags.clone(),
        }
    }
}

impl Engine {
    /// The plan for changing the tags of `items`. Reads the tags, changes nothing.
    pub fn plan_set_tags(&self, items: &[PathBuf], change: &TagChange) -> Plan {
        let mut plan = Plan::empty(
            OpKind::SetTags,
            format!(
                "{} on {}",
                change.describe(),
                if items.len() == 1 {
                    display::path(&items[0])
                } else {
                    format!("{} items", items.len())
                }
            ),
        );
        let mut ws = WarningSet::default();
        let Some(store) = self.platform.tags() else {
            ws.add_with(
                WarningKind::CannotTag,
                Severity::Blocking,
                None,
                Some("this system keeps no tags".into()),
            );
            plan.warnings = ws.finish();
            return plan;
        };

        // What each item has now.
        let mut current: Vec<(PathBuf, FileKind, Vec<Tag>)> = Vec::new();
        for p in items {
            match self.fs.lstat(p) {
                Err(_) => ws.add(WarningKind::Missing, Severity::Warning, Some(p)),
                Ok(m) if m.is_symlink() => ws.add_with(
                    WarningKind::CannotTag,
                    Severity::Warning,
                    Some(p),
                    Some("a link has no tags of its own".into()),
                ),
                Ok(m) => match store.read(p) {
                    Ok(tags) => current.push((p.clone(), m.kind, tags)),
                    Err(e) => ws.add_with(
                        WarningKind::CannotTag,
                        Severity::Warning,
                        Some(p),
                        Some(e.to_string()),
                    ),
                },
            }
        }
        let everyone_has = match change {
            TagChange::Toggle { tag } => {
                !current.is_empty()
                    && current
                        .iter()
                        .all(|(_, _, t)| t.iter().any(|x| x.is_called(&tag.name)))
            }
            _ => false,
        };
        for (path, kind, before) in current {
            let after = change.apply(&before, everyone_has);
            if after == before {
                continue;
            }
            plan.items.push(ItemSummary {
                path: path.clone(),
                kind,
                action: ItemAction::Retag,
                files: u64::from(kind != FileKind::Dir),
                dirs: u64::from(kind == FileKind::Dir),
                symlinks: 0,
                bytes: 0,
                target: None,
            });
            plan.steps.push(Step::SetTags {
                path,
                before,
                after,
            });
        }
        plan.totals.items = plan.steps.len() as u64;
        plan.totals.files = plan.items.iter().map(|i| i.files).sum();
        plan.totals.dirs = plan.items.iter().map(|i| i.dirs).sum();
        if plan.steps.is_empty() && plan.warnings.is_empty() {
            ws.add_with(
                WarningKind::Other,
                Severity::Info,
                None,
                Some("the items already have these tags: nothing changes".into()),
            );
        }
        plan.warnings = ws.finish();
        plan
    }
}
