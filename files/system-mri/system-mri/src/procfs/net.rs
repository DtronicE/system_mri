//! `/proc/<pid>/net/{tcp,tcp6,udp,udp6,unix}` — the kernel's socket tables.
//!
//! These tables are *system-wide per network namespace*; they know nothing
//! about processes. The join key is the **inode**: each socket fd of a process
//! resolves to `socket:[inode]`, and the same inode appears in these tables.
//! That is precisely how `lsof -i` and `ss -p` attribute sockets to PIDs.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::error::{read_file, Result};

use super::proc_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Tcp,
    Tcp6,
    Udp,
    Udp6,
}

impl Proto {
    pub fn label(self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Tcp6 => "tcp6",
            Proto::Udp => "udp",
            Proto::Udp6 => "udp6",
        }
    }
    fn is_udp(self) -> bool {
        matches!(self, Proto::Udp | Proto::Udp6)
    }
}

/// TCP state machine values from `include/net/tcp_states.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Established,
    SynSent,
    SynRecv,
    FinWait1,
    FinWait2,
    TimeWait,
    Close,
    CloseWait,
    LastAck,
    Listen,
    Closing,
    Unknown(u8),
}

impl TcpState {
    pub fn from_hex(v: u8) -> Self {
        match v {
            0x01 => TcpState::Established,
            0x02 => TcpState::SynSent,
            0x03 => TcpState::SynRecv,
            0x04 => TcpState::FinWait1,
            0x05 => TcpState::FinWait2,
            0x06 => TcpState::TimeWait,
            0x07 => TcpState::Close,
            0x08 => TcpState::CloseWait,
            0x09 => TcpState::LastAck,
            0x0A => TcpState::Listen,
            0x0B => TcpState::Closing,
            other => TcpState::Unknown(other),
        }
    }

    pub fn label(self) -> String {
        match self {
            TcpState::Established => "ESTABLISHED".into(),
            TcpState::SynSent => "SYN_SENT".into(),
            TcpState::SynRecv => "SYN_RECV".into(),
            TcpState::FinWait1 => "FIN_WAIT1".into(),
            TcpState::FinWait2 => "FIN_WAIT2".into(),
            TcpState::TimeWait => "TIME_WAIT".into(),
            TcpState::Close => "CLOSE".into(),
            TcpState::CloseWait => "CLOSE_WAIT".into(),
            TcpState::LastAck => "LAST_ACK".into(),
            TcpState::Listen => "LISTEN".into(),
            TcpState::Closing => "CLOSING".into(),
            TcpState::Unknown(v) => format!("UNKNOWN({v:#x})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InetSocket {
    pub proto: Proto,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: TcpState,
    pub tx_queue: u32,
    pub rx_queue: u32,
    pub uid: u32,
    pub inode: u64,
}

impl InetSocket {
    /// For UDP the kernel reuses the TCP state numbers: 07 = unconnected.
    pub fn state_label(&self) -> String {
        if self.proto.is_udp() {
            match self.state {
                TcpState::Close => "UNCONN".into(),
                TcpState::Established => "CONNECTED".into(),
                other => other.label(),
            }
        } else {
            self.state.label()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixSocket {
    pub inode: u64,
    pub kind: &'static str,
    pub state: &'static str,
    pub path: Option<String>,
    pub ref_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketInfo {
    Inet(InetSocket),
    Unix(UnixSocket),
}

impl SocketInfo {
    pub fn inode(&self) -> u64 {
        match self {
            SocketInfo::Inet(s) => s.inode,
            SocketInfo::Unix(s) => s.inode,
        }
    }
}

/// Decode `0100007F:1F90` → `127.0.0.1:8080`.
///
/// The kernel prints each 32-bit word of the address with `%08X` in *host*
/// byte order, so on little-endian machines `0100007F` is `7F 00 00 01`.
pub fn parse_addr(s: &str) -> Option<SocketAddr> {
    let (ip, port) = s.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let ip = match ip.len() {
        8 => {
            let w = u32::from_str_radix(ip, 16).ok()?;
            IpAddr::V4(Ipv4Addr::from(w.to_ne_bytes()))
        }
        32 => {
            let mut bytes = [0u8; 16];
            for i in 0..4 {
                let w = u32::from_str_radix(&ip[i * 8..i * 8 + 8], 16).ok()?;
                bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_ne_bytes());
            }
            IpAddr::V6(Ipv6Addr::from(bytes))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

pub fn parse_inet(text: &str, proto: Proto) -> Vec<InetSocket> {
    text.lines()
        .skip(1) // header
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                return None;
            }
            let (tx, rx) = f[4].split_once(':')?;
            Some(InetSocket {
                proto,
                local: parse_addr(f[1])?,
                remote: parse_addr(f[2])?,
                state: TcpState::from_hex(u8::from_str_radix(f[3], 16).ok()?),
                tx_queue: u32::from_str_radix(tx, 16).ok()?,
                rx_queue: u32::from_str_radix(rx, 16).ok()?,
                uid: f[7].parse().ok()?,
                inode: f[9].parse().ok()?,
            })
        })
        .collect()
}

pub fn parse_unix(text: &str) -> Vec<UnixSocket> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 7 {
                return None;
            }
            let flags = u32::from_str_radix(f[3], 16).ok()?;
            let kind = match u32::from_str_radix(f[4], 16).ok()? {
                1 => "stream",
                2 => "dgram",
                5 => "seqpacket",
                _ => "other",
            };
            let state = if flags & 0x1_0000 != 0 {
                "LISTEN" // __SO_ACCEPTCON
            } else {
                match f[5] {
                    "01" => "UNCONNECTED",
                    "02" => "CONNECTING",
                    "03" => "CONNECTED",
                    "04" => "DISCONNECTING",
                    _ => "?",
                }
            };
            Some(UnixSocket {
                inode: f[6].parse().ok()?,
                kind,
                state,
                path: f.get(7).map(|s| s.to_string()),
                ref_count: u32::from_str_radix(f[1], 16).unwrap_or(0),
            })
        })
        .collect()
}

/// Read every socket table visible in the process's network namespace.
/// Missing tables (e.g. IPv6 disabled) are skipped silently.
pub fn read_all(pid: u32) -> Result<Vec<SocketInfo>> {
    let mut out = Vec::new();
    for (file, proto) in [
        ("net/tcp", Proto::Tcp),
        ("net/tcp6", Proto::Tcp6),
        ("net/udp", Proto::Udp),
        ("net/udp6", Proto::Udp6),
    ] {
        if let Ok(text) = read_file(proc_path(pid, file)) {
            out.extend(parse_inet(&text, proto).into_iter().map(SocketInfo::Inet));
        }
    }
    if let Ok(text) = read_file(proc_path(pid, "net/unix")) {
        out.extend(parse_unix(&text).into_iter().map(SocketInfo::Unix));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_endian = "little")]
    #[test]
    fn decodes_ipv4() {
        assert_eq!(
            parse_addr("0100007F:1F90").unwrap().to_string(),
            "127.0.0.1:8080"
        );
        assert_eq!(
            parse_addr("00000000:0050").unwrap().to_string(),
            "0.0.0.0:80"
        );
    }

    #[cfg(target_endian = "little")]
    #[test]
    fn decodes_ipv6_loopback() {
        let a = parse_addr("00000000000000000000000001000000:01BB").unwrap();
        assert_eq!(a.to_string(), "[::1]:443");
    }

    #[cfg(target_endian = "little")]
    #[test]
    fn parses_tcp_table() {
        let t = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4242 1 0000 100 0 0 10 0\n\
   1: 0100007F:D2F0 0100007F:1F90 01 00000010:00000020 00:00000000 00000000  1000        0 4343 1 0000 20 4 30 10 -1\n";
        let v = parse_inet(t, Proto::Tcp);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].state, v[0].inode), (TcpState::Listen, 4242));
        assert_eq!(
            (v[1].state, v[1].tx_queue, v[1].rx_queue),
            (TcpState::Established, 16, 32)
        );
        assert_eq!(v[1].remote.port(), 8080);
    }

    #[test]
    fn parses_unix_table() {
        let t = "Num       RefCount Protocol Flags    Type St Inode Path\n\
ffff8881: 00000002 00000000 00010000 0001 01 9001 /run/foo.sock\n\
ffff8882: 00000003 00000000 00000000 0001 03 9002\n";
        let v = parse_unix(t);
        assert_eq!(v.len(), 2);
        assert_eq!(
            (v[0].state, v[0].kind, v[0].path.as_deref()),
            ("LISTEN", "stream", Some("/run/foo.sock"))
        );
        assert_eq!((v[1].state, v[1].path.clone()), ("CONNECTED", None));
    }
}
