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

*Not on a phone or a camera:* there the file is written under its final name (the device takes
the data when the file is closed, and cannot be trusted with a rename), and is removed again if
the copy does not complete. See the limits below.

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
inode) taken when the step ran. A file with several names has no status-change time in its
fingerprint (making a link changes it for every name), so one more check covers it: a copy
records a hash of its contents, and an undo that is about to remove its **last** name compares
it. Contents rewritten with the same size and times are kept and reported; a name that has
another name elsewhere is removed as usual, because the contents stay. A copy made by an older
rada has no hash recorded and is judged by the fingerprint alone. Read-only folders come back read-only; undo opens a folder
only for as long as the run needs, and puts its permissions back.

A file that changed after the operation is not removed even if it changes after the plan was
shown: undo looks at it again when it gets to it. *Tests:* `remove_race.rs` replaces the copy
between the plan and the run, and in the instant before it is removed, and puts a link in its
place; `fs/remove.rs` swaps the folder for a link after it was opened. `hardlink_undo.rs` rewrites
a hard-linked copy keeping its size and times.

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

An operation that is still running, in this rada or in another one, is never touched, however
long it has been going on (a copy to a phone can take a day). Each running operation holds a
lock: a small file in `locks/` next to the journal, locked with the system's own file lock
(`flock` on Unix, `LockFileEx` on Windows), from its first journal record to its last. The
start-up recovery settles an operation only after taking that lock, and the system lets go of
it when the process ends in any way, so a killed process never leaves an operation that looks
alive. Only an operation begun by an older rada, which wrote no lock, is judged by its process
id and its age (older than 24 hours counts as dead, in case the number was reused). An undo holds a lock of its
own while it runs (`locks/<operation>.undo.lock`), so two instances of rada cannot undo the same
operation at the same time: the second is refused with a message, and so is a plan that was shown
before the other instance undid the operation. An undo that was
cut short remembers the steps it had done and resumes from where it stopped. A half-written
last line in the journal (the process died while writing it) is ignored, and the next record
written closes it with a mark (`#torn`) so that it is not glued to the fragment.

An unreadable line is one of three things. The last line of the file without its newline is a
write cut short by a crash, and a well-formed line of a record type this version does not know
is from a newer rada: both are skipped. Any other line that cannot be read, with more lines after
it, is **damage** (an edit by hand, a full disk, a bad block). rada then stops trusting the
journal from that point: the operations that had not finished before the damaged line are not
settled at start and not offered for undo, and you are told, at start and when you try to undo
one, which line it is and where `journal.jsonl` is. The operations before it are not affected.
Repair or remove the line, or restore the file from a backup.

*Tests:* `crash.rs` kills a real child process with `SIGKILL` before a file, in the middle of
a big file, just after the rename, inside a folder, in the middle of a cross-filesystem move,
and between the copy and the removal of a move. `op_lock.rs` ages an operation held by a live
child to two days and checks it is left alone, then kills the child and checks it is settled.
`journal_lines.rs` covers the three kinds of unreadable line, and `undo_twice.rs` the second
instance that undoes the same operation while the first runs and after it has finished.

### An update keeps a way back, whole at every moment

Before a new program replaces the running one, the old one is copied to the folder of saved
programs (`update/previous`), under a name made for the occasion, and written to the disk.
`previous.json` says which version it is, where it was installed, which file it is and how
long that file is; it is written whole and atomically (a temporary file, written to the disk,
then renamed over it), and the write is what makes the copy "the saved program". The old copy
and the old description stay valid until the new description replaces them, and only then is
the old copy removed. A stop at any step (a power cut, `kill -9`) leaves either the old way
back or the new one, never a description without its program or with the wrong one; a copy
that was cut short, or whose length is not the one written down, is not offered to
`--rollback`; what a stop left behind is removed by the next save. Descriptions written by
earlier versions are still read.

The new program is written beside the one it replaces under a name with a random part, to a
file that must not exist yet (`create_new`; on Unix also `O_NOFOLLOW`), so a symbolic link left
at that name is never written through. The same holds for the copy made for `--rollback` and
for the program a source update has built.

On Windows a running program cannot be overwritten but can be renamed, so the new program is
swapped in with one `ReplaceFileW` call, which moves the old file aside (`.rada-old-…exe`, in
the same folder) and puts the new one in its place. That call is not transactional (nothing on
Windows is, short of the deprecated transactional file system), so a power cut inside it can
still leave `rada.exe` missing, and with it nothing to start by that name. What rada does about
it: the old file is a complete, working program, and when it is started it sees that the name
is empty, takes it back, says so and exits; the same can be done by hand by renaming
`.rada-old-…exe` to `rada.exe`. The fallback, when `ReplaceFileW` cannot be used, is the two
renames, the second undone if it fails.

*Tests:* `update/install.rs` stops the saving after each of its steps and reads the way back,
leaves a cut copy and a named but undescribed one in the folder, truncates the saved program,
refuses a description that names a path, and reads and replaces the layout of earlier versions.
It also plants links at the names a new program is written to (from an archive, from a
rollback) and checks nothing behind them changes. On Windows the CI replaces a copy of the
test program that is running, with `ReplaceFileW` alone and through `replace_exe`.

### Nothing follows a link out of a tree

Symbolic links are copied as links (broken and circular ones too) and never followed. The
scanner remembers the identity of every folder above it, so a mount or bind cycle cannot make
it loop. A filesystem root is never accepted as a source.

### Hostile names are harmless

Newlines, escape sequences, bidirectional controls and invalid UTF-8 in names are shown as
visible escapes. Paths are never handled as text.

The same goes for every other string that does not begin inside rada: the answer of a program
(`gio`, `udisksctl`, `git`, `pdfinfo`...), the label of a disk or a phone, the notes of a
release, the lines of the history, what a file says about itself (the title of a PDF, the
interpreter of a program, the name of a tag). A worker's event is cleaned once, as it enters the
interface (`display::text`: `\e`, `\n`, `\r`, `\u{202e}`), not in each window; a line being
edited is cleaned as it is drawn and left untouched as it is typed. The tests of the interface
check every frame they draw: no cell may hold a control character or a bidirectional mark.
`hostile_text.rs` brings such text from each of those places.

Where the filesystem ignores case (macOS, Windows), a rename that changes only the case of a
name is a rename, not a conflict: the new name is taken by the file itself, which is checked
(same folder, same file, and the folder lists only the old spelling) before the rename goes on,
and the undo, alone or as part of a bulk rename, compares names without regard to case.
`rename_case.rs` runs this on a stand-in filesystem that ignores case, so on every system.

A name that starts with `-` (`-rf`, `--help`) is never read by a program rada runs as an
option: paths are handed over absolute, and a relative one gets `./` in front. Git is given the
folder as the value of `-C`, the archive tools get `--` before the archive, and `zoxide` too.
`dash_names.rs` runs the poppler tools (stand-ins that write down what they were given) and a
real git on folders and files with such names.

Where Windows rules apply, the names it keeps for devices are refused for a rename, a new
folder or file, every member of an archive and the folder an extraction makes: `CON`, `PRN`,
`AUX`, `NUL`, `COM1`-`COM9` and `LPT1`-`LPT9` in any case, with an extension (`NUL.txt`), with
trailing dots or spaces (`NUL.`, `CON .txt`) and with superscript digits (`COM¹`). The tests
(`windows_names.rs`) run these rules on every system.

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
- **Permissions are not a way in.** Setuid, setgid and sticky bits are dropped from everything
  an archive extracts, files and folders alike (the plan counts the setuid files); the executor
  drops them again from a plan it is given. Other permissions and times are kept as the archive
  gives them. A copy of your own files is a different thing and keeps all of these bits, as
  said under "Copies keep what they can".
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
- **Removing a name is checked, not guaranteed.** Before it removes a file (undo of a copy,
  delete) rada compares the file with its fingerprint (size, times, kind) at the last moment:
  on Linux and macOS it opens the folder once, looks at the name with `fstatat` (never
  following a link) and takes it off with `unlinkat` through that same open folder, so a
  folder swapped for a link in between is not followed; on Windows it opens the file so that
  nobody can write, rename or delete it, looks at it through that handle and deletes it through
  the same handle. What stays open on Linux and macOS is the instant between `fstatat` and
  `unlinkat`: POSIX has no call that removes a name only if it still points at the same file,
  so a program that renames another file over the name in that instant has it removed. The
  window is a few microseconds and needs another program aimed at exactly that name; the
  fingerprint check on either side of it is what makes it unlikely to matter. A file that is
  open for writing in another program cannot be deleted on Windows (the system refuses; rada
  says so and nothing is lost).
- **Phones and cameras** (MTP and PTP, through the desktop's virtual filesystem) are not a
  filesystem rada can make promises about. A copy there is the same plan with a progress bar,
  a cancel and a journal entry, and the plan says what is different before anything is done:
  the file is written under its final name and reaches the device when it is closed (what the
  device kept is measured, and a move or a verified copy is read back, before the source is
  touched); permissions, owner and attributes are not kept; links are skipped; renaming and
  moving are not atomic; there is no trash (a delete goes through a verified copy into the
  trash of this computer); and **undo is not guaranteed**, because the device cannot promise
  that what is on it is as rada left it. Undo of a copy *from* a phone removes the copy, here,
  and is as sure as any. This was written from the documented behaviour of gvfs and tested
  against folders standing in for devices, not against a real phone.
- **Windows and macOS** are a pre-release. The engine, the journal and undo are the same
  code and the same tests as on Linux, run in CI on both; the Recycle Bin (Windows) and
  `~/.Trash` (macOS) are real. What is not done: preservation of attributes (ACLs and
  alternate data streams on Windows, extended attributes and resource forks on macOS) is a
  documented stub, and the plan says "this platform does not carry attributes over" instead
  of pretending. The one exception is **tags**: on macOS a copy or a move carries the
  Finder's tags over (and only them, of the extended attributes). A change of tags is an
  ordinary operation, with a plan, a journal entry and an undo that refuses to overwrite
  tags somebody changed after it. A move inside a volume is a rename on every system; across volumes it is a
  copy that is verified before the source is removed, as on Linux. Nothing here has been
  used for long on a real machine, so keep backups.
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

Outside the test run, `fuzz/` holds `cargo-fuzz` targets for the names of archive members, the
journal and the zip and tar listings (see `fuzz/README.md`). They are run by hand and not by
the CI. What they find becomes an ordinary test in `archive_fuzz.rs`.

Review of the operations engine (`crates/core/src/ops/`, `journal.rs`) by people who did not
write it is the most valuable contribution this project can get. See
[CONTRIBUTING.md](../CONTRIBUTING.md).
