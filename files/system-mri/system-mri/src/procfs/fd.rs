//! `/proc/<pid>/fd/*` and `/proc/<pid>/fdinfo/*` — the open file descriptor
//! table. Each entry in `fd/` is a magic symlink; where it points tells you
//! what kind of kernel object the descriptor refers to.

use std::fs;

use crate::error::{Error, Result};

use super::proc_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FdKind {
    File,
    Directory,
    Device,
    Socket(u64),
    Pipe(u64),
    /// `anon_inode:[eventfd]`, `anon_inode:[eventpoll]`, `anon_inode:[timerfd]` …
    AnonInode(String),
    Other,
}

impl FdKind {
    pub fn classify(target: &str, is_dir: bool) -> FdKind {
        let inode = |prefix: &str| {
            target
                .strip_prefix(prefix)
                .and_then(|r| r.strip_suffix(']'))
                .and_then(|n| n.parse().ok())
        };
        if let Some(i) = inode("socket:[") {
            FdKind::Socket(i)
        } else if let Some(i) = inode("pipe:[") {
            FdKind::Pipe(i)
        } else if let Some(name) = target.strip_prefix("anon_inode:") {
            FdKind::AnonInode(name.trim_matches(|c| c == '[' || c == ']').to_string())
        } else if target.starts_with("/dev/") {
            FdKind::Device
        } else if target.starts_with('/') {
            if is_dir {
                FdKind::Directory
            } else {
                FdKind::File
            }
        } else {
            FdKind::Other
        }
    }

    pub fn label(&self) -> String {
        match self {
            FdKind::File => "file".into(),
            FdKind::Directory => "dir".into(),
            FdKind::Device => "device".into(),
            FdKind::Socket(_) => "socket".into(),
            FdKind::Pipe(_) => "pipe".into(),
            FdKind::AnonInode(n) => n.clone(),
            FdKind::Other => "other".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFd {
    pub fd: u32,
    /// Raw `readlink` result, e.g. `/etc/passwd`, `socket:[12345]`.
    pub target: String,
    pub kind: FdKind,
    /// File offset (from fdinfo).
    pub pos: Option<u64>,
    /// `open(2)` flags (from fdinfo, octal).
    pub flags: Option<u32>,
}

/// Decode `O_*` flag bits (Linux/asm-generic values, identical on x86_64 and arm64).
pub fn describe_open_flags(flags: u32) -> String {
    const TABLE: &[(u32, &str)] = &[
        (0o100, "O_CREAT"),
        (0o200, "O_EXCL"),
        (0o1000, "O_TRUNC"),
        (0o2000, "O_APPEND"),
        (0o4000, "O_NONBLOCK"),
        (0o200000, "O_DIRECTORY"),
        (0o2000000, "O_CLOEXEC"),
    ];
    let mut parts = vec![match flags & 0o3 {
        0 => "O_RDONLY",
        1 => "O_WRONLY",
        2 => "O_RDWR",
        _ => "O_?",
    }];
    parts.extend(
        TABLE
            .iter()
            .filter(|(bit, _)| flags & bit != 0)
            .map(|(_, n)| *n),
    );
    parts.join("|")
}

/// Parse `pos:` and `flags:` out of an fdinfo file.
pub fn parse_fdinfo(text: &str) -> (Option<u64>, Option<u32>) {
    let (mut pos, mut flags) = (None, None);
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("pos:") {
            pos = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("flags:") {
            flags = u32::from_str_radix(v.trim(), 8).ok();
        }
    }
    (pos, flags)
}

pub fn read(pid: u32) -> Result<Vec<OpenFd>> {
    let dir = proc_path(pid, "fd");
    let entries = fs::read_dir(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        // Descriptors can close between readdir and readlink; skip those.
        let Ok(link) = fs::read_link(entry.path()) else {
            continue;
        };
        let target = link.to_string_lossy().into_owned();
        let is_dir = fs::metadata(entry.path())
            .map(|m| m.is_dir())
            .unwrap_or(false);
        let (pos, flags) = fs::read_to_string(proc_path(pid, &format!("fdinfo/{fd}")))
            .map(|t| parse_fdinfo(&t))
            .unwrap_or((None, None));
        out.push(OpenFd {
            fd,
            kind: FdKind::classify(&target, is_dir),
            target,
            pos,
            flags,
        });
    }
    out.sort_by_key(|f| f.fd);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_targets() {
        assert_eq!(
            FdKind::classify("socket:[4242]", false),
            FdKind::Socket(4242)
        );
        assert_eq!(FdKind::classify("pipe:[7]", false), FdKind::Pipe(7));
        assert_eq!(
            FdKind::classify("anon_inode:[eventpoll]", false),
            FdKind::AnonInode("eventpoll".into())
        );
        assert_eq!(FdKind::classify("/dev/pts/0", false), FdKind::Device);
        assert_eq!(FdKind::classify("/etc/passwd", false), FdKind::File);
        assert_eq!(FdKind::classify("/tmp", true), FdKind::Directory);
        assert_eq!(FdKind::classify("weird", false), FdKind::Other);
    }

    #[test]
    fn decodes_flags() {
        assert_eq!(describe_open_flags(0o2), "O_RDWR");
        assert_eq!(
            describe_open_flags(0o2002101),
            "O_WRONLY|O_CREAT|O_APPEND|O_CLOEXEC"
        );
    }

    #[test]
    fn parses_fdinfo() {
        let (pos, flags) = parse_fdinfo("pos:\t1024\nflags:\t02100002\nmnt_id:\t26\n");
        assert_eq!(pos, Some(1024));
        assert_eq!(flags, Some(0o2100002));
    }
}
