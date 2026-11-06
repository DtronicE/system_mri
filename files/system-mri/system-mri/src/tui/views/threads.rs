use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Cell, Row, Table},
    Frame,
};

use crate::{procfs::system::clk_tck, tui::app::Inspector};

use super::common::*;

pub fn render(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    let Some(sample) = &ins.sample else { return };
    let tck = clk_tck() as f64;
    let rows: Vec<Row> = sample
        .snap
        .threads
        .iter()
        .map(|t| {
            Row::new(vec![
                Cell::from(t.tid.to_string()),
                Cell::from(t.comm.clone()),
                Cell::from(t.state.to_string()).style(Style::new().fg(state_color(t.state))),
                Cell::from(format!("{:.2}s", t.cpu_ticks as f64 / tck)),
                Cell::from(t.processor.to_string()),
                Cell::from(
                    t.syscall
                        .as_ref()
                        .map(|s| s.label())
                        .unwrap_or_else(|| "(unreadable)".into()),
                ),
                Cell::from(t.wchan.clone().unwrap_or_default()),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(17),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(22),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new([
            "TID",
            "NAME",
            "ST",
            "CPU TIME",
            "CPU",
            "NOW IN SYSCALL",
            "WCHAN",
        ])
        .style(header_style()),
    )
    .block(panel(format!("Threads ({})", sample.snap.threads.len())))
    .highlight_style(selected_style())
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut ins.threads_state);
}
