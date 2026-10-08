//! The JSON schemas of the plan and of operation requests live in the repository
//! (`schema/`). This test keeps them honest: it fails when the types change without the
//! schema files being regenerated (`VELA_UPDATE_SCHEMA=1 cargo test -p vela-core --test schema`).

use std::path::PathBuf;

use schemars::schema_for;
use vela_core::ops::{OpRequest, Plan};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema")
}

fn check(name: &str, generated: serde_json::Value) {
    let text = serde_json::to_string_pretty(&generated).unwrap() + "\n";
    let file = dir().join(name);
    if std::env::var_os("VELA_UPDATE_SCHEMA").is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "{} is missing: run with VELA_UPDATE_SCHEMA=1",
            file.display()
        )
    });
    assert_eq!(
        on_disk, text,
        "{name} is out of date: run `VELA_UPDATE_SCHEMA=1 cargo test -p vela-core --test schema`"
    );
}

#[test]
fn the_plan_schema_is_up_to_date() {
    check(
        "plan.schema.json",
        serde_json::to_value(schema_for!(Plan)).unwrap(),
    );
}

#[test]
fn the_request_schema_is_up_to_date() {
    check(
        "request.schema.json",
        serde_json::to_value(schema_for!(OpRequest)).unwrap(),
    );
}

#[test]
fn the_request_schema_describes_every_operation() {
    let v = serde_json::to_value(schema_for!(OpRequest)).unwrap();
    let text = v.to_string();
    for op in [
        "copy",
        "move",
        "rename",
        "bulk_rename",
        "make_dir",
        "trash",
        "delete",
        "undo",
    ] {
        assert!(
            text.contains(&format!("\"{op}\"")),
            "{op} missing from the schema"
        );
    }
    assert!(text.contains("conflict"));
}
