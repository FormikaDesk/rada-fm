//! rada's own files in its state folder (the journal, the interface state, the open-with
//! choices, the update check): written whole and atomically, and readable by the user alone.
//!
//! The journal lists every file the user moved, deleted or opened lately, and the state folder
//! is under a home folder that is open to other users on some systems (macOS, many Linux
//! distributions): so the folder is `0700` and the files `0600` on Unix. On Windows the folder
//! inherits the rights of the user's profile.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Create `dir` and the folders above it that are missing, readable by the user alone (Unix).
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

/// Open options for a file of the state folder: created readable by the user alone (Unix).
pub fn private_file_options() -> OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut o = OpenOptions::new();
        o.mode(0o600);
        o
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
    }
}

/// Write `bytes` as the whole content of `path`: into a file of its own name beside it, written
/// to the disk, then renamed over `path`. A reader sees the old content or the new, never half
/// of it, and after a power cut the name is never left on an empty file. The folder is created
/// (private) when it is missing. Two programs writing at once do not share the temporary file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_private_dir(dir)?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a file name is needed to write",
        )
    })?;
    let tmp: PathBuf = dir.join(format!(
        ".{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| -> io::Result<()> {
        let mut f = private_file_options()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    // The new name itself, too. (Windows has no such call for a folder.)
    #[cfg(unix)]
    if let Err(e) = fs::File::open(dir).and_then(|d| d.sync_all()) {
        tracing::debug!("sync {}: {e}", dir.display());
    }
    Ok(())
}

/// Make what an earlier version left behind private: the folder `0700`, its files `0600`, and
/// the same for the folders inside (the logs). Run at start; it changes only what is more open
/// than that, and never follows a link. Errors are ignored: a file that cannot be changed is
/// not worth stopping for.
pub fn tighten(dir: &Path) {
    #[cfg(unix)]
    tighten_dir(dir, 0);
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(unix)]
fn tighten_dir(dir: &Path, depth: u32) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let restrict = |p: &Path, meta: &fs::Metadata, want: u32| {
        // Only what is mine to change, and only when it is more open than wanted.
        if meta.mode() & 0o777 & !want != 0 {
            let _ = fs::set_permissions(p, fs::Permissions::from_mode(meta.mode() & want));
        }
    };
    match fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => restrict(dir, &m, 0o700),
        _ => return,
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(m) = fs::symlink_metadata(e.path()) else {
            continue;
        };
        if m.is_file() {
            restrict(&e.path(), &m, 0o600);
        } else if m.is_dir() && depth < 3 {
            tighten_dir(&e.path(), depth + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::MetadataExt;
        fs::symlink_metadata(p).unwrap().mode() & 0o777
    }

    #[test]
    fn a_file_is_written_whole_and_replaces_the_old_one() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("state/rada/ui.json");
        write_atomic(&f, b"one").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"one");
        write_atomic(&f, b"two, longer").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"two, longer");
        // Nothing of the temporary file is left.
        let names: Vec<_> = fs::read_dir(f.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["ui.json"]);
    }

    #[test]
    fn a_failed_write_leaves_the_old_content_and_no_temporary_file() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("x");
        write_atomic(&f, b"kept").unwrap();
        // The name is taken by a folder: the rename cannot replace it.
        let blocked = t.path().join("dir");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("inside"), "x").unwrap();
        assert!(write_atomic(&blocked, b"new").is_err());
        assert_eq!(fs::read(&f).unwrap(), b"kept");
        let leftovers = fs::read_dir(t.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[cfg(unix)]
    #[test]
    fn what_is_written_is_private_to_the_user() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("a/b/rada");
        let f = dir.join("update.json");
        write_atomic(&f, b"{}").unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(dir.parent().unwrap()), 0o700);
        assert_eq!(mode(&f), 0o600);
        // Again, over the existing file: still private.
        write_atomic(&f, b"{ }").unwrap();
        assert_eq!(mode(&f), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn an_older_install_with_open_permissions_is_closed_at_start() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let t = tempfile::tempdir().unwrap();
        let outside = t.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("theirs"), "x").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(outside.join("theirs"), fs::Permissions::from_mode(0o644)).unwrap();

        let dir = t.path().join("rada");
        fs::create_dir_all(dir.join("log")).unwrap();
        for f in [
            "journal.jsonl",
            "ui.json",
            "update.json",
            "log/rada.log.2026-10-14",
        ] {
            fs::write(dir.join(f), "x").unwrap();
            fs::set_permissions(dir.join(f), fs::Permissions::from_mode(0o644)).unwrap();
        }
        // A file that is already stricter stays as it is.
        fs::write(dir.join("strict"), "x").unwrap();
        fs::set_permissions(dir.join("strict"), fs::Permissions::from_mode(0o400)).unwrap();
        symlink(&outside, dir.join("link-out")).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(dir.join("log"), fs::Permissions::from_mode(0o755)).unwrap();

        tighten(&dir);

        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("log")), 0o700);
        for f in [
            "journal.jsonl",
            "ui.json",
            "update.json",
            "log/rada.log.2026-10-14",
        ] {
            assert_eq!(mode(&dir.join(f)), 0o600, "{f}");
        }
        assert_eq!(mode(&dir.join("strict")), 0o400);
        // A link is not followed: what it points at is not touched.
        assert_eq!(mode(&outside), 0o755);
        assert_eq!(mode(&outside.join("theirs")), 0o644);
        // And a state folder that is itself a link is left alone.
        let alias = t.path().join("alias");
        symlink(&outside, &alias).unwrap();
        tighten(&alias);
        assert_eq!(mode(&outside), 0o755);
    }

    #[test]
    fn tightening_a_folder_that_is_not_there_does_nothing() {
        let t = tempfile::tempdir().unwrap();
        tighten(&t.path().join("absent"));
    }
}
