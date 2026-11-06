//! `/proc/<pid>/smaps` (and the leaner `maps`) — the process's virtual address
//! space, one entry per VMA (virtual memory area).
//!
//! `maps` gives the layout; `smaps` adds per-VMA accounting (RSS, PSS, dirty
//! pages, swap) and is what `pmap -X` reads. Both share the same header-line
//! format, so one parser handles both.

use std::fmt;

use crate::error::{read_file, Result};

use super::proc_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Perms {
    pub read: bool,
    pub write: bool,
    pub exec: bool,
    /// `true` = shared (`s`), `false` = private copy-on-write (`p`).
    pub shared: bool,
}

impl Perms {
    fn parse(s: &str) -> Self {
        let b = s.as_bytes();
        let at = |i: usize, c: u8| b.get(i) == Some(&c);
        Perms {
            read: at(0, b'r'),
            write: at(1, b'w'),
            exec: at(2, b'x'),
            shared: at(3, b's'),
        }
    }
}

impl fmt::Display for Perms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            if self.read { 'r' } else { '-' },
            if self.write { 'w' } else { '-' },
            if self.exec { 'x' } else { '-' },
            if self.shared { 's' } else { 'p' }
        )
    }
}

/// Semantic classification of a VMA, derived from its path and permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegionKind {
    Code,
    FileData,
    Heap,
    Stack,
    Anonymous,
    SharedMemory,
    Vdso,
    Other,
}

impl RegionKind {
    pub const ALL: [RegionKind; 8] = [
        RegionKind::Code,
        RegionKind::FileData,
        RegionKind::Heap,
        RegionKind::Stack,
        RegionKind::Anonymous,
        RegionKind::SharedMemory,
        RegionKind::Vdso,
        RegionKind::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RegionKind::Code => "code (file, exec)",
            RegionKind::FileData => "file data/rodata",
            RegionKind::Heap => "heap (brk)",
            RegionKind::Stack => "stack",
            RegionKind::Anonymous => "anonymous (mmap/malloc)",
            RegionKind::SharedMemory => "shared memory",
            RegionKind::Vdso => "vdso/vvar/vsyscall",
            RegionKind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub end: u64,
    pub perms: Perms,
    pub offset: u64,
    pub dev: String,
    pub inode: u64,
    pub path: String,
    // smaps accounting (kB); all zero when only `maps` was readable.
    pub rss_kb: u64,
    pub pss_kb: u64,
    pub dirty_kb: u64,
    pub swap_kb: u64,
}

impl Region {
    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn kind(&self) -> RegionKind {
        let p = self.path.as_str();
        match p {
            "[heap]" => RegionKind::Heap,
            "[vdso]" | "[vvar]" | "[vvar_vclock]" | "[vsyscall]" => RegionKind::Vdso,
            _ if p.starts_with("[stack") => RegionKind::Stack,
            _ if p.starts_with("/dev/shm")
                || p.starts_with("/memfd:")
                || p.starts_with("/SYSV")
                || (self.perms.shared && self.perms.write && p.is_empty()) =>
            {
                RegionKind::SharedMemory
            }
            _ if p.starts_with('/') => {
                if self.perms.exec {
                    RegionKind::Code
                } else {
                    RegionKind::FileData
                }
            }
            "" => RegionKind::Anonymous,
            _ if p.starts_with("[anon") => RegionKind::Anonymous,
            _ => RegionKind::Other,
        }
    }

    /// Human label for the path column.
    pub fn display_path(&self) -> &str {
        if self.path.is_empty() {
            "[anon]"
        } else {
            &self.path
        }
    }
}

fn skip_tokens(mut s: &str, n: usize) -> &str {
    for _ in 0..n {
        s = s.trim_start();
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        s = &s[end..];
    }
    s.trim()
}

fn parse_header(line: &str) -> Option<Region> {
    let mut it = line.split_whitespace();
    let (range, perms, offset, dev, inode) =
        (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
    let (start, end) = range.split_once('-')?;
    Some(Region {
        start: u64::from_str_radix(start, 16).ok()?,
        end: u64::from_str_radix(end, 16).ok()?,
        perms: Perms::parse(perms),
        offset: u64::from_str_radix(offset, 16).ok()?,
        dev: dev.to_string(),
        inode: inode.parse().ok()?,
        // The path may contain spaces, so take the remainder of the line.
        path: skip_tokens(line, 5).to_string(),
        rss_kb: 0,
        pss_kb: 0,
        dirty_kb: 0,
        swap_kb: 0,
    })
}

/// Parse the contents of either `maps` or `smaps`.
pub fn parse(text: &str) -> Vec<Region> {
    let mut regions: Vec<Region> = Vec::new();
    for line in text.lines() {
        let first = line.split_whitespace().next().unwrap_or("");
        if first.ends_with(':') {
            // An smaps attribute line ("Rss:   12 kB") belonging to the last VMA.
            let Some(r) = regions.last_mut() else {
                continue;
            };
            let val: u64 = line
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            match first {
                "Rss:" => r.rss_kb = val,
                "Pss:" => r.pss_kb = val,
                "Shared_Dirty:" | "Private_Dirty:" => r.dirty_kb += val,
                "Swap:" => r.swap_kb = val,
                _ => {}
            }
        } else if let Some(r) = parse_header(line) {
            regions.push(r);
        }
    }
    regions
}

/// Prefer `smaps` (accounting); fall back to `maps` (layout only).
pub fn read(pid: u32) -> Result<Vec<Region>> {
    match read_file(proc_path(pid, "smaps")) {
        Ok(t) => Ok(parse(&t)),
        Err(_) => Ok(parse(&read_file(proc_path(pid, "maps"))?)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindSummary {
    pub kind: RegionKind,
    pub count: usize,
    pub size: u64,
    pub rss_kb: u64,
    pub pss_kb: u64,
}

/// Aggregate regions per [`RegionKind`] (the data behind the layout bars).
pub fn summarize(regions: &[Region]) -> Vec<KindSummary> {
    RegionKind::ALL
        .iter()
        .map(|&kind| {
            let mut s = KindSummary {
                kind,
                count: 0,
                size: 0,
                rss_kb: 0,
                pss_kb: 0,
            };
            for r in regions.iter().filter(|r| r.kind() == kind) {
                s.count += 1;
                s.size += r.size();
                s.rss_kb += r.rss_kb;
                s.pss_kb += r.pss_kb;
            }
            s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMAPS: &str = "\
55d0c0a00000-55d0c0a21000 r-xp 00001000 08:01 131075   /usr/bin/my prog
Size:                132 kB
Rss:                 100 kB
Pss:                  80 kB
Private_Dirty:         4 kB
Shared_Dirty:          2 kB
Swap:                  0 kB
VmFlags: rd ex mr mw me
55d0c1a00000-55d0c1a21000 rw-p 00000000 00:00 0        [heap]
Rss:                  40 kB
7ffd1c000000-7ffd1c021000 rw-p 00000000 00:00 0        [stack]
7f0000000000-7f0000100000 rw-p 00000000 00:00 0
7f0000200000-7f0000300000 rw-s 00000000 00:01 77       /dev/shm/x
7ffd1c0f0000-7ffd1c0f2000 r-xp 00000000 00:00 0        [vdso]
";

    #[test]
    fn parses_headers_and_attributes() {
        let r = parse(SMAPS);
        assert_eq!(r.len(), 6);
        assert_eq!(r[0].path, "/usr/bin/my prog"); // spaces preserved
        assert_eq!(r[0].perms.to_string(), "r-xp");
        assert_eq!(r[0].offset, 0x1000);
        assert_eq!((r[0].rss_kb, r[0].pss_kb, r[0].dirty_kb), (100, 80, 6));
        assert_eq!(r[0].size(), 0x21000);
        assert_eq!(r[1].rss_kb, 40);
    }

    #[test]
    fn classifies_regions() {
        let k: Vec<_> = parse(SMAPS).iter().map(Region::kind).collect();
        use RegionKind::*;
        assert_eq!(k, [Code, Heap, Stack, Anonymous, SharedMemory, Vdso]);
    }

    #[test]
    fn summary_adds_up() {
        let regions = parse(SMAPS);
        let sum = summarize(&regions);
        let total: u64 = sum.iter().map(|s| s.size).sum();
        assert_eq!(total, regions.iter().map(Region::size).sum::<u64>());
        assert_eq!(sum[0].rss_kb, 100);
    }

    #[test]
    fn plain_maps_work_too() {
        let r = parse("00400000-00401000 r--p 00000000 08:01 5 /bin/true\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].rss_kb, 0);
    }
}
