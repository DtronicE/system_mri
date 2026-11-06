//! `mri` — command-line entry point for System MRI.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;

/// X-ray vision for Linux processes.
#[derive(Parser, Debug)]
#[command(name = "mri", version, about, long_about = None)]
struct Cli {
    /// PID to inspect. Omit to pick a process interactively.
    pid: Option<u32>,

    /// Refresh interval in milliseconds.
    #[arg(short, long, default_value_t = 1000)]
    interval: u64,

    /// Print a one-shot plain-text report instead of launching the TUI.
    #[arg(long)]
    dump: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.dump {
        let pid = cli.pid.context("--dump requires a PID")?;
        return system_mri::report::print(pid).map_err(Into::into);
    }
    system_mri::tui::run(cli.pid, Duration::from_millis(cli.interval.max(100)))
}
