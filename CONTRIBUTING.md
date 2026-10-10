# Contributing to rada

Thank you for looking. rada is a small project with a large promise — your files are safe in
it — so the most useful contributions are not always code.

## Where help is needed most

1. **Review of the operations engine.** `crates/core/src/ops/` (planner, executor, undo),
   `crates/core/src/journal.rs` and `crates/core/src/ops/recover.rs` decide whether a file is
   ever lost. They were written by one person. A second pair of eyes — reading a step, asking
   "what if the process dies *here*?" — is worth more than any feature. Start with
   [docs/SAFETY.md](docs/SAFETY.md), which states what is promised and what is not, and try
   to break a promise.
2. **Windows and macOS.** The platform layer (`crates/core/src/platform/`) has a real trash,
   volumes, standard folders and opening files for both, and CI runs the whole suite on
   them. What it needs is someone who uses those systems every day, and the parts still
   missing: ACLs and alternate data streams (Windows), extended attributes and resource
   forks (macOS), junctions, the Finder's "Put Back".
3. **Packaging.** Distribution packages and a simple install path.
4. **Terminals.** Reports of what works and what does not in your terminal: image protocols,
   keys, colours (version, terminal, `$TERM`, and what you saw).
5. **Real-world bug reports.** Especially anything involving odd file names, network or FUSE
   filesystems, very large folders, or a crash in the middle of an operation.

If rada ever loses or damages a file, please say so first and keep the journal
(`$XDG_STATE_HOME/rada/journal.jsonl`): it usually shows exactly what happened.

## Build and run

You need Rust 1.90 or newer ([rustup](https://rustup.rs)) on Linux, or on Windows or macOS (pre-release).

```sh
git clone https://github.com/formikadesk/rada-fm rada
cd rada
cargo run -p rada -- ~/some/folder        # run it
cargo build --release                     # target/release/rada
```

PDF previews use poppler (`pdftoppm`, `pdfinfo`, `pdftotext`); without it they fall back to
text and say how to install it.

## Run the tests

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass. Tests never touch your real folders: each one runs in a sandbox with
`HOME` and every `XDG_*` variable pointed into it, and a guard fails the run if anything in
your real trash, config, state or cache changed.

Useful to know:

| What | Where |
|---|---|
| Copy, move, undo, plan behaviour | `crates/core/tests/` |
| Fault injection (full disk, revoked rights, vanishing files) | `crates/core/tests/faults.rs`, `FaultFs` in `testutil.rs` |
| Killing a real process in the middle of an operation | `crates/core/tests/crash.rs` |
| Random trees: copy → undo and move → undo restore the start | `crates/core/tests/properties.rs` (`PROPTEST_CASES=1000` for a long run) |
| Screens and keys, headless | `crates/tui/tests/` (snapshots: `INSTA_UPDATE=always cargo test -p rada-tui --test snapshots`, then review the diff) |
| The JSON schemas of requests and plans | `schema/` — regenerated with `RADA_UPDATE_SCHEMA=1 cargo test -p rada-core --test schema` |

## Rules of the house

- **Operations are plan steps that know their own inverse.** A new operation is a new way of
  producing steps, not new code that touches files on its own.
- **Nothing blocks the interface thread.** I/O happens in workers.
- **The engine never branches on the operating system.** Anything that differs goes behind the
  `Platform` trait, with a Linux implementation and a stub that says what is missing.
- **Paths are `Path`/`OsString`, never text.**
- A change to the look comes with updated snapshots whose diff you reviewed.
- A bug fix comes with a test that fails without it.
- Small commits, with a message that says *why*.

## Proposing a change

Open an issue first for anything bigger than a fix, so we can agree on the direction. Pull
requests should say what changed, how you checked it, and which of the promises in
[docs/SAFETY.md](docs/SAFETY.md) they touch, if any.

## Design note: tabs and a future split view

A tab's state (`TabState` in `crates/tui/src/app/tabs.rs`: folder, history, cursor, scroll, selection, sort, view, filter and its `Viewport`) does not know where on the screen it is drawn. The list and the grid (`ui/list.rs`, `ui/grid.rs`) draw the front tab into whatever `Rect` they are given; what the terminal's width decides (which columns exist) is passed in as `ListOpts`. This is on purpose: a later release may show two tabs side by side, with one of them focused and copy / move towards the other. Keep it that way: nothing a tab remembers should depend on the layout of the screen, and nothing that draws a tab should read the whole screen's size.
