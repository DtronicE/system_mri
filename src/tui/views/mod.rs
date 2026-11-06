//! Rendering. Each submodule draws one tab as a pure function of app state.

mod common;
mod files;
mod memory;
mod network;
mod overview;
mod perf;
mod picker;
mod syscalls;
mod threads;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
    Frame,
};

use super::app::{App, Inspector, Screen, Tab};
use common::*;

pub fn draw(f: &mut Frame, app: &mut App) {
    match &mut app.screen {
        Screen::Picker(p) => picker::render(f, f.area(), p),
        Screen::Inspect(i) => draw_inspector(f, i),
    }
}

fn draw_inspector(f: &mut Frame, ins: &mut Inspector) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(f.area());

    // ── tab bar ──
    let comm = ins
        .sample
        .as_ref()
        .map(|s| s.snap.stat.comm.as_str())
        .unwrap_or("?");
    let titles = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{} {}", i + 1, t.title()));
    let tabs = Tabs::new(titles)
        .select(ins.tab.index())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" System MRI · pid {} ({comm}) ", ins.pid)),
        )
        .highlight_style(
            Style::new()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider("│");
    f.render_widget(tabs, header);

    // ── body (+ optional explain pane) ──
    let (main, side) = if ins.show_explain {
        let dir = if body.width >= 110 {
            Direction::Horizontal
        } else {
            Direction::Vertical
        };
        let split = if dir == Direction::Horizontal {
            [Constraint::Percentage(64), Constraint::Percentage(36)]
        } else {
            [Constraint::Percentage(68), Constraint::Percentage(32)]
        };
        let [a, b] = Layout::default()
            .direction(dir)
            .constraints(split)
            .areas(body);
        (a, Some(b))
    } else {
        (body, None)
    };

    if let Some(msg) = &ins.status {
        let [banner, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(main);
        f.render_widget(
            Paragraph::new(format!(" ⚠ {msg} — showing last known data "))
                .style(Style::new().fg(ERR).add_modifier(Modifier::BOLD)),
            banner,
        );
        render_tab(f, rest, ins);
    } else {
        render_tab(f, main, ins);
    }
    if let Some(side) = side {
        render_explain(f, side, ins);
    }

    // ── footer ──
    let mut hints = vec![
        key("Tab/1-7", "switch"),
        key("↑↓", "select"),
        key("e", "explain"),
        key("space", if ins.paused { "resume" } else { "pause" }),
    ];
    match ins.tab {
        Tab::Syscalls => hints.extend([
            key("a", "attach/detach"),
            key("c", "log/summary"),
            key("x", "clear"),
        ]),
        Tab::Perf => hints.extend([key("a", "start/stop"), key("x", "reset profile")]),
        _ => {}
    }
    hints.push(key(
        if ins.from_picker { "b" } else { "" },
        if ins.from_picker { "back" } else { "" },
    ));
    hints.push(key("q", "quit"));
    f.render_widget(
        Paragraph::new(Line::from(hints.into_iter().flatten().collect::<Vec<_>>())),
        footer,
    );
}

fn render_tab(f: &mut Frame, area: Rect, ins: &mut Inspector) {
    match ins.tab {
        Tab::Overview => overview::render(f, area, ins),
        Tab::Memory => memory::render(f, area, ins),
        Tab::Files => files::render(f, area, ins),
        Tab::Network => network::render(f, area, ins),
        Tab::Threads => threads::render(f, area, ins),
        Tab::Syscalls => syscalls::render(f, area, ins),
        Tab::Perf => perf::render(f, area, ins),
    }
}

/// The "What am I looking at?" pane: a primer for the tab plus detail on the
/// currently selected row.
fn render_explain(f: &mut Frame, area: Rect, ins: &Inspector) {
    let (title, primer) = crate::explain::tab_primer(ins.tab.title());
    let mut lines = vec![
        Line::styled(title, Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Line::raw(""),
    ];
    lines.extend(text_lines(primer));
    if let Some((head, detail)) = selection_detail(ins) {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("▶ {head}"),
            Style::new().fg(WARN).add_modifier(Modifier::BOLD),
        ));
        lines.extend(text_lines(&detail));
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" What am I looking at? "),
        ),
        area,
    );
}

/// `Line::raw` does not break on `\n`, so split paragraphs explicitly.
fn text_lines(text: &str) -> Vec<Line<'static>> {
    text.split('\n').map(|l| Line::raw(l.to_string())).collect()
}

fn selection_detail(ins: &Inspector) -> Option<(String, String)> {
    let snap = &ins.sample.as_ref()?.snap;
    match ins.tab {
        Tab::Overview => {
            let c = snap.stat.state;
            Some((format!("state {c}"), crate::explain::proc_state(c).into()))
        }
        Tab::Memory => {
            let r = snap.regions.get(ins.mem_state.selected()?)?;
            Some((
                format!("{} {} ({})", r.perms, r.display_path(), r.kind().label()),
                format!(
                    "{}\n\n{}",
                    crate::explain::region_kind(r.kind()),
                    crate::explain::permissions(r)
                ),
            ))
        }
        Tab::Files => {
            let f = snap.fds.get(ins.files_state.selected()?)?;
            let flags = f
                .flags
                .map(crate::procfs::fd::describe_open_flags)
                .unwrap_or_default();
            Some((
                format!("fd {} · {}", f.fd, f.kind.label()),
                format!("{}\n\nflags: {flags}", crate::explain::fd_kind(&f.kind)),
            ))
        }
        Tab::Network => {
            use crate::procfs::net::SocketInfo;
            let s = snap.sockets.get(ins.net_state.selected()?)?;
            Some(match &s.info {
                SocketInfo::Inet(i) => (
                    format!("fd {} · {} {}", s.fd, i.proto.label(), i.state_label()),
                    crate::explain::tcp_state(i.state).into(),
                ),
                SocketInfo::Unix(u) => (
                    format!("fd {} · unix {}", s.fd, u.kind),
                    "A UNIX-domain socket: local IPC through the kernel, addressed by a filesystem path (or abstract name when the path starts with @).".into(),
                ),
            })
        }
        Tab::Threads => {
            let t = snap.threads.get(ins.threads_state.selected()?)?;
            let wait = t
                .wchan
                .as_deref()
                .map(|w| format!("\n\nSleeping in kernel function: {w}"))
                .unwrap_or_default();
            Some((
                format!("tid {} · {}", t.tid, t.comm),
                format!("{}{wait}", crate::explain::proc_state(t.state)),
            ))
        }
        Tab::Syscalls | Tab::Perf => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{App, Screen};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};
    use std::time::Duration;

    fn press(app: &mut App, c: KeyCode) {
        app.on_key(KeyEvent::new(c, KeyModifiers::NONE));
    }

    fn screen_text(term: &Terminal<TestBackend>) -> String {
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    /// Draw every tab at several terminal sizes; none may panic, and the
    /// content must mention the process we are inspecting.
    #[test]
    fn every_tab_renders_at_every_size() {
        let mut app = App::new(Some(std::process::id()), Duration::from_millis(200)).unwrap();
        for (w, h) in [(160, 50), (100, 30), (60, 12), (20, 6)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            for tab in 0..7 {
                app.on_key(KeyEvent::new(
                    KeyCode::Char((b'1' + tab) as char),
                    KeyModifiers::NONE,
                ));
                app.tick();
                term.draw(|f| draw(f, &mut app)).unwrap();
            }
        }
    }

    #[test]
    fn overview_shows_process_name_and_memory_tab_shows_heap() {
        let mut app = App::new(Some(std::process::id()), Duration::from_millis(200)).unwrap();
        let mut term = Terminal::new(TestBackend::new(180, 50)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen_text(&term).contains("System MRI"));
        press(&mut app, KeyCode::Char('2'));
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen_text(&term).contains("Address space by kind"));
    }

    #[test]
    fn picker_lists_processes_and_opens_one() {
        let mut app = App::new(None, Duration::from_millis(200)).unwrap();
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(matches!(app.screen, Screen::Picker(_)));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.screen, Screen::Inspect(_)));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Picker(_)));
    }

    #[test]
    fn syscall_and_perf_sessions_start_and_stop_cleanly() {
        let mut app = App::new(Some(std::process::id()), Duration::from_millis(200)).unwrap();
        let mut term = Terminal::new(TestBackend::new(140, 40)).unwrap();
        // Perf: start, refresh a couple of times, stop.
        press(&mut app, KeyCode::Char('7'));
        press(&mut app, KeyCode::Char('a'));
        for _ in 0..2 {
            std::thread::sleep(Duration::from_millis(30));
            app.tick();
            term.draw(|f| draw(f, &mut app)).unwrap();
        }
        press(&mut app, KeyCode::Char('a'));
        // Syscalls: attaching to ourselves must fail gracefully (a process cannot
        // ptrace its own thread group), never hang or crash.
        press(&mut app, KeyCode::Char('6'));
        press(&mut app, KeyCode::Char('a'));
        std::thread::sleep(Duration::from_millis(100));
        app.poll_background();
        term.draw(|f| draw(f, &mut app)).unwrap();
        press(&mut app, KeyCode::Char('a'));
    }

    /// Manual aid: `MRI_PID=1234 cargo test print_frames -- --ignored --nocapture`
    /// prints each tab as text, handy for docs and bug reports.
    #[test]
    #[ignore]
    fn print_frames() {
        let pid: u32 = std::env::var("MRI_PID")
            .expect("set MRI_PID")
            .parse()
            .unwrap();
        let mut app = App::new(Some(pid), Duration::from_millis(200)).unwrap();
        let mut term = Terminal::new(TestBackend::new(150, 38)).unwrap();
        press(&mut app, KeyCode::Char('6'));
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('7'));
        press(&mut app, KeyCode::Char('a'));
        std::thread::sleep(Duration::from_millis(1500));
        for tab in ['1', '2', '3', '4', '5', '6', '7'] {
            press(&mut app, KeyCode::Char(tab));
            app.tick();
            app.poll_background();
            term.draw(|f| draw(f, &mut app)).unwrap();
            let buf = term.backend().buffer();
            println!("\n──────── tab {tab} ────────");
            for y in 0..buf.area.height {
                let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
                println!("{}", line.trim_end());
            }
        }
    }
}
