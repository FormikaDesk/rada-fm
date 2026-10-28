# Fuzzing

Targets for `cargo-fuzz` (needs a nightly Rust). Not part of the workspace, not published, and
not run by the CI.

| Target | What it feeds |
|---|---|
| `sanitize` | the name of an archive member (`archive::entry::sanitize`) |
| `journal` | a `journal.jsonl` (`Journal::open`, `damage`, `entries`) |
| `zip_index` | a ZIP archive to `archive::Index::open` |
| `tar_index` | a tar archive to `archive::Index::open` |

```sh
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run sanitize -- -max_total_time=600
```

A crash is written to `fuzz/artifacts/<target>/`. Turn it into a normal test next to the code it
broke, fix the code, and keep the input as the test's data.

Known: `tar_index` finds a 7z header that stops the program by itself (any input that starts
with the 7z signature goes to the 7z library, which asks for memory for the number of files the
header declares). The input is `crates/core/tests/fixtures/fuzz/7z-huge-file-count.bin`; see the
limits in `docs/SAFETY.md`. Until the library is updated it will be found again.
