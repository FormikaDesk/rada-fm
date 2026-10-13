# Debug builds of rada, one target per system.
#
# Each target is meant to be run on the system it names: cross-compiling needs the Rust
# target (`rustup target add <triple>`) and a linker for it, which this file does not set up.
# The triples can be changed from the command line, e.g.
#   make debug_osx OSX_TARGET=x86_64-apple-darwin

CARGO          ?= cargo
WINDOWS_TARGET ?= x86_64-pc-windows-msvc
LINUX_TARGET   ?= x86_64-unknown-linux-gnu
OSX_TARGET     ?= aarch64-apple-darwin

.PHONY: debug_windows debug_linux debug_osx

# target/x86_64-pc-windows-msvc/debug/rada.exe
debug_windows:
	$(CARGO) build -p rada --target $(WINDOWS_TARGET)

# target/x86_64-unknown-linux-gnu/debug/rada
debug_linux:
	$(CARGO) build -p rada --target $(LINUX_TARGET)

# target/aarch64-apple-darwin/debug/rada
debug_osx:
	$(CARGO) build -p rada --target $(OSX_TARGET)
