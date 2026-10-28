//! The member list of a zip archive made of whatever bytes: it ends with a list or an error,
//! in bounded time and memory.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rada_core::archive::{Index, ListControl};
use std::time::{Duration, Instant};

fuzz_target!(|data: &[u8]| {
    let mut bytes = data.to_vec();
    // The magic number is given, so that the time goes to the listing and not to the sniffing.
    if bytes.len() >= 4 {
        bytes[..4].copy_from_slice(b"PK\x03\x04");
    }
    let path = std::env::temp_dir().join(format!("rada-fuzz-zip-{}", std::process::id()));
    if std::fs::write(&path, &bytes).is_err() {
        return;
    }
    let never = || false;
    let mut ctl = ListControl::new(&never);
    ctl.deadline = Some(Instant::now() + Duration::from_secs(5));
    ctl.max_entries = 10_000;
    let _ = Index::open(&path, &mut ctl);
});
