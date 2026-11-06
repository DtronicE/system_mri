//! Application state and input handling. Nothing here draws.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;

use crate::{
    perf::{CounterReading, Counters, Event, WaitProfiler},
    procfs::system::{self, ProcSummary},
    sampler::{Sample, Sampler},
    trace::{self, SyscallEvent, SyscallStats, TraceHandle, TraceMsg},
};

// ───────────────────────────── App ─────────────────────────────

pub enum Screen {
    Picker(Picker),
    Inspect(Box<Inspector>),
}

pub struct App {
    pub screen: Screen,
    pub interval: Duration,
    pub quit: bool,
}

impl App {
    pub fn new(pid: Option<u32>, interval: Duration) -> Result<Self> {
        let screen = match pid {
            Some(p) => {
                let mut ins =
                    Inspector::new(p).with_context(|| format!("cannot inspect pid {p}"))?;
                ins.from_picker = false;
                Screen::Inspect(Box::new(ins))
            }
            None => Screen::Picker(Picker::new()),
        };
        Ok(Self {
            screen,
            interval,
            quit: false,
        })
    }

    pub fn tick(&mut self) {
        match &mut self.screen {
            Screen::Picker(p) => p.refresh(),
            Screen::Inspect(i) => i.refresh(),
        }
    }

    pub fn poll_background(&mut self) {
        if let Screen::Inspect(i) = &mut self.screen {
            i.drain_trace();
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match &mut self.screen {
            Screen::Picker(p) => match p.on_key(key) {
                PickerAction::Quit => self.quit = true,
                PickerAction::Open(pid) => match Inspector::new(pid) {
                    Ok(ins) => self.screen = Screen::Inspect(Box::new(ins)),
                    Err(e) => p.status = Some(format!("cannot inspect {pid}: {e}")),
                },
                PickerAction::None => {}
            },
            Screen::Inspect(i) => match i.on_key(key) {
                InspectAction::Quit => self.quit = true,
                InspectAction::Back => self.screen = Screen::Picker(Picker::new()),
                InspectAction::None => {}
            },
        }
    }
}

fn move_selection(state: &mut TableState, len: usize, delta: isize) {
    if len == 0 {
        state.select(None);
        return;
    }
    let cur = state.selected().unwrap_or(0) as isize;
    state.select(Some((cur + delta).clamp(0, len as isize - 1) as usize));
}

// ──────────────────────────── Picker ────────────────────────────

pub enum PickerAction {
    None,
    Quit,
    Open(u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Cpu,
    Memory,
    Pid,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Cpu => "CPU%",
            SortKey::Memory => "RSS",
            SortKey::Pid => "PID",
        }
    }
    fn next(self) -> Self {
        match self {
            SortKey::Cpu => SortKey::Memory,
            SortKey::Memory => SortKey::Pid,
            SortKey::Pid => SortKey::Cpu,
        }
    }
}

pub struct PickerRow {
    pub p: ProcSummary,
    pub cpu: f64,
}

pub struct Picker {
    pub rows: Vec<PickerRow>,
    pub state: TableState,
    pub sort: SortKey,
    pub filter: String,
    pub editing_filter: bool,
    pub status: Option<String>,
    prev_ticks: HashMap<u32, u64>,
    prev_at: Option<Instant>,
}

impl Picker {
    pub fn new() -> Self {
        let mut p = Picker {
            rows: Vec::new(),
            state: TableState::default(),
            sort: SortKey::Cpu,
            filter: String::new(),
            editing_filter: false,
            status: None,
            prev_ticks: HashMap::new(),
            prev_at: None,
        };
        p.refresh();
        p.state.select(Some(0));
        p
    }

    pub fn refresh(&mut self) {
        let selected_pid = self
            .state
            .selected()
            .and_then(|i| self.rows.get(i))
            .map(|r| r.p.pid);
        let now = Instant::now();
        let dt = self
            .prev_at
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        let tck = system::clk_tck() as f64;
        let needle = self.filter.to_lowercase();

        let all = system::list_processes();
        let mut rows: Vec<PickerRow> = all
            .iter()
            .filter(|p| {
                needle.is_empty()
                    || p.comm.to_lowercase().contains(&needle)
                    || p.pid.to_string().contains(&needle)
            })
            .map(|p| {
                let cpu = match self.prev_ticks.get(&p.pid) {
                    Some(&prev) if dt > 0.0 => {
                        p.cpu_ticks.saturating_sub(prev) as f64 / tck / dt * 100.0
                    }
                    _ => 0.0,
                };
                PickerRow { p: p.clone(), cpu }
            })
            .collect();

        // Remember ticks for *all* processes (not just the filtered ones).
        self.prev_ticks = all.iter().map(|p| (p.pid, p.cpu_ticks)).collect();
        self.prev_at = Some(now);

        match self.sort {
            SortKey::Cpu => {
                rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.p.rss_kb.cmp(&a.p.rss_kb)))
            }
            SortKey::Memory => rows.sort_by_key(|r| std::cmp::Reverse(r.p.rss_kb)),
            SortKey::Pid => rows.sort_by_key(|r| r.p.pid),
        }
        self.rows = rows;

        // Keep the cursor on the same PID across re-sorts.
        let idx = selected_pid
            .and_then(|pid| self.rows.iter().position(|r| r.p.pid == pid))
            .or(if self.rows.is_empty() {
                None
            } else {
                Some(self.state.selected().unwrap_or(0).min(self.rows.len() - 1))
            });
        self.state.select(idx);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> PickerAction {
        if self.editing_filter {
            match key.code {
                KeyCode::Esc => {
                    self.filter.clear();
                    self.editing_filter = false;
                }
                KeyCode::Enter => self.editing_filter = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                }
                KeyCode::Char(c) => self.filter.push(c),
                _ => {}
            }
            self.refresh();
            return PickerAction::None;
        }
        let len = self.rows.len();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return PickerAction::Quit,
            KeyCode::Up | KeyCode::Char('k') => move_selection(&mut self.state, len, -1),
            KeyCode::Down | KeyCode::Char('j') => move_selection(&mut self.state, len, 1),
            KeyCode::PageUp => move_selection(&mut self.state, len, -15),
            KeyCode::PageDown => move_selection(&mut self.state, len, 15),
            KeyCode::Home => move_selection(&mut self.state, len, isize::MIN / 2),
            KeyCode::End => move_selection(&mut self.state, len, isize::MAX / 2),
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.refresh();
            }
            KeyCode::Char('/') => self.editing_filter = true,
            KeyCode::Enter => {
                if let Some(r) = self.state.selected().and_then(|i| self.rows.get(i)) {
                    return PickerAction::Open(r.p.pid);
                }
            }
            _ => {}
        }
        PickerAction::None
    }
}

// ─────────────────────────── Inspector ───────────────────────────

pub enum InspectAction {
    None,
    Quit,
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Memory,
    Files,
    Network,
    Threads,
    Syscalls,
    Perf,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Overview,
        Tab::Memory,
        Tab::Files,
        Tab::Network,
        Tab::Threads,
        Tab::Syscalls,
        Tab::Perf,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Memory => "Memory",
            Tab::Files => "Files",
            Tab::Network => "Network",
            Tab::Threads => "Threads",
            Tab::Syscalls => "Syscalls",
            Tab::Perf => "Perf",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|&t| t == self).unwrap()
    }

    fn step(self, delta: isize) -> Tab {
        let n = Tab::ALL.len() as isize;
        Tab::ALL[(self.index() as isize + delta).rem_euclid(n) as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TraceView {
    Log,
    Summary,
}

const LOG_CAP: usize = 5_000;

pub struct TraceSession {
    handle: TraceHandle,
    pub log: VecDeque<SyscallEvent>,
    pub notes: VecDeque<String>,
    pub stats: SyscallStats,
    pub attached_threads: Option<usize>,
    pub error: Option<String>,
    pub ended: bool,
}

impl TraceSession {
    pub fn dropped(&self) -> u64 {
        self.handle.dropped()
    }
}

pub struct PerfSession {
    counters: Counters,
    pub profiler: WaitProfiler,
    pub readings: Vec<CounterReading>,
    /// Per-second rate of each counter between the last two refreshes.
    pub rates: Vec<Option<f64>>,
    prev: Option<(Instant, Vec<Option<u64>>)>,
}

impl PerfSession {
    pub fn value(&self, ev: Event) -> Option<u64> {
        self.readings
            .iter()
            .find(|r| r.event == ev)
            .and_then(|r| r.value.as_ref().ok().copied())
    }
}

pub struct Inspector {
    pub pid: u32,
    pub from_picker: bool,
    pub tab: Tab,
    pub show_explain: bool,
    pub paused: bool,
    pub status: Option<String>,
    pub sampler: Sampler,
    pub sample: Option<Sample>,
    pub mem_state: TableState,
    pub files_state: TableState,
    pub net_state: TableState,
    pub threads_state: TableState,
    pub summary_state: TableState,
    pub trace: Option<TraceSession>,
    pub trace_view: TraceView,
    /// How many lines the log is scrolled back from the tail (0 = follow).
    pub trace_scroll: usize,
    pub perf: Option<PerfSession>,
}

impl Inspector {
    pub fn new(pid: u32) -> Result<Self> {
        let mut sampler = Sampler::new(pid);
        let first = sampler.sample()?;
        let sel = || {
            let mut s = TableState::default();
            s.select(Some(0));
            s
        };
        Ok(Self {
            pid,
            from_picker: true,
            tab: Tab::Overview,
            show_explain: true,
            paused: false,
            status: None,
            sampler,
            sample: Some(first),
            mem_state: sel(),
            files_state: sel(),
            net_state: sel(),
            threads_state: sel(),
            summary_state: sel(),
            trace: None,
            trace_view: TraceView::Log,
            trace_scroll: 0,
            perf: None,
        })
    }

    /// Called once per refresh interval.
    pub fn refresh(&mut self) {
        if self.paused || self.status.is_some() {
            return;
        }
        match self.sampler.sample() {
            Ok(s) => self.sample = Some(s),
            Err(e) if e.is_not_found() => {
                self.status = Some(format!("process {} has exited", self.pid))
            }
            Err(e) => self.status = Some(format!("sampling failed: {e}")),
        }
        if let Some(p) = &mut self.perf {
            let readings = p.counters.read();
            let now = Instant::now();
            let values: Vec<Option<u64>> = readings
                .iter()
                .map(|r| r.value.as_ref().ok().copied())
                .collect();
            p.rates = match &p.prev {
                Some((t0, prev)) => {
                    let dt = now.duration_since(*t0).as_secs_f64().max(1e-6);
                    values
                        .iter()
                        .zip(prev)
                        .map(|(n, o)| Some(n.as_ref()?.saturating_sub((*o)?) as f64 / dt))
                        .collect()
                }
                None => vec![None; values.len()],
            };
            p.prev = Some((now, values));
            p.readings = readings;
        }
    }

    pub fn drain_trace(&mut self) {
        let Some(t) = &mut self.trace else { return };
        for _ in 0..10_000 {
            match t.handle.rx.try_recv() {
                Ok(TraceMsg::Attached { threads }) => t.attached_threads = Some(threads),
                Ok(TraceMsg::Syscall(ev)) => {
                    t.stats.record(&ev);
                    if t.log.len() == LOG_CAP {
                        t.log.pop_front();
                    }
                    t.log.push_back(ev);
                }
                Ok(TraceMsg::Signal { tid, name }) => {
                    push_note(&mut t.notes, format!("[{tid}] signal {name}"))
                }
                Ok(TraceMsg::ThreadExited { tid, detail }) => {
                    push_note(&mut t.notes, format!("[{tid}] {detail}"))
                }
                Ok(TraceMsg::Error(e)) => t.error = Some(e),
                Ok(TraceMsg::Ended) => t.ended = true,
                Err(_) => break,
            }
        }
    }

    fn toggle_trace(&mut self) {
        if self.trace.take().is_some() {
            return; // Drop detaches the tracer and resumes the process.
        }
        self.trace = Some(TraceSession {
            handle: trace::spawn(self.pid),
            log: VecDeque::new(),
            notes: VecDeque::new(),
            stats: SyscallStats::default(),
            attached_threads: None,
            error: None,
            ended: false,
        });
        self.trace_scroll = 0;
    }

    fn toggle_perf(&mut self) {
        if self.perf.take().is_some() {
            return;
        }
        self.perf = Some(PerfSession {
            counters: Counters::new(self.pid, &Event::ALL),
            profiler: WaitProfiler::start(self.pid, Duration::from_millis(10)),
            readings: Vec::new(),
            rates: Vec::new(),
            prev: None,
        });
    }

    fn current_table(&mut self) -> Option<(&mut TableState, usize)> {
        let snap = &self.sample.as_ref()?.snap;
        match self.tab {
            Tab::Memory => Some((&mut self.mem_state, snap.regions.len())),
            Tab::Files => Some((&mut self.files_state, snap.fds.len())),
            Tab::Network => Some((&mut self.net_state, snap.sockets.len())),
            Tab::Threads => Some((&mut self.threads_state, snap.threads.len())),
            Tab::Syscalls if self.trace_view == TraceView::Summary => {
                let n = self
                    .trace
                    .as_ref()
                    .map(|t| t.stats.sorted().len())
                    .unwrap_or(0);
                Some((&mut self.summary_state, n))
            }
            _ => None,
        }
    }

    fn scroll(&mut self, delta: isize) {
        if self.tab == Tab::Syscalls && self.trace_view == TraceView::Log {
            // Up = older lines.
            self.trace_scroll = (self.trace_scroll as isize - delta).max(0) as usize;
        } else if let Some((state, len)) = self.current_table() {
            move_selection(state, len, delta);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> InspectAction {
        match key.code {
            KeyCode::Char('q') => return InspectAction::Quit,
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('b') if self.from_picker => {
                return InspectAction::Back
            }
            KeyCode::Esc => return InspectAction::Quit,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.tab = self.tab.step(1),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.tab = self.tab.step(-1),
            KeyCode::Char(c @ '1'..='7') => self.tab = Tab::ALL[c as usize - '1' as usize],
            KeyCode::Up | KeyCode::Char('k') => self.scroll(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll(1),
            KeyCode::PageUp => self.scroll(-10),
            KeyCode::PageDown => self.scroll(10),
            KeyCode::Home => self.scroll(isize::MIN / 2),
            KeyCode::End => self.scroll(isize::MAX / 2),
            KeyCode::Char('e') => self.show_explain = !self.show_explain,
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('a') => match self.tab {
                Tab::Syscalls => self.toggle_trace(),
                Tab::Perf => self.toggle_perf(),
                _ => {}
            },
            KeyCode::Char('c') if self.tab == Tab::Syscalls => {
                self.trace_view = match self.trace_view {
                    TraceView::Log => TraceView::Summary,
                    TraceView::Summary => TraceView::Log,
                };
            }
            KeyCode::Char('x') => match self.tab {
                Tab::Syscalls => {
                    if let Some(t) = &mut self.trace {
                        t.log.clear();
                        t.stats.clear();
                        self.trace_scroll = 0;
                    }
                }
                Tab::Perf => {
                    if let Some(p) = &self.perf {
                        p.profiler.reset();
                    }
                }
                _ => {}
            },
            _ => {}
        }
        InspectAction::None
    }
}

fn push_note(notes: &mut VecDeque<String>, s: String) {
    if notes.len() == 50 {
        notes.pop_front();
    }
    notes.push_back(s);
}
