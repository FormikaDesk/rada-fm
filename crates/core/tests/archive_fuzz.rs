//! Inputs the fuzzer (`fuzz/`) found, kept as ordinary tests: listing an archive made of hostile
//! bytes ends with a list or an error, never with the program stopping.

use rada_core::archive::{ArchiveLimits, Index, ListControl};

fn list(bytes: &[u8]) -> Result<Index, rada_core::archive::ArchiveError> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input");
    std::fs::write(&path, bytes).unwrap();
    let never = || false;
    let mut ctl = ListControl::new(&never);
    ctl.max_entries = ArchiveLimits::default().max_entries;
    Index::open(&path, &mut ctl)
}

/// A 7z header of 106 bytes that says the archive holds some 2^60 files: the 7z library
/// (`sevenz-rust2` 0.20.2) asks for memory for all of them and the allocator stops the program.
/// Later versions of the library bound the count by the size of the header, but they need Rust
/// 1.93 and the minimum version of this program is 1.90: when that is raised and the library
/// updated, remove `ignore`. (Run as it is, this test ends the test program, not just itself.)
#[test]
#[ignore = "stops the program until sevenz-rust2 is updated (needs Rust 1.93): see docs/SAFETY.md"]
fn a_7z_header_that_declares_an_enormous_number_of_files_is_refused() {
    let bytes = include_bytes!("fixtures/fuzz/7z-huge-file-count.bin");
    assert!(list(bytes).is_err());
}
