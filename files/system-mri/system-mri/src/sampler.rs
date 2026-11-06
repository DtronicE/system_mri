//! Turns raw snapshots into *rates*: CPU %, I/O throughput, plus a short
//! history ring for sparklines. Counters in `/proc` are cumulative, so every
//! rate is `Δcounter / Δtime` between two consecutive samples.

use std::{collections::VecDeque, time::Instant};

use crate::{
    error::Result,
    procfs::system::{clk_tck, uptime_secs},
    snapshot::{self, Snapshot},
};

const HISTORY: usize = 120;

#[derive(Debug, Clone)]
pub struct Sample {
    pub snap: Snapshot,
    /// 100 % = one fully busy core; may exceed 100 for multithreaded processes.
    pub cpu_percent: f64,
    pub read_bps: f64,
    pub write_bps: f64,
    /// Seconds the process has been alive.
    pub age_secs: Option<f64>,
}

struct Prev {
    at: Instant,
    cpu_ticks: u64,
    read_bytes: u64,
    write_bytes: u64,
}

pub struct Sampler {
    pid: u32,
    prev: Option<Prev>,
    pub cpu_history: VecDeque<u64>,
    pub rss_history: VecDeque<u64>,
}

fn push(buf: &mut VecDeque<u64>, v: u64) {
    if buf.len() == HISTORY {
        buf.pop_front();
    }
    buf.push_back(v);
}

impl Sampler {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            prev: None,
            cpu_history: VecDeque::new(),
            rss_history: VecDeque::new(),
        }
    }

    pub fn sample(&mut self) -> Result<Sample> {
        let snap = snapshot::collect(self.pid)?;
        let cpu_ticks = snap.stat.cpu_ticks();
        let (rb, wb) = snap
            .io
            .map(|i| (i.read_bytes, i.write_bytes))
            .unwrap_or((0, 0));

        let (mut cpu, mut rbps, mut wbps) = (0.0, 0.0, 0.0);
        if let Some(p) = &self.prev {
            let dt = snap.taken.duration_since(p.at).as_secs_f64();
            if dt > 0.0 {
                cpu = cpu_ticks.saturating_sub(p.cpu_ticks) as f64 / clk_tck() as f64 / dt * 100.0;
                rbps = rb.saturating_sub(p.read_bytes) as f64 / dt;
                wbps = wb.saturating_sub(p.write_bytes) as f64 / dt;
            }
        }
        self.prev = Some(Prev {
            at: snap.taken,
            cpu_ticks,
            read_bytes: rb,
            write_bytes: wb,
        });
        push(&mut self.cpu_history, cpu.round() as u64);
        push(&mut self.rss_history, snap.rss_kb());

        let age_secs = uptime_secs()
            .ok()
            .map(|up| up - snap.stat.starttime as f64 / clk_tck() as f64);
        Ok(Sample {
            snap,
            cpu_percent: cpu,
            read_bps: rbps,
            write_bps: wbps,
            age_secs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_sample_produces_rates() {
        let mut s = Sampler::new(std::process::id());
        let first = s.sample().unwrap();
        assert_eq!(first.cpu_percent, 0.0); // no baseline yet
                                            // Burn a little CPU so the delta is non-zero.
        let t = Instant::now();
        while t.elapsed().as_millis() < 60 {
            std::hint::black_box(1u64.wrapping_mul(3));
        }
        let second = s.sample().unwrap();
        assert!(second.cpu_percent >= 0.0);
        assert_eq!(s.cpu_history.len(), 2);
        assert!(second.age_secs.unwrap() >= 0.0);
    }
}
