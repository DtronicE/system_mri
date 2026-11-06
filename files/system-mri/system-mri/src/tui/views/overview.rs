use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Gauge, Paragraph, Sparkline, Wrap},
    Frame,
};

use crate::{
    explain,
    procfs::system::{clk_tck, mem_total_kb},
    tui::app::Inspector,
    util::{fmt_duration, fmt_kb, fmt_rate},
};

use super::common::*;

fn kv(k: &str, v: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<11}"), Style::new().fg(DIM)),
        Span::raw(v.into()),
    ])
}

fn tail(buf: &std::collections::VecDeque<u64>, width: usize) -> Vec<u64> {
    buf.iter()
        .skip(buf.len().saturating_sub(width))
        .copied()
        .collect()
}

pub fn render(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    let Some(sample) = &ins.sample else { return };
    let snap = &sample.snap;
    let st = &snap.stat;

    let [ident, graphs, mem] = Layout::vertical([
        Constraint::Length(11),
        Constraint::Length(6),
        Constraint::Min(5),
    ])
    .areas(area);

    // ── identity ──
    let cpu_secs = st.cpu_ticks() as f64 / clk_tck() as f64;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} ", st.comm),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("[{}] ", st.state),
                Style::new().fg(state_color(st.state)),
            ),
            Span::styled(explain::proc_state(st.state), Style::new().fg(DIM)),
        ]),
        kv(
            "pid/ppid",
            format!(
                "{} / {}    threads {}    nice {}    last cpu {}",
                st.pid, st.ppid, st.num_threads, st.nice, st.processor
            ),
        ),
        kv(
            "age",
            sample
                .age_secs
                .map(fmt_duration)
                .unwrap_or_else(|| "?".into()),
        ),
        kv(
            "cpu time",
            format!(
                "{:.2}s total ({:.2}s user, {:.2}s kernel)",
                cpu_secs,
                st.utime as f64 / clk_tck() as f64,
                st.stime as f64 / clk_tck() as f64
            ),
        ),
        kv(
            "exe",
            snap.exe
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(unreadable)".into()),
        ),
        kv(
            "cwd",
            snap.cwd
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(unreadable)".into()),
        ),
        kv("cmdline", snap.cmdline.join(" ")),
        kv(
            "faults",
            format!(
                "{} minor (no disk) · {} major (disk read)",
                st.minflt, st.majflt
            ),
        ),
    ];
    if let Some(s) = &snap.status {
        lines.push(kv(
            "ctx switch",
            format!(
                "{} voluntary (waited) · {} involuntary (preempted)",
                s.voluntary_ctxt_switches, s.nonvoluntary_ctxt_switches
            ),
        ));
    }
    for w in &snap.warnings {
        lines.push(Line::styled(format!("⚠ {w}"), Style::new().fg(WARN)));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel("Process")),
        ident,
    );

    // ── sparklines ──
    let [cpu_a, rss_a] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(graphs);
    let cpu_data = tail(
        &ins.sampler.cpu_history,
        cpu_a.width.saturating_sub(2) as usize,
    );
    f.render_widget(
        Sparkline::default()
            .block(panel(format!(
                "CPU {:.1}% (100% = one core)",
                sample.cpu_percent
            )))
            .data(&cpu_data)
            .style(Style::new().fg(ratatui::style::Color::Green)),
        cpu_a,
    );
    let rss_data = tail(
        &ins.sampler.rss_history,
        rss_a.width.saturating_sub(2) as usize,
    );
    let rss_min = rss_data.iter().copied().min().unwrap_or(0);
    // Sparkline bars start at 0; subtract the minimum so *changes* are visible.
    let rss_rel: Vec<u64> = rss_data.iter().map(|v| v - rss_min).collect();
    f.render_widget(
        Sparkline::default()
            .block(panel(format!(
                "RSS {} (relative trend)",
                fmt_kb(snap.rss_kb())
            )))
            .data(&rss_rel)
            .style(Style::new().fg(ratatui::style::Color::Yellow)),
        rss_a,
    );

    // ── memory + I/O ──
    let [mem_a, io_a] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(mem);
    let inner = panel("Memory");
    let inner_area = inner.inner(mem_a);
    f.render_widget(inner, mem_a);
    let rows = Layout::vertical([Constraint::Length(1); 5]).split(inner_area);
    if let Some(m) = &snap.status {
        let total = mem_total_kb().unwrap_or(1).max(1);
        let rss = m.vm_rss.unwrap_or(0);
        f.render_widget(
            Gauge::default()
                .ratio((rss as f64 / total as f64).clamp(0.0, 1.0))
                .label(format!("RSS {} of {} RAM", fmt_kb(rss), fmt_kb(total)))
                .gauge_style(Style::new().fg(ratatui::style::Color::Yellow)),
            rows[0],
        );
        let parts = [
            ("anon", m.rss_anon),
            ("file", m.rss_file),
            ("shmem", m.rss_shmem),
        ];
        let sum: u64 = parts.iter().filter_map(|(_, v)| *v).sum::<u64>().max(1);
        for (i, (name, v)) in parts.iter().enumerate() {
            let v = v.unwrap_or(0);
            f.render_widget(
                Paragraph::new(format!(
                    "{name:<6}{} {}",
                    bar(v as f64 / sum as f64, 20),
                    fmt_kb(v)
                )),
                rows[i + 1],
            );
        }
        f.render_widget(
            Paragraph::new(format!(
                "VSZ {} (reserved) · peak RSS {} · swap {}",
                fmt_kb(m.vm_size.unwrap_or(0)),
                fmt_kb(m.vm_hwm.unwrap_or(0)),
                fmt_kb(m.vm_swap.unwrap_or(0))
            )),
            rows[4],
        );
    } else {
        f.render_widget(Paragraph::new("status unreadable"), inner_area);
    }

    let mut io_lines = Vec::new();
    match &snap.io {
        Some(io) => {
            io_lines.push(kv(
                "read",
                format!("{}   ({} calls)", fmt_rate(sample.read_bps), io.syscr),
            ));
            io_lines.push(kv(
                "write",
                format!("{}   ({} calls)", fmt_rate(sample.write_bps), io.syscw),
            ));
            io_lines.push(kv(
                "rchar",
                format!("{} through read()", fmt_kb(io.rchar / 1024)),
            ));
            io_lines.push(kv(
                "wchar",
                format!("{} through write()", fmt_kb(io.wchar / 1024)),
            ));
            io_lines.push(Line::styled(
                "rchar counts cache hits; read/write rates count real disk I/O.",
                Style::new().fg(DIM),
            ));
        }
        None => io_lines.push(Line::raw(
            "I/O counters unavailable (need same uid or root)",
        )),
    }
    f.render_widget(
        Paragraph::new(io_lines)
            .wrap(Wrap { trim: true })
            .block(panel("Block I/O")),
        io_a,
    );
}
