# rada

A safe harbor for your files: a terminal file manager where every operation shows its plan
before it runs, every change is written to a journal that can take it back, and copying
checks what it wrote.

![rada: look around, select three items, copy them with a plan first, see the card, take it back with Ctrl+Z](https://raw.githubusercontent.com/formikadesk/rada-fm/main/docs/media/demo.gif)

## Install

```sh
cargo install rada --locked
```

You need Rust 1.90 or newer ([rustup](https://rustup.rs)). The program is put in
`~/.cargo/bin`; update it with the same command. Prebuilt binaries for Linux, macOS and
Windows are on the [releases page](https://github.com/formikadesk/rada-fm/releases).

While only pre-releases are published, name the version: `cargo install rada --locked --version 0.6.0-rc.15`.

## More

- [Features](https://github.com/formikadesk/rada-fm/blob/main/docs/features.md) and [using rada](https://github.com/formikadesk/rada-fm/blob/main/docs/usage.md)
- [What is guaranteed and what is not](https://github.com/formikadesk/rada-fm/blob/main/docs/SAFETY.md)
- [Installing](https://github.com/formikadesk/rada-fm/blob/main/docs/install.md) and [updates](https://github.com/formikadesk/rada-fm/blob/main/docs/updates.md)
- [Source and issues](https://github.com/formikadesk/rada-fm)

This crate is the program. The engine is [`rada-core`](https://crates.io/crates/rada-core) and
the interface is [`rada-tui`](https://crates.io/crates/rada-tui); they are released together
with it and have no stable API of their own.

MIT licence.
