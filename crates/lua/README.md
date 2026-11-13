# rada-lua

The recipes of [rada](https://github.com/formikadesk/rada-fm), a terminal file manager: small
Lua 5.4 programs that look at the files chosen and ask rada for a plan. They run in a closed
state (no `io`, `os`, `require`, `load` or bytecode; limits on instructions, memory and time)
and can never change a file themselves: what they ask is shown in rada's plan window and done
only if confirmed.

This crate is published so that `cargo install rada --locked` can build the program. It is
released together with `rada`, `rada-core` and `rada-tui`, and **has no stable API**: any
release may change it. If you want a file manager, install [`rada`](https://crates.io/crates/rada).

MIT licence.
