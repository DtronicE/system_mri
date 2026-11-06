//! System-wide constants and the process list (the `htop` part).

use std::{fs, os::unix::fs::MetadataExt, sync::OnceLock};

use crate::error::{read_file, Error, Result};

use super::stat::Stat;

/// Clock ticks per second (`sysconf(_SC_CLK_TCK)`); `utime`/`stime` use this unit.
pub fn clk_tck() -> u64 {
    static V: OnceLock<u64> = OnceLock::new();
    *V.get_or_init(|| {
        // SAFETY: sysconf has no preconditions.
        let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        if v > 0 {
            v as u64
        } else {
            100
        }
    })
}

/// Virtual-memory page size in bytes; `stat.rss` is expressed in pages.
pub fn page_size() -> u64 {
    static V: OnceLock<u64> = OnceLock::new();
    *V.get_or_init(|| {
        // SAFETY: sysconf has no preconditions.
        let v = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if v > 0 {
            v as u64
        } else {
            4096
        }
    })
}

/// Seconds since boot (`/proc/uptime`, first column).
pub fn uptime_secs() -> Result<f64> {
    let text = read_file("/proc/uptime")?;
    text.split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| Error::parse("uptime", text.clone()))
}

/// `MemTotal` from `/proc/meminfo`, in kB.
pub fn mem_total_kb() -> Option<u64> {
    read_file("/proc/meminfo")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

#[derive(Debug, Clone)]
pub struct ProcSummary {
    pub pid: u32,
    pub comm: String,
    pub state: char,
    pub cpu_ticks: u64,
    pub rss_kb: u64,
    pub threads: i64,
    pub uid: u32,
}

/// Enumerate every numeric directory in `/proc`. Processes that vanish while
/// we iterate are skipped — a normal, expected race.
pub fn list_processes() -> Vec<ProcSummary> {
    let page_kb = page_size() / 1024;
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(text) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Ok(stat) = Stat::parse(&text) else {
            continue;
        };
        let uid = entry.metadata().map(|m| m.uid()).unwrap_or(0);
        out.push(ProcSummary {
            pid,
            comm: stat.comm.clone(),
            state: stat.state,
            cpu_ticks: stat.cpu_ticks(),
            rss_kb: stat.rss_pages.max(0) as u64 * page_kb,
            threads: stat.num_threads,
            uid,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_sane() {
        assert!(clk_tck() >= 1);
        assert!(page_size().is_power_of_two());
    }

    #[test]
    fn can_list_ourselves() {
        let me = std::process::id();
        assert!(list_processes().iter().any(|p| p.pid == me));
    }

    #[test]
    fn uptime_is_positive() {
        assert!(uptime_secs().unwrap() > 0.0);
    }
}
