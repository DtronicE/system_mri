//! Phase 2 — a clean-room `strace`: observe every syscall a process makes.
//!
//! ## How it works (the 60-second version)
//!
//! 1. `PTRACE_SEIZE` each thread of the target (attaching without stopping it).
//! 2. `PTRACE_INTERRUPT` to get a first stop, then repeatedly resume with
//!    `PTRACE_SYSCALL`: the kernel stops the tracee at **every syscall entry
//!    and exit** and notifies us through `waitpid(2)`.
//! 3. At each stop, `PTRACE_GET_SYSCALL_INFO` (Linux ≥ 5.3) tells us whether it
//!    is an entry or an exit, the syscall number, its six arguments and the
//!    return value — independent of CPU architecture.
//! 4. String arguments (paths) are read out of the tracee with `/proc/<tid>/mem`.
//!
//! The tracer runs on its own thread (ptrace requests must come from the
//! thread that attached) and streams [`TraceMsg`]s over a bounded channel, so a
//! slow UI can never stall the tracee — excess events are counted as dropped.

mod decode;
mod stats;
mod tracer;

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{sync_channel, Receiver},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub use stats::{SyscallStat, SyscallStats};

/// One completed syscall (entry paired with its exit).
#[derive(Debug, Clone)]
pub struct SyscallEvent {
    /// Time since tracing started.
    pub at: Duration,
    pub tid: u32,
    pub nr: i64,
    pub name: String,
    /// Pre-formatted arguments, e.g. `AT_FDCWD, "/etc/passwd", O_RDONLY|O_CLOEXEC`.
    pub args: String,
    pub ret: i64,
    /// `Some(errno)` when the call failed.
    pub errno: Option<i32>,
    /// Wall time between entry and exit stops (includes ptrace overhead).
    pub duration: Duration,
}

impl SyscallEvent {
    pub fn errno_name(&self) -> Option<String> {
        self.errno
            .map(|e| format!("{:?}", nix::errno::Errno::from_raw(e)))
    }

    /// strace-like one-liner: `openat(AT_FDCWD, "/x", O_RDONLY) = -1 ENOENT`.
    pub fn render(&self) -> String {
        let ret = match self.errno_name() {
            Some(e) => format!("-1 {e}"),
            None => self.ret.to_string(),
        };
        format!("{}({}) = {}", self.name, self.args, ret)
    }
}

#[derive(Debug, Clone)]
pub enum TraceMsg {
    Attached { threads: usize },
    Syscall(SyscallEvent),
    Signal { tid: u32, name: String },
    ThreadExited { tid: u32, detail: String },
    Error(String),
    Ended,
}

/// Owns the tracer thread. Dropping it detaches from the target and lets the
/// process continue normally.
pub struct TraceHandle {
    pub rx: Receiver<TraceMsg>,
    dropped: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    pthread: Arc<AtomicU64>,
    join: Option<JoinHandle<()>>,
}

impl TraceHandle {
    /// Events discarded because the UI could not keep up.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Start tracing `pid`. Attach errors (e.g. `EPERM` under Yama) arrive as a
/// [`TraceMsg::Error`] on the channel.
pub fn spawn(pid: u32) -> TraceHandle {
    let (tx, rx) = sync_channel(65_536);
    let dropped = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let pthread = Arc::new(AtomicU64::new(0));

    let ctx = tracer::Context {
        pid,
        tx,
        dropped: dropped.clone(),
        stop: stop.clone(),
        pthread: pthread.clone(),
    };
    let done_flag = done.clone();
    let join = thread::Builder::new()
        .name("mri-tracer".into())
        .spawn(move || {
            tracer::run(ctx);
            done_flag.store(true, Ordering::SeqCst);
        })
        .expect("failed to spawn tracer thread");

    TraceHandle {
        rx,
        dropped,
        stop,
        done,
        pthread,
        join: Some(join),
    }
}

impl Drop for TraceHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The tracer is usually parked inside waitpid(2). A no-op SIGUSR1 aimed
        // at *that thread* makes the syscall return EINTR so it can notice the
        // stop flag. We repeat it until the thread reports completion, which
        // closes the race where the signal lands just before waitpid begins.
        while !self.done.load(Ordering::SeqCst) {
            let t = self.pthread.load(Ordering::SeqCst);
            if t != 0 {
                // SAFETY: the thread has not been joined, so its pthread_t is valid.
                unsafe { libc::pthread_kill(t as libc::pthread_t, libc::SIGUSR1) };
            }
            thread::sleep(Duration::from_millis(5));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
