//! Shared styling and tiny widgets.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders},
};

use crate::procfs::maps::RegionKind;

pub const ACCENT: Color = Color::Cyan;
pub const WARN: Color = Color::Yellow;
pub const ERR: Color = Color::Red;
pub const DIM: Color = Color::DarkGray;

pub fn panel(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title.into()))
}

pub fn header_style() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::new()
        .bg(Color::Rgb(40, 50, 70))
        .add_modifier(Modifier::BOLD)
}

/// `[key] label` footer hint (empty key → nothing).
pub fn key(k: &str, label: &str) -> Vec<Span<'static>> {
    if k.is_empty() {
        return vec![];
    }
    vec![
        Span::styled(format!(" {k} "), Style::new().fg(Color::Black).bg(DIM)),
        Span::raw(format!(" {label}  ")),
    ]
}

/// A horizontal bar `████░░░░` of `width` cells filled to `frac` ∈ [0, 1].
pub fn bar(frac: f64, width: usize) -> String {
    let filled = ((frac.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

pub fn kind_color(kind: RegionKind) -> Color {
    match kind {
        RegionKind::Code => Color::Green,
        RegionKind::FileData => Color::LightGreen,
        RegionKind::Heap => Color::Yellow,
        RegionKind::Stack => Color::Magenta,
        RegionKind::Anonymous => Color::Blue,
        RegionKind::SharedMemory => Color::Cyan,
        RegionKind::Vdso => Color::Gray,
        RegionKind::Other => Color::DarkGray,
    }
}

pub fn state_color(state: char) -> Color {
    match state {
        'R' => Color::Green,
        'D' => Color::Red,
        'Z' => Color::Magenta,
        'T' | 't' => Color::Yellow,
        _ => Color::Gray,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_fills_proportionally() {
        assert_eq!(bar(0.5, 4), "██░░");
        assert_eq!(bar(0.0, 3), "░░░");
        assert_eq!(bar(2.0, 3), "███");
    }
}
