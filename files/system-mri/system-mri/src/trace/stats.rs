//! Aggregation behind the "syscall summary" view (the `strace -c` idea).

use std::{collections::HashMap, time::Duration};

use super::SyscallEvent;

#[derive(Debug, Clone, Default)]
pub struct SyscallStat {
    pub calls: u64,
    pub errors: u64,
    pub total: Duration,
}

impl SyscallStat {
    pub fn avg(&self) -> Duration {
        if self.calls == 0 {
            Duration::ZERO
        } else {
            self.total / self.calls as u32
        }
    }
}

#[derive(Debug, Default)]
pub struct SyscallStats {
    by_name: HashMap<String, SyscallStat>,
    pub total_calls: u64,
}

impl SyscallStats {
    pub fn record(&mut self, ev: &SyscallEvent) {
        let s = self.by_name.entry(ev.name.clone()).or_default();
        s.calls += 1;
        s.total += ev.duration;
        if ev.errno.is_some() {
            s.errors += 1;
        }
        self.total_calls += 1;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn total_time(&self) -> Duration {
        self.by_name.values().map(|s| s.total).sum()
    }

    /// Descending by total time spent inside the call.
    pub fn sorted(&self) -> Vec<(&str, &SyscallStat)> {
        let mut v: Vec<_> = self.by_name.iter().map(|(k, v)| (k.as_str(), v)).collect();
        v.sort_by(|a, b| b.1.total.cmp(&a.1.total).then(a.0.cmp(b.0)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(name: &str, micros: u64, errno: Option<i32>) -> SyscallEvent {
        SyscallEvent {
            at: Duration::ZERO,
            tid: 1,
            nr: 0,
            name: name.into(),
            args: String::new(),
            ret: 0,
            errno,
            duration: Duration::from_micros(micros),
        }
    }

    #[test]
    fn aggregates_and_sorts() {
        let mut s = SyscallStats::default();
        s.record(&ev("read", 10, None));
        s.record(&ev("read", 30, Some(11)));
        s.record(&ev("futex", 500, None));
        let sorted = s.sorted();
        assert_eq!(sorted[0].0, "futex");
        assert_eq!(sorted[1].1.calls, 2);
        assert_eq!(sorted[1].1.errors, 1);
        assert_eq!(sorted[1].1.avg(), Duration::from_micros(20));
        assert_eq!(s.total_calls, 3);
    }
}
