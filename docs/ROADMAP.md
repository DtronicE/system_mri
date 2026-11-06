# Roadmap

Phases 1–3 are implemented. Later phases are designs, not promises.

## Phase 1 — MVP ✅
procfs layer, process picker, Overview / Memory / Files / Network / Threads, explain pane, `--dump`.

## Phase 2 — Syscall tracing ✅
ptrace tracer, argument decoding (fds, paths, `open` flags), log + summary views.
*Next:* decode `sockaddr` for `connect`/`bind`, `mmap` prot/flags, `-f` follow
fork/exec (`PTRACE_O_TRACEFORK|VFORK|EXEC`), filtering (`/errors`, `/openat`).

## Phase 3 — Counters & wait states ✅
Per-thread `perf_event_open` counters, IPC / miss ratios, wait-state profiler.
*Next:* `perf_event_open` **groups** so ratios use perfectly aligned counts;
multiplexing scaling (`time_enabled`/`time_running`).

## Phase 4 — Sampling profiler (design)
* `perf_event_open` with `sample_period` + `PERF_SAMPLE_IP|TID|CALLCHAIN`, mmap'd
  ring buffer, parse `PERF_RECORD_SAMPLE`.
* Symbolize: `/proc/<pid>/maps` → ELF `.symtab`/`.dynsym` (`object` crate) →
  demangle (`rustc-demangle`, `cpp_demangle`); DWARF line info via `addr2line`.
* Aggregate into a call tree; render as a TUI flame graph and export
  folded stacks for external flame-graph tools.

## Phase 5 — Deeper insight (design)
* `/proc/<pid>/limits`, cgroup limits/usage (`/sys/fs/cgroup`), namespaces.
* eBPF (via `aya`) for low-overhead syscall latency histograms and off-CPU stacks.
* `sock_diag` netlink for exact socket stats (RTT, cwnd) as in `ss -ti`.
* Page-level memory: `/proc/<pid>/pagemap` + `kpageflags` (root) to show residency maps.

## Phase 6 — Time travel (design)
Record snapshots + trace to a file, replay and diff in the TUI ("what changed
between 14:02 and 14:05?"). Export JSON.

## Hardening backlog
aarch64 syscall table; `smaps_rollup` fast path; MSRV pinning in CI; fuzzing the
`/proc` parsers (`cargo-fuzz`); packaging (`cargo-dist`, AUR, Homebrew-on-Linux).
