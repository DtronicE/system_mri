//! Syscall metadata: number → name → argument shapes.
//!
//! The table is **x86_64-only** (syscall numbers are an ABI per architecture).
//! On other architectures lookups simply return `None` and the UI falls back
//! to `syscall_<nr>`. Add a table for your arch in a PR — see CONTRIBUTING.md.

/// How an argument should be rendered by the tracer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arg {
    /// File descriptor (decimal; `AT_FDCWD` shown symbolically).
    Fd,
    /// Signed integer.
    Int,
    /// Unsigned size / length.
    Size,
    /// Pointer printed in hex (`NULL` for 0).
    Ptr,
    /// NUL-terminated string read from tracee memory.
    Str,
    /// `open(2)` flag bits.
    OpenFlags,
}

use Arg::*;

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub nr: i64,
    pub name: &'static str,
    pub args: &'static [Arg],
}

const fn s(nr: i64, name: &'static str, args: &'static [Arg]) -> Spec {
    Spec { nr, name, args }
}

/// Sorted by `nr` (verified by a unit test) so we can binary-search.
#[cfg(target_arch = "x86_64")]
static TABLE: &[Spec] = &[
    s(0, "read", &[Fd, Ptr, Size]),
    s(1, "write", &[Fd, Ptr, Size]),
    s(2, "open", &[Str, OpenFlags, Int]),
    s(3, "close", &[Fd]),
    s(4, "stat", &[Str, Ptr]),
    s(5, "fstat", &[Fd, Ptr]),
    s(6, "lstat", &[Str, Ptr]),
    s(7, "poll", &[Ptr, Int, Int]),
    s(8, "lseek", &[Fd, Int, Int]),
    s(9, "mmap", &[Ptr, Size, Int, Int, Fd, Int]),
    s(10, "mprotect", &[Ptr, Size, Int]),
    s(11, "munmap", &[Ptr, Size]),
    s(12, "brk", &[Ptr]),
    s(13, "rt_sigaction", &[Int, Ptr, Ptr, Size]),
    s(14, "rt_sigprocmask", &[Int, Ptr, Ptr, Size]),
    s(15, "rt_sigreturn", &[]),
    s(16, "ioctl", &[Fd, Int, Ptr]),
    s(17, "pread64", &[Fd, Ptr, Size, Int]),
    s(18, "pwrite64", &[Fd, Ptr, Size, Int]),
    s(19, "readv", &[Fd, Ptr, Int]),
    s(20, "writev", &[Fd, Ptr, Int]),
    s(21, "access", &[Str, Int]),
    s(22, "pipe", &[Ptr]),
    s(23, "select", &[Int, Ptr, Ptr, Ptr, Ptr]),
    s(24, "sched_yield", &[]),
    s(25, "mremap", &[Ptr, Size, Size, Int, Ptr]),
    s(28, "madvise", &[Ptr, Size, Int]),
    s(32, "dup", &[Fd]),
    s(33, "dup2", &[Fd, Fd]),
    s(34, "pause", &[]),
    s(35, "nanosleep", &[Ptr, Ptr]),
    s(39, "getpid", &[]),
    s(41, "socket", &[Int, Int, Int]),
    s(42, "connect", &[Fd, Ptr, Int]),
    s(43, "accept", &[Fd, Ptr, Ptr]),
    s(44, "sendto", &[Fd, Ptr, Size, Int, Ptr, Int]),
    s(45, "recvfrom", &[Fd, Ptr, Size, Int, Ptr, Ptr]),
    s(46, "sendmsg", &[Fd, Ptr, Int]),
    s(47, "recvmsg", &[Fd, Ptr, Int]),
    s(48, "shutdown", &[Fd, Int]),
    s(49, "bind", &[Fd, Ptr, Int]),
    s(50, "listen", &[Fd, Int]),
    s(51, "getsockname", &[Fd, Ptr, Ptr]),
    s(52, "getpeername", &[Fd, Ptr, Ptr]),
    s(56, "clone", &[Int, Ptr, Ptr, Ptr, Int]),
    s(57, "fork", &[]),
    s(58, "vfork", &[]),
    s(59, "execve", &[Str, Ptr, Ptr]),
    s(60, "exit", &[Int]),
    s(61, "wait4", &[Int, Ptr, Int, Ptr]),
    s(62, "kill", &[Int, Int]),
    s(72, "fcntl", &[Fd, Int, Int]),
    s(78, "getdents", &[Fd, Ptr, Size]),
    s(79, "getcwd", &[Ptr, Size]),
    s(80, "chdir", &[Str]),
    s(82, "rename", &[Str, Str]),
    s(83, "mkdir", &[Str, Int]),
    s(84, "rmdir", &[Str]),
    s(87, "unlink", &[Str]),
    s(89, "readlink", &[Str, Ptr, Size]),
    s(90, "chmod", &[Str, Int]),
    s(96, "gettimeofday", &[Ptr, Ptr]),
    s(97, "getrlimit", &[Int, Ptr]),
    s(102, "getuid", &[]),
    s(110, "getppid", &[]),
    s(158, "arch_prctl", &[Int, Ptr]),
    s(186, "gettid", &[]),
    s(202, "futex", &[Ptr, Int, Int, Ptr, Ptr, Int]),
    s(217, "getdents64", &[Fd, Ptr, Size]),
    s(218, "set_tid_address", &[Ptr]),
    s(219, "restart_syscall", &[]),
    s(228, "clock_gettime", &[Int, Ptr]),
    s(230, "clock_nanosleep", &[Int, Int, Ptr, Ptr]),
    s(231, "exit_group", &[Int]),
    s(232, "epoll_wait", &[Fd, Ptr, Int, Int]),
    s(233, "epoll_ctl", &[Fd, Int, Fd, Ptr]),
    s(257, "openat", &[Fd, Str, OpenFlags, Int]),
    s(262, "newfstatat", &[Fd, Str, Ptr, Int]),
    s(270, "pselect6", &[Int, Ptr, Ptr, Ptr, Ptr, Ptr]),
    s(271, "ppoll", &[Ptr, Int, Ptr, Ptr, Size]),
    s(273, "set_robust_list", &[Ptr, Size]),
    s(281, "epoll_pwait", &[Fd, Ptr, Int, Int, Ptr, Size]),
    s(288, "accept4", &[Fd, Ptr, Ptr, Int]),
    s(290, "eventfd2", &[Int, Int]),
    s(291, "epoll_create1", &[Int]),
    s(292, "dup3", &[Fd, Fd, Int]),
    s(293, "pipe2", &[Ptr, Int]),
    s(302, "prlimit64", &[Int, Int, Ptr, Ptr]),
    s(318, "getrandom", &[Ptr, Size, Int]),
    s(332, "statx", &[Fd, Str, Int, Int, Ptr]),
    s(334, "rseq", &[Ptr, Int, Int, Int]),
    s(435, "clone3", &[Ptr, Size]),
    s(437, "openat2", &[Fd, Str, Ptr, Size]),
];

#[cfg(not(target_arch = "x86_64"))]
static TABLE: &[Spec] = &[];

pub fn lookup(nr: i64) -> Option<&'static Spec> {
    TABLE
        .binary_search_by_key(&nr, |s| s.nr)
        .ok()
        .map(|i| &TABLE[i])
}

pub fn name(nr: i64) -> Option<&'static str> {
    lookup(nr).map(|s| s.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(TABLE.windows(2).all(|w| w[0].nr < w[1].nr));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn well_known_numbers() {
        assert_eq!(name(0), Some("read"));
        assert_eq!(name(1), Some("write"));
        assert_eq!(name(257), Some("openat"));
        assert_eq!(name(231), Some("exit_group"));
        assert_eq!(name(99_999), None);
    }
}
