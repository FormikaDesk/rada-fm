//! The JSON schemas of the plan and of operation requests live in the repository
//! (`schema/`). This test keeps them honest: it fails when the types change without the
//! schema files being regenerated (`RADA_UPDATE_SCHEMA=1 cargo test -p rada-core --test schema`).

use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema")
}

fn check(name: &str, text: String) {
    let file = dir().join(name);
    if std::env::var_os("RADA_UPDATE_SCHEMA").is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "{} is missing: run with RADA_UPDATE_SCHEMA=1",
            file.display()
        )
    });
    assert_eq!(
        on_disk, text,
        "{name} is out of date: run `RADA_UPDATE_SCHEMA=1 cargo test -p rada-core --test schema`"
    );
}

#[test]
fn the_plan_schema_is_up_to_date() {
    check("plan.schema.json", rada_core::schema::plan());
}

#[test]
fn the_request_schema_is_up_to_date() {
    check("request.schema.json", rada_core::schema::request());
}

#[test]
fn the_request_schema_describes_every_operation() {
    let text = rada_core::schema::request();
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
