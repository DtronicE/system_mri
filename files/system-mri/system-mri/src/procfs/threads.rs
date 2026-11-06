//! `/proc/<pid>/task/<tid>/*` — per-thread view.
//!
//! In Linux a "thread" is just a task that shares an address space; each one
//! has its own `stat`, and (if we may ptrace-inspect it) a `syscall` file that
//! reveals what the thread is blocked on *right now*.

use std::fs;

use crate::error::{read_file, Error, Result};
use crate::syscalls;

use super::stat::Stat;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyscallState {
    /// Currently executing in user space or runnable (`running`).
    Running,
    /// Blocked, but not inside a syscall (e.g. page fault) — file says `-1`.
    NotInSyscall,
    InSyscall {
        nr: i64,
        args: [u64; 6],
    },
}

impl SyscallState {
    pub fn label(&self) -> String {
        match self {
            SyscallState::Running => "running".into(),
            SyscallState::NotInSyscall => "(not in syscall)".into(),
            SyscallState::InSyscall { nr, .. } => syscalls::name(*nr)
                .map(str::to_string)
                .unwrap_or_else(|| format!("syscall_{nr}")),
        }
    }
}

pub fn parse_syscall(s: &str) -> Option<SyscallState> {
    let s = s.trim();
    if s == "running" {
        return Some(SyscallState::Running);
    }
    let mut it = s.split_whitespace();
    let nr: i64 = it.next()?.parse().ok()?;
    if nr < 0 {
        return Some(SyscallState::NotInSyscall);
    }
    let mut args = [0u64; 6];
    for a in args.iter_mut() {
        *a = u64::from_str_radix(it.next()?.trim_start_matches("0x"), 16).ok()?;
    }
    Some(SyscallState::InSyscall { nr, args })
}

#[derive(Debug, Clone)]
pub struct Thread {
    pub tid: u32,
    pub comm: String,
    pub state: char,
    pub cpu_ticks: u64,
    pub processor: i32,
    pub syscall: Option<SyscallState>,
    /// Kernel function the task sleeps in (`/proc/<tid>/wchan`), if any.
    pub wchan: Option<String>,
}

pub fn read(pid: u32) -> Result<Vec<Thread>> {
    let dir = format!("/proc/{pid}/task");
    let entries = fs::read_dir(&dir).map_err(|source| Error::Io {
        path: dir.clone().into(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Some(tid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(text) = read_file(entry.path().join("stat")) else {
            continue; // thread exited meanwhile
        };
        let Ok(stat) = Stat::parse(&text) else {
            continue;
        };
        let syscall = read_file(entry.path().join("syscall"))
            .ok()
            .and_then(|t| parse_syscall(&t));
        let wchan = read_file(entry.path().join("wchan"))
            .ok()
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty() && w != "0");
        out.push(Thread {
            tid,
            comm: stat.comm.clone(),
            state: stat.state,
            cpu_ticks: stat.cpu_ticks(),
            processor: stat.processor,
            syscall,
            wchan,
        });
    }
    out.sort_by_key(|t| t.tid);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_syscall_file() {
        assert_eq!(parse_syscall("running\n"), Some(SyscallState::Running));
        assert_eq!(
            parse_syscall("-1 0x7ffc 0x55aa\n"),
            Some(SyscallState::NotInSyscall)
        );
        let s =
            parse_syscall("0 0x3 0x7ffd0000 0x400 0x0 0x0 0x0 0x7ffd1000 0x7f00aabb\n").unwrap();
        match s {
            SyscallState::InSyscall { nr, args } => {
                assert_eq!(nr, 0);
                assert_eq!(args[..3], [3, 0x7ffd0000, 0x400]);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(parse_syscall("garbage"), None);
    }
}
