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
use crate::statefile;
use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The most a program inside an archive may weigh: far above any real build, far below
/// something that fills a disk.
const MAX_BINARY_BYTES: u64 = 512 << 20;

/// The process id and a random number, for a name nobody else can know ahead of time. The id
/// comes first: `clean_leftovers` tells by it whose file a name is.
fn unique() -> String {
    format!(
        "{}-{:016x}",
        std::process::id(),
        RandomState::new().build_hasher().finish()
    )
}

/// Name of the temporary file a new program is written to, beside the one it will replace.
fn sibling(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rada".into());
    let name = match exe.extension() {
        Some(ext) => format!(".{stem}-{tag}-{}.{}", unique(), ext.to_string_lossy()),
        None => format!(".{stem}-{tag}-{}", unique()),
    };
    exe.with_file_name(name)
}

/// A name for the file the new program is written to, different every time.
pub fn stage_path(exe: &Path) -> PathBuf {
    sibling(exe, "update")
}

/// Create `path` as a new, empty file to write: it must not exist, and a link in its place is
/// never followed (so it cannot send the write to another file).
pub(crate) fn create_new_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

/// Copy `from` to the new file `to` (see `create_new_file`), all the way to the disk. What is
/// half written is removed.
pub fn copy_new(from: &Path, to: &Path) -> io::Result<()> {
    let mut src = fs::File::open(from)?;
    let mut out = create_new_file(to)?;
    let copied = io::copy(&mut src, &mut out)
        .and_then(|_| out.set_permissions(src.metadata()?.permissions()))
        .and_then(|()| out.sync_all());
    drop(out);
    if copied.is_err() {
        let _ = fs::remove_file(to);
    }
    copied
}

/// Take the program `inner` (a path inside the archive, `/`-separated) out of `archive` and
/// write it to `dest`.
pub fn extract_binary(archive: &Path, inner: &str, dest: &Path) -> Result<(), String> {
    let is_zip = inner.ends_with(".exe") || archive.extension().is_some_and(|e| e == "zip");
    if is_zip {
        extract_from_zip(archive, inner, dest)
    } else {
        extract_from_tar_gz(archive, inner, dest)
    }
}

/// Write `from` to the new file `dest`; if that fails after the file is made, it is removed.
fn copy_limited(mut from: impl Read, dest: &Path) -> Result<(), String> {
    let mut out =
        create_new_file(dest).map_err(|e| tf(Key::UpCannotCreate, &[&dest.display(), &e]))?;
    let copied = copy_checked(&mut from, &mut out);
    drop(out);
    if copied.is_err() {
        let _ = fs::remove_file(dest);
    }
    copied
}

fn copy_checked(from: &mut impl Read, out: &mut fs::File) -> Result<(), String> {
    let n = io::copy(&mut from.take(MAX_BINARY_BYTES + 1), out)
        .map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))?;
    if n > MAX_BINARY_BYTES {
        return Err(Key::UpArchiveHuge.tr().to_string());
    }
    if n == 0 {
        return Err(Key::UpArchiveEmpty.tr().to_string());
    }
    out.sync_all().map_err(|e| tf(Key::UpArchiveDamaged, &[&e]))
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
        swap_running(stage, exe)
    }
}

/// Windows: a running program cannot be overwritten, but it can be renamed. One call to
/// `ReplaceFileW` moves the old file aside and the new one into its place, and puts the first
/// back itself if the second fails; if the call cannot be used the two renames are done here,
/// the second undone when it fails.
#[cfg(windows)]
fn swap_running(stage: &Path, exe: &Path) -> Result<(), String> {
    let away = sibling(exe, "old");
    if replace_file(exe, stage, &away).is_ok() {
        return Ok(());
    }
    // A call that stopped between its renames left the name empty.
    if !exe.exists() && away.exists() {
        let _ = fs::rename(&away, exe);
    }
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

/// `ReplaceFileW`: `stage` takes the name of `exe`, and `exe` becomes `backup`.
#[cfg(windows)]
pub(crate) fn replace_file(exe: &Path, stage: &Path, backup: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (replaced, replacement, saved) = (wide(exe), wide(stage), wide(backup));
    // SAFETY: three NUL-terminated strings that live through the call; the two optional
    // pointers are null, as the documentation allows.
    let done = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            saved.as_ptr(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if done != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The name a program had before an update moved it aside (`.rada-old-<id>[.exe]` is what the
/// update on Windows calls the old file), if `running` is such a file.
fn name_before_move(running: &Path) -> Option<PathBuf> {
    let name = running.file_name()?.to_string_lossy();
    let (stem, tail) = name.strip_prefix('.')?.split_once("-old-")?;
    let original = match tail.rsplit_once('.') {
        Some((_, ext)) => format!("{stem}.{ext}"),
        None => stem.to_string(),
    };
    (!stem.is_empty()).then(|| running.with_file_name(original))
}

/// An update on Windows moves the old program aside and then puts the new one in its place; a
/// stop between the two leaves the program's own name empty, and the old file is the only
/// program that can run. When `running` is that file and the name is empty, give it the name
/// back and return the name. Nothing else is touched.
pub fn restore_missing(running: &Path) -> Option<PathBuf> {
    let target = name_before_move(running)?;
    if fs::symlink_metadata(&target).is_ok() {
        return None;
    }
    fs::rename(running, &target).ok()?;
    Some(target)
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
        // The pid is what follows, up to the next `-` or the extension; a file of this very
        // process (the old program, still running) is not ours to remove.
        if rest.split(['-', '.']).next() != Some(me.as_str()) {
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
    /// The name of the saved program in its folder. A name different each time, so that a new
    /// copy is written beside the old one and the switch is the one atomic write of this file;
    /// files from before that do not say, and the program is `rada`.
    #[serde(default = "plain_backup_name")]
    pub file: String,
    /// How long the saved program is, to tell a whole copy from one that was cut short.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

fn plain_backup_name() -> String {
    if cfg!(windows) { "rada.exe" } else { "rada" }.to_string()
}

fn backup_meta(dir: &Path) -> PathBuf {
    dir.join("previous.json")
}

/// A name that is one plain file name, not a path: the description is read from a file.
fn is_plain_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

/// The saved previous program, if there is one: its description is readable and the program
/// it names is there, whole.
pub fn read_backup(dir: &Path) -> Option<(PathBuf, BackupInfo)> {
    let info: BackupInfo = serde_json::from_slice(&fs::read(backup_meta(dir)).ok()?).ok()?;
    if !is_plain_name(&info.file) {
        return None;
    }
    let bin = dir.join(&info.file);
    let found = fs::metadata(&bin).ok()?;
    let whole = info.size.is_none_or(|n| n == found.len());
    (found.is_file() && whole).then_some((bin, info))
}

/// Keep a copy of `exe` (which is `version`) to go back to.
pub fn save_backup(exe: &Path, version: &str, dir: &Path) -> Result<(), String> {
    let incoming = copy_incoming(exe, dir)?;
    commit_backup(&incoming, exe, version, dir)
}

/// Copy `exe` into a new file of the saved programs' folder, complete on the disk, under a
/// name that marks it as not yet kept.
fn copy_incoming(exe: &Path, dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(dir).map_err(|e| tf(Key::UpCannotCreate, &[&dir.display(), &e]))?;
    let incoming = dir.join(format!("incoming-{}", unique()));
    copy_new(exe, &incoming).map_err(|e| tf(Key::UpCannotKeepCopy, &[&exe.display(), &e]))?;
    Ok(incoming)
}

/// Make the copy `incoming` the saved program. The way back is whole at every step: the old
/// copy and its description stay as they are until the new description, which names the new
/// copy, replaces them in one atomic write; only then is the old copy removed.
fn commit_backup(incoming: &Path, exe: &Path, version: &str, dir: &Path) -> Result<(), String> {
    let result = name_incoming(incoming, dir).and_then(|(file, size)| {
        let info = BackupInfo {
            version: version.to_string(),
            exe: exe.to_path_buf(),
            file,
            size: Some(size),
        };
        publish_backup(dir, &info).inspect_err(|_| {
            let _ = fs::remove_file(dir.join(&info.file));
        })?;
        remove_stale_backups(dir, &info.file);
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_file(incoming);
    }
    result
}

/// Give the copy its name in the folder; returns the name and the length of the copy.
fn name_incoming(incoming: &Path, dir: &Path) -> Result<(String, u64), String> {
    let name = match Path::new(&plain_backup_name()).extension() {
        Some(ext) => format!("rada-{}.{}", unique(), ext.to_string_lossy()),
        None => format!("rada-{}", unique()),
    };
    let bin = dir.join(&name);
    fs::rename(incoming, &bin).map_err(|e| tf(Key::UpCannotKeepOld, &[&e]))?;
    let size = fs::metadata(&bin)
        .map_err(|e| tf(Key::UpCannotKeepOld, &[&e]))?
        .len();
    Ok((name, size))
}

/// Write the description of the saved program: from this moment it is the way back.
fn publish_backup(dir: &Path, info: &BackupInfo) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(info).map_err(|e| e.to_string())?;
    statefile::write_atomic(&backup_meta(dir), &bytes)
        .map_err(|e| tf(Key::UpCannotWrite, &[&backup_meta(dir).display(), &e]))
}

/// Remove what is in the folder and is not the saved program `keep`: the copy it replaces, and
/// what an update that was cut short left.
fn remove_stale_backups(dir: &Path, keep: &str) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let old = plain_backup_name();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let ours = name == old
            || ["incoming-", "rada-", ".previous.json."]
                .iter()
                .any(|p| name.starts_with(p));
        if ours && name != keep {
            let _ = fs::remove_file(e.path());
        }
    }
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
    copy_new(&bin, &stage).map_err(|e| tf(Key::UpCannotWrite, &[&stage.display(), &e]))?;
    if run_check {
        check_saved(&stage)?;
    }
    let outgoing = copy_incoming(exe, dir).inspect_err(|_| {
        let _ = fs::remove_file(&stage);
    })?;
    if let Err(e) = replace_exe(&stage, exe) {
        let _ = fs::remove_file(&outgoing);
        return Err(e);
    }
    commit_backup(&outgoing, exe, current_version, dir)?;
    Ok(info.version)
}

/// Run the saved program once, to see that it is `rada`; removes it when it is not.
fn check_saved(stage: &Path) -> Result<(), String> {
    let said = make_executable(stage)
        .map_err(|e| e.to_string())
        .and_then(|()| run_version(stage, Duration::from_secs(10)));
    match said {
        Ok(out) if out.starts_with("rada ") => Ok(()),
        Ok(out) => {
            let _ = fs::remove_file(stage);
            Err(tf(Key::UpSavedSays, &[&out]))
        }
        Err(e) => {
            let _ = fs::remove_file(stage);
            Err(e)
        }
    }
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
    fn a_save_that_fails_halfway_never_pairs_a_version_with_the_wrong_program() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("rada");
        let keep = d.path().join("keep");
        fs::write(&exe, b"v1").unwrap();
        save_backup(&exe, "0.1.0", &keep).unwrap();
        fs::write(&exe, b"v2").unwrap();
        // The description of the saved program cannot be written in place.
        fs::set_permissions(backup_meta(&keep), fs::Permissions::from_mode(0o444)).unwrap();

        let _ = save_backup(&exe, "0.2.0", &keep);

        let (bin, info) = read_backup(&keep).expect("a way back is still there");
        let want: &[u8] = if info.version == "0.1.0" {
            b"v1"
        } else {
            b"v2"
        };
        assert_eq!(fs::read(bin).unwrap(), want, "version {}", info.version);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_name_a_copy_is_written_to_is_not_followed() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("rada");
        let keep = d.path().join("keep");
        let victim = d.path().join("victim");
        fs::write(&exe, b"v1").unwrap();
        fs::write(&victim, b"precious").unwrap();
        fs::create_dir(&keep).unwrap();
        std::os::unix::fs::symlink(&victim, keep.join("incoming")).unwrap();

        let _ = save_backup(&exe, "0.1.0", &keep);

        assert_eq!(fs::read(&victim).unwrap(), b"precious");
    }

    #[test]
    fn the_name_a_new_program_is_written_to_cannot_be_known_ahead() {
        let exe = Path::new("/somewhere/rada");
        let (a, b) = (stage_path(exe), stage_path(exe));
        assert_ne!(a, b);
        assert_eq!(a.parent(), exe.parent());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_where_the_new_program_is_to_be_written_is_not_followed() {
        let d = tempfile::tempdir().unwrap();
        let victim = d.path().join("victim");
        fs::write(&victim, b"precious").unwrap();
        let stage = d.path().join("stage");
        std::os::unix::fs::symlink(&victim, &stage).unwrap();

        let tar = tar_gz(d.path(), "a.tar.gz", &[("rada-1/rada", b"the program")]);
        assert!(extract_binary(&tar, "rada-1/rada", &stage).is_err());
        let zip = zip_file(d.path(), "a.zip", &[("rada-1/rada.exe", b"MZ program")]);
        assert!(extract_binary(&zip, "rada-1/rada.exe", &stage).is_err());

        assert_eq!(fs::read(&victim).unwrap(), b"precious");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_staging_name_of_a_rollback_is_not_written_through() {
        let d = tempfile::tempdir().unwrap();
        let exe = exe_in(d.path());
        let keep = d.path().join("keep");
        let victim = d.path().join("victim");
        fs::write(&exe, b"v1").unwrap();
        fs::write(&victim, b"precious").unwrap();
        save_backup(&exe, "0.1.0", &keep).unwrap();
        fs::write(&exe, b"v2").unwrap();
        std::os::unix::fs::symlink(&victim, stage_path(&exe)).unwrap();

        let _ = rollback(&exe, "0.2.0", &keep, false);

        assert_eq!(fs::read(&victim).unwrap(), b"precious");
    }

    #[test]
    fn the_leftovers_of_other_processes_go_and_those_of_this_one_stay() {
        let d = tempfile::tempdir().unwrap();
        let exe = exe_in(d.path());
        fs::write(&exe, b"v1").unwrap();
        let mine = stage_path(&exe);
        fs::write(&mine, b"x").unwrap();
        let ext = if cfg!(windows) { ".exe" } else { "" };
        let other = d
            .path()
            .join(format!(".rada-update-1-0123456789abcdef{ext}"));
        let older = d.path().join(format!(".rada-update-2{ext}"));
        fs::write(&other, b"x").unwrap();
        fs::write(&older, b"x").unwrap();

        clean_leftovers(&exe);

        assert!(mine.exists() && exe.exists());
        assert!(!other.exists() && !older.exists());
    }

    #[test]
    fn the_old_file_of_a_cut_update_takes_the_name_of_the_program_back() {
        let d = tempfile::tempdir().unwrap();
        let ext = if cfg!(windows) { ".exe" } else { "" };
        let exe = d.path().join(format!("rada{ext}"));
        fs::write(&exe, b"old").unwrap();
        // The state a stop between the two renames leaves: the old file under its other
        // name, the new one finished beside it, and nothing under the program's name.
        let away = sibling(&exe, "old");
        fs::rename(&exe, &away).unwrap();
        fs::write(stage_path(&exe), b"new").unwrap();

        assert_eq!(restore_missing(&away), Some(exe.clone()));
        assert_eq!(fs::read(&exe).unwrap(), b"old");
        assert!(!away.exists());
    }

    #[test]
    fn a_name_that_is_taken_or_a_program_that_is_not_an_old_file_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let ext = if cfg!(windows) { ".exe" } else { "" };
        let exe = d.path().join(format!("rada{ext}"));
        fs::write(&exe, b"new").unwrap();
        let away = sibling(&exe, "old");
        fs::write(&away, b"old").unwrap();
        assert_eq!(restore_missing(&away), None);
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert!(away.exists());
        // The program itself, and a file that only looks a little like one.
        assert_eq!(restore_missing(&exe), None);
        let odd = d.path().join(".-old-1");
        fs::write(&odd, b"x").unwrap();
        assert_eq!(restore_missing(&odd), None);
        assert!(odd.exists());
    }

    /// Not a test: a process of this very program that does nothing for a while, so that a
    /// copy of the test program can be run and kept running.
    #[test]
    #[ignore = "started by the tests that need a program that is running"]
    fn hold_still() {
        if std::env::var_os("RADA_TEST_HOLD").is_some() {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    /// A running process, killed when this is dropped, test failed or not.
    #[cfg(windows)]
    struct Running(std::process::Child);

    #[cfg(windows)]
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// `rada.exe` in `dir`: a copy of the test program, running.
    #[cfg(windows)]
    fn running_exe(dir: &Path) -> (PathBuf, Running) {
        let exe = dir.join("rada.exe");
        fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let child = Command::new(&exe)
            .args(["--ignored", "--exact", "update::install::tests::hold_still"])
            .env("RADA_TEST_HOLD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut running = Running(child);
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            running.0.try_wait().unwrap().is_none(),
            "it did not stay up"
        );
        (exe, running)
    }

    #[cfg(windows)]
    #[test]
    fn replacefilew_replaces_a_program_that_is_running_and_keeps_its_file_aside() {
        let d = tempfile::tempdir().unwrap();
        let (exe, _running) = running_exe(d.path());
        let stage = stage_path(&exe);
        fs::write(&stage, b"new").unwrap();
        let aside = sibling(&exe, "old");

        replace_file(&exe, &stage, &aside).expect("the swap works on a running program");

        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert!(!stage.exists());
        assert!(
            fs::metadata(&aside).unwrap().len() > 1000,
            "the old program is kept"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_running_program_is_replaced_whole_by_replace_exe() {
        let d = tempfile::tempdir().unwrap();
        let (exe, running) = running_exe(d.path());
        let stage = stage_path(&exe);
        fs::write(&stage, b"new").unwrap();

        replace_exe(&stage, &exe).unwrap();

        assert_eq!(fs::read(&exe).unwrap(), b"new");
        let aside: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".rada-old-"))
            .collect();
        assert_eq!(aside.len(), 1);
        // Once the old program has stopped its file can go.
        drop(running);
        fs::remove_file(aside[0].path()).unwrap();
    }

    fn exe_in(dir: &Path) -> PathBuf {
        dir.join(if cfg!(windows) { "rada.exe" } else { "rada" })
    }

    /// The saved program as `rollback` would find it: its version and what the file holds.
    fn way_back(keep: &Path) -> (String, Vec<u8>) {
        let (bin, info) = read_backup(keep).expect("a way back exists");
        (info.version, fs::read(bin).unwrap())
    }

    #[test]
    fn a_way_back_exists_after_a_stop_at_every_step_of_saving() {
        let d = tempfile::tempdir().unwrap();
        let exe = exe_in(d.path());
        let keep = d.path().join("keep");
        fs::write(&exe, b"v1").unwrap();
        save_backup(&exe, "0.1.0", &keep).unwrap();
        fs::write(&exe, b"v2").unwrap();
        let old = || (String::from("0.1.0"), b"v1".to_vec());

        // Each line is a stop after one more step; what the stop leaves is what a later start
        // finds.
        let incoming = copy_incoming(&exe, &keep).unwrap();
        assert_eq!(way_back(&keep), old(), "after copying");
        let (file, size) = name_incoming(&incoming, &keep).unwrap();
        assert_eq!(way_back(&keep), old(), "after naming the copy");
        let info = BackupInfo {
            version: "0.2.0".into(),
            exe: exe.clone(),
            file: file.clone(),
            size: Some(size),
        };
        publish_backup(&keep, &info).unwrap();
        assert_eq!(
            way_back(&keep),
            ("0.2.0".into(), b"v2".to_vec()),
            "after the switch"
        );
        remove_stale_backups(&keep, &file);
        assert_eq!(
            way_back(&keep),
            ("0.2.0".into(), b"v2".to_vec()),
            "after the clean-up"
        );
        assert_eq!(
            fs::read_dir(&keep).unwrap().count(),
            2,
            "the copy and its description"
        );
    }

    #[test]
    fn a_cut_copy_is_never_taken_for_the_way_back_and_the_next_save_removes_it() {
        let d = tempfile::tempdir().unwrap();
        let exe = exe_in(d.path());
        let keep = d.path().join("keep");
        fs::write(&exe, b"v1").unwrap();
        save_backup(&exe, "0.1.0", &keep).unwrap();
        // A copy that stopped half way, and a new one named but never described.
        fs::write(keep.join("incoming-1-ab"), b"v").unwrap();
        fs::write(keep.join("rada-1-cd"), b"v").unwrap();
        assert_eq!(way_back(&keep).0, "0.1.0");

        fs::write(&exe, b"v2").unwrap();
        save_backup(&exe, "0.2.0", &keep).unwrap();
        assert_eq!(way_back(&keep), ("0.2.0".into(), b"v2".to_vec()));
        assert_eq!(fs::read_dir(&keep).unwrap().count(), 2);
    }

    #[test]
    fn a_saved_program_that_is_not_the_length_written_down_is_not_a_way_back() {
        let d = tempfile::tempdir().unwrap();
        let exe = exe_in(d.path());
        let keep = d.path().join("keep");
        fs::write(&exe, b"a whole program").unwrap();
        save_backup(&exe, "0.1.0", &keep).unwrap();
        let (bin, _) = read_backup(&keep).unwrap();
        fs::write(&bin, b"a whole").unwrap();
        assert!(read_backup(&keep).is_none());
        assert!(rollback(&exe, "0.2.0", &keep, false).is_err());
    }

    #[test]
    fn a_saved_program_of_an_earlier_rada_is_read_and_then_replaced() {
        let d = tempfile::tempdir().unwrap();
        let exe = exe_in(d.path());
        let keep = d.path().join("keep");
        fs::create_dir(&keep).unwrap();
        fs::write(&exe, b"v2").unwrap();
        // The layout of before: the program under its plain name, a description without more.
        fs::write(keep.join(plain_backup_name()), b"v1").unwrap();
        let meta = format!(
            r#"{{"version":"0.1.0","exe":{:?}}}"#,
            exe.display().to_string()
        );
        fs::write(backup_meta(&keep), meta).unwrap();
        assert_eq!(way_back(&keep), ("0.1.0".into(), b"v1".to_vec()));

        save_backup(&exe, "0.2.0", &keep).unwrap();
        assert_eq!(way_back(&keep), ("0.2.0".into(), b"v2".to_vec()));
        assert!(!keep.join(plain_backup_name()).exists());
    }

    #[test]
    fn a_description_that_names_a_path_is_not_followed() {
        let d = tempfile::tempdir().unwrap();
        let keep = d.path().join("keep");
        fs::create_dir(&keep).unwrap();
        fs::write(d.path().join("elsewhere"), b"x").unwrap();
        let meta = r#"{"version":"0.1.0","exe":"/x","file":"../elsewhere"}"#;
        fs::write(backup_meta(&keep), meta).unwrap();
        assert!(read_backup(&keep).is_none());
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
