use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

use crate::{
    procfs::maps::summarize,
    tui::app::Inspector,
    util::{fmt_bytes, fmt_kb, truncate_middle},
};

use super::common::*;

pub fn render(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    let Some(sample) = &ins.sample else { return };
    let regions = &sample.snap.regions;

    let summary: Vec<_> = summarize(regions)
        .into_iter()
        .filter(|k| k.count > 0)
        .collect();
    let [top, bottom] = Layout::vertical([
        Constraint::Length(summary.len() as u16 + 2),
        Constraint::Min(5),
    ])
    .areas(area);

    // ── layout summary: how the resident memory is distributed ──
    let total_rss: u64 = summary.iter().map(|k| k.rss_kb).sum::<u64>().max(1);
    let have_smaps = regions.iter().any(|r| r.rss_kb > 0);
    let lines: Vec<Line> = summary
        .iter()
        .map(|k| {
            let frac = k.rss_kb as f64 / total_rss as f64;
            Line::from(vec![
                Span::styled(
                    format!("{:<24}", k.kind.label()),
                    Style::new().fg(kind_color(k.kind)),
                ),
                Span::styled(bar(frac, 24), Style::new().fg(kind_color(k.kind))),
                Span::raw(format!(
                    " rss {:>10} ({:>4.1}%)  pss {:>10}  virt {:>10}  {} VMAs",
                    fmt_kb(k.rss_kb),
                    frac * 100.0,
                    fmt_kb(k.pss_kb),
                    fmt_bytes(k.size),
                    k.count
                )),
            ])
        })
        .collect();
    let title = if have_smaps {
        "Address space by kind (bars = share of resident memory)"
    } else {
        "Address space by kind (smaps unreadable: sizes only)"
    };
    f.render_widget(Paragraph::new(lines).block(panel(title)), top);

    // ── VMA table (pmap -X) ──
    let rows: Vec<Row> = regions
        .iter()
        .map(|r| {
            let color = kind_color(r.kind());
            Row::new(vec![
                Cell::from(format!("{:012x}-{:012x}", r.start, r.end)),
                Cell::from(r.perms.to_string()),
                Cell::from(fmt_bytes(r.size())),
                Cell::from(fmt_kb(r.rss_kb)),
                Cell::from(fmt_kb(r.pss_kb)),
                Cell::from(fmt_kb(r.dirty_kb)),
                Cell::from(truncate_middle(r.display_path(), 60)).style(Style::new().fg(color)),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(25),
            Constraint::Length(5),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(["ADDRESS", "PERM", "SIZE", "RSS", "PSS", "DIRTY", "MAPPING"])
            .style(header_style()),
    )
    .block(panel(format!("Virtual memory areas ({})", regions.len())))
    .highlight_style(selected_style())
    .highlight_symbol("▶ ");
    f.render_stateful_widget(table, bottom, &mut ins.mem_state);
}
