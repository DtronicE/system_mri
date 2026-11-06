//! The tracer thread: ptrace attach → syscall-stop loop → clean detach.

use std::{
    collections::HashMap,
    fs,
    os::raw::c_void,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{SyncSender, TrySendError},
        Arc, Once,
    },
    thread,
    time::{Duration, Instant},
};

use nix::{
    errno::Errno,
    sys::{
        ptrace::{self, Options},
        signal::{sigaction, SaFlags, SigAction, SigHandler, SigSet, Signal},
        wait::{waitpid, WaitPidFlag, WaitStatus},
    },
    unistd::Pid,
};

use super::{decode, SyscallEvent, TraceMsg};
use crate::syscalls;

pub struct Context {
    pub pid: u32,
    pub tx: SyncSender<TraceMsg>,
    pub dropped: Arc<AtomicU64>,
    pub stop: Arc<AtomicBool>,
    pub pthread: Arc<AtomicU64>,
}

impl Context {
    fn send(&self, msg: TraceMsg) {
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => self.stop.store(true, Ordering::SeqCst),
        }
    }
}

// ---- PTRACE_GET_SYSCALL_INFO (not wrapped by nix) -------------------------

const PTRACE_GET_SYSCALL_INFO: u32 = 0x420e;
const OP_ENTRY: u8 = 1;
const OP_EXIT: u8 = 2;

/// Mirror of `struct ptrace_syscall_info` from `<linux/ptrace.h>`.
/// The union is sized for its largest member (`seccomp`: 8 + 6*8 + 4 → 64 B).
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SyscallInfo {
    op: u8,
    _pad: [u8; 3],
    arch: u32,
    instruction_pointer: u64,
    stack_pointer: u64,
    /// entry: `nr, args[6]` · exit: `rval (i64), is_error (u8)`.
    data: [u64; 8],
}

fn get_syscall_info(tid: Pid) -> Result<SyscallInfo, Errno> {
    let mut info = SyscallInfo::default();
    // SAFETY: we pass a correctly sized, writable buffer; the kernel fills at
    // most `size_of::<SyscallInfo>()` bytes and returns the size it needed.
    let rc = unsafe {
        libc::ptrace(
            PTRACE_GET_SYSCALL_INFO as _,
            tid.as_raw(),
            std::mem::size_of::<SyscallInfo>() as *mut c_void,
            &mut info as *mut SyscallInfo as *mut c_void,
        )
    };
    if rc < 0 {
        Err(Errno::last())
    } else {
        Ok(info)
    }
}

// ---- signal plumbing so Drop can interrupt waitpid ------------------------

extern "C" fn noop_handler(_: libc::c_int) {}

fn install_wakeup_handler() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // No SA_RESTART: we *want* waitpid to fail with EINTR.
        let action = SigAction::new(
            SigHandler::Handler(noop_handler),
            SaFlags::empty(),
            SigSet::empty(),
        );
        // SAFETY: the handler is async-signal-safe (it does nothing).
        unsafe { sigaction(Signal::SIGUSR1, &action) }.expect("install SIGUSR1 handler");
    });
}

// ---------------------------------------------------------------------------

struct Pending {
    nr: i64,
    args: String,
    entered: Instant,
}

pub fn run(ctx: Context) {
    install_wakeup_handler();
    // SAFETY: pthread_self is always safe to call.
    ctx.pthread
        .store(unsafe { libc::pthread_self() } as u64, Ordering::SeqCst);
    if let Err(e) = trace(&ctx) {
        ctx.send(TraceMsg::Error(e));
    }
    ctx.send(TraceMsg::Ended);
}

fn list_tids(pid: u32) -> Vec<Pid> {
    fs::read_dir(format!("/proc/{pid}/task"))
        .map(|d| {
            d.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
                .map(Pid::from_raw)
                .collect()
        })
        .unwrap_or_default()
}

fn attach_error(e: Errno, pid: u32) -> String {
    match e {
        Errno::EPERM => format!(
            "ptrace attach to {pid} denied (EPERM). Run as root, or relax Yama: \
             `sudo sysctl kernel.yama.ptrace_scope=0`"
        ),
        Errno::ESRCH => format!("process {pid} does not exist"),
        other => format!("ptrace attach to {pid} failed: {other}"),
    }
}

fn trace(ctx: &Context) -> Result<(), String> {
    let main = Pid::from_raw(ctx.pid as i32);
    let opts = Options::PTRACE_O_TRACESYSGOOD | Options::PTRACE_O_TRACECLONE;

    // Seize the main thread first so a permission error is reported cleanly.
    ptrace::seize(main, opts).map_err(|e| attach_error(e, ctx.pid))?;
    let mut tracees: HashMap<Pid, Option<Pending>> = HashMap::new();
    tracees.insert(main, None);
    for tid in list_tids(ctx.pid) {
        if tid != main && ptrace::seize(tid, opts).is_ok() {
            tracees.insert(tid, None);
        }
    }
    // SEIZE does not stop the tracee; INTERRUPT forces a first stop that
    // arrives as a PTRACE_EVENT_STOP, where we start the PTRACE_SYSCALL loop.
    for &tid in tracees.keys() {
        let _ = ptrace::interrupt(tid);
    }
    ctx.send(TraceMsg::Attached {
        threads: tracees.len(),
    });

    let start = Instant::now();
    let result = event_loop(ctx, &mut tracees, start);
    detach_all(tracees.keys().copied().collect());
    result
}

fn event_loop(
    ctx: &Context,
    tracees: &mut HashMap<Pid, Option<Pending>>,
    start: Instant,
) -> Result<(), String> {
    while !ctx.stop.load(Ordering::SeqCst) {
        let status = match waitpid(None, Some(WaitPidFlag::__WALL)) {
            Ok(s) => s,
            Err(Errno::EINTR) => continue, // woken by Drop → re-check stop flag
            Err(Errno::ECHILD) => return Ok(()), // no tracees left
            Err(e) => return Err(format!("waitpid failed: {e}")),
        };
        match status {
            WaitStatus::PtraceSyscall(tid) => {
                on_syscall_stop(ctx, tracees, tid, start);
                resume(tid, None);
            }
            // Clone events, the initial INTERRUPT stop, group-stops …
            WaitStatus::PtraceEvent(tid, _, _) => {
                tracees.entry(tid).or_insert(None);
                resume(tid, None);
            }
            // Signal-delivery-stop: record it and pass the signal through.
            WaitStatus::Stopped(tid, sig) => {
                tracees.entry(tid).or_insert(None);
                if sig != Signal::SIGTRAP {
                    ctx.send(TraceMsg::Signal {
                        tid: tid.as_raw() as u32,
                        name: sig.as_str().to_string(),
                    });
                }
                resume(tid, (sig != Signal::SIGTRAP).then_some(sig));
            }
            WaitStatus::Exited(tid, code) => {
                tracees.remove(&tid);
                ctx.send(TraceMsg::ThreadExited {
                    tid: tid.as_raw() as u32,
                    detail: format!("exit({code})"),
                });
            }
            WaitStatus::Signaled(tid, sig, _) => {
                tracees.remove(&tid);
                ctx.send(TraceMsg::ThreadExited {
                    tid: tid.as_raw() as u32,
                    detail: format!("killed by {}", sig.as_str()),
                });
            }
            _ => {}
        }
        if tracees.is_empty() {
            return Ok(());
        }
    }
    Ok(())
}

fn resume(tid: Pid, sig: Option<Signal>) {
    // ESRCH just means the thread died between the stop and now.
    let _ = ptrace::syscall(tid, sig);
}

fn on_syscall_stop(
    ctx: &Context,
    tracees: &mut HashMap<Pid, Option<Pending>>,
    tid: Pid,
    start: Instant,
) {
    let Ok(info) = get_syscall_info(tid) else {
        return;
    };
    let slot = tracees.entry(tid).or_insert(None);
    let raw_tid = tid.as_raw() as u32;
    match info.op {
        OP_ENTRY => {
            let nr = info.data[0] as i64;
            let mut args = [0u64; 6];
            args.copy_from_slice(&info.data[1..7]);
            *slot = Some(Pending {
                nr,
                args: decode::format_args(raw_tid, nr, &args),
                entered: Instant::now(),
            });
        }
        OP_EXIT => {
            // An exit without a matching entry happens once, right after attach.
            let Some(p) = slot.take() else { return };
            let rval = info.data[0] as i64;
            let is_error = (info.data[1] & 0xff) != 0;
            ctx.send(TraceMsg::Syscall(SyscallEvent {
                at: p.entered.duration_since(start),
                tid: raw_tid,
                nr: p.nr,
                name: syscalls::name(p.nr)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("syscall_{}", p.nr)),
                args: p.args,
                ret: rval,
                errno: is_error.then_some((-rval) as i32),
                duration: p.entered.elapsed(),
            }));
        }
        _ => {}
    }
}

/// Stop every tracee and detach so the target resumes untouched.
fn detach_all(tids: Vec<Pid>) {
    use std::collections::HashSet;
    let mut pending: HashSet<Pid> = tids.iter().copied().collect();
    for &t in &tids {
        let _ = ptrace::interrupt(t);
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    while !pending.is_empty() && Instant::now() < deadline {
        match waitpid(None, Some(WaitPidFlag::__WALL | WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => thread::sleep(Duration::from_millis(1)),
            Ok(WaitStatus::Exited(p, _)) | Ok(WaitStatus::Signaled(p, _, _)) => {
                pending.remove(&p);
            }
            Ok(other) => {
                if let Some(p) = other.pid() {
                    // Any ptrace-stop is a valid moment to detach. Signals seen
                    // here are not re-injected (negligible at shutdown).
                    let _ = ptrace::detach(p, None);
                    pending.remove(&p);
                }
            }
            Err(Errno::EINTR) => {}
            Err(_) => break,
        }
    }
}
