//! End-to-end: trace a real child process with ptrace and observe a failing
//! `stat` on a path that does not exist.
//!
//! Skipped (not failed) when the environment forbids ptrace, e.g. in some
//! containers.

use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use system_mri::trace::{self, TraceMsg};

#[test]
fn traces_a_failing_stat_in_a_child() {
    // dash/bash builtin `[ -e path ]` performs a stat-family syscall in-process.
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("sleep 0.5; i=0; while [ $i -lt 200000 ]; do [ -e /nonexistent_mri_probe ]; i=$((i+1)); done")
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn sh");

    let handle = trace::spawn(child.id());
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut found = false;
    let mut skipped = false;

    while Instant::now() < deadline && !found {
        match handle.rx.recv_timeout(Duration::from_millis(200)) {
            Ok(TraceMsg::Syscall(ev)) => {
                if ev.args.contains("/nonexistent_mri_probe") {
                    assert!(
                        ev.errno_name().as_deref() == Some("ENOENT"),
                        "{}",
                        ev.render()
                    );
                    found = true;
                }
            }
            Ok(TraceMsg::Error(e)) => {
                eprintln!("skipping: {e}");
                skipped = true;
                break;
            }
            _ => {}
        }
    }
    drop(handle); // must detach and return promptly
    let _ = child.kill();
    let _ = child.wait();
    assert!(found || skipped, "never saw the probe syscall");
}
