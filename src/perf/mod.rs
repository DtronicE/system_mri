//! Phase 3 — a clean-room slice of `perf`: hardware/software counters via
//! `perf_event_open(2)`, plus a cheap statistical wait-state profiler.
//!
//! `perf_event_open` returns a file descriptor; `read(2)` on it yields a 64-bit
//! counter that the kernel (and the CPU's PMU) keeps up to date. One fd counts
//! one event for one thread, so a process is measured by opening one fd per
//! thread and summing — exactly what `perf stat -p <pid>` does.

mod counters;
mod profile;

pub use counters::{CounterReading, Counters, Event};
pub use profile::{WaitProfile, WaitProfiler};
