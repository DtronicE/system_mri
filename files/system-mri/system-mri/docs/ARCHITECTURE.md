# Architecture

## Layers

```mermaid
flowchart TB
    subgraph UI["tui (ratatui + crossterm)"]
        APP["app.rs<br/>state machine + input"]
        VIEWS["views/*<br/>pure render functions"]
    end
    subgraph CORE["core (no UI dependency)"]
        SAMPLER["sampler<br/>deltas → CPU%, I/O rates, history"]
        SNAP["snapshot<br/>one process, one moment"]
        EXPLAIN["explain<br/>teaching text"]
    end
    subgraph SRC["kernel-facing"]
        PROCFS["procfs/*<br/>parse() + read()"]
        TRACE["trace/*<br/>ptrace tracer thread"]
        PERF["perf/*<br/>perf_event_open + wait profiler"]
    end
    KERNEL[("Linux kernel<br/>/proc · ptrace(2) · perf_event_open(2)")]

    APP --> VIEWS
    VIEWS --> EXPLAIN
    APP --> SAMPLER --> SNAP --> PROCFS --> KERNEL
    APP -- "mpsc (bounded)" --> TRACE --> KERNEL
    APP --> PERF --> KERNEL
    PERF -. "reads" .-> PROCFS
```

The dependency rule is strictly downward: `procfs` knows nothing about the UI, and
`views` never touch the kernel — they only read state. This is why every tab can
be rendered in tests against a `TestBackend`.

## Threads and data flow

```mermaid
sequenceDiagram
    participant UI as UI thread (event loop)
    participant S as Sampler
    participant T as mri-tracer thread
    participant W as mri-waitprof thread
    participant K as Kernel

    loop every --interval (default 1 s)
        UI->>S: sample()
        S->>K: read /proc/<pid>/{stat,status,smaps,fd,net,task}
        S-->>UI: Sample (snapshot + rates)
    end
    UI->>T: spawn(pid)  [press a on Syscalls]
    T->>K: PTRACE_SEIZE / INTERRUPT / SYSCALL
    loop every syscall entry & exit
        K-->>T: waitpid → stop
        T->>K: PTRACE_GET_SYSCALL_INFO, read /proc/<tid>/mem
        T-->>UI: TraceMsg::Syscall (try_send, drop if full)
    end
    UI->>T: Drop → SIGUSR1 wakes waitpid → interrupt + detach
    UI->>W: start(pid) [press a on Perf]
    loop every 10 ms
        W->>K: read /proc/<tid>/{stat,syscall}
    end
```

Key design decisions:

| Decision | Why |
|---|---|
| Parsers take `&str` | Unit-testable with fixtures; no root or live process needed |
| Soft failures → `Option` + warning | Inspecting another user's process must still show what *is* readable |
| Tracer on its own thread, bounded channel, `try_send` | ptrace requests must come from the attaching thread; a slow UI must never stall the tracee |
| Drop = detach | Quitting or toggling always resumes the target; no leaked traces |
| `PTRACE_GET_SYSCALL_INFO` (Linux ≥ 5.3) | Distinguishes entry from exit reliably and avoids per-arch register layouts |
| Raw `perf_event_open` struct (112 B, `ATTR_SIZE_VER5`) | No heavy dependency; kernel accepts older attr sizes; layout asserted by a test |
| Counters per thread, summed, last value kept for dead threads | `perf_event_open` binds to a TID, not a process |

## Module map

| Path | Responsibility |
|---|---|
| `procfs/stat.rs` | `/proc/<pid>/stat` — handles `comm` containing spaces and `)` |
| `procfs/status.rs` | RSS split, context switches |
| `procfs/maps.rs` | VMAs from `smaps`/`maps`, `RegionKind` classification, per-kind summary |
| `procfs/fd.rs` | fd table, `fdinfo`, `O_*` decoding |
| `procfs/net.rs` | TCP/UDP/UNIX tables, little-endian hex addresses, TCP states |
| `procfs/threads.rs` | per-task stat, `syscall`, `wchan` |
| `procfs/system.rs` | `CLK_TCK`, page size, process list |
| `snapshot.rs` | one reading of everything; joins sockets to fds by inode |
| `sampler.rs` | cumulative counters → rates; sparkline history |
| `syscalls.rs` | x86_64 number → name + argument shapes |
| `trace/` | ptrace tracer, argument decoding, `strace -c`-style stats |
| `perf/` | counters + wait-state profiler |
| `explain.rs` | teaching text, separate from rendering |
| `tui/` | app state, input, views |

## Known limitations

* Group-stops (Ctrl-Z) of a traced process are resumed by the tracer.
* Threads created while tracing are followed (`PTRACE_O_TRACECLONE`); child
  *processes* (`fork`/`exec`) are not.
* Counters of threads that start after a perf session begins count from their
  creation; `task-clock` etc. are therefore slight under-estimates for
  thread-spawning workloads.
* smaps parsing is O(VMAs); very large processes (10⁵ VMAs) make a 1 s refresh
  expensive. A cheaper `smaps_rollup`-only mode is on the roadmap.
