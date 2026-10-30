# Development

## How it is built

A Cargo workspace:

- `crates/core` — filesystem model, the plan/execute/undo engine, journal, watcher, workers, platform layer. No dependency on any interface.
- `crates/tui` — the interface (ratatui + crossterm).
- `crates/rada` — the `rada` binary.

Every operation is expressed as a list of atomic **steps** (`MakeDir`, `CopyFile`, `CopySymlink`, `ExtractFile`, `Compress`, `Rename`, `TrashItem`, `RemoveFile`, …). A step knows its own inverse, so the same machinery plans, executes, journals and undoes. Everything that depends on the operating system sits behind one `Platform` trait (trash, volumes, file attributes, opening files, path rules); the engine never branches on the OS. The trait models what Windows needs (drive letters, NTFS junctions and reparse points, hidden/system attributes, the Recycle Bin). Linux is complete; Windows and macOS are a pre-release: trash, volumes, standard folders and opening files are implemented and tested in CI, and carrying attributes over (ACLs, alternate data streams, resource forks) is not done. See [platforms.md](platforms.md).

### Tests

```sh
cargo test --workspace
```

The interface is tested headlessly with real workers on a sandboxed filesystem: every action through both key schemes, simulated mouse events (click, double click, wheel, right click, modifiers), and committed text snapshots of the screens (`crates/tui/tests/snapshots/`, pinned clock and home folder; update with `INSTA_UPDATE=always cargo test -p rada-tui --test snapshots` and review the diff).

The suite includes regression tests for real bugs found in other terminal file managers: extraction into the wrong folder when a folder above has a dot in its name, `tar.xz` and `tar.bz2` that failed without an error, folders with dots in their names, symlinks (to files, to folders, broken, circular) inside copied trees, an unreadable file in a copied folder, moving and trashing across filesystems (tmpfs ↔ disk), Unicode/emoji/special/non-UTF-8 names, and undo of every kind of operation including "modified after the operation".

Tests never touch your real folders: each one runs in a temporary sandbox with `HOME` and all `XDG_*` variables pointing into it, and a guard fingerprints your real trash, config, state and cache before the first test and fails the test if anything under them changed.

CI (GitHub Actions) checks formatting and lints and builds with the minimum Rust version on Linux, and runs the whole suite — core, interface and program — on Linux, Windows and macOS; all of it must pass. A test that cannot run on a system (POSIX permissions on Windows, a second filesystem to move across, the freedesktop trash layout, names that are not valid UTF-8 on macOS) is marked `#[ignore = "<reason>"]`, so the reason shows in the output. The tests on Windows and macOS run on GitHub's machines, with the trash kept in a folder of the sandbox: they say that the code works there, not that people have lived with it.

The fuzz targets in `fuzz/` (names of archive members, the journal, ZIP and tar archives) run for five minutes each in a weekly workflow, `.github/workflows/fuzz.yml`, and by hand from the Actions tab; a crash is kept as an artifact. They do not run on a push or a pull request. See [fuzz/README.md](../fuzz/README.md).

## Contributing code

Bug reports and ideas are welcome as issues; the templates ask for what helps most (version, terminal, steps). If rada ever loses or damages a file, say so first: that is the one kind of bug that matters most, and the journal (`U`) may still be able to undo it. The best thing you can do for the project is to read the operations engine and try to break it: see [CONTRIBUTING.md](../CONTRIBUTING.md) for how to build, test and where help is needed.

For code: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass. Changes to the look of the interface come with updated screen snapshots (`INSTA_UPDATE=always cargo test -p rada-tui --test snapshots`) whose diff you have reviewed. New operations must be expressed as plan steps that know their own inverse. Tests never touch your real folders and must keep it that way.
