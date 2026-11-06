//! Typed, testable readers for the `/proc` pseudo-filesystem.
//!
//! Every submodule follows the same pattern:
//!
//! * a **pure parser** (`parse(&str) -> T`) that never touches the disk, so it
//!   can be unit-tested against fixture text, and
//! * a thin **reader** (`read(pid) -> Result<T>`) that fetches the file and
//!   calls the parser.

pub mod fd;
pub mod io;
pub mod maps;
pub mod net;
pub mod stat;
pub mod status;
pub mod system;
pub mod threads;

use std::path::PathBuf;

pub(crate) fn proc_path(pid: u32, file: &str) -> PathBuf {
    PathBuf::from(format!("/proc/{pid}/{file}"))
}
