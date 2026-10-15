//! Putting a new program in place of the running one, and taking it back.
//!
//! The new file is written next to the old one (so that the final step is a rename inside one
//! folder, which is atomic), started once with `--version` to see that it is the program it
//! should be, and only then swapped in. At no point is there a moment without a working
//! `rada`: the old file stays until the rename, and on Windows, where a running program
//! cannot be overwritten, it is renamed out of the way first (a running program can be
//! renamed) and put back if the second rename fails.

use crate::i18n::{Key, tf};
use crate::platform::spawn::spawn_retrying_busy;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The most a program inside an archive may weigh: far above any real build, far below
/// something that fills a disk.
const MAX_BINARY_BYTES: u64 = 512 << 20;

/// Name of the temporary file a new program is written to, beside the one it will replace.
fn sibling(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rada".into());
    let name = match exe.extension() {
        Some(ext) => format!(
            ".{stem}-{tag}-{}.{}",
            std::process::id(),
            ext.to_string_lossy()
        ),
        None => format!(".{stem}-{tag}-{}", std::process::id()),
    };
    exe.with_file_name(name)
}

/// The file the new program is written to.
pub fn stage_path(exe: &Path) -> PathBuf {
    sibling(exe, "update")
}

/// Take the program `inner` (a path inside the archive, `/`-separated) out of `archive` and
/// write it to `dest`.
pub fn extract_binary(archive: &Path, inner: &str, dest: &Path) -> Result<(), String> {
    let is_zip = inner.ends_with(".exe") || archive.extension().is_some_and(|e| e == "zip");
    let result = if is_zip {
        extract_from_zip(archive, inner, dest)
    } else {
        extract_from_tar_gz(archive, inner, dest)
    };
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

fn copy_limited(mut from: impl Read, dest: &Path) -> Result<(), String> {
    let mut out =
        fs::File::create(dest).map_err(|e| tf(Key::UpCannotCreate, &[&dest.display(), &e]))?;
    let n = io::copy(&mut (&mut from).take(MAX_BINARY_BYTES + 1), &mut out)
        .map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))?;
    if n > MAX_BINARY_BYTES {
        return Err(Key::UpArchiveHuge.tr().to_string());
    }
    if n == 0 {
        return Err(Key::UpArchiveEmpty.tr().to_string());
    }
    Ok(())
}

fn extract_from_tar_gz(archive: &Path, inner: &str, dest: &Path) -> Result<(), String> {
    let file =
        fs::File::open(archive).map_err(|e| tf(Key::UpCannotOpen, &[&archive.display(), &e]))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let entries = tar
        .entries()
        .map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))?;
    for entry in entries {
        let entry = entry.map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))?;
        let path = entry
            .path()
            .map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))?
            .to_string_lossy()
            .replace('\\', "/");
        if path == inner && entry.header().entry_type().is_file() {
            return copy_limited(entry, dest);
        }
    }
    Err(tf(Key::UpArchiveLacks, &[&inner]))
}

fn extract_from_zip(archive: &Path, inner: &str, dest: &Path) -> Result<(), String> {
    let file =
        fs::File::open(archive).map_err(|e| tf(Key::UpCannotOpen, &[&archive.display(), &e]))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))?;
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))?;
        let name = entry.name().map(|n| n.replace('\\', "/"));
        if name.as_deref().ok() == Some(inner) && entry.is_file() {
            return copy_limited(entry, dest);
        }
    }
    Err(tf(Key::UpArchiveLacks, &[&inner]))
}

/// Start `program --version` and return what it printed, giving up after `limit`.
pub fn run_version(program: &Path, limit: Duration) -> Result<String, String> {
    let mut child = spawn_version(program).map_err(|e| tf(Key::UpNewNoStart, &[&e]))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut so) = child.stdout.take() {
                    let _ = so.read_to_string(&mut out);
                }
                if !status.success() {
                    return Err(tf(Key::UpNewExit, &[&status]));
                }
                return Ok(out.trim().to_string());
            }
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Key::UpNewSlow.tr().to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(tf(Key::UpNewWait, &[&e])),
        }
    }
}

/// Start `program --version`.
fn spawn_version(program: &Path) -> io::Result<std::process::Child> {
    spawn_retrying_busy(
        Command::new(program)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )
}

/// Mark the file runnable.
fn make_executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Whether the file carries macOS's download quarantine flag (`com.apple.quarantine`), the
/// one that makes Gatekeeper ask about a program. Always `false` elsewhere.
pub fn has_quarantine(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let Ok(p) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let name = c"com.apple.quarantine";
        // SAFETY: both strings are NUL-terminated; a null buffer of size 0 asks only for the
        // size of the attribute, or an error when there is none.
        let n = unsafe {
            libc::getxattr(
                p.as_ptr(),
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                libc::XATTR_NOFOLLOW,
            )
        };
        n >= 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// Take the quarantine flag off a file (macOS). A file this program wrote itself never has
/// it, since only the programs that opt in (browsers, mostly) put it there; this makes sure
/// that stays true. `true` when there was a flag to remove.
pub fn clear_quarantine(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if !has_quarantine(path) {
            return false;
        }
        let Ok(p) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let name = c"com.apple.quarantine";
        // SAFETY: both strings are NUL-terminated.
        unsafe { libc::removexattr(p.as_ptr(), name.as_ptr(), libc::XATTR_NOFOLLOW) == 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// Put `stage` (a finished file in the same folder as `exe`) in the place of `exe`.
///
/// On failure `exe` is as it was and `stage` is removed.
pub fn replace_exe(stage: &Path, exe: &Path) -> Result<(), String> {
    let result = replace_inner(stage, exe);
    if result.is_err() {
        let _ = fs::remove_file(stage);
    }
    result
}

fn replace_inner(stage: &Path, exe: &Path) -> Result<(), String> {
    make_executable(stage).map_err(|e| tf(Key::UpCannotRunnable, &[&e]))?;
    clear_quarantine(stage);
    #[cfg(not(windows))]
    {
        // A rename gives the new program a file of its own: the running one keeps its old file
        // until it exits, and nothing ever sees a half-written program.
        fs::rename(stage, exe).map_err(|e| tf(Key::UpCannotPut, &[&exe.display(), &e]))
    }
    #[cfg(windows)]
    {
        let away = sibling(exe, "old");
        let _ = fs::remove_file(&away);
        fs::rename(exe, &away).map_err(|e| tf(Key::UpCannotMove, &[&exe.display(), &e]))?;
        if let Err(e) = fs::rename(stage, exe) {
            let back = fs::rename(&away, exe);
            return Err(match back {
                Ok(()) => tf(Key::UpCannotPut, &[&exe.display(), &e]),
                Err(b) => tf(
                    Key::UpCannotPutKept,
                    &[&exe.display(), &e, &away.display(), &b],
                ),
            });
        }
        Ok(())
    }
}

/// Remove what an earlier update left beside the program: the old file on Windows, or a
/// half-written new one after a crash.
pub fn clean_leftovers(exe: &Path) {
    let Some(dir) = exe.parent() else { return };
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rada".into());
    let prefixes = [format!(".{stem}-old-"), format!(".{stem}-update-")];
    let me = std::process::id().to_string();
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(rest) = prefixes.iter().find_map(|p| name.strip_prefix(p.as_str())) else {
            continue;
        };
        // The pid is what follows, up to the extension; a file of this very process (the old
        // program, still running) is not ours to remove.
        if rest.split('.').next() != Some(me.as_str()) {
            let _ = fs::remove_file(e.path());
        }
    }
}

// ----------------------------------------------------------------------- the old program

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    /// The version the saved program is.
    pub version: String,
    /// Where it was installed.
    pub exe: PathBuf,
}

fn backup_binary(dir: &Path) -> PathBuf {
    dir.join(if cfg!(windows) { "rada.exe" } else { "rada" })
}

fn backup_meta(dir: &Path) -> PathBuf {
    dir.join("previous.json")
}

/// The saved previous program, if there is one.
pub fn read_backup(dir: &Path) -> Option<(PathBuf, BackupInfo)> {
    let info: BackupInfo = serde_json::from_slice(&fs::read(backup_meta(dir)).ok()?).ok()?;
    let bin = backup_binary(dir);
    bin.is_file().then_some((bin, info))
}

/// Keep a copy of `exe` (which is `version`) to go back to.
pub fn save_backup(exe: &Path, version: &str, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| tf(Key::UpCannotCreate, &[&dir.display(), &e]))?;
    let tmp = dir.join("incoming");
    fs::copy(exe, &tmp).map_err(|e| tf(Key::UpCannotKeepCopy, &[&exe.display(), &e]))?;
    commit_backup(&tmp, exe, version, dir)
}

fn commit_backup(tmp: &Path, exe: &Path, version: &str, dir: &Path) -> Result<(), String> {
    let bin = backup_binary(dir);
    let _ = fs::remove_file(&bin);
    fs::rename(tmp, &bin).map_err(|e| tf(Key::UpCannotKeepOld, &[&e]))?;
    let info = BackupInfo {
        version: version.to_string(),
        exe: exe.to_path_buf(),
    };
    fs::write(
        backup_meta(dir),
        serde_json::to_vec_pretty(&info).map_err(|e| e.to_string())?,
    )
    .map_err(|e| tf(Key::UpCannotWrite, &[&backup_meta(dir).display(), &e]))
}

/// Go back to the saved program. The program being replaced becomes the saved one, so that
/// `--rollback` twice is the update again. Returns the version restored.
pub fn rollback(
    exe: &Path,
    current_version: &str,
    dir: &Path,
    run_check: bool,
) -> Result<String, String> {
    let Some((bin, info)) = read_backup(dir) else {
        return Err(Key::UpNoEarlier.tr().to_string());
    };
    let stage = stage_path(exe);
    fs::copy(&bin, &stage).map_err(|e| tf(Key::UpCannotWrite, &[&stage.display(), &e]))?;
    if run_check {
        make_executable(&stage).map_err(|e| e.to_string())?;
        match run_version(&stage, Duration::from_secs(10)) {
            Ok(out) if out.starts_with("rada ") => {}
            Ok(out) => {
                let _ = fs::remove_file(&stage);
                return Err(tf(Key::UpSavedSays, &[&out]));
            }
            Err(e) => {
                let _ = fs::remove_file(&stage);
                return Err(e);
            }
        }
    }
    let outgoing = dir.join("incoming");
    fs::create_dir_all(dir).map_err(|e| tf(Key::UpCannotCreate, &[&dir.display(), &e]))?;
    if let Err(e) = fs::copy(exe, &outgoing) {
        let _ = fs::remove_file(&stage);
        return Err(tf(Key::UpCannotKeepCopy, &[&exe.display(), &e]));
    }
    replace_exe(&stage, exe)?;
    commit_backup(&outgoing, exe, current_version, dir)?;
    Ok(info.version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[cfg(unix)]
    #[test]
    fn a_program_still_open_for_writing_is_tried_again_not_reported_as_broken() {
        use crate::platform::spawn::is_text_file_busy;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("rada");
        fs::write(&exe, "#!/bin/sh\necho \"rada 9.9.9\"\n").unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        // Another process that was starting at that moment holds the file open for writing:
        // running it fails with "Text file busy" until that is closed.
        let writer = fs::OpenOptions::new().write(true).open(&exe).unwrap();
        if !Command::new(&exe)
            .output()
            .is_err_and(|e| is_text_file_busy(&e))
        {
            // This system runs a file that is open for writing (macOS does): there is no
            // refusal to get over.
            return;
        }
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(writer);
        });
        let said = run_version(&exe, Duration::from_secs(20)).expect("it started in the end");
        release.join().unwrap();
        assert_eq!(said, "rada 9.9.9");
    }

    pub(crate) fn tar_gz(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join(name);
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        for (n, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, n, *data).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        path
    }

    pub(crate) fn zip_file(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (n, data) in files {
            zip.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn the_program_is_taken_out_of_a_tar_gz() {
        let d = tempfile::tempdir().unwrap();
        let a = tar_gz(
            d.path(),
            "a.tar.gz",
            &[
                ("rada-1/README.md", b"readme"),
                ("rada-1/rada", b"the program"),
            ],
        );
        let out = d.path().join("out");
        extract_binary(&a, "rada-1/rada", &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"the program");
    }

    #[test]
    fn the_program_is_taken_out_of_a_zip() {
        let d = tempfile::tempdir().unwrap();
        let a = zip_file(
            d.path(),
            "a.zip",
            &[
                ("rada-1/LICENSE", b"mit"),
                ("rada-1/rada.exe", b"MZ program"),
            ],
        );
        let out = d.path().join("out.exe");
        extract_binary(&a, "rada-1/rada.exe", &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), b"MZ program");
    }

    #[test]
    fn an_archive_without_the_program_or_a_damaged_one_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("out");
        let a = tar_gz(d.path(), "a.tar.gz", &[("rada-1/other", b"x")]);
        let e = extract_binary(&a, "rada-1/rada", &out).unwrap_err();
        assert!(e.contains("does not contain"), "{e}");
        assert!(!out.exists());

        // Only the exact path counts: a file of the same name elsewhere is not the program.
        let a = tar_gz(d.path(), "b.tar.gz", &[("other/rada-1/rada", b"x")]);
        assert!(extract_binary(&a, "rada-1/rada", &out).is_err());

        fs::write(d.path().join("bad.tar.gz"), b"this is not gzip").unwrap();
        assert!(extract_binary(&d.path().join("bad.tar.gz"), "rada-1/rada", &out).is_err());
        fs::write(d.path().join("bad.zip"), b"PK but not really").unwrap();
        assert!(extract_binary(&d.path().join("bad.zip"), "rada-1/rada.exe", &out).is_err());
        assert!(!out.exists());

        let empty = tar_gz(d.path(), "e.tar.gz", &[("rada-1/rada", b"")]);
        assert!(extract_binary(&empty, "rada-1/rada", &out).is_err());
    }

    #[test]
    fn a_new_program_replaces_the_old_one_whole() {
        let d = tempfile::tempdir().unwrap();
        let exe = d
            .path()
            .join(if cfg!(windows) { "rada.exe" } else { "rada" });
        fs::write(&exe, b"old").unwrap();
        let stage = stage_path(&exe);
        fs::write(&stage, b"new").unwrap();
        replace_exe(&stage, &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert!(!stage.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        clean_leftovers(&exe);
    }

    #[test]
    fn a_failed_replacement_leaves_the_old_program_alone() {
        let d = tempfile::tempdir().unwrap();
        let exe = d
            .path()
            .join(if cfg!(windows) { "rada.exe" } else { "rada" });
        fs::write(&exe, b"old").unwrap();
        let missing = stage_path(&exe);
        // No staged file: the rename cannot work.
        assert!(replace_exe(&missing, &exe).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"old");
    }

    #[test]
    fn the_previous_program_is_kept_and_can_be_brought_back() {
        let d = tempfile::tempdir().unwrap();
        let exe = d
            .path()
            .join(if cfg!(windows) { "rada.exe" } else { "rada" });
        let keep = d.path().join("keep");
        fs::write(&exe, b"v1").unwrap();
        assert!(read_backup(&keep).is_none());
        assert!(rollback(&exe, "0.2.0", &keep, false).is_err());

        save_backup(&exe, "0.1.0", &keep).unwrap();
        let (_, info) = read_backup(&keep).unwrap();
        assert_eq!(info.version, "0.1.0");
        fs::write(&exe, b"v2").unwrap();

        assert_eq!(rollback(&exe, "0.2.0", &keep, false).unwrap(), "0.1.0");
        assert_eq!(fs::read(&exe).unwrap(), b"v1");
        // The one that was replaced is now the one to go back to.
        let (bin, info) = read_backup(&keep).unwrap();
        assert_eq!(info.version, "0.2.0");
        assert_eq!(fs::read(bin).unwrap(), b"v2");
        assert_eq!(rollback(&exe, "0.1.0", &keep, false).unwrap(), "0.2.0");
        assert_eq!(fs::read(&exe).unwrap(), b"v2");
        clean_leftovers(&exe);
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_does_not_run_is_found_out_before_it_is_installed() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let good = d.path().join("good");
        fs::write(&good, "#!/bin/sh\necho rada 9.9.9\n").unwrap();
        fs::set_permissions(&good, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            run_version(&good, Duration::from_secs(5)).unwrap(),
            "rada 9.9.9"
        );

        let bad = d.path().join("bad");
        fs::write(&bad, "#!/bin/sh\nexit 3\n").unwrap();
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(run_version(&bad, Duration::from_secs(5)).is_err());

        let slow = d.path().join("slow");
        fs::write(&slow, "#!/bin/sh\nsleep 5\n").unwrap();
        fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
        let e = run_version(&slow, Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("in time"), "{e}");

        assert!(run_version(&d.path().join("missing"), Duration::from_secs(1)).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_rada_writes_has_no_quarantine_and_one_that_has_it_loses_it() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("rada");
        fs::write(&f, b"x").unwrap();
        // What rada writes is clean.
        assert!(!has_quarantine(&f));
        // A file a browser had saved is not, and is cleaned.
        let p = CString::new(f.as_os_str().as_bytes()).unwrap();
        let value = b"0083;5f000000;Safari;";
        let r = unsafe {
            libc::setxattr(
                p.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert_eq!(r, 0, "cannot set the flag for the test");
        assert!(has_quarantine(&f));
        assert!(clear_quarantine(&f));
        assert!(!has_quarantine(&f));
    }
}
