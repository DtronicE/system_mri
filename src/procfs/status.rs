//! `/proc/<pid>/status` — the human-readable `key: value` view of the same
//! kernel structures, with the useful extras (RSS breakdown, context switches).

use crate::error::{read_file, Result};

use super::proc_path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub name: String,
    pub state: String,
    pub tgid: u32,
    pub ppid: u32,
    pub uid: u32,
    pub gid: u32,
    pub threads: u32,
    // Memory (kB). `None` for kernel threads, which have no address space.
    pub vm_peak: Option<u64>,
    pub vm_size: Option<u64>,
    pub vm_hwm: Option<u64>,
    pub vm_rss: Option<u64>,
    pub rss_anon: Option<u64>,
    pub rss_file: Option<u64>,
    pub rss_shmem: Option<u64>,
    pub vm_data: Option<u64>,
    pub vm_stk: Option<u64>,
    pub vm_exe: Option<u64>,
    pub vm_lib: Option<u64>,
    pub vm_swap: Option<u64>,
    pub voluntary_ctxt_switches: u64,
    pub nonvoluntary_ctxt_switches: u64,
}

fn kb(v: &str) -> Option<u64> {
    v.trim().trim_end_matches("kB").trim().parse().ok()
}

fn first_num<T: std::str::FromStr + Default>(v: &str) -> T {
    v.split_whitespace()
        .next()
        .and_then(|t| t.parse().ok())
        .unwrap_or_default()
}

impl Status {
    pub fn parse(text: &str) -> Self {
        let mut s = Status::default();
        for line in text.lines() {
            let Some((key, val)) = line.split_once(':') else {
                continue;
            };
            let val = val.trim();
            match key {
                "Name" => s.name = val.to_string(),
                "State" => s.state = val.to_string(),
                "Tgid" => s.tgid = first_num(val),
                "PPid" => s.ppid = first_num(val),
                "Uid" => s.uid = first_num(val), // real uid is the first column
                "Gid" => s.gid = first_num(val),
                "Threads" => s.threads = first_num(val),
                "VmPeak" => s.vm_peak = kb(val),
                "VmSize" => s.vm_size = kb(val),
                "VmHWM" => s.vm_hwm = kb(val),
                "VmRSS" => s.vm_rss = kb(val),
                "RssAnon" => s.rss_anon = kb(val),
                "RssFile" => s.rss_file = kb(val),
                "RssShmem" => s.rss_shmem = kb(val),
                "VmData" => s.vm_data = kb(val),
                "VmStk" => s.vm_stk = kb(val),
                "VmExe" => s.vm_exe = kb(val),
                "VmLib" => s.vm_lib = kb(val),
                "VmSwap" => s.vm_swap = kb(val),
                "voluntary_ctxt_switches" => s.voluntary_ctxt_switches = first_num(val),
                "nonvoluntary_ctxt_switches" => s.nonvoluntary_ctxt_switches = first_num(val),
                _ => {}
            }
        }
        s
    }
}

pub fn read(pid: u32) -> Result<Status> {
    Ok(Status::parse(&read_file(proc_path(pid, "status"))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "Name:\tfirefox\nState:\tS (sleeping)\nTgid:\t42\nPPid:\t1\n\
Uid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\nVmPeak:\t  2048 kB\nVmSize:\t  1024 kB\n\
VmRSS:\t   512 kB\nRssAnon:\t   400 kB\nRssFile:\t   100 kB\nRssShmem:\t 12 kB\nThreads:\t7\n\
voluntary_ctxt_switches:\t10\nnonvoluntary_ctxt_switches:\t3\n";

    #[test]
    fn parses_fixture() {
        let s = Status::parse(FIXTURE);
        assert_eq!(s.name, "firefox");
        assert_eq!((s.tgid, s.ppid, s.uid, s.threads), (42, 1, 1000, 7));
        assert_eq!(
            (s.vm_size, s.vm_rss, s.rss_anon),
            (Some(1024), Some(512), Some(400))
        );
        assert_eq!(s.vm_swap, None);
        assert_eq!(
            (s.voluntary_ctxt_switches, s.nonvoluntary_ctxt_switches),
            (10, 3)
        );
    }

    #[test]
    fn kernel_threads_have_no_vm_fields() {
        let s = Status::parse("Name:\tkthreadd\nThreads:\t1\n");
        assert_eq!(s.vm_rss, None);
    }
}
