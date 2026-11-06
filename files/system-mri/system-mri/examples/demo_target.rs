//! A deliberately interesting process to point System MRI at.
//!
//! ```text
//! cargo run --example demo_target      # prints its PID
//! mri <pid>                            # in another terminal
//! ```
//!
//! It has: a busy thread (on-CPU), a thread that writes to a file and sleeps
//! (I/O + nanosleep), a thread blocked on a channel (futex), a TCP listener
//! with one established connection, a ~64 MiB heap allocation, and a leaked fd.

use std::{
    fs::OpenOptions,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::Duration,
};

fn main() -> std::io::Result<()> {
    println!("demo_target pid = {}", std::process::id());

    // Memory: touch every page so it shows up in RSS, not just VSZ.
    let mut big = vec![0u8; 64 << 20];
    for page in big.chunks_mut(4096) {
        page[0] = 1;
    }

    // Network: a listener and one established loopback connection.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    println!("listening on {addr}");
    let mut client = TcpStream::connect(addr)?;
    let (mut server_side, _) = listener.accept()?;

    // Leaked descriptor.
    let leaked = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/mri_demo.log")?;
    std::mem::forget(leaked);

    // Thread 1: burn CPU.
    thread::Builder::new().name("busy".into()).spawn(|| {
        let mut x = 1u64;
        loop {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            std::hint::black_box(x);
        }
    })?;

    // Thread 2: file + socket I/O, then sleep.
    thread::Builder::new().name("io".into()).spawn(move || {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/mri_demo.log")
            .unwrap();
        let mut buf = [0u8; 64];
        loop {
            writeln!(f, "tick").unwrap();
            let _ = std::fs::metadata("/nonexistent/mri-demo"); // provoke ENOENT
            client.write_all(b"ping").unwrap();
            let _ = server_side.read(&mut buf).unwrap();
            thread::sleep(Duration::from_millis(50));
        }
    })?;

    // Thread 3: blocked forever (futex wait).
    let (_tx, rx) = mpsc::channel::<()>();
    thread::Builder::new()
        .name("waiter".into())
        .spawn(move || {
            let _ = rx.recv();
        })?;

    loop {
        big[0] = big[0].wrapping_add(1);
        thread::sleep(Duration::from_secs(1));
    }
}
