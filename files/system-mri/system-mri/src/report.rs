//! `mri --dump <pid>`: a one-shot plain-text report. Handy for scripting, bug
//! reports, and for testing the collectors without a terminal.

use crate::procfs::net::SocketInfo;
use crate::{
    error::Result,
    procfs::maps,
    sampler::Sampler,
    snapshot::collect,
    util::{fmt_bytes, fmt_duration, fmt_kb},
};

pub fn print(pid: u32) -> Result<()> {
    let s = collect(pid)?;
    let st = &s.stat;
    println!("== process {} ({}) ==", s.pid, st.comm);
    println!(
        "state     : {} — {}",
        st.state,
        crate::explain::proc_state(st.state)
    );
    println!(
        "ppid      : {}    threads: {}    nice: {}",
        st.ppid, st.num_threads, st.nice
    );
    println!(
        "exe       : {}",
        s.exe
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "?".into())
    );
    println!("cmdline   : {}", s.cmdline.join(" "));
    println!("faults    : {} minor, {} major", st.minflt, st.majflt);
    if let Some(a) = Sampler::new(pid).sample().ok().and_then(|x| x.age_secs) {
        println!("age       : {}", fmt_duration(a));
    }
    if let Some(m) = &s.status {
        println!(
            "memory    : VSZ {}  RSS {} (anon {}, file {}, shmem {})  swap {}",
            fmt_kb(m.vm_size.unwrap_or(0)),
            fmt_kb(m.vm_rss.unwrap_or(0)),
            fmt_kb(m.rss_anon.unwrap_or(0)),
            fmt_kb(m.rss_file.unwrap_or(0)),
            fmt_kb(m.rss_shmem.unwrap_or(0)),
            fmt_kb(m.vm_swap.unwrap_or(0)),
        );
    }

    println!("\n== address space by kind ==");
    for k in maps::summarize(&s.regions).iter().filter(|k| k.count > 0) {
        println!(
            "{:<26} {:>4} VMAs  size {:>10}  rss {:>10}  pss {:>10}",
            k.kind.label(),
            k.count,
            fmt_bytes(k.size),
            fmt_kb(k.rss_kb),
            fmt_kb(k.pss_kb)
        );
    }

    println!("\n== open files ({}) ==", s.fds.len());
    for f in &s.fds {
        println!("{:>4}  {:<10} {}", f.fd, f.kind.label(), f.target);
    }

    println!("\n== sockets ({}) ==", s.sockets.len());
    for sock in &s.sockets {
        match &sock.info {
            SocketInfo::Inet(i) => println!(
                "{:>4}  {:<5} {:<24} {:<24} {}",
                sock.fd,
                i.proto.label(),
                i.local,
                i.remote,
                i.state_label()
            ),
            SocketInfo::Unix(u) => println!(
                "{:>4}  unix  {:<9} {:<13} {}",
                sock.fd,
                u.kind,
                u.state,
                u.path.as_deref().unwrap_or("")
            ),
        }
    }

    println!("\n== threads ({}) ==", s.threads.len());
    for t in &s.threads {
        println!(
            "{:>7}  {:<16} {}  {}",
            t.tid,
            t.comm,
            t.state,
            t.syscall.as_ref().map(|x| x.label()).unwrap_or_default()
        );
    }
    for w in &s.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}
