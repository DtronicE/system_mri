use std::{
    collections::HashMap,
    fs,
    fs::File,
    io::Read,
    os::fd::{FromRawFd, OwnedFd},
};

/// `perf_event_attr` up to `PERF_ATTR_SIZE_VER5` (112 bytes). The kernel
/// accepts older/smaller sizes, which keeps us portable across versions.
#[repr(C)]
#[derive(Default)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    /// Bitfield: bit0 disabled, bit1 inherit, … bit5 exclude_kernel, bit6 exclude_hv.
    flags: u64,
    wakeup_events: u32,
    bp_type: u32,
    config1: u64,
    config2: u64,
    branch_sample_type: u64,
    sample_regs_user: u64,
    sample_stack_user: u32,
    clockid: i32,
    sample_regs_intr: u64,
    aux_watermark: u32,
    sample_max_stack: u16,
    reserved2: u16,
}

const PERF_TYPE_HARDWARE: u32 = 0;
const PERF_TYPE_SOFTWARE: u32 = 1;
const FLAG_EXCLUDE_KERNEL: u64 = 1 << 5;
const FLAG_EXCLUDE_HV: u64 = 1 << 6;
const PERF_FLAG_FD_CLOEXEC: libc::c_ulong = 1 << 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    Cycles,
    Instructions,
    CacheReferences,
    CacheMisses,
    Branches,
    BranchMisses,
    TaskClockNs,
    PageFaults,
    ContextSwitches,
    CpuMigrations,
}

impl Event {
    pub const ALL: [Event; 10] = [
        Event::Cycles,
        Event::Instructions,
        Event::CacheReferences,
        Event::CacheMisses,
        Event::Branches,
        Event::BranchMisses,
        Event::TaskClockNs,
        Event::PageFaults,
        Event::ContextSwitches,
        Event::CpuMigrations,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Event::Cycles => "cycles",
            Event::Instructions => "instructions",
            Event::CacheReferences => "cache-references",
            Event::CacheMisses => "cache-misses",
            Event::Branches => "branches",
            Event::BranchMisses => "branch-misses",
            Event::TaskClockNs => "task-clock (ns)",
            Event::PageFaults => "page-faults",
            Event::ContextSwitches => "context-switches",
            Event::CpuMigrations => "cpu-migrations",
        }
    }

    /// `(perf_type, config)` from `<linux/perf_event.h>`.
    fn raw(self) -> (u32, u64) {
        match self {
            Event::Cycles => (PERF_TYPE_HARDWARE, 0),
            Event::Instructions => (PERF_TYPE_HARDWARE, 1),
            Event::CacheReferences => (PERF_TYPE_HARDWARE, 2),
            Event::CacheMisses => (PERF_TYPE_HARDWARE, 3),
            Event::Branches => (PERF_TYPE_HARDWARE, 4),
            Event::BranchMisses => (PERF_TYPE_HARDWARE, 5),
            Event::TaskClockNs => (PERF_TYPE_SOFTWARE, 1),
            Event::PageFaults => (PERF_TYPE_SOFTWARE, 2),
            Event::ContextSwitches => (PERF_TYPE_SOFTWARE, 3),
            Event::CpuMigrations => (PERF_TYPE_SOFTWARE, 4),
        }
    }
}

fn open_event(tid: u32, event: Event) -> Result<File, std::io::Error> {
    let (type_, config) = event.raw();
    let attr = PerfEventAttr {
        type_,
        size: std::mem::size_of::<PerfEventAttr>() as u32,
        config,
        // User-space only: allowed even with kernel.perf_event_paranoid=2.
        flags: FLAG_EXCLUDE_KERNEL | FLAG_EXCLUDE_HV,
        ..Default::default()
    };
    // SAFETY: `attr` is a valid, fully initialised perf_event_attr; the other
    // arguments are plain integers (pid=tid, cpu=-1 any, group=-1 none).
    let fd = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            &attr as *const PerfEventAttr,
            tid as libc::pid_t,
            -1 as libc::c_int,
            -1 as libc::c_int,
            PERF_FLAG_FD_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: we just received a fresh descriptor that nobody else owns.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd as i32) }))
}

#[derive(Debug, Clone)]
pub struct CounterReading {
    pub event: Event,
    /// `Err` carries a human reason ("not supported by this CPU/VM", …).
    pub value: Result<u64, String>,
}

struct PerEvent {
    files: HashMap<u32, File>,
    /// Last value read per thread, so counts of exited threads are retained.
    last: HashMap<u32, u64>,
    error: Option<String>,
}

/// A set of counters following one process across all of its threads.
pub struct Counters {
    pid: u32,
    events: Vec<(Event, PerEvent)>,
}

fn explain_errno(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(libc::EACCES) | Some(libc::EPERM) => {
            "permission denied: lower /proc/sys/kernel/perf_event_paranoid or run as root".into()
        }
        Some(libc::ENOENT) | Some(libc::ENODEV) | Some(libc::EOPNOTSUPP) => {
            "not supported here (virtual machine without a PMU?)".into()
        }
        _ => e.to_string(),
    }
}

impl Counters {
    pub fn new(pid: u32, events: &[Event]) -> Self {
        let mut c = Counters {
            pid,
            events: events
                .iter()
                .map(|&e| {
                    (
                        e,
                        PerEvent {
                            files: HashMap::new(),
                            last: HashMap::new(),
                            error: None,
                        },
                    )
                })
                .collect(),
        };
        c.refresh_threads();
        c
    }

    fn tids(&self) -> Vec<u32> {
        fs::read_dir(format!("/proc/{}/task", self.pid))
            .map(|d| {
                d.flatten()
                    .filter_map(|e| e.file_name().to_str()?.parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Open counters for threads that appeared since the last call.
    pub fn refresh_threads(&mut self) {
        let tids = self.tids();
        for (event, per) in &mut self.events {
            if per.error.is_some() {
                continue; // unsupported event: don't retry every tick
            }
            for &tid in &tids {
                if per.files.contains_key(&tid) || per.last.contains_key(&tid) {
                    continue;
                }
                match open_event(tid, *event) {
                    Ok(f) => {
                        per.files.insert(tid, f);
                    }
                    // The thread may simply have exited meanwhile.
                    Err(e) if e.raw_os_error() == Some(libc::ESRCH) => {}
                    Err(e) => {
                        per.error = Some(explain_errno(&e));
                        per.files.clear();
                        break;
                    }
                }
            }
        }
    }

    pub fn read(&mut self) -> Vec<CounterReading> {
        self.refresh_threads();
        self.events
            .iter_mut()
            .map(|(event, per)| {
                if let Some(err) = &per.error {
                    return CounterReading {
                        event: *event,
                        value: Err(err.clone()),
                    };
                }
                let mut dead = Vec::new();
                for (&tid, file) in per.files.iter_mut() {
                    let mut buf = [0u8; 8];
                    match file.read_exact(&mut buf) {
                        Ok(()) => {
                            per.last.insert(tid, u64::from_ne_bytes(buf));
                        }
                        Err(_) => dead.push(tid), // thread gone: keep `last`
                    }
                }
                for tid in dead {
                    per.files.remove(&tid);
                }
                CounterReading {
                    event: *event,
                    value: Ok(per.last.values().sum()),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attr_layout_matches_kernel_ver5() {
        assert_eq!(std::mem::size_of::<PerfEventAttr>(), 112);
    }

    #[test]
    fn software_counters_work_on_ourselves_or_report_why() {
        let mut c = Counters::new(std::process::id(), &[Event::TaskClockNs, Event::PageFaults]);
        // Touch fresh memory to provoke page faults.
        let v = vec![1u8; 8 << 20];
        std::hint::black_box(&v);
        let readings = c.read();
        assert_eq!(readings.len(), 2);
        for r in readings {
            // In locked-down sandboxes perf may be forbidden; either outcome is valid.
            match r.value {
                Ok(n) => assert!(
                    n > 0 || r.event == Event::CpuMigrations,
                    "{:?} = {n}",
                    r.event
                ),
                Err(reason) => eprintln!("{:?} unavailable: {reason}", r.event),
            }
        }
    }
}
