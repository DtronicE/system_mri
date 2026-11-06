//! Small formatting helpers.

pub fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn fmt_kb(kb: u64) -> String {
    fmt_bytes(kb.saturating_mul(1024))
}

pub fn fmt_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", fmt_bytes(bytes_per_sec.max(0.0) as u64))
}

pub fn fmt_duration(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    let (d, h, m, s) = (
        total / 86_400,
        total / 3600 % 24,
        total / 60 % 60,
        total % 60,
    );
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, _) => format!("{m}m{s:02}s"),
        (0, _, _) => format!("{h}h{m:02}m"),
        _ => format!("{d}d{h:02}h"),
    }
}

/// Shorten `s` to at most `max` chars, keeping both ends: `/usr/…/libc.so.6`.
pub fn truncate_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max || max < 5 {
        return s.to_string();
    }
    let keep = max - 1;
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_humanised() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(1023), "1023 B");
        assert_eq!(fmt_bytes(1536), "1.5 KiB");
        assert_eq!(fmt_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn durations_are_humanised() {
        assert_eq!(fmt_duration(5.0), "5s");
        assert_eq!(fmt_duration(125.0), "2m05s");
        assert_eq!(fmt_duration(3.0 * 3600.0 + 60.0), "3h01m");
        assert_eq!(fmt_duration(2.0 * 86_400.0), "2d00h");
    }

    #[test]
    fn truncation_keeps_both_ends() {
        let t = truncate_middle("/usr/lib/x86_64-linux-gnu/libc.so.6", 16);
        assert_eq!(t.chars().count(), 16);
        assert!(t.starts_with("/usr/li") && t.ends_with("c.so.6"));
        assert_eq!(truncate_middle("short", 16), "short");
    }
}
