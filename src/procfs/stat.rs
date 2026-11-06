//! `/proc/<pid>/stat` — the single-line, space-separated scheduler/VM summary.
//!
//! The format is documented in `proc_pid_stat(5)`. The one trap: field 2
//! (`comm`) is wrapped in parentheses and may itself contain spaces and `)`,
//! so we locate the *last* `)` instead of splitting naively.

use std::str::FromStr;

use crate::error::{read_file, Error, Result};

use super::proc_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub pid: u32,
    pub comm: String,
    /// R, S, D, Z, T, t, X, I …
    pub state: char,
    pub ppid: u32,
    pub pgrp: i32,
    pub session: i32,
    pub tty_nr: i32,
    pub minflt: u64,
    pub majflt: u64,
    /// CPU time in user mode, in clock ticks (`sysconf(_SC_CLK_TCK)`).
    pub utime: u64,
    /// CPU time in kernel mode, in clock ticks.
    pub stime: u64,
    pub priority: i64,
    pub nice: i64,
    pub num_threads: i64,
    /// Start time after boot, in clock ticks.
    pub starttime: u64,
    /// Virtual memory size in bytes.
    pub vsize: u64,
    /// Resident set size in *pages*.
    pub rss_pages: i64,
    /// CPU the task last ran on.
    pub processor: i32,
}

impl Stat {
    pub fn parse(s: &str) -> Result<Self> {
        let bad = |d: &str| Error::parse("stat", d);
        let open = s.find('(').ok_or_else(|| bad("missing '('"))?;
        let close = s.rfind(')').ok_or_else(|| bad("missing ')'"))?;
        if close < open {
            return Err(bad("')' before '('"));
        }
        let pid = s[..open].trim().parse().map_err(|_| bad("pid"))?;
        let comm = s[open + 1..close].to_string();
        let rest: Vec<&str> = s[close + 1..].split_whitespace().collect();
        if rest.len() < 37 {
            return Err(bad("too few fields"));
        }
        // Man-page field N (1-based, counting pid as 1) lives at rest[N - 3].
        fn f<T: FromStr>(rest: &[&str], n: usize) -> Result<T> {
            rest[n - 3]
                .parse()
                .map_err(|_| Error::parse("stat", format!("field {n}")))
        }
        Ok(Stat {
            pid,
            comm,
            state: rest[0].chars().next().unwrap_or('?'),
            ppid: f(&rest, 4)?,
            pgrp: f(&rest, 5)?,
            session: f(&rest, 6)?,
            tty_nr: f(&rest, 7)?,
            minflt: f(&rest, 10)?,
            majflt: f(&rest, 12)?,
            utime: f(&rest, 14)?,
            stime: f(&rest, 15)?,
            priority: f(&rest, 18)?,
            nice: f(&rest, 19)?,
            num_threads: f(&rest, 20)?,
            starttime: f(&rest, 22)?,
            vsize: f(&rest, 23)?,
            rss_pages: f(&rest, 24)?,
            processor: f(&rest, 39)?,
        })
    }

    /// Total CPU ticks consumed (user + kernel).
    pub fn cpu_ticks(&self) -> u64 {
        self.utime + self.stime
    }
}

pub fn read(pid: u32) -> Result<Stat> {
    Stat::parse(&read_file(proc_path(pid, "stat"))?)
}

pub fn read_task(pid: u32, tid: u32) -> Result<Stat> {
    Stat::parse(&read_file(format!("/proc/{pid}/task/{tid}/stat"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(comm: &str) -> String {
        format!(
            "1234 ({comm}) S 1 1234 1234 0 -1 4194560 100 0 5 0 70 30 0 0 20 0 4 0 12345 \
             104857600 2560 {} 3",
            "0 ".repeat(14).trim_end()
        )
    }

    #[test]
    fn parses_a_plain_line() {
        let s = Stat::parse(&sample("bash")).unwrap();
        assert_eq!(
            (s.pid, s.comm.as_str(), s.state, s.ppid),
            (1234, "bash", 'S', 1)
        );
        assert_eq!((s.minflt, s.majflt), (100, 5));
        assert_eq!((s.utime, s.stime, s.cpu_ticks()), (70, 30, 100));
        assert_eq!((s.nice, s.num_threads, s.starttime), (0, 4, 12345));
        assert_eq!((s.vsize, s.rss_pages, s.processor), (104_857_600, 2560, 3));
    }

    #[test]
    fn survives_hostile_comm_names() {
        let s = Stat::parse(&sample("my (weird) proc")).unwrap();
        assert_eq!(s.comm, "my (weird) proc");
        assert_eq!(s.state, 'S');
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(Stat::parse("1 (x) S 0").is_err());
        assert!(Stat::parse("garbage").is_err());
    }
}
