//! # System MRI
//!
//! A library (and TUI) that reveals the internals of a running Linux process
//! by reading the interfaces the kernel already exposes: `/proc`, `ptrace(2)`
//! and `perf_event_open(2)`.
//!
//! Layering (see `docs/ARCHITECTURE.md`):
//!
//! ```text
//! tui  ──►  sampler / snapshot  ──►  procfs (pure parsers + readers)
//!  │                                    ▲
//!  ├──►  trace  (ptrace)                │ explain (educational text)
//!  └──►  perf   (perf_event_open)       │
//! ```

#[cfg(not(target_os = "linux"))]
compile_error!("System MRI relies on Linux-specific kernel interfaces (/proc, ptrace, perf).");

pub mod error;
pub mod explain;
pub mod perf;
pub mod procfs;
pub mod report;
pub mod sampler;
pub mod snapshot;
pub mod syscalls;
pub mod trace;
pub mod tui;
pub mod util;
