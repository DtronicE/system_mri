use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::{perf::Event, tui::app::Inspector};

use super::common::*;

fn human(n: f64) -> String {
    match n {
        n if n >= 1e9 => format!("{:.2} G", n / 1e9),
        n if n >= 1e6 => format!("{:.2} M", n / 1e6),
        n if n >= 1e3 => format!("{:.1} k", n / 1e3),
        n => format!("{n:.0}"),
    }
}

pub fn render(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    let Some(p) = &ins.perf else {
        let msg = vec![
            Line::styled("Perf sessions are OFF.", Style::new().add_modifier(Modifier::BOLD)),
            Line::raw(""),
            Line::raw("Press `a` to open perf_event_open(2) counters for every thread and start the wait-state profiler."),
            Line::raw(""),
            Line::styled(
                "Counting has negligible overhead (the CPU's PMU does the work). In VMs, hardware counters are often unavailable; \
software counters (page faults, context switches) still work.",
                Style::new().fg(DIM),
            ),
        ];
        f.render_widget(
            Paragraph::new(msg)
                .wrap(Wrap { trim: true })
                .block(panel("Perf")),
            area,
        );
        return;
    };

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
    let [table_a, derived_a] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(5)]).areas(left);

    // ── counters ──
    let rows: Vec<Row> = if p.readings.is_empty() {
        vec![Row::new(vec!["collecting first sample…"])]
    } else {
        p.readings
            .iter()
            .zip(p.rates.iter().chain(std::iter::repeat(&None)))
            .map(|(r, rate)| match &r.value {
                Ok(v) => Row::new(vec![
                    Cell::from(r.event.label()),
                    Cell::from(human(*v as f64)),
                    Cell::from(rate.map(|x| format!("{}/s", human(x))).unwrap_or_default()),
                ]),
                Err(why) => Row::new(vec![
                    Cell::from(r.event.label()),
                    Cell::from("n/a").style(Style::new().fg(ERR)),
                    Cell::from(why.clone()).style(Style::new().fg(DIM)),
                ]),
            })
            .collect()
    };
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(18),
                Constraint::Length(12),
                Constraint::Min(10),
            ],
        )
        .header(Row::new(["COUNTER", "TOTAL", "RATE"]).style(header_style()))
        .block(panel("Counters (user-space only)")),
        table_a,
    );

    // ── derived metrics ──
    let ratio = |num: Event, den: Event| {
        Some(p.value(num)? as f64 / p.value(den)? as f64).filter(|x| x.is_finite())
    };
    let mut derived = Vec::new();
    match ratio(Event::Instructions, Event::Cycles) {
        Some(ipc) => derived.push(Line::from(vec![
            Span::raw("IPC (instructions/cycle): "),
            Span::styled(
                format!("{ipc:.2}"),
                Style::new()
                    .fg(if ipc < 1.0 { WARN } else { ACCENT })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if ipc < 1.0 {
                    "  ← likely stalled on memory"
                } else {
                    ""
                },
                Style::new().fg(DIM),
            ),
        ])),
        None => derived.push(Line::raw("IPC: n/a (needs cycles + instructions)")),
    }
    if let Some(m) = ratio(Event::CacheMisses, Event::CacheReferences) {
        derived.push(Line::raw(format!("cache miss ratio: {:.2}%", m * 100.0)));
    }
    if let Some(m) = ratio(Event::BranchMisses, Event::Branches) {
        derived.push(Line::raw(format!("branch miss ratio: {:.2}%", m * 100.0)));
    }
    f.render_widget(Paragraph::new(derived).block(panel("Derived")), derived_a);

    // ── wait-state profile ──
    let prof = p.profiler.snapshot();
    let total = prof.samples.max(1) as f64;
    let lines: Vec<Line> = prof
        .sorted()
        .into_iter()
        .take(right.height.saturating_sub(2) as usize)
        .map(|(label, n)| {
            let frac = n as f64 / total;
            Line::from(vec![
                Span::styled(format!("{:>5.1}% ", frac * 100.0), Style::new().fg(ACCENT)),
                Span::raw(format!("{} ", bar(frac, 12))),
                Span::raw(label.to_string()),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(panel(format!(
            "Wait-state profile ({} samples @100 Hz/thread)",
            prof.samples
        ))),
        right,
    );
}
