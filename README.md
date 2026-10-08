# vela

**A terminal file manager you can trust.**

vela is a fast, keyboard-driven file manager for the terminal, written in Rust. It is built around three promises:

1. **Plan first.** Every operation on files shows a *plan* before anything is touched: what will be done, how many files and bytes, and warnings about symlinks, permissions and name conflicts. You confirm, or you don't.
2. **Undo always.** Every operation that finishes is recorded in a persistent journal and can be undone, even after a restart or a crash. If a file was modified after the operation, undo leaves it alone and tells you, instead of overwriting it.
3. **Robust by construction.** Progress is counted in bytes. A failure on one file never stops the rest: you choose to skip, retry or abort, and the message always carries the full path. Symlinks stay symlinks (even broken or circular ones), nothing follows a link out of the tree, and a half-written file never appears under its final name.

> **Status:** early (phase 1). Linux is fully supported. Windows and macOS are **in development**: the code is structured for them and compiles, but they are not supported yet.

## Why it is different

| | |
|---|---|
| **Plan window** | Copy, move, rename, bulk rename, new folder, trash, delete: all show a plan with totals and warnings first. Permanent deletion states clearly that it cannot be undone and asks you to type `yes`. |
| **Journal + undo** | `u` undoes the last operation (with its own plan, so you see what undo will do). `U` shows the history. Undo of a copy removes exactly what the copy created; undo of a move moves back; undo of trash restores from the trash; undo of an overwrite brings the old file back. |
| **Per-file errors** | One unreadable file does not abort a 10,000-file copy. Errors are handled per step: skip / skip all / retry / abort. |
| **Byte progress** | A real progress bar with throughput and ETA, not "3 of 4 items". |
| **Safe across filesystems** | Moves between filesystems are copy → verify (checksum of what was written) → remove source, file by file; the source is never deleted for something that failed to copy. Trashing follows the freedesktop.org specification, including per-device trash directories. |
| **Live** | The folder view updates by itself (inotify via `notify`), with debouncing. Sorting is stable and deterministic and applies instantly. |
| **Nothing blocks the UI** | All I/O happens in worker threads; the interface thread only draws and handles keys. Volumes are listed by a worker, never on a keypress, with a timeout per mount so a hung network share cannot freeze anything. |
| **Hostile names are harmless** | Newlines, escape sequences, bidi controls and invalid UTF-8 in file names are shown as visible escapes. Paths are `Path`/`OsString` everywhere, never text. |

## Install

Requires a recent stable Rust toolchain.

```sh
git clone <this repository> vela
cd vela
cargo install --path crates/vela
```

Or just build it: `cargo build --release` and use `target/release/vela`.

Optional: make your shell follow vela's last folder (`v` instead of `vela`):

```sh
eval "$(vela --init bash)"        # also: zsh, fish, nushell, powershell
```

## Usage

```sh
vela [PATH] [--icons nerd|unicode|none] [--hidden]
```

Icons default to plain Unicode markers. With a [Nerd Font](https://www.nerdfonts.com/) installed, use `--icons nerd` (or `icons = "nerd"` in the config file).

Optional config: `$XDG_CONFIG_HOME/vela/config.toml`

```toml
icons = "nerd"        # nerd | unicode | none
show_hidden = false
sort = "name"         # name | size | date
reverse = false
```

State lives in `$XDG_STATE_HOME/vela/` (`journal.jsonl`, `log/`). Set `VELA_LOG=debug` for more logging (written to a file, never to the screen).

## Keys

| Key | Action |
|---|---|
| `j` `k` / `↓` `↑` | move |
| `l` `→` `Enter` | open folder / open file with the system opener |
| `h` `←` `Backspace` | parent folder (the cursor returns to where you were) |
| `g` `G` · `PgUp` `PgDn` | top / bottom · page |
| `J` `K` | scroll the preview |
| `Space` | mark and move down · `Ctrl-a` marks everything |
| `y` `x` `p` | copy · cut · paste (shows a plan first) |
| `d` | move to trash |
| `D` | delete permanently (cannot be undone; type `yes`) |
| `r` / `F2` | rename |
| `R` | bulk rename with a pattern: `{name}` `{ext}` `{n}` `{n:3}` `{parent}` `{name:lower}` or `s/find/replace/` |
| `n` | new folder |
| `u` | undo the last operation |
| `U` | history of operations (Enter undoes the selected one) |
| `s` `S` | cycle sort key (name, size, date) · reverse |
| `.` | show / hide hidden files |
| `m` `~` | volumes and places · home |
| `Esc` | cancel the running operation · clear marks |
| `?` | help |
| `q` | quit |

In the plan window: `Enter` runs, `c` changes how name conflicts are resolved (skip existing / keep both / overwrite, where the old file goes to the trash so undo can restore it), arrows scroll, `Esc` cancels.

## How it is built

A Cargo workspace:

- `crates/core` — filesystem model, the plan/execute/undo engine, journal, watcher, workers, platform layer. No dependency on any interface.
- `crates/tui` — the interface (ratatui + crossterm).
- `crates/vela` — the `vela` binary.

Every operation is expressed as a list of atomic **steps** (`MakeDir`, `CopyFile`, `CopySymlink`, `Rename`, `TrashItem`, `RemoveFile`, …). A step knows its own inverse, so the same machinery plans, executes, journals and undoes. Everything that depends on the operating system sits behind one `Platform` trait (trash, volumes, file attributes, opening files, path rules); the engine never branches on the OS. The trait already models what Windows needs (drive letters, NTFS junctions and reparse points, hidden/system attributes, the Recycle Bin), with a complete Linux implementation and compilable stubs for Windows and macOS.

### Tests

```sh
cargo test --workspace
```

The suite includes regression tests for real bugs found in other terminal file managers: folders with dots in their names, symlinks (to files, to folders, broken, circular) inside copied trees, an unreadable file in a copied folder, moving and trashing across filesystems (tmpfs ↔ disk), Unicode/emoji/special/non-UTF-8 names, and undo of every kind of operation including "modified after the operation".

Tests never touch your real folders: each one runs in a temporary sandbox with `HOME` and all `XDG_*` variables pointing into it, and a guard fingerprints your real trash, config, state and cache before the first test and fails the test if anything under them changed.

CI (GitHub Actions) builds and runs the core tests on Linux, Windows and macOS. Tests that are not supported yet on Windows/macOS are marked `#[ignore = "<reason>"]`.

## Roadmap

Phase 2 and beyond (not in phase 1):

- Dual pane, tabs, layout restore
- Archives (browse and extract safely, all common formats)
- Git integration
- Image preview (Kitty / iTerm2 / Sixel, terminal capability detection)
- Syntax-highlighted preview, scrollable preview focus
- Mouse support
- Search and filter, recursive sizes
- Trash browser (list, restore, empty)
- Compare and sync two folders with a dry-run plan
- Remote filesystems (SFTP) through the same engine
- Plugins
- Preserve extended attributes, ACLs, hard links and sparse files when copying
- Full Windows and macOS support (Recycle Bin, Trash, volumes, drive letters, junctions)
- Signed binaries and distribution packages

## License

MIT © Alessandro Formica. See [LICENSE](LICENSE).
