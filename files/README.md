# System MRI

**X-ray vision for Linux processes.** Point it at any PID and see what the kernel
knows: memory layout, open files, sockets, threads, live syscalls and CPU
counters — each paired with a plain-English note on *where Linux gets that data*.

```text
┌ System MRI · pid 725 (demo_target) ────────────────────────────────────────────┐
│ 1 Overview │ 2 Memory │ 3 Files │ 4 Network │ 5 Threads │ 6 Syscalls │ 7 Perf │
└────────────────────────────────────────────────────────────────────────────────┘
┌ Syscall log (following) ───────────────────────────┐┌ What am I looking at? ────┐
│ 1.433  728 statx(AT_FDCWD, "/nonexistent/mri-demo",││Mechanism: ptrace(2). We   │
│            0, 4095, 0x7f6f573fc310) = -1 ENOENT    ││PTRACE_SEIZE the threads   │
│ 1.433  728 sendto(4, 0x5583d8fd9b48, 4, …) = 4 <30µs>││and resume with            │
│ 1.433  728 recvfrom(5, 0x7f6f573fc9e8, 64, …) = 4  ││PTRACE_SYSCALL; …          │
└────────────────────────────────────────────────────┘└───────────────────────────┘
```

It is a **learning-first** tool that merges the ideas of six classic utilities,
implemented from scratch (see [clean-room policy](docs/CLEAN_ROOM.md)):

| Tab | Idea borrowed from | Kernel interface |
|---|---|---|
| Overview, process picker | `htop` | `/proc/<pid>/{stat,status,io}` |
| Memory | `pmap` | `/proc/<pid>/smaps` |
| Files | `lsof` | `/proc/<pid>/fd`, `fdinfo` |
| Network | `netstat` / `ss` | `/proc/<pid>/net/*` joined by socket inode |
| Syscalls | `strace` | `ptrace(2)` + `PTRACE_GET_SYSCALL_INFO` |
| Perf | `perf stat` | `perf_event_open(2)` + `/proc/<tid>/syscall` |

## Install & run

```bash
cargo install --path .        # installs the `mri` binary
mri                           # pick a process interactively
mri 1234                      # inspect PID 1234
mri --dump 1234               # one-shot text report (scripts, bug reports)
```

Try it on the bundled demo process (threads, a TCP connection, 64 MiB of heap, a leaked fd):

```bash
cargo run --example demo_target &      # prints its pid
mri $!
```

**Permissions.** Reading your own processes just works. Other users' processes
hide `fd`, `io`, `smaps` and `syscall` — run with `sudo` for those (the UI says
exactly which panels are degraded). Attaching the syscall tracer additionally
requires root or `sudo sysctl kernel.yama.ptrace_scope=0`.

## Keys

| Key | Action | Key | Action |
|---|---|---|---|
| `Tab` / `1`–`7` | switch tab | `e` | toggle explain pane |
| `↑↓` `j k` `PgUp/PgDn` | move / scroll | `space` | pause refresh |
| `a` | attach/detach tracer · start/stop perf | `c` | syscall log ⇄ summary |
| `x` | clear trace / reset profile | `b` / `Esc` | back to picker |
| `/` `s` | filter / sort (picker) | `q` | quit |

## Status

| Phase | Scope | State |
|---|---|---|
| 1 — MVP | procfs readers, process picker, Overview/Memory/Files/Network/Threads, explain pane | ✅ implemented |
| 2 — Tracing | ptrace syscall log + `strace -c`-style summary | ✅ implemented |
| 3 — Counters | `perf_event_open` counters, IPC, wait-state profiler | ✅ implemented |
| 4+ | sampling profiler, flame graphs, eBPF, record/replay | 📐 designed, see [ROADMAP](docs/ROADMAP.md) |

Verified: 46 automated tests (parsers against fixtures; self-inspection; a real
ptrace session against a child process; headless TUI rendering of every tab at
four terminal sizes); `clippy -D warnings` clean. Developed and tested on
x86_64 Linux with Rust 1.99. **Not yet verified:** hardware PMU counters on
bare metal (the development VM had no PMU — the UI degrades gracefully and says
so), non-x86_64 syscall name tables, kernels older than 5.3.

## Documentation

* [Architecture](docs/ARCHITECTURE.md) — layers, threads, data flow, diagrams
* [Linux internals](docs/LINUX_INTERNALS.md) — how `/proc`, ptrace and perf work, and how each tool maps to them
* [Roadmap](docs/ROADMAP.md)
* [Clean-room policy](docs/CLEAN_ROOM.md) · [Contributing](CONTRIBUTING.md)

## License

MIT
