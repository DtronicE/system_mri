//! A poor man's off-CPU profiler.
//!
//! Every few milliseconds we read each thread's `state` and — if it is blocked
//! in a syscall — `/proc/<tid>/syscall`. Aggregating thousands of such samples
//! answers "what is this process *waiting* on?" with no ptrace stop and
//! negligible overhead. It cannot see *where in user code* CPU time goes
//! (that needs sampling PMU events; see the roadmap).

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::procfs::threads::{self, SyscallState};

#[derive(Debug, Clone, Default)]
pub struct WaitProfile {
    pub samples: u64,
    /// label → number of (thread, tick) samples. Labels look like
    /// `on-cpu`, `blocked: futex`, `sleeping`, `disk wait (D)`.
    pub counts: HashMap<String, u64>,
}

impl WaitProfile {
    pub fn sorted(&self) -> Vec<(&str, u64)> {
        let mut v: Vec<_> = self.counts.iter().map(|(k, &c)| (k.as_str(), c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        v
    }
}

pub struct WaitProfiler {
    shared: Arc<Mutex<WaitProfile>>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

fn classify(state: char, sc: &Option<SyscallState>) -> String {
    match (state, sc) {
        ('R', _) => "on-cpu / runnable".into(),
        ('D', _) => "uninterruptible wait (D, usually disk)".into(),
        ('T' | 't', _) => "stopped / traced".into(),
        (_, Some(s @ SyscallState::InSyscall { .. })) => format!("blocked: {}", s.label()),
        ('S', _) => "sleeping (not in syscall)".into(),
        (c, _) => format!("state {c}"),
    }
}

impl WaitProfiler {
    pub fn start(pid: u32, period: Duration) -> Self {
        let shared = Arc::new(Mutex::new(WaitProfile::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, st2) = (shared.clone(), stop.clone());
        let join = thread::Builder::new()
            .name("mri-waitprof".into())
            .spawn(move || {
                while !st2.load(Ordering::Relaxed) {
                    let Ok(ts) = threads::read(pid) else { break };
                    {
                        let mut p = s2.lock().unwrap();
                        for t in &ts {
                            p.samples += 1;
                            *p.counts.entry(classify(t.state, &t.syscall)).or_default() += 1;
                        }
                    }
                    thread::sleep(period);
                }
            })
            .expect("spawn profiler thread");
        Self {
            shared,
            stop,
            join: Some(join),
        }
    }

    pub fn snapshot(&self) -> WaitProfile {
        self.shared.lock().unwrap().clone()
    }

    pub fn reset(&self) {
        *self.shared.lock().unwrap() = WaitProfile::default();
    }
}

impl Drop for WaitProfiler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_states() {
        assert_eq!(classify('R', &None), "on-cpu / runnable");
        let futex = Some(SyscallState::InSyscall {
            nr: 202,
            args: [0; 6],
        });
        #[cfg(target_arch = "x86_64")]
        assert_eq!(classify('S', &futex), "blocked: futex");
        assert_eq!(classify('S', &None), "sleeping (not in syscall)");
    }

    #[test]
    fn profiles_ourselves() {
        let p = WaitProfiler::start(std::process::id(), Duration::from_millis(5));
        thread::sleep(Duration::from_millis(60));
        let snap = p.snapshot();
        drop(p);
        assert!(snap.samples > 0);
        assert!(!snap.sorted().is_empty());
    }
}
