//! Rendering raw syscall arguments the way a human would want to read them.

use std::{fs::File, os::unix::fs::FileExt};

use crate::{
    procfs::fd::describe_open_flags,
    syscalls::{self, Arg},
};

const AT_FDCWD: i64 = -100;
const MAX_STR: usize = 96;

/// Read a NUL-terminated string from the tracee via `/proc/<tid>/mem`.
/// Returns `None` if the address is unreadable.
pub fn read_cstring(tid: u32, addr: u64) -> Option<String> {
    if addr == 0 {
        return None;
    }
    let mem = File::open(format!("/proc/{tid}/mem")).ok()?;
    let mut buf = [0u8; MAX_STR];
    // A short read at a page boundary is fine: we only need up to the NUL.
    let n = mem.read_at(&mut buf, addr).ok()?;
    let end = buf[..n].iter().position(|&b| b == 0);
    let truncated = end.is_none() && n == MAX_STR;
    let s = String::from_utf8_lossy(&buf[..end.unwrap_or(n)]);
    Some(if truncated {
        format!("{s}…")
    } else {
        s.into_owned()
    })
}

/// Format the arguments of syscall `nr`. Unknown syscalls show raw hex.
pub fn format_args(tid: u32, nr: i64, args: &[u64; 6]) -> String {
    let Some(spec) = syscalls::lookup(nr) else {
        return args
            .iter()
            .map(|a| format!("{a:#x}"))
            .collect::<Vec<_>>()
            .join(", ");
    };
    spec.args
        .iter()
        .zip(args.iter())
        .map(|(kind, &raw)| match kind {
            Arg::Fd => {
                let v = raw as i32 as i64;
                if v == AT_FDCWD {
                    "AT_FDCWD".to_string()
                } else {
                    v.to_string()
                }
            }
            Arg::Int => (raw as i64).to_string(),
            Arg::Size => raw.to_string(),
            Arg::Ptr => {
                if raw == 0 {
                    "NULL".into()
                } else {
                    format!("{raw:#x}")
                }
            }
            Arg::Str => match read_cstring(tid, raw) {
                Some(s) => format!("{s:?}"),
                None => format!("{raw:#x}"),
            },
            Arg::OpenFlags => describe_open_flags(raw as u32),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_memory() {
        let s = std::ffi::CString::new("hello from the tracee").unwrap();
        let got = read_cstring(std::process::id(), s.as_ptr() as u64).unwrap();
        assert_eq!(got, "hello from the tracee");
    }

    #[test]
    fn null_pointer_is_none() {
        assert_eq!(read_cstring(std::process::id(), 0), None);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn formats_openat() {
        let path = std::ffi::CString::new("/etc/hosts").unwrap();
        let out = format_args(
            std::process::id(),
            257,
            &[
                (-100i32) as u32 as u64,
                path.as_ptr() as u64,
                0o2000000,
                0,
                0,
                0,
            ],
        );
        assert_eq!(out, "AT_FDCWD, \"/etc/hosts\", O_RDONLY|O_CLOEXEC, 0");
    }

    #[test]
    fn unknown_syscall_prints_hex() {
        assert!(format_args(1, 99_999, &[1, 2, 0, 0, 0, 0]).starts_with("0x1, 0x2"));
    }
}
