//! The name of an archive member, as stored, cleaned into a path that stays inside the folder
//! it is extracted to.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rada_core::archive::entry::sanitize;
use std::path::Component;

fuzz_target!(|data: &[u8]| {
    let Some((flag, name)) = data.split_first() else {
        return;
    };
    let (path, issue) = sanitize(name, flag & 1 == 1);
    // Whatever the name was, what comes back has plain names and nothing else: no root, no
    // drive, no `.`, no `..`.
    assert!(
        path.components().all(|c| matches!(c, Component::Normal(_))),
        "{name:?} -> {path:?} ({issue:?})"
    );
    // A name with a `..` or a root in it is said to have an issue.
    let text = String::from_utf8_lossy(name);
    if text.split('/').any(|c| c == "..") {
        assert!(issue.is_some(), "{name:?} -> {path:?}");
    }
});
