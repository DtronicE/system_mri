use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use crate::{tui::app::Picker, util::fmt_kb};

use super::common::*;

pub fn render(f: &mut Frame, area: Rect, p: &mut Picker) {
    let [body, footer] = Layout::vertical([Constraint::Min(5), Constraint::Length(1)]).areas(area);

    let rows: Vec<Row> = p
        .rows
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(r.p.pid.to_string()),
                Cell::from(r.p.uid.to_string()),
                Cell::from(r.p.state.to_string()).style(Style::new().fg(state_color(r.p.state))),
                Cell::from(r.p.threads.to_string()),
                Cell::from(format!("{:5.1}", r.cpu)),
                Cell::from(fmt_kb(r.p.rss_kb)),
                Cell::from(r.p.comm.clone()),
            ])
        })
        .collect();

    let filter = if p.editing_filter {
        format!("  filter: {}█", p.filter)
    } else if !p.filter.is_empty() {
        format!("  filter: {}", p.filter)
    } else {
        String::new()
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Min(10),
        ],
    )
    .header(Row::new(["PID", "UID", "ST", "THR", "CPU%", "RSS", "NAME"]).style(header_style()))
    .block(panel(format!(
        "System MRI · pick a process ({} shown, sorted by {}){filter}",
        p.rows.len(),
        p.sort.label()
    )))
    .highlight_style(selected_style())
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, body, &mut p.state);

    let mut hints = Vec::new();
    for (k, l) in [
        ("↑↓", "move"),
        ("Enter", "inspect"),
        ("s", "sort"),
        ("/", "filter"),
        ("q", "quit"),
    ] {
        hints.extend(key(k, l));
    }
    if let Some(s) = &p.status {
        hints.push(ratatui::text::Span::styled(
            format!(" {s}"),
            Style::new().fg(ERR).add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(hints)), footer);
}
