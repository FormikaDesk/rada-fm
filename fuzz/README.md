# Fuzzing

Targets for `cargo-fuzz` (needs a nightly Rust). Not part of the workspace, not published, and
not run on a push or a pull request: a weekly workflow (`.github/workflows/fuzz.yml`, also
started by hand) runs each one for five minutes and keeps the input of a crash as an artifact.

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
