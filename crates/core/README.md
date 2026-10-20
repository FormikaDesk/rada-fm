# rada-core

The engine of [rada](https://github.com/formikadesk/rada-fm), a terminal file manager: the
filesystem model, the plan / execute / undo engine, the journal, archives, previews, updates
and the layer that talks to each operating system.

This crate is published so that `cargo install rada --locked` can build the program. It is
released together with `rada` and `rada-tui`, and **has no stable API**: any release may change
it. If you want a file manager, install [`rada`](https://crates.io/crates/rada).

MIT licence.
