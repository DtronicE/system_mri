# How Linux exposes process internals

Linux keeps all of this state in kernel memory (`task_struct`, `mm_struct`,
`files_struct`, socket structs). User space never reads those directly. Instead
the kernel offers three kinds of windows.

## 1. `/proc` — files generated on demand

`/proc` is a *pseudo*-filesystem: nothing is stored on disk. When you `read()`
`/proc/1234/stat`, the kernel formats the live `task_struct` into text at that
instant. Consequences worth knowing:

* **Not atomic across files.** `stat` and `status` can disagree by microseconds.
  Processes can vanish mid-read (`ENOENT`/`ESRCH`) — readers must tolerate it.
* **Counters are cumulative.** CPU time, faults and I/O bytes only grow; a rate
  is always `Δvalue / Δtime` between two reads.
* **Access control** is per file: `fd/`, `io`, `syscall`, `smaps` need ptrace-level
  access (same uid + Yama rules, or root).

| What | File | Notes |
|---|---|---|
| State, ppid, CPU ticks, faults, vsize, rss | `/proc/<pid>/stat` | one line; `comm` is parenthesised and may contain `)` |
| Same, readable + RSS split | `status` | `RssAnon`/`RssFile`/`RssShmem`, context switches |
| Address space layout | `maps` | one VMA per line: `start-end perms offset dev inode path` |
| Per-VMA accounting | `smaps` | Size, Rss, **Pss**, Private/Shared Dirty, Swap |
| Open descriptors | `fd/N` (symlinks), `fdinfo/N` | link target names the kernel object: `socket:[ino]`, `pipe:[ino]`, `anon_inode:[eventpoll]` |
| Sockets | `net/tcp`, `tcp6`, `udp`, `udp6`, `unix` | per *network namespace*, no owner info |
| Threads | `task/<tid>/…` | every thread is a task with its own tree |
| Blocked-in syscall | `task/<tid>/syscall` | `nr arg0..arg5 sp pc`, or `running`, or `-1 sp pc` |
| Kernel sleep point | `task/<tid>/wchan` | function name where the task sleeps |
| I/O accounting | `io` | `rchar` includes page-cache hits; `read_bytes` is real disk |

### Memory: why RSS, PSS and VSZ differ

* **VSZ** — address space *reserved* (`mmap`, `brk`). Costs nothing until touched.
* **RSS** — pages currently resident in RAM, counting shared pages fully in every process.
* **PSS** — RSS where each shared page is divided by the number of processes mapping it; PSS values add up to real memory use.
* Pages arrive lazily: the first touch of a reserved page triggers a **minor page fault** (zero-fill or page-cache hit); a **major fault** needs disk I/O.
* A VMA's kind follows from its path and permissions: `[heap]` (grown by `brk`), `[stack]` (grown on fault), file-backed `r-x` = code, anonymous = `malloc` arenas / thread stacks, `[vdso]` = kernel-provided fast syscalls.

### Sockets: the inode join

`/proc/net/tcp` lists every socket in the namespace with an **inode**. A process's
socket fd is a symlink `socket:[inode]`. Matching the two attributes sockets to
processes — this is the whole trick behind `lsof -i` and `ss -p`. System MRI does
the join in `snapshot::collect`.

Addresses are printed by the kernel as host-endian hex words, so on
little-endian machines `0100007F:1F90` is `127.0.0.1:8080` (`procfs/net.rs`
tests this, including IPv6).

## 2. `ptrace(2)` — controlling another process

ptrace lets one process (the *tracer*) observe and control another (the *tracee*).
The syscall-tracing recipe used in `trace/`:

1. `PTRACE_SEIZE` attaches without stopping or sending signals.
2. `PTRACE_INTERRUPT` forces the first stop.
3. `PTRACE_SYSCALL` resumes the tracee until the next syscall **entry or exit**;
   with `PTRACE_O_TRACESYSGOOD` such stops are reported as `SIGTRAP|0x80`.
4. `waitpid(-1, __WALL)` blocks until some thread stops.
5. `PTRACE_GET_SYSCALL_INFO` (Linux 5.3+) returns `op` (entry/exit), syscall
   number, six arguments, and the return value + error flag.
6. Pointer arguments are dereferenced by reading `/proc/<tid>/mem` at the pointer value.
7. `PTRACE_O_TRACECLONE` auto-attaches new threads.
8. `PTRACE_DETACH` (from a ptrace-stop) lets the process run free again.

Costs and rules: two tracer round-trips per syscall; only one tracer per task;
permission = same uid **and** Yama `ptrace_scope` allows it (default `1` = only
descendants), or `CAP_SYS_PTRACE`. Requests must come from the thread that
attached — hence the dedicated tracer thread and the `SIGUSR1`-to-`EINTR` trick
used to wake it for shutdown.

## 3. `perf_event_open(2)` — the performance monitoring unit

`perf_event_open(&attr, pid, cpu, group_fd, flags)` returns an fd. `read()` yields
a 64-bit counter. Hardware events (`cycles`, `instructions`, `cache-misses`,
`branch-misses`) are counted by the CPU's PMU registers, which the kernel
saves/restores on context switch so counts are per-thread. Software events
(`page-faults`, `context-switches`, `task-clock`) are kernel counters.

* One fd = one event on one **TID**; a process is the sum over its threads.
* `exclude_kernel`/`exclude_hv` are set so that unprivileged use works at
  `kernel.perf_event_paranoid=2`.
* IPC = instructions ÷ cycles. IPC ≪ 1 typically means the core is stalled on memory.
* VMs often expose no PMU → `ENOENT`; System MRI shows the reason instead of failing.
* Sampling (`sample_period`) with the mmap ring buffer is the basis of profilers —
  Phase 4 on the roadmap.

### The wait-state profiler (no perf needed)

Reading each thread's `state` and `syscall` file ~100×/s and tallying answers
"what is this process waiting on?" statistically. It is blind to *where in user
code* CPU time goes, which requires PMU sampling.

## Mapping the classic tools

| Tool | Essentially does | System MRI |
|---|---|---|
| `htop` | walk `/proc/*`, parse `stat`/`status`, compute CPU% from tick deltas | `procfs::system::list_processes`, `Sampler` |
| `pmap -X` | parse `smaps` | `procfs::maps` |
| `lsof` | `readlink` every `fd/N`, join sockets via inode | `procfs::fd`, `snapshot::collect` |
| `netstat`/`ss` | parse `/proc/net/*` (ss prefers netlink `sock_diag`) | `procfs::net` |
| `strace` | ptrace loop above | `trace/` |
| `perf stat` | `perf_event_open` per thread, sum | `perf::Counters` |
