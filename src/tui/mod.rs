//! Terminal UI built on `ratatui` + `crossterm`.
//!
//! * [`app`] — state machine and input handling (no drawing).
//! * [`views`] — one drawing module per tab; pure functions of the state.

mod app;
mod views;

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use app::App;

/// Launch the TUI. With `pid = None` a process picker is shown first.
pub fn run(pid: Option<u32>, interval: Duration) -> Result<()> {
    // Build state *before* touching the terminal so startup errors print normally.
    let mut app = App::new(pid, interval)?;
    let mut terminal = ratatui::init(); // raw mode + alt screen + panic hook
    let result = event_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    let mut last_tick = Instant::now();
    while !app.quit {
        terminal.draw(|frame| views::draw(frame, app))?;
        if event::poll(Duration::from_millis(40))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }
        }
        app.poll_background(); // drain tracer events without waiting for a tick
        if last_tick.elapsed() >= app.interval {
            app.tick();
            last_tick = Instant::now();
        }
    }
    Ok(())
}
