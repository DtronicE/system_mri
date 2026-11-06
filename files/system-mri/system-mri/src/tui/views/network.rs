use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Style},
    widgets::{Cell, Row, Table},
    Frame,
};

use crate::{
    procfs::net::{SocketInfo, TcpState},
    tui::app::Inspector,
};

use super::common::*;

fn state_style(s: TcpState) -> Style {
    Style::new().fg(match s {
        TcpState::Established => Color::Green,
        TcpState::Listen => ACCENT,
        TcpState::CloseWait | TcpState::LastAck => WARN,
        TcpState::TimeWait => Color::Gray,
        _ => Color::White,
    })
}

pub fn render(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    let Some(sample) = &ins.sample else { return };
    let socks = &sample.snap.sockets;
    let rows: Vec<Row> = socks
        .iter()
        .map(|s| match &s.info {
            SocketInfo::Inet(i) => Row::new(vec![
                Cell::from(s.fd.to_string()),
                Cell::from(i.proto.label()),
                Cell::from(i.local.to_string()),
                Cell::from(i.remote.to_string()),
                Cell::from(i.state_label()).style(state_style(i.state)),
                Cell::from(format!("{} / {}", i.tx_queue, i.rx_queue)),
            ]),
            SocketInfo::Unix(u) => Row::new(vec![
                Cell::from(s.fd.to_string()),
                Cell::from("unix"),
                Cell::from(u.path.clone().unwrap_or_else(|| "(unnamed)".into())),
                Cell::from("-"),
                Cell::from(format!("{} {}", u.kind, u.state)),
                Cell::from(format!("refs {}", u.ref_count)),
            ]),
        })
        .collect();
    let inet = socks
        .iter()
        .filter(|s| matches!(s.info, SocketInfo::Inet(_)))
        .count();
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Min(24),
            Constraint::Min(24),
            Constraint::Length(20),
            Constraint::Length(14),
        ],
    )
    .header(
        Row::new(["FD", "PROTO", "LOCAL", "REMOTE", "STATE", "TX / RX Q"]).style(header_style()),
    )
    .block(panel(format!(
        "Sockets ({} inet, {} unix)",
        inet,
        socks.len() - inet
    )))
    .highlight_style(selected_style())
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut ins.net_state);
}
