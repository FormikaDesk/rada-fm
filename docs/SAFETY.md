# What rada guarantees, and what it does not

rada moves your files around, so this page says plainly what you can rely on, how that is
checked, and where the promises stop. Every statement here is backed by a test in
`crates/core/tests/` unless it is listed under [Limits](#limits-known-and-by-design).

If rada ever loses or damages a file in a way this page says it should not, that is the most
important kind of bug report there is.

## The model in one paragraph

An operation is first turned into a **plan**: a list of small steps (make a folder, copy a
file, rename, remove…). Nothing is touched until you confirm. The executor then runs the steps
one by one. Each step that finishes writes **how to undo it** to the journal
(`$XDG_STATE_HOME/rada/journal.jsonl`) at once. Undo is itself a plan, built from those
records, that you see and confirm.

## Guarantees

### A file never appears half-written

A file is copied to a hidden temporary name next to its destination
(`.rada-part-<pid>-<n>`) and renamed to its final name only when complete, with a rename that
refuses to replace anything (`renameat2` with `RENAME_NOREPLACE`). A name that appeared in the
meantime is never overwritten; the step fails with "already exists".

*Tests:* `faults.rs` (full disk in the middle of a file, rights revoked, destination unplugged,
file created meanwhile), `crash.rs`.

### One failure does not stop the rest

Errors are handled per step. You choose to skip, skip all, retry or abort; the message always
names the full path. A source that shrinks, changes or disappears while it is being copied is
reported for that file and the others carry on.

### Moves across filesystems cannot lose data

A move across filesystems is copy, **verify** (a checksum of what was written, read back),
then remove the source, file by file. The source is removed only after its copy verified. If
the source vanishes during the copy, the copy is kept.

### Copies keep what they can, and say what they could not

Kept, on Linux: contents, permissions including setuid/setgid/sticky, modification and access
times (files and folders), extended attributes (every namespace the system lets you read and write;
`trusted.*` needs privilege), POSIX ACLs, hard links **between the files you copy together**, and holes in
sparse files (a 1 GiB file that holds 4 KiB of data takes 4 KiB; progress counts real bytes).
Owner and group are kept when the system lets you (root, or the same user).

Before you confirm, the plan lists what the destination cannot hold (for instance a FAT
or exFAT drive has no owners, no ACLs, no hard links, no extended attributes). During the run,
anything that could not be kept for a file is collected and shown in the result window; it is
never silently dropped, and it is never an error that stops the copy.

*Tests:* `fidelity.rs`.

### Undo puts things back exactly

Undo of a copy removes exactly what the copy created (every name of a hard-linked file, and
nothing else). Undo of a move moves back, with the same contents, permissions, times, hard
links and holes. Undo of trash restores from the trash. Undo of an overwrite brings the old
file back. A file that was **modified after** the operation is left alone and reported,
rather than overwritten or deleted; this is decided by comparing a fingerprint (size, times,
inode) taken when the step ran. Read-only folders come back read-only; undo opens a folder
only for as long as the run needs, and puts its permissions back.

*Tests:* `properties.rs` generates random trees (symlinks including broken ones, hard links,
sparse files, odd names, read-only files and folders) and checks that copy → undo and
move → undo, on one filesystem and across two, give back exactly the starting state, and that
a copy is the same tree as its original. They are run with a few dozen cases by default and
`PROPTEST_CASES=1000 cargo test -p rada-core --test properties --release` runs a long hunt.

### A killed process leaves a coherent state

rada writes each journal record to the operating system as soon as it is produced, and writes
a *pending* record before each step that creates something. If the process is killed
(`SIGKILL`, the terminal closing, an out-of-memory kill) at any moment, the next start looks
at the one step that was in doubt and settles it:

- the half-written temporary file is deleted;
- a copy that had finished but was not yet recorded is added to the journal, so undo still
  covers it;
- a move killed between the copy and the removal of the source keeps **both**; undo then
  removes the copy;
- the operation is marked *interrupted* in the history, and you are told once.

An operation that is still running in another rada instance is never touched. An undo that was
cut short remembers the steps it had done and resumes from where it stopped. A half-written
last line in the journal (the process died while writing it) is ignored.

*Tests:* `crash.rs` kills a real child process with `SIGKILL` before a file, in the middle of
a big file, just after the rename, inside a folder, in the middle of a cross-filesystem move,
and between the copy and the removal of a move.

### Nothing follows a link out of a tree

Symbolic links are copied as links (broken and circular ones too) and never followed. The
scanner remembers the identity of every folder above it, so a mount or bind cycle cannot make
it loop. A filesystem root is never accepted as a source.

### Hostile names are harmless

Newlines, escape sequences, bidirectional controls and invalid UTF-8 in names are shown as
visible escapes. Paths are never handled as text.

### Archives are read, never trusted

An archive is data from a stranger, and rada treats it so. The checks below are on the plan
and on the executor, and each has a test (`archives.rs`, `archives_write.rs`,
`archives_crash.rs`, `archives_browse.rs`).

- **Nothing is written outside the destination.** A member whose path has `..`, starts at the
  root or a drive, contains a NUL, or (in a ZIP) uses `\` to climb out is **not extracted**; the
  plan names how many and why. This holds for ZIP, tar, 7z and RAR alike. Nor does a member
  go through a symbolic link that the same archive created (the classic way round a path check).
- **Links stay links.** A symlink in an archive is created as a link and never followed. If it
  points outside the extracted folder the plan says so first.
- **Permissions are not a way in.** Setuid and setgid bits are dropped from extracted files (the
  plan counts them). Other permissions and times are kept as the archive gives them.
- **Bombs ask for a typed `yes`.** An extraction that would write more than 8 GiB, or that
  unpacks to more than 200 times the archive's size (from 256 MiB up), carries a warning
  that has to be confirmed by typing; both limits are in `[archives]`. Independently of the
  list, a member is never allowed to write more bytes than the archive declared for it, so a
  lying header cannot get round the check.
- **A file never appears half-written, and a killed extraction is cleaned up.** Members are
  written under the same hidden temporary name and renamed into place as every copy is; each
  one is recorded in the journal as it completes. After a `SIGKILL` the next start removes
  the temporary file and undo removes exactly what was extracted (a member you edited since
  is left alone and reported).
- **Errors are never silent.** A truncated or damaged archive is reported when it is read
  (what could be listed is offered, with a warning), and when it breaks during extraction the
  member that failed is reported with its full path. A full disk is an error for that
  member and leaves nothing under its name.
- **Archives are read-only.** Inside one, rename, move, delete, new folder and paste are
  refused with the way to do what was meant (copy the items out).
- **Passwords are not supported yet.** A protected ZIP or 7z is recognised and the plan says
  so; nothing is extracted. A password is never asked for, stored or logged because it is
  never used.
- **Compressing writes a new archive under a temporary name** and renames it into place;
  cancelling removes it, and undo removes the finished one (if you changed it, it is kept).

*Tests:* zip-slip, tar-slip and link tricks in `archives.rs`; bombs and declared sizes;
password-protected ZIP and 7z made with real tools; a cut or damaged archive before and after
the plan; the full disk; `SIGKILL` in the middle of a big member in `archives_crash.rs`; and
`archives_write.rs`, where random trees (odd names, symlinks, read-only folders, permissions)
are compressed and extracted in all four formats and must come back identical.

## Limits, known and by design

These are real, and we would rather you read them here than discover them.

- **Power loss and kernel crashes are not the same as a killed process.** The journal is
  forced to disk when an operation begins and ends, but the records of individual steps are
  only guaranteed to have reached the operating system. File contents are forced to disk only
  when the copy is verified (moves across filesystems always are; copies are not). After a
  power cut, a copy that rada reported as finished can be missing or incomplete, and the
  journal can lack its last few steps. The temporary-name-and-rename rule means a *partial*
  file does not carry a final name on filesystems that order rename after data (ext4 and
  btrfs do in practice), but rada cannot promise it for every filesystem.
- **Extended attributes on tmpfs.** Before Linux 6.6, `tmpfs` refuses `user.*` attributes.
  The plan says so before you confirm and the result window lists the files that lost them.
- **Owner and group** are kept only when the system allows changing them (normally root).
  Otherwise the files belong to you and the plan says so.
- **Symbolic links** are recreated with the same target, but their own owner and times are not
  carried over.
- **Hard links outside the selection.** If you copy only one name of a file that has other
  names elsewhere, the copy is an independent file (a warning in the plan says so). Hard
  links are recreated only between files copied in the same operation.
- **Some destinations cannot hold links.** If creating a hard link at the destination is not
  possible, rada makes a separate copy of the file and notes it in the result.
- **ACLs and attributes other than POSIX ACLs and xattrs** (for example immutable flags,
  `chattr`, file capabilities that the destination refuses) are not copied beyond what the
  extended-attribute interface carries.
- **Undo is bounded by what is still true.** If you changed or removed a file after the
  operation, or the destination is gone, undo does what is safe and tells you what it left.
- **A file that changes while it is being copied** is copied as it was read; with verification
  (cross-filesystem moves) the mismatch is detected and the step fails, with the source kept.
- **Races you cause.** rada checks that a destination name is free at the moment it creates
  it, never earlier, but it cannot stop another program from changing the folder between two
  of its steps.
- **Windows and macOS** are not supported. The platform layer compiles for both, and
  preservation of attributes there is a documented stub: the plan says "this platform does
  not carry attributes over" instead of pretending.
- **The journal is a file you own.** Anyone who can edit `journal.jsonl` can change what undo
  will propose. Undo still checks the fingerprints, and shows its plan before doing anything.
- **Network and FUSE filesystems** may report sizes, times and links the way their server
  decides. Nothing here has been tested against a flaky network share beyond the
  per-mount timeout used when listing disks.
- **Archives.** RAR is read only through `7z`, `7zz` or `unrar` when one is installed; that
  path is tested with 7-Zip (on a 7z) and with a stand-in program that prints what `unrar` prints,
  not with a real RAR file, which cannot be made without the proprietary compressor. Archives
  you create hold contents, permissions, times and symlinks, **not** owner, extended attributes,
  ACLs, hard links or sparse holes (a hard-linked file is stored twice). ZIP keeps times to the
  second (rada also writes the exact UTC time, but other programs may read only the 2-second
  DOS one). Extraction does not restore the owner stored in a tar. A compressed tar has to be
  read once to be listed and once to be extracted. 7z cannot be created, only read. Listing a
  very large compressed archive in the preview stops after about 1.5 s and shows "at least".

## How this is tested

`cargo test --workspace` runs everything above. The tests never touch your real folders: each
runs in a sandbox with `HOME` and every `XDG_*` variable pointed into it. Cross-filesystem
tests use `/dev/shm` against the project disk. The fault tests inject failures (a write that
fails after N bytes, a rename that returns `EACCES`) through a wrapper filesystem; the crash
tests run a child process and kill it for real.

Review of the operations engine (`crates/core/src/ops/`, `journal.rs`) by people who did not
write it is the most valuable contribution this project can get. See
[CONTRIBUTING.md](../CONTRIBUTING.md).
