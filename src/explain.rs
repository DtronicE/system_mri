//! The "teaching" layer: short, accurate explanations of *what you are looking
//! at* and *where the kernel gets it from*. Kept separate from the UI so the
//! text is easy to review, translate and test.

use crate::procfs::{
    fd::FdKind,
    maps::{Region, RegionKind},
    net::TcpState,
};

pub fn region_kind(kind: RegionKind) -> &'static str {
    match kind {
        RegionKind::Code => "Executable code mapped from a file (the program or a shared library). \
Pages are demand-loaded from the page cache, shared between every process that maps the same file, \
and never written back — so they are cheap: PSS divides their cost among all users.",
        RegionKind::FileData => "Read-only data (.rodata, relocation tables) or writable file data (.data) \
mapped from an ELF file. Private writable pages start shared and are duplicated on first write \
(copy-on-write).",
        RegionKind::Heap => "The classic heap: a contiguous region that grows via brk(2). glibc malloc \
uses it for small allocations; large ones (≥128 KiB by default) go to separate anonymous mmaps.",
        RegionKind::Stack => "The main thread's stack. The kernel grows it downward on demand when a page \
fault hits the guard gap, up to RLIMIT_STACK (see ulimit -s). Other threads' stacks are anonymous mmaps.",
        RegionKind::Anonymous => "Memory with no file behind it: large malloc blocks, thread stacks, JIT code, \
language-runtime arenas. Zero-filled on first touch; swapped out (not written back) under pressure.",
        RegionKind::SharedMemory => "Memory shared between processes: POSIX shm (/dev/shm), memfd, SysV segments. \
Counted fully in RSS of every attached process, which is why PSS exists.",
        RegionKind::Vdso => "Kernel-provided pages mapped into every process. The vDSO lets clock_gettime() and \
friends run entirely in user space without a syscall.",
        RegionKind::Other => "Special mapping.",
    }
}

pub fn permissions(r: &Region) -> String {
    let mut s = Vec::new();
    if r.perms.read {
        s.push("readable");
    }
    if r.perms.write {
        s.push("writable");
    }
    if r.perms.exec {
        s.push("executable");
    }
    let mut text = if s.is_empty() {
        "no access (guard page: touching it raises SIGSEGV)".to_string()
    } else {
        s.join(", ")
    };
    if r.perms.write && r.perms.exec {
        text.push_str(" — W+X is a security red flag (JIT or exploit)");
    }
    text.push_str(if r.perms.shared {
        ". Shared: writes are visible to other mappers."
    } else {
        ". Private: copy-on-write."
    });
    text
}

pub fn fd_kind(kind: &FdKind) -> &'static str {
    match kind {
        FdKind::File => "A regular file. `pos` is the current offset; each open() creates its own file description.",
        FdKind::Directory => "An open directory (used by readdir/openat).",
        FdKind::Device => "A device node such as a terminal or /dev/null.",
        FdKind::Socket(_) => "A network or UNIX-domain socket. The number in socket:[N] is the inode that links it to /proc/net/*.",
        FdKind::Pipe(_) => "One end of a pipe. Both ends share the same inode number.",
        FdKind::AnonInode(_) => "An anonymous kernel object exposed as a file: eventfd, epoll, timerfd, signalfd, inotify …",
        FdKind::Other => "Unclassified kernel object.",
    }
}

pub fn tcp_state(state: TcpState) -> &'static str {
    match state {
        TcpState::Listen => "Waiting for incoming connections (passive open).",
        TcpState::Established => "Connection is open and data can flow both ways.",
        TcpState::SynSent => "We sent SYN and wait for SYN-ACK (active open in progress).",
        TcpState::SynRecv => "SYN received on a listener; the handshake is half-complete.",
        TcpState::FinWait1 | TcpState::FinWait2 => "We closed our side and wait for the peer to close theirs.",
        TcpState::TimeWait => "Closed cleanly; the kernel keeps the 4-tuple for ~60 s to absorb stray packets.",
        TcpState::CloseWait => "Peer closed, but THIS PROCESS has not called close() — a classic fd-leak symptom if it persists.",
        TcpState::LastAck => "We closed after the peer; waiting for the final ACK.",
        TcpState::Closing => "Both sides closed simultaneously.",
        TcpState::Close => "Unconnected / closed.",
        TcpState::Unknown(_) => "Unknown state value.",
    }
}

pub fn proc_state(state: char) -> &'static str {
    match state {
        'R' => "Running or runnable (on a CPU or in the run queue).",
        'S' => "Interruptible sleep — waiting for an event (the normal idle state).",
        'D' => "Uninterruptible sleep — usually blocked on disk/NFS I/O; cannot be killed until it returns.",
        'Z' => "Zombie — exited, waiting for its parent to wait(2) and collect the exit status.",
        'T' => "Stopped by a job-control signal.",
        't' => "Stopped by a tracer (ptrace/debugger).",
        'I' => "Idle kernel thread.",
        'X' => "Dead.",
        _ => "Unknown state.",
    }
}

/// Title + "how Linux exposes this" text for each tab.
pub fn tab_primer(tab: &str) -> (&'static str, &'static str) {
    match tab {
        "Overview" => (
            "Process identity & vitals",
            "Sources: /proc/<pid>/stat (scheduler & VM counters, one line), /proc/<pid>/status (same data, \
readable, with RSS split into anon/file/shmem), /proc/<pid>/io (I/O accounting), /proc/<pid>/{exe,cwd,cmdline}.\n\n\
CPU % is derived: utime+stime are cumulative clock ticks, so we sample twice and divide the delta by elapsed time. \
RSS is pages actually in RAM; VSZ is address space merely reserved — a big VSZ is normal.",
        ),
        "Memory" => (
            "Virtual address space (pmap)",
            "Source: /proc/<pid>/smaps — one block per VMA (virtual memory area) with Size, Rss, Pss, dirty and swap counters. \
/proc/<pid>/maps is the same list without accounting.\n\n\
Size = address space reserved · RSS = resident in RAM · PSS = RSS with shared pages divided among their users \
(the fair 'cost' of the mapping). Pages are allocated lazily: mmap reserves, the first touch (a page fault) populates.",
        ),
        "Files" => (
            "Open file descriptors (lsof)",
            "Source: /proc/<pid>/fd/* are magic symlinks to the kernel object behind each descriptor; /proc/<pid>/fdinfo/<fd> \
adds the offset and open flags. Everything is a file in Unix: regular files, pipes, sockets, epoll instances, timers.\n\n\
A steadily growing fd count is a leak; the limit is RLIMIT_NOFILE (ulimit -n).",
        ),
        "Network" => (
            "Sockets (netstat / ss)",
            "Sources: /proc/<pid>/net/{tcp,tcp6,udp,udp6,unix} list every socket in the process's network namespace, \
without owners. We join them to this process by inode: fd → socket:[inode] → row in the table. That join is how lsof -i and ss -p \
assign sockets to PIDs.\n\nQueues: tx = bytes not yet acknowledged by the peer, rx = bytes waiting for the app to read().",
        ),
        "Threads" => (
            "Tasks (htop -H)",
            "Source: /proc/<pid>/task/<tid>/. Linux has no separate 'thread' object: a thread is a task that shares an address \
space with its siblings (clone(CLONE_VM|…)). Each has its own stat, state and — if ptrace-permitted — a `syscall` file \
showing the syscall it is blocked in this very instant, plus `wchan`, the kernel function it sleeps in.",
        ),
        "Syscalls" => (
            "System-call trace (strace)",
            "Mechanism: ptrace(2). We PTRACE_SEIZE the threads and resume with PTRACE_SYSCALL; the kernel stops the tracee at every \
syscall entry and exit and wakes us via waitpid(2). PTRACE_GET_SYSCALL_INFO returns number, args and result. Strings are read \
from /proc/<tid>/mem.\n\nCost: two context switches per syscall — a chatty process can slow down 10–100×. Only one tracer may attach \
to a process at a time, and attaching needs CAP_SYS_PTRACE or Yama ptrace_scope=0.",
        ),
        "Perf" => (
            "CPU counters & wait states (perf)",
            "Counters: perf_event_open(2) returns an fd per (event, thread); read(2) yields the running 64-bit value maintained by the \
CPU's Performance Monitoring Unit (cycles, instructions, cache misses) or the kernel (page faults, context switches). \
IPC = instructions/cycles: below ~1 usually means memory stalls.\n\nWait profile: we sample each thread's state and \
/proc/<tid>/syscall ~100×/s and tally them — a statistical view of what the process spends its time WAITING on.",
        ),
        _ => ("", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_region_kind_has_text() {
        for k in RegionKind::ALL {
            assert!(!region_kind(k).is_empty());
        }
    }

    #[test]
    fn every_tab_has_a_primer() {
        for t in [
            "Overview", "Memory", "Files", "Network", "Threads", "Syscalls", "Perf",
        ] {
            assert!(!tab_primer(t).0.is_empty(), "{t}");
        }
    }
}
