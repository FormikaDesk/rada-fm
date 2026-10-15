//! The state folder holds the journal of everything the user moved, deleted or opened: it is
//! private to the user, however it was made and whichever version made it.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use rada_core::journal::Journal;
use rada_core::platform::openwith::Prefs;
use rada_core::{statefile, uistate};

fn mode(p: &Path) -> u32 {
    fs::symlink_metadata(p).unwrap().mode() & 0o777
}

#[test]
fn a_new_state_folder_and_every_file_in_it_are_private() {
    // The umask of the process is whatever it is: the modes must not depend on it.
    let t = tempfile::tempdir().unwrap();
    let state = t.path().join("state/rada");
    let journal = Journal::open(state.join("journal.jsonl")).unwrap();
    drop(journal);
    uistate::save(&state, &uistate::UiState::default()).unwrap();
    Prefs::default().save(&state).unwrap();
    for dir in [&state, &state.parent().unwrap().to_path_buf()] {
        assert_eq!(mode(dir), 0o700, "{}", dir.display());
    }
    for f in ["journal.jsonl", "ui.json", "open_with.json"] {
        assert_eq!(mode(&state.join(f)), 0o600, "{f}");
    }
}

#[test]
fn the_state_of_an_older_install_is_closed_at_start_and_stays_closed() {
    let t = tempfile::tempdir().unwrap();
    let state = t.path().join("state/rada");
    // As an earlier version left it: a folder of 755 and files of 644.
    fs::create_dir_all(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).unwrap();
    for f in ["journal.jsonl", "ui.json", "update.json", "recent.json"] {
        fs::write(state.join(f), "{}\n").unwrap();
        fs::set_permissions(state.join(f), fs::Permissions::from_mode(0o644)).unwrap();
    }

    statefile::tighten(&state);

    assert_eq!(mode(&state), 0o700);
    for f in ["journal.jsonl", "ui.json", "update.json", "recent.json"] {
        assert_eq!(mode(&state.join(f)), 0o600, "{f}");
    }
    // What the program writes afterwards, over the same names, keeps them closed, and the
    // journal that already has records is still read.
    let journal = Journal::open(state.join("journal.jsonl")).unwrap();
    drop(journal);
    uistate::save(&state, &uistate::UiState::default()).unwrap();
    assert_eq!(mode(&state.join("ui.json")), 0o600);
    assert_eq!(mode(&state.join("journal.jsonl")), 0o600);
    assert_eq!(mode(&state), 0o700);
}
