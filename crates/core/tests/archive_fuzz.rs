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

/// A 7z header of 106 bytes that says the archive holds some 2^60 files: reading it asked for
/// memory for all of them and the allocator stopped the program. The 7z library now bounds the
/// count by the size of the header.
#[test]
fn a_7z_header_that_declares_an_enormous_number_of_files_is_refused() {
    let bytes = include_bytes!("fixtures/fuzz/7z-huge-file-count.bin");
    assert!(list(bytes).is_err());
}
