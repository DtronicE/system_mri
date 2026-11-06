use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Cell, Row, Table},
    Frame,
};

use crate::{
    procfs::fd::{describe_open_flags, FdKind},
    tui::app::Inspector,
    util::truncate_middle,
};

use super::common::*;

pub fn render(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    let Some(sample) = &ins.sample else { return };
    let rows: Vec<Row> = sample
        .snap
        .fds
        .iter()
        .map(|d| {
            let color = match d.kind {
                FdKind::Socket(_) => ACCENT,
                FdKind::Pipe(_) => WARN,
                FdKind::File | FdKind::Directory => ratatui::style::Color::Green,
                _ => ratatui::style::Color::Gray,
            };
            Row::new(vec![
                Cell::from(d.fd.to_string()),
                Cell::from(d.kind.label()).style(Style::new().fg(color)),
                Cell::from(d.pos.map(|p| p.to_string()).unwrap_or_default()),
                Cell::from(d.flags.map(describe_open_flags).unwrap_or_default()),
                Cell::from(truncate_middle(&d.target, 80)),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(28),
            Constraint::Min(10),
        ],
    )
    .header(Row::new(["FD", "TYPE", "OFFSET", "FLAGS", "TARGET"]).style(header_style()))
    .block(panel(format!(
        "Open file descriptors ({})",
        sample.snap.fds.len()
    )))
    .highlight_style(selected_style())
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut ins.files_state);
}
