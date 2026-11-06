use std::time::Duration;

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::tui::app::{Inspector, TraceSession, TraceView};

use super::common::*;

fn fmt_dur(d: Duration) -> String {
    let us = d.as_micros();
    if us >= 1_000_000 {
        format!("{:.2}s", d.as_secs_f64())
    } else if us >= 1_000 {
        format!("{:.2}ms", us as f64 / 1000.0)
    } else {
        format!("{us}µs")
    }
}

pub fn render(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    let view = ins.trace_view;
    let scroll = ins.trace_scroll;
    let Some(t) = &ins.trace else {
        let msg = vec![
            Line::styled("Syscall tracing is OFF.", Style::new().add_modifier(Modifier::BOLD)),
            Line::raw(""),
            Line::raw("Press `a` to attach with ptrace(2)."),
            Line::raw(""),
            Line::styled(
                "Heads-up: while attached, every syscall of the target stops twice, so busy processes slow down noticeably. \
Detaching (`a` again, or quitting) restores full speed.",
                Style::new().fg(WARN),
            ),
            Line::raw(""),
            Line::raw("Attaching to a process you did not start needs root, or `sudo sysctl kernel.yama.ptrace_scope=0`."),
        ];
        f.render_widget(
            Paragraph::new(msg)
                .wrap(Wrap { trim: true })
                .block(panel("Syscalls")),
            area,
        );
        return;
    };

    let [status, main] = Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(area);
    render_status(f, status, t);

    match view {
        TraceView::Log => render_log(f, main, t, scroll),
        TraceView::Summary => render_summary(f, main, t, &mut ins.summary_state),
    }
}

fn render_status(f: &mut Frame, area: Rect, t: &TraceSession) {
    let mut spans = vec![];
    if let Some(e) = &t.error {
        spans.push(Span::styled(
            format!("✖ {e}"),
            Style::new().fg(ERR).add_modifier(Modifier::BOLD),
        ));
    } else if t.ended {
        spans.push(Span::styled(
            "■ target exited / tracer stopped",
            Style::new().fg(WARN),
        ));
    } else if let Some(n) = t.attached_threads {
        spans.push(Span::styled(
            format!("● tracing {n} thread(s)"),
            Style::new().fg(Color::Green),
        ));
    } else {
        spans.push(Span::raw("… attaching"));
    }
    spans.push(Span::raw(format!("   {} calls", t.stats.total_calls)));
    if t.dropped() > 0 {
        spans.push(Span::styled(
            format!("   {} dropped (UI too slow)", t.dropped()),
            Style::new().fg(WARN),
        ));
    }
    if let Some(n) = t.notes.back() {
        spans.push(Span::styled(
            format!("   last event: {n}"),
            Style::new().fg(DIM),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).block(panel("Tracer")),
        area,
    );
}

fn render_log(f: &mut Frame, area: Rect, t: &TraceSession, scroll: usize) {
    let height = area.height.saturating_sub(2) as usize;
    let len = t.log.len();
    let scroll = scroll.min(len.saturating_sub(height));
    let end = len - scroll;
    let start = end.saturating_sub(height);
    let lines: Vec<Line> = t
        .log
        .iter()
        .skip(start)
        .take(end - start)
        .map(|e| {
            let failed = e.errno.is_some();
            let slow = e.duration > Duration::from_millis(10);
            let style = if failed {
                Style::new().fg(ERR)
            } else if slow {
                Style::new().fg(WARN)
            } else {
                Style::new()
            };
            Line::from(vec![
                Span::styled(
                    format!("{:>9.3} ", e.at.as_secs_f64()),
                    Style::new().fg(DIM),
                ),
                Span::styled(format!("{:>7} ", e.tid), Style::new().fg(DIM)),
                Span::styled(e.render(), style),
                Span::styled(format!("  <{}>", fmt_dur(e.duration)), Style::new().fg(DIM)),
            ])
        })
        .collect();
    let title = if scroll == 0 {
        "Syscall log (following)".to_string()
    } else {
        format!("Syscall log (scrolled back {scroll} lines — End to follow)")
    };
    f.render_widget(Paragraph::new(lines).block(panel(title)), area);
}

fn render_summary(
    f: &mut Frame,
    area: Rect,
    t: &TraceSession,
    state: &mut ratatui::widgets::TableState,
) {
    let total = t.stats.total_time().as_secs_f64().max(1e-9);
    let rows: Vec<Row> = t
        .stats
        .sorted()
        .into_iter()
        .map(|(name, s)| {
            let pct = s.total.as_secs_f64() / total * 100.0;
            Row::new(vec![
                Cell::from(name.to_string()),
                Cell::from(s.calls.to_string()),
                Cell::from(s.errors.to_string()).style(Style::new().fg(if s.errors > 0 {
                    ERR
                } else {
                    DIM
                })),
                Cell::from(fmt_dur(s.total)),
                Cell::from(fmt_dur(s.avg())),
                Cell::from(format!("{pct:5.1}%  {}", bar(pct / 100.0, 12))),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(11),
            Constraint::Length(11),
            Constraint::Min(14),
        ],
    )
    .header(Row::new(["SYSCALL", "CALLS", "ERRORS", "TOTAL", "AVG", "% OF TIME"]).style(header_style()))
    .block(panel("Syscall summary (sorted by time spent) — blocking calls like futex/poll dominate by design"))
    .highlight_style(selected_style());
    f.render_stateful_widget(table, area, state);
}
