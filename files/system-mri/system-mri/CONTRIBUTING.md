# Contributing

1. `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
2. **Parsers are pure functions.** New `/proc` readers get a `parse(&str)` that is
   unit-tested against a fixture, plus a thin `read(pid)` wrapper.
3. **Degrade, don't die.** A permission error must become a warning in the
   snapshot, never a crash.
4. **Teach.** Every new panel needs a primer in `src/explain.rs` saying where the
   data comes from.
5. **Clean room.** Read man pages and kernel docs, not the source of the tools we
   mirror. See [docs/CLEAN_ROOM.md](docs/CLEAN_ROOM.md).

Good first issues: syscall tables for aarch64 (`src/syscalls.rs`), decoding of
`sockaddr` for `connect`/`bind`, `mmap` prot/flag names, `/proc/<pid>/limits` tab.
