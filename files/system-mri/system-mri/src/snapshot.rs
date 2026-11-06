//! A [`Snapshot`] is one consistent-ish reading of everything we know about a
//! process at a moment in time. Readers that fail (typically `EACCES` when
//! inspecting another user's process) degrade to `None` plus a warning
//! instead of failing the whole snapshot.

use std::{fs, path::PathBuf, time::Instant};

use crate::{
    error::{read_bytes, Error, Result},
    procfs::{
        self,
        fd::{FdKind, OpenFd},
        io::IoStats,
        maps::Region,
        net::SocketInfo,
        stat::Stat,
        status::Status,
        threads::Thread,
    },
};

/// A socket owned by the process, joined from fd table ⨝ kernel socket tables.
#[derive(Debug, Clone)]
pub struct ProcessSocket {
    pub fd: u32,
    pub info: SocketInfo,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub pid: u32,
    pub taken: Instant,
    pub stat: Stat,
    pub status: Option<Status>,
    pub io: Option<IoStats>,
    pub threads: Vec<Thread>,
    pub regions: Vec<Region>,
    pub fds: Vec<OpenFd>,
    pub sockets: Vec<ProcessSocket>,
    pub cmdline: Vec<String>,
    pub exe: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    /// Non-fatal problems ("fds: permission denied") surfaced in the UI.
    pub warnings: Vec<String>,
}

/// Record a soft failure and turn it into an `Option`.
fn soft<T>(warnings: &mut Vec<String>, what: &str, r: Result<T>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) if e.is_permission_denied() => {
            warnings.push(format!("{what}: permission denied (try sudo)"));
            None
        }
        Err(e) if e.is_not_found() => None, // process raced away; stat will catch it
        Err(e) => {
            warnings.push(format!("{what}: {e}"));
            None
        }
    }
}

/// Collect a full snapshot. Fails only if `/proc/<pid>/stat` cannot be read,
/// i.e. the process does not exist.
pub fn collect(pid: u32) -> Result<Snapshot> {
    let stat = procfs::stat::read(pid)?;
    let mut warnings = Vec::new();

    let status = soft(&mut warnings, "status", procfs::status::read(pid));
    let io = soft(&mut warnings, "io", procfs::io::read(pid));
    let threads = soft(&mut warnings, "threads", procfs::threads::read(pid)).unwrap_or_default();
    let regions = soft(&mut warnings, "memory map", procfs::maps::read(pid)).unwrap_or_default();
    let fds = soft(&mut warnings, "fds", procfs::fd::read(pid)).unwrap_or_default();

    // Join: socket fd → inode → entry in the kernel's socket tables.
    let tables = procfs::net::read_all(pid).unwrap_or_default();
    let sockets = fds
        .iter()
        .filter_map(|f| match f.kind {
            FdKind::Socket(inode) => {
                tables
                    .iter()
                    .find(|s| s.inode() == inode)
                    .map(|info| ProcessSocket {
                        fd: f.fd,
                        info: info.clone(),
                    })
            }
            _ => None,
        })
        .collect();

    let cmdline = read_bytes(procfs::proc_path(pid, "cmdline"))
        .map(|b| {
            b.split(|&c| c == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect()
        })
        .unwrap_or_default();

    Ok(Snapshot {
        pid,
        taken: Instant::now(),
        stat,
        status,
        io,
        threads,
        regions,
        fds,
        sockets,
        cmdline,
        exe: fs::read_link(procfs::proc_path(pid, "exe")).ok(),
        cwd: fs::read_link(procfs::proc_path(pid, "cwd")).ok(),
        warnings,
    })
}

impl Snapshot {
    pub fn rss_kb(&self) -> u64 {
        self.status
            .as_ref()
            .and_then(|s| s.vm_rss)
            .unwrap_or_else(|| {
                self.stat.rss_pages.max(0) as u64 * procfs::system::page_size() / 1024
            })
    }
}

impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        std::io::Error::other(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_snapshot_ourselves() {
        let snap = collect(std::process::id()).expect("self snapshot");
        assert_eq!(snap.stat.pid, std::process::id());
        assert!(!snap.threads.is_empty());
        assert!(snap.regions.iter().any(|r| r.path == "[stack]"));
        assert!(snap.fds.iter().any(|f| f.fd == 0));
        assert!(snap.rss_kb() > 0);
    }

    #[test]
    fn nonexistent_pid_is_an_error() {
        // PID 0 never appears in /proc.
        assert!(collect(0).is_err());
    }
}
