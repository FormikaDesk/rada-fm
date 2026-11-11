//! Running a step that makes one new file with another program, or from what it reads.
//!
//! In this order: the folder can take the file; the inputs are still what the plan saw; a work
//! folder of the step's own is made next to the destination; the producer writes there; the
//! inputs are looked at again; the output is checked and sent to the disk; the journal is told
//! its size and time; and only then it gets its name, by a rename that cannot replace
//! anything. Whatever happens, the work folder goes.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::action::{self, Arg, Collect, Command, InputStamp, Producer};
use super::engine::Engine;
use super::exec::Ctl;
use super::plan::StepResult;
use super::{names, programs};
use crate::fs::{CopyControl, FileKind, FsMeta, Stamp};
use crate::platform::process::{self, DEFAULT_CAPTURE, EnvPolicy, ProcessOutcome, ProcessSpec};
use crate::{Error, Result, display};

/// The names tried when the one asked for is taken: `name (1).ext` to `name (50).ext`.
const MAX_NAMES: u32 = 50;

static WORK_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The tag of a work folder when the journal gives none.
fn own_tag() -> String {
    let n = WORK_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

fn program_error(program: &str, message: impl Into<String>) -> Error {
    Error::Program {
        program: display::text(program),
        message: message.into(),
    }
}

/// Where the programs of a chain read and write.
struct Layout<'a> {
    work: &'a Path,
    /// The extension the file will have, with its dot: programs choose a format by it.
    ext: OsString,
    inputs: &'a [InputStamp],
}

impl Layout<'_> {
    /// The file program `index` is to write: the last one writes the output, the others what
    /// the next one reads.
    fn out_of(&self, index: usize, last: bool) -> PathBuf {
        let mut name = if last {
            OsString::from("out")
        } else {
            OsString::from(format!("stage-{index}"))
        };
        name.push(&self.ext);
        self.work.join(name)
    }

    fn args(&self, command: &Command, prev: Option<&Path>, out: &Path) -> Result<Vec<OsString>> {
        let mut args = Vec::new();
        for arg in &command.args {
            match arg {
                Arg::Lit(s) => args.push(OsString::from(s)),
                Arg::Input(n) => args.push(self.input(*n)?),
                Arg::Inputs => args.extend(self.inputs.iter().map(|i| i.path.clone().into())),
                Arg::Prev => args.push(
                    prev.ok_or_else(|| Error::Invalid("nothing was written before".into()))?
                        .into(),
                ),
                Arg::Out => args.push(out.into()),
                Arg::OutDir => args.push(self.work.into()),
                Arg::ProfileUrl => args.push(profile_option(self.work).into()),
            }
        }
        Ok(args)
    }

    fn input(&self, n: usize) -> Result<OsString> {
        self.inputs
            .get(n)
            .map(|i| i.path.clone().into())
            .ok_or_else(|| Error::Invalid(format!("there is no input {n}")))
    }
}

/// LibreOffice's option that puts its profile in the work folder, as a `file:` address.
fn profile_option(work: &Path) -> String {
    let path = work.join("profile").to_string_lossy().replace('\\', "/");
    let mut url = String::from("-env:UserInstallation=file://");
    if !path.starts_with('/') {
        url.push('/');
    }
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// The line `sha256sum` writes for a file: the hash, two spaces, the name. A name with a
/// backslash or a line break is written escaped, and the line starts with a backslash to say so.
fn checksum_line(hash: &str, name: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    let raw = std::os::unix::ffi::OsStrExt::as_bytes(name).to_vec();
    #[cfg(not(unix))]
    let raw = name.to_string_lossy().into_owned().into_bytes();
    let escaped = raw.iter().any(|b| matches!(b, b'\\' | b'\n' | b'\r'));
    let mut line = Vec::new();
    if escaped {
        line.push(b'\\');
    }
    line.extend_from_slice(hash.as_bytes());
    line.extend_from_slice(b"  ");
    for b in raw {
        match b {
            b'\\' => line.extend_from_slice(b"\\\\"),
            b'\n' => line.extend_from_slice(b"\\n"),
            b'\r' => line.extend_from_slice(b"\\r"),
            other => line.push(other),
        }
    }
    line.push(b'\n');
    line
}

/// The SHA-256 of the file at `path`, in hex.
fn sha256_of(path: &Path, ctl: &mut Ctl<'_>) -> Result<String> {
    let read = |e| Error::io("read", path, e);
    let mut file = std::fs::File::open(path).map_err(read)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(read(e)),
        };
        hasher.update(&buf[..n]);
        if !ctl.advance(0) {
            return Err(Error::Cancelled);
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The last line a program wrote to say what went wrong, if it wrote any.
fn last_words(stderr: &str) -> String {
    stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| format!(": {}", display::truncate(l.trim(), 200)))
        .unwrap_or_default()
}

impl Engine {
    pub(super) fn produce_file(
        &self,
        ctl: &mut Ctl<'_>,
        dst: &Path,
        producer: &Producer,
        inputs: &[InputStamp],
        size_hint: Option<u64>,
    ) -> Result<StepResult> {
        self.can_take(dst, size_hint)?;
        self.inputs_unchanged(inputs)?;
        let tag = ctl.handler.temp_tag().unwrap_or_else(own_tag);
        let work = action::work_dir(dst, &tag);
        self.make_work_dir(&work)?;
        let published = self.produce_in(ctl, &work, dst, producer, inputs);
        self.drop_work_dir(&work);
        let name = published?;
        Ok(StepResult {
            after: self.fs.lstat(&name).ok().map(|m| m.fingerprint()),
            published_as: (name != dst).then_some(name),
            ..StepResult::done()
        })
    }

    /// The work folder goes, whatever happened in it. A program that was just killed can
    /// hold it for a moment more (Windows keeps the folder a process runs in), so it is tried
    /// a few times before giving up.
    fn drop_work_dir(&self, work: &Path) {
        for attempt in 0..5 {
            match self.fs.remove_dir_all(work) {
                Ok(()) => return,
                Err(e) if attempt == 4 => tracing::warn!("cannot remove {}: {e}", work.display()),
                Err(_) => std::thread::sleep(Duration::from_millis(60)),
            }
        }
    }

    /// Everything that happens while the work folder exists; the name the file got.
    fn produce_in(
        &self,
        ctl: &mut Ctl<'_>,
        work: &Path,
        dst: &Path,
        producer: &Producer,
        inputs: &[InputStamp],
    ) -> Result<PathBuf> {
        let out = match producer {
            Producer::Checksum { .. } => self.write_checksum(ctl, work, dst, inputs)?,
            Producer::Program { chain, .. } => {
                let out = self.run_chain(ctl, work, dst, producer, inputs)?;
                let last = chain.last().map_or("", |c| c.program.name.as_str());
                self.whole_output(&out, last)?;
                out
            }
        };
        // What was read while the program ran may not be what the plan saw: then what it
        // wrote is not what was asked for.
        self.inputs_unchanged(inputs)?;
        let meta = self.sent_to_disk(&out, dst)?;
        self.publish(ctl, &out, dst, &meta)
    }

    /// The folder of `dst` can take a new file of about `size_hint` bytes.
    fn can_take(&self, dst: &Path, size_hint: Option<u64>) -> Result<()> {
        if self.platform.on_device(dst) {
            return Err(Error::Unsupported("actions on a phone or a camera"));
        }
        let parent = dst
            .parent()
            .ok_or_else(|| Error::Invalid(format!("{} has no folder", display::path(dst))))?;
        if !self.fs.can_write(parent) {
            let denied = io::Error::from(io::ErrorKind::PermissionDenied);
            return Err(Error::io("write in", parent, denied));
        }
        if let Some(need) = size_hint
            && let Ok(free) = self.fs.available_space(parent)
            && need > free
        {
            let why = format!(
                "need {}, {} free",
                display::bytes(need),
                display::bytes(free)
            );
            let full = io::Error::new(io::ErrorKind::StorageFull, why);
            return Err(Error::io("write", dst, full));
        }
        Ok(())
    }

    /// Every input is still the file the plan saw.
    fn inputs_unchanged(&self, inputs: &[InputStamp]) -> Result<()> {
        for input in inputs {
            let now = self
                .fs
                .lstat(&input.path)
                .map_err(|e| Error::io("inspect", &input.path, e))?;
            if let Some(reason) = input.expect.diff(&now.fingerprint()) {
                return Err(Error::Modified {
                    path: input.path.clone(),
                    reason,
                });
            }
        }
        Ok(())
    }

    /// A folder nobody else has: one that is there already is somebody else's.
    fn make_work_dir(&self, work: &Path) -> Result<()> {
        self.fs.create_dir(work, Some(0o700)).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                Error::AlreadyExists(work.to_path_buf())
            } else {
                Error::io("create folder", work, e)
            }
        })
    }

    fn write_checksum(
        &self,
        ctl: &mut Ctl<'_>,
        work: &Path,
        dst: &Path,
        inputs: &[InputStamp],
    ) -> Result<PathBuf> {
        let [input] = inputs else {
            return Err(Error::Invalid("a checksum is of one file".into()));
        };
        let hash = sha256_of(&input.path, ctl)?;
        let name = input.path.file_name().unwrap_or(input.path.as_os_str());
        let layout = Layout {
            work,
            ext: dst.extension().map(dotted).unwrap_or_default(),
            inputs,
        };
        let out = layout.out_of(0, true);
        let mut sink = self
            .fs
            .create_file(&out, 0o644)
            .map_err(|e| Error::io("create", dst, e))?;
        sink.write_all(&checksum_line(&hash, name))
            .map_err(|e| Error::io("write", dst, e))?;
        Ok(out)
    }

    /// Run the programs one after the other; the file the last one wrote.
    fn run_chain(
        &self,
        ctl: &mut Ctl<'_>,
        work: &Path,
        dst: &Path,
        producer: &Producer,
        inputs: &[InputStamp],
    ) -> Result<PathBuf> {
        let Producer::Program { chain, collect, .. } = producer else {
            return Err(Error::Invalid("not a program".into()));
        };
        action::check_chain(chain, collect, inputs.len()).map_err(Error::Invalid)?;
        let layout = Layout {
            work,
            ext: names::split_ext(dst.file_name().unwrap_or_default()).1,
            inputs,
        };
        let mut prev: Option<PathBuf> = None;
        for (i, command) in chain.iter().enumerate() {
            let out = layout.out_of(i, i + 1 == chain.len());
            let args = layout.args(command, prev.as_deref(), &out)?;
            self.run_program(ctl, command, args, work, producer)?;
            prev = Some(out);
        }
        match (collect, prev, chain.last()) {
            (Collect::Single { ext }, _, Some(last)) => {
                self.single_output(work, ext, &last.program.name)
            }
            (_, Some(out), _) => Ok(out),
            _ => Err(Error::Invalid("no program to run".into())),
        }
    }

    /// Run one program of a chain and judge how it ended.
    fn run_program(
        &self,
        ctl: &mut Ctl<'_>,
        command: &Command,
        args: Vec<OsString>,
        work: &Path,
        producer: &Producer,
    ) -> Result<()> {
        let Producer::Program {
            timeout_secs,
            ok_exit,
            ..
        } = producer
        else {
            return Ok(());
        };
        let (name, file) = (&command.program.name, &command.program.path);
        if programs::is_refused(name) || programs::refused_file(file) {
            return Err(program_error(name, "an action never starts it"));
        }
        let spec = ProcessSpec {
            program: file.clone(),
            args,
            cwd: work.to_path_buf(),
            env: EnvPolicy::new(self.temp_dir(work)?),
            timeout: Duration::from_secs(*timeout_secs),
            max_capture: DEFAULT_CAPTURE,
        };
        let cancel = ctl.cancel;
        let ended = process::run(&spec, &|| cancel.is_cancelled(), &mut |_| {
            ctl.advance(0);
        })
        .map_err(|e| Error::io("run", file, e))?;
        judge(name, &ended, *timeout_secs, ok_exit)
    }

    /// The folder the programs keep their temporary files in, inside the work folder.
    fn temp_dir(&self, work: &Path) -> Result<PathBuf> {
        let temp = work.join("tmp");
        match self.fs.create_dir(&temp, Some(0o700)) {
            Ok(()) => Ok(temp),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(temp),
            Err(e) => Err(Error::io("create folder", &temp, e)),
        }
    }

    /// The one file with extension `ext` a program left in the work folder.
    fn single_output(&self, work: &Path, ext: &str, program: &str) -> Result<PathBuf> {
        let listing = self
            .fs
            .read_dir(work)
            .map_err(|e| Error::io("read", work, e))?;
        let mut found = listing.items.into_iter().filter(|item| {
            let wanted = item
                .path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(ext));
            let stage = item.name.to_string_lossy().starts_with("stage-");
            wanted && !stage && item.meta.as_ref().is_ok_and(|m| m.kind == FileKind::File)
        });
        match (found.next(), found.next()) {
            (Some(one), None) => Ok(one.path),
            (None, _) => Err(program_error(program, format!("wrote no .{ext} file"))),
            (Some(_), Some(_)) => Err(program_error(
                program,
                format!("wrote more than one .{ext} file"),
            )),
        }
    }

    /// The output is a file, not a link, and not empty: an exit code alone proves nothing.
    fn whole_output(&self, out: &Path, program: &str) -> Result<()> {
        let meta = match self.fs.lstat(out) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(program_error(program, "ended well but wrote no file"));
            }
            Err(e) => return Err(Error::io("inspect", out, e)),
        };
        if meta.kind != FileKind::File {
            return Err(program_error(program, "did not write a regular file"));
        }
        if meta.size == 0 {
            return Err(program_error(program, "wrote an empty file"));
        }
        Ok(())
    }

    /// The bytes reach the disk before the name does; what the file is then.
    fn sent_to_disk(&self, out: &Path, dst: &Path) -> Result<FsMeta> {
        match self.fs.sync_file(out) {
            Err(e) if e.kind() != io::ErrorKind::Unsupported => {
                return Err(Error::io("write", dst, e));
            }
            _ => {}
        }
        self.fs.lstat(out).map_err(|e| Error::io("inspect", dst, e))
    }

    /// Give the output its name: `dst`, or the next free one if that was taken meanwhile.
    /// Each name is written in the journal before it is tried, and none is ever replaced.
    fn publish(&self, ctl: &mut Ctl<'_>, out: &Path, dst: &Path, meta: &FsMeta) -> Result<PathBuf> {
        let name = dst.file_name().unwrap_or_default();
        for n in 0..=MAX_NAMES {
            let candidate = match n {
                0 => dst.to_path_buf(),
                n => dst.with_file_name(names::numbered(name, n)),
            };
            let mtime = meta.mtime.map(Stamp::from);
            if !ctl.handler.publishing(&candidate, meta.size, mtime) {
                return Err(Error::Journal(
                    "it could not be written, so the new file was not given its name".into(),
                ));
            }
            match self.fs.rename_noreplace_strict(out, &candidate) {
                Ok(()) => return Ok(candidate),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) if e.kind() == io::ErrorKind::Unsupported => {
                    return Err(Error::io("publish", &candidate, no_safe_rename()));
                }
                Err(e) => return Err(Error::io("publish", &candidate, e)),
            }
        }
        Err(Error::AlreadyExists(dst.to_path_buf()))
    }
}

/// Whether a program that ran did what was asked, from how it ended.
fn judge(name: &str, ended: &ProcessOutcome, timeout_secs: u64, ok_exit: &[i32]) -> Result<()> {
    if ended.interrupted {
        return Err(Error::Cancelled);
    }
    if ended.timed_out {
        let why = format!("took more than {timeout_secs} s and was stopped");
        return Err(program_error(name, why));
    }
    match ended.status {
        Some(code) if ok_exit.contains(&code) => Ok(()),
        Some(code) => {
            let why = format!("ended with code {code}{}", last_words(&ended.stderr_tail));
            Err(program_error(name, why))
        }
        None => Err(program_error(name, "was killed")),
    }
}

/// Said when the folder cannot give a name without the risk of replacing a file: an action
/// never settles for looking first and renaming after.
fn no_safe_rename() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "this filesystem cannot give a new file its name without the risk of replacing another, so the file was not published",
    )
}

/// `ext` with its dot.
fn dotted(ext: &OsStr) -> OsString {
    let mut out = OsString::from(".");
    out.push(ext);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_line_is_the_one_sha256sum_writes() {
        let hash = "ab".repeat(32);
        assert_eq!(
            checksum_line(&hash, OsStr::new("a file.txt")),
            format!("{hash}  a file.txt\n").into_bytes()
        );
        assert_eq!(
            checksum_line(&hash, OsStr::new("odd\\na\nme")),
            format!("\\{hash}  odd\\\\na\\nme\n").into_bytes()
        );
    }

    #[test]
    fn the_profile_is_a_file_address_inside_the_work_folder() {
        assert_eq!(
            profile_option(Path::new("/tmp/a b/.rada-work-1")),
            "-env:UserInstallation=file:///tmp/a%20b/.rada-work-1/profile"
        );
    }

    #[test]
    fn a_failure_is_told_with_the_last_thing_the_program_said() {
        assert_eq!(last_words("one\ntwo\n\n"), ": two");
        assert_eq!(last_words(""), "");
    }
}
