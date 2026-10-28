//! The journal file read back: whatever is in it, reading it ends with entries or with an
//! error, and says where the damage is.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rada_core::journal::Journal;

fuzz_target!(|data: &[u8]| {
    let dir = std::env::temp_dir().join(format!("rada-fuzz-journal-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("journal.jsonl");
    if std::fs::write(&path, data).is_err() {
        return;
    }
    if let Ok(journal) = Journal::open(&path) {
        let _ = journal.damage();
        let _ = journal.entries();
    }
});
