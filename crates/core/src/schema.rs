//! JSON Schemas of the data the engine exchanges with programs: the operation request
//! it accepts and the plan it answers with. The same text is committed under `schema/`
//! (a test keeps the files current).

use schemars::schema_for;

use crate::ops::{OpRequest, Plan};

fn pretty<T: serde::Serialize>(schema: &T) -> String {
    serde_json::to_string_pretty(schema).expect("a schema is always valid JSON") + "\n"
}

/// Schema of [`OpRequest`], pretty-printed.
pub fn request() -> String {
    pretty(&schema_for!(OpRequest))
}

/// Schema of [`Plan`], pretty-printed.
pub fn plan() -> String {
    pretty(&schema_for!(Plan))
}
