//! Tags change through a plan, the journal and undo like every other operation, and copies
//! and moves keep them. They are kept in a user extended attribute on Linux, so these tests
//! need a filesystem that has them (the target folder does; one that does not skips them).
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};

use rada_core::journal::UndoState;
use rada_core::ops::*;
use rada_core::platform::tags::xattr;
use rada_core::platform::{Tag, TagColor};
use rada_core::testutil::*;

fn supported(sb: &Sandbox) -> bool {
    let probe = sb.write("probe", "x");
    let ok = xattr::set(&probe, "user.rada-probe", b"1").is_ok();
    if !ok {
        eprintln!("this filesystem keeps no user attributes: test skipped");
    }
    ok
}

fn tags_of(sb: &Sandbox, p: &Path) -> Vec<String> {
    sb.platform()
        .tags()
        .expect("a platform with tags")
        .read(p)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect()
}

fn set(sb: &Sandbox, p: &Path, names: &[&str]) {
    let tags: Vec<Tag> = names.iter().map(|n| Tag::named(n)).collect();
    sb.platform().tags().unwrap().write(p, &tags).unwrap();
}

fn add(items: &[PathBuf], name: &str) -> TagChange {
    let _ = items;
    TagChange::Add {
        tag: Tag::named(name),
    }
}

fn plan(e: &Engine, items: &[PathBuf], change: &TagChange) -> Plan {
    e.plan_set_tags(items, change)
}

#[test]
fn a_tag_is_planned_added_journaled_and_undone() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("a.txt", "hello");
    let items = vec![f.clone()];
    let p = plan(&e, &items, &add(&items, "Red"));
    assert_eq!(p.kind, OpKind::SetTags);
    assert!(p.is_executable(), "{:?}", p.warnings);
    assert_eq!(p.steps.len(), 1);
    assert_eq!(p.items[0].action, ItemAction::Retag);
    assert!(p.title.contains("tag Red"), "{}", p.title);
    // Planning writes nothing.
    assert!(tags_of(&sb, &f).is_empty());

    let out = run_journaled(&e, &j, &p);
    assert_eq!(out.report.status(), RunStatus::Completed, "{:?}", out.report.failed);
    assert_eq!(tags_of(&sb, &f), ["Red"]);
    assert_eq!(j.entries().unwrap()[0].describe(), "tag change of 1 item");

    let (up, rep) = undo(&e, &j, &out.id);
    assert!(up.blocked.is_empty(), "{:?}", up.blocked);
    assert_eq!(rep.status(), RunStatus::Completed, "{:?}", rep.failed);
    assert!(tags_of(&sb, &f).is_empty());
    assert_eq!(j.entries().unwrap()[0].undo_state, UndoState::Undone);
}

#[test]
fn several_items_each_keep_what_they_had() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let (e, j) = (sb.engine(), sb.journal());
    let (a, b, c) = (sb.write("a", "1"), sb.write("b", "2"), sb.mkdir("c"));
    set(&sb, &a, &["Work"]);
    set(&sb, &b, &["Red", "Work"]);
    let items = vec![a.clone(), b.clone(), c.clone()];
    let p = plan(&e, &items, &add(&items, "Red"));
    // b has Red already: no step for it.
    assert_eq!(p.steps.len(), 2, "{:?}", p.steps);
    let out = run_journaled(&e, &j, &p);
    assert_eq!(out.report.status(), RunStatus::Completed);
    assert_eq!(tags_of(&sb, &a), ["Work", "Red"]);
    assert_eq!(tags_of(&sb, &b), ["Red", "Work"]);
    assert_eq!(tags_of(&sb, &c), ["Red"], "a folder too");
    undo(&e, &j, &out.id);
    assert_eq!(tags_of(&sb, &a), ["Work"]);
    assert_eq!(tags_of(&sb, &b), ["Red", "Work"], "the one that had it is as it was");
    assert!(tags_of(&sb, &c).is_empty());
}

#[test]
fn clicking_a_colour_toggles_it_for_the_whole_selection() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let e = sb.engine();
    let (a, b) = (sb.write("a", "1"), sb.write("b", "2"));
    set(&sb, &a, &["Red"]);
    let items = vec![a.clone(), b.clone()];
    let toggle = TagChange::Toggle {
        tag: Tag::standard(TagColor::Red),
    };
    // Only one has it: it is given to both.
    let p = plan(&e, &items, &toggle);
    assert_eq!(p.steps.len(), 1);
    assert!(matches!(&p.steps[0], Step::SetTags { path, after, .. } if *path == b && after.len() == 1));
    run(&e, &p);
    assert_eq!(tags_of(&sb, &b), ["Red"]);
    // Both have it now: it is taken off both.
    let p = plan(&e, &items, &toggle);
    assert_eq!(p.steps.len(), 2);
    run(&e, &p);
    assert!(tags_of(&sb, &a).is_empty() && tags_of(&sb, &b).is_empty());
}

#[test]
fn remove_clear_and_set_do_what_they_say() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let e = sb.engine();
    let a = sb.write("a", "1");
    set(&sb, &a, &["Red", "Work", "Blue"]);
    let items = vec![a.clone()];
    run(&e, &plan(&e, &items, &TagChange::Remove { name: "work".into() }));
    assert_eq!(tags_of(&sb, &a), ["Red", "Blue"]);
    run(
        &e,
        &plan(
            &e,
            &items,
            &TagChange::Set {
                tags: vec![Tag::named("Only")],
            },
        ),
    );
    assert_eq!(tags_of(&sb, &a), ["Only"]);
    run(&e, &plan(&e, &items, &TagChange::Clear));
    assert!(tags_of(&sb, &a).is_empty());
}

#[test]
fn a_change_that_changes_nothing_says_so_and_has_no_steps() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let e = sb.engine();
    let a = sb.write("a", "1");
    set(&sb, &a, &["Red"]);
    let items = vec![a];
    let p = plan(&e, &items, &add(&items, "red"));
    assert!(p.steps.is_empty());
    assert!(p.warnings.iter().any(|w| w.message.contains("nothing changes")), "{:?}", p.warnings);
}

#[test]
fn links_and_vanished_items_are_told_about_and_the_rest_goes_ahead() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let e = sb.engine();
    let a = sb.write("a", "1");
    let link = sb.symlink(&a, "link");
    let gone = sb.path("gone");
    let items = vec![a.clone(), link, gone];
    let p = plan(&e, &items, &add(&items, "Red"));
    assert_eq!(p.steps.len(), 1);
    assert!(p.warnings.iter().any(|w| w.kind == WarningKind::CannotTag), "{:?}", p.warnings);
    assert!(p.warnings.iter().any(|w| w.kind == WarningKind::Missing), "{:?}", p.warnings);
    assert!(p.is_executable());
}

#[test]
fn undo_leaves_tags_alone_that_were_changed_after_the_operation() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let (e, j) = (sb.engine(), sb.journal());
    let a = sb.write("a", "1");
    let items = vec![a.clone()];
    let out = run_journaled(&e, &j, &plan(&e, &items, &add(&items, "Red")));
    // Somebody tags it again in the meantime.
    set(&sb, &a, &["Red", "Later"]);
    let (up, rep) = undo(&e, &j, &out.id);
    assert_eq!(tags_of(&sb, &a), ["Red", "Later"], "what they did is not overwritten");
    assert!(
        !up.blocked.is_empty() || !rep.failed.is_empty(),
        "and the undo says why it did not"
    );
}

#[test]
fn tags_changed_between_the_plan_and_the_run_are_not_overwritten() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let e = sb.engine();
    let a = sb.write("a", "1");
    let items = vec![a.clone()];
    let p = plan(&e, &items, &add(&items, "Red"));
    set(&sb, &a, &["Sneaky"]);
    let rep = run(&e, &p);
    assert_eq!(rep.failed.len(), 1);
    assert!(rep.failed[0].error.contains("tags changed"), "{:?}", rep.failed);
    assert_eq!(tags_of(&sb, &a), ["Sneaky"]);
}

#[test]
fn copies_and_moves_keep_the_tags() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let e = sb.engine();
    let a = sb.write("src/a.txt", "hello");
    set(&sb, &a, &["Red", "Work"]);
    let dst = sb.mkdir("dst");
    let (_, rep) = do_copy(&e, &[a.clone()], &dst, ConflictPolicy::Skip);
    assert!(rep.failed.is_empty(), "{:?}", rep.failed);
    assert_eq!(tags_of(&sb, &dst.join("a.txt")), ["Red", "Work"]);
    assert_eq!(tags_of(&sb, &a), ["Red", "Work"], "the original keeps its own");
    // A folder's tags go with it, and so do those of the files inside.
    let d = sb.mkdir("src/folder");
    sb.write("src/folder/in.txt", "x");
    set(&sb, &d, &["Blue"]);
    set(&sb, &d.join("in.txt"), &["Green"]);
    let dst2 = sb.mkdir("dst2");
    do_copy(&e, &[d.clone()], &dst2, ConflictPolicy::Skip);
    assert_eq!(tags_of(&sb, &dst2.join("folder")), ["Blue"]);
    assert_eq!(tags_of(&sb, &dst2.join("folder/in.txt")), ["Green"]);
    // Moving keeps them too.
    let dst3 = sb.mkdir("dst3");
    let (_, rep) = do_move(&e, &[a.clone()], &dst3, ConflictPolicy::Skip);
    assert!(rep.failed.is_empty());
    assert_eq!(tags_of(&sb, &dst3.join("a.txt")), ["Red", "Work"]);
}

#[test]
fn the_request_has_a_json_form_and_is_checked() {
    let json = r#"{"op":"set_tags","items":["/tmp/a"],"change":{"how":"add","tag":{"name":"Red","color":"red"}}}"#;
    let req: OpRequest = serde_json::from_str(json).expect("the documented form");
    assert!(req.validate().is_ok());
    assert!(matches!(
        &req,
        OpRequest::SetTags { change: TagChange::Add { tag }, .. } if tag.color == Some(TagColor::Red)
    ));
    // Back to JSON and again.
    let again: OpRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
    assert_eq!(again, req);
    let toggle = r#"{"op":"set_tags","items":["/a"],"change":{"how":"toggle","tag":{"name":"Work"}}}"#;
    assert!(serde_json::from_str::<OpRequest>(toggle).unwrap().validate().is_ok());
    let clear = r#"{"op":"set_tags","items":["/a"],"change":{"how":"clear"}}"#;
    assert!(serde_json::from_str::<OpRequest>(clear).unwrap().validate().is_ok());

    // Relative paths, bad names and nothing to tag are refused before anything is read.
    for bad in [
        r#"{"op":"set_tags","items":["rel"],"change":{"how":"clear"}}"#,
        r#"{"op":"set_tags","items":["/a"],"change":{"how":"add","tag":{"name":"a,b"}}}"#,
        r#"{"op":"set_tags","items":["/a"],"change":{"how":"add","tag":{"name":""}}}"#,
        r#"{"op":"set_tags","items":[],"change":{"how":"clear"}}"#,
    ] {
        let r: OpRequest = serde_json::from_str(bad).unwrap();
        assert!(r.validate().is_err(), "{bad}");
    }
    assert!(serde_json::from_str::<OpRequest>(r#"{"op":"set_tags","items":["/a"],"change":{"how":"explode"}}"#).is_err());
}

#[test]
fn a_request_goes_through_the_engine_like_any_other() {
    let sb = Sandbox::new();
    if !supported(&sb) {
        return;
    }
    let e = sb.engine();
    let a = sb.write("a", "1");
    let req = OpRequest::SetTags {
        items: vec![a.clone()],
        change: TagChange::Add {
            tag: Tag::standard(TagColor::Green),
        },
    };
    let planned = e
        .plan_request(&req, None, ScanControl { cancel: Cancel::new().flag(), progress: &mut |_| {} })
        .unwrap();
    assert_eq!(planned.plan.kind, OpKind::SetTags);
    assert_eq!(planned.plan.steps.len(), 1);
}
