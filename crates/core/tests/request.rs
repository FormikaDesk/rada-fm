//! Requests as data: JSON in, plan out, same journal, same undo; plus redo.

use std::path::PathBuf;

use rada_core::journal::UndoState;
use rada_core::ops::*;
use rada_core::testutil::*;

fn plan_of(e: &Engine, req: &OpRequest, j: Option<&rada_core::journal::Journal>) -> Plan {
    let cancel = Cancel::new();
    e.plan_request(
        req,
        j,
        ScanControl {
            cancel: cancel.flag(),
            progress: &mut |_| {},
        },
    )
    .unwrap_or_else(|err| panic!("{req:?}: {err}"))
    .plan
}

fn parse(json: &str) -> OpRequest {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

#[test]
fn every_request_kind_round_trips_through_json_with_sensible_defaults() {
    let docs = [
        r#"{"op":"copy","sources":["/a/x","/a/y"],"destination":"/b"}"#,
        r#"{"op":"move","sources":["/a/x"],"destination":"/b","conflict":"keep_both","verify":true}"#,
        r#"{"op":"rename","path":"/a/x","new_name":"y"}"#,
        r#"{"op":"bulk_rename","items":["/a/1","/a/2"],"pattern":"{name}_{n:2}.{ext}","counter_start":5}"#,
        r#"{"op":"make_dir","parent":"/a","name":"new"}"#,
        r#"{"op":"trash","sources":["/a/x"]}"#,
        r#"{"op":"delete","sources":["/a/x"]}"#,
        r#"{"op":"undo"}"#,
        r#"{"op":"undo","entry":"abc-1"}"#,
    ];
    for d in docs {
        let req = parse(d);
        let again = parse(&serde_json::to_string(&req).unwrap());
        assert_eq!(req, again, "{d}");
    }
    // Omitted fields take their defaults.
    let OpRequest::Copy {
        conflict, verify, ..
    } = parse(docs[0])
    else {
        panic!()
    };
    assert_eq!((conflict, verify), (ConflictPolicy::Skip, false));
    // Unknown fields and unknown operations are refused, not ignored.
    assert!(
        serde_json::from_str::<OpRequest>(
            r#"{"op":"copy","sources":["/a"],"destination":"/b","force":true}"#
        )
        .is_err()
    );
    assert!(serde_json::from_str::<OpRequest>(r#"{"op":"format_disk"}"#).is_err());
}

#[test]
fn a_request_yields_the_same_plan_as_building_it_by_hand() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "a");
    sb.write("src/sub/b.txt", "bb");
    sb.write("dest/a.txt", "already here");
    let e = sb.engine();
    let by_hand = e.plan_transfer(
        &scan(&e, &[sb.path("src")]),
        &sb.path("dest"),
        &TransferOptions::copy(ConflictPolicy::KeepBoth),
    );
    let req = OpRequest::Copy {
        sources: vec![sb.path("src")],
        destination: sb.path("dest"),
        conflict: ConflictPolicy::KeepBoth,
        verify: false,
    };
    let from_request = plan_of(&e, &req, None);
    assert_eq!(by_hand.steps, from_request.steps);
    assert_eq!(by_hand.totals, from_request.totals);
    assert_eq!(by_hand.items, from_request.items);
    assert_eq!(
        from_request.request.as_ref(),
        Some(&req),
        "the plan remembers what it answers"
    );
    // Planning changed nothing.
    assert!(!sb.path("dest/src").exists());
}

#[test]
fn bad_requests_are_refused_before_anything_is_read() {
    let sb = Sandbox::new();
    let e = sb.engine();
    let cancel = Cancel::new();
    let try_plan = |req: OpRequest| {
        e.plan_request(
            &req,
            None,
            ScanControl {
                cancel: cancel.flag(),
                progress: &mut |_| {},
            },
        )
        .err()
        .map(|x| x.to_string())
    };
    let err = try_plan(OpRequest::Trash {
        sources: vec![PathBuf::from("relative/file")],
    })
    .unwrap();
    assert!(err.contains("absolute"), "{err}");
    assert!(
        try_plan(OpRequest::Delete { sources: vec![] })
            .unwrap()
            .contains("no sources")
    );
    assert!(
        try_plan(OpRequest::BulkRename {
            items: vec![sb.path("a")],
            pattern: "{nope}".into(),
            counter_start: None,
            counter_step: None
        })
        .is_some()
    );
    assert!(
        try_plan(OpRequest::Rename {
            path: sb.path("a"),
            new_name: String::new()
        })
        .is_some()
    );
    assert!(
        try_plan(OpRequest::Undo { entry: None })
            .unwrap()
            .contains("journal")
    );
}

#[test]
fn a_delete_request_is_still_flagged_irreversible_and_needs_the_confirmation_flow() {
    let sb = Sandbox::new();
    let f = sb.write("x.txt", "x");
    let e = sb.engine();
    let plan = plan_of(
        &e,
        &OpRequest::Delete {
            sources: vec![f.clone()],
        },
        None,
    );
    assert!(!plan.reversible);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.kind == WarningKind::Irreversible)
    );
    assert_eq!(plan.kind, OpKind::Delete);
    assert!(f.exists(), "a request alone never deletes anything");
}

#[test]
fn plans_serialize_to_json_and_back_including_non_utf8_names() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "a");
    // macOS refuses a name that is not valid UTF-8.
    #[cfg(all(unix, not(target_os = "macos")))]
    sb.write(
        std::path::Path::new("src").join(os_from_bytes(b"bad-\xff-name")),
        "z",
    );
    sb.write("dest/a.txt", "clash");
    let e = sb.engine();
    let plan = plan_of(
        &e,
        &OpRequest::Copy {
            sources: vec![sb.path("src")],
            destination: sb.path("dest"),
            conflict: ConflictPolicy::Skip,
            verify: false,
        },
        None,
    );
    let json = plan_to_json(&plan).unwrap();
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(plan.steps, back.steps);
    assert_eq!(plan.items, back.items);
    assert_eq!(plan.totals, back.totals);
    assert_eq!(plan.destination, back.destination);
    assert_eq!(plan.warnings, back.warnings);
    assert_eq!(plan.request, back.request);
    assert_eq!(json, plan_to_json(&back).unwrap(), "stable form");
    // Items summarise what happens to the selection.
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].action, ItemAction::Copy);
}

#[test]
fn item_summaries_describe_each_selected_item() {
    let sb = Sandbox::new();
    sb.write("src/new.txt", "n");
    sb.write("src/clash.txt", "src");
    sb.write("src/dir/f", "d");
    sb.write("dest/clash.txt", "dest");
    let e = sb.engine();
    let sources = vec![
        sb.path("src/new.txt"),
        sb.path("src/clash.txt"),
        sb.path("src/dir"),
    ];
    let action = |policy| {
        plan_of(
            &e,
            &OpRequest::Copy {
                sources: sources.clone(),
                destination: sb.path("dest"),
                conflict: policy,
                verify: false,
            },
            None,
        )
        .items
        .iter()
        .map(|i| {
            (
                i.path.file_name().unwrap().to_string_lossy().into_owned(),
                i.action,
            )
        })
        .collect::<Vec<_>>()
    };
    assert_eq!(
        action(ConflictPolicy::Skip),
        [
            ("new.txt".into(), ItemAction::Copy),
            ("clash.txt".into(), ItemAction::Skip),
            ("dir".into(), ItemAction::Copy)
        ]
    );
    assert_eq!(action(ConflictPolicy::KeepBoth)[1].1, ItemAction::KeepBoth);
    #[cfg(target_os = "linux")]
    assert_eq!(
        action(ConflictPolicy::Overwrite)[1].1,
        ItemAction::Overwrite
    );
    let trash = plan_of(
        &e,
        &OpRequest::Trash {
            sources: sources.clone(),
        },
        None,
    );
    assert!(trash.items.iter().all(|i| i.action == ItemAction::Trash));
    assert_eq!(trash.items[2].files, 1);
}

#[test]
fn request_plan_run_journal_undo_and_redo_use_the_same_machinery() {
    let sb = Sandbox::new();
    sb.write("src/a.txt", "alpha");
    sb.write("src/sub/b.txt", "beta");
    let dest = sb.mkdir("dest");
    let (e, j) = (sb.engine(), sb.journal());
    let req = parse(&format!(
        r#"{{"op":"copy","sources":[{:?}],"destination":{:?}}}"#,
        sb.path("src").to_string_lossy(),
        dest.to_string_lossy()
    ));

    // run through the journal
    let plan = plan_of(&e, &req, Some(&j));
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(out.report.status(), RunStatus::Completed);
    assert!(dest.join("src/sub/b.txt").exists());
    let entry = j.entries().unwrap().pop().unwrap();
    assert_eq!(
        entry.request.as_ref(),
        Some(&req),
        "the journal keeps the request"
    );
    assert!(!entry.redoable, "nothing to redo before an undo");
    assert!(j.last_redoable().unwrap().is_none());

    // undo, expressed as a request too
    let undo_plan = {
        let cancel = Cancel::new();
        e.plan_request(
            &OpRequest::Undo { entry: None },
            Some(&j),
            ScanControl {
                cancel: cancel.flag(),
                progress: &mut |_| {},
            },
        )
        .unwrap()
    };
    let up = undo_plan.undo.expect("undo plan");
    e.run_undo(&j, &up, &mut SkipErrors, &Cancel::new())
        .unwrap();
    assert!(!dest.join("src").exists());
    let entry = j.entries().unwrap().pop().unwrap();
    assert_eq!(entry.undo_state, UndoState::Undone);
    assert!(entry.redoable);

    // redo: the same request is planned again, shown, confirmed, journaled
    let redo = j.last_redoable().unwrap().expect("redoable");
    let mut plan = plan_of(&e, redo.request.as_ref().unwrap(), Some(&j));
    plan.redo_of = Some(redo.id.clone());
    let out = run_journaled(&e, &j, &plan);
    assert_eq!(out.report.status(), RunStatus::Completed);
    assert_eq!(
        std::fs::read_to_string(dest.join("src/sub/b.txt")).unwrap(),
        "beta"
    );
    assert!(
        j.last_redoable().unwrap().is_none(),
        "a redone operation is no longer redoable"
    );
    // and the redone operation itself can be undone again
    assert!(j.last_undoable().unwrap().is_some());
}

#[test]
fn redo_follows_the_undo_order_and_a_new_operation_ends_it() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let a = run_journaled(
        &e,
        &j,
        &plan_of(
            &e,
            &OpRequest::MakeDir {
                parent: sb.work.clone(),
                name: "one".into(),
            },
            None,
        ),
    );
    let b = run_journaled(
        &e,
        &j,
        &plan_of(
            &e,
            &OpRequest::MakeDir {
                parent: sb.work.clone(),
                name: "two".into(),
            },
            None,
        ),
    );
    undo(&e, &j, &b.id);
    undo(&e, &j, &a.id);
    // Undone last = redone first.
    assert_eq!(j.last_redoable().unwrap().unwrap().id, a.id);
    // A brand-new operation closes the redo history.
    run_journaled(
        &e,
        &j,
        &plan_of(
            &e,
            &OpRequest::MakeDir {
                parent: sb.work.clone(),
                name: "three".into(),
            },
            None,
        ),
    );
    assert!(j.last_redoable().unwrap().is_none());
}

#[test]
fn deletions_are_never_redoable_and_the_journal_says_why() {
    let sb = Sandbox::new();
    let (e, j) = (sb.engine(), sb.journal());
    let f = sb.write("x", "x");
    let out = run_journaled(
        &e,
        &j,
        &plan_of(&e, &OpRequest::Delete { sources: vec![f] }, None),
    );
    assert!(e.plan_undo(&j, &out.id).is_err());
    assert!(j.last_redoable().unwrap().is_none());
}
