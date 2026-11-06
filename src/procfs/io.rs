//! `/proc/<pid>/io` — per-process I/O accounting (requires same-uid or root).

use crate::error::{read_file, Result};

use super::proc_path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IoStats {
    /// Bytes passed through `read(2)`-family syscalls (includes page-cache hits).
    pub rchar: u64,
    pub wchar: u64,
    pub syscr: u64,
    pub syscw: u64,
    /// Bytes that actually hit the block layer.
    pub read_bytes: u64,
    pub write_bytes: u64,
}

impl IoStats {
    pub fn parse(text: &str) -> Self {
        let mut s = IoStats::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let v = v.trim().parse().unwrap_or(0);
            match k {
                "rchar" => s.rchar = v,
                "wchar" => s.wchar = v,
                "syscr" => s.syscr = v,
                "syscw" => s.syscw = v,
                "read_bytes" => s.read_bytes = v,
                "write_bytes" => s.write_bytes = v,
                _ => {}
            }
        }
        s
    }
}

pub fn read(pid: u32) -> Result<IoStats> {
    Ok(IoStats::parse(&read_file(proc_path(pid, "io"))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_io() {
        let s = IoStats::parse("rchar: 100\nwchar: 200\nsyscr: 3\nsyscw: 4\nread_bytes: 4096\nwrite_bytes: 8192\ncancelled_write_bytes: 0\n");
        assert_eq!((s.rchar, s.wchar, s.syscr, s.syscw), (100, 200, 3, 4));
        assert_eq!((s.read_bytes, s.write_bytes), (4096, 8192));
    }
}
