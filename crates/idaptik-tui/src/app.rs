//! The interactive terminal loop: a real-time accumulator that steps the sim at
//! a fixed 60 Hz regardless of render rate, with clean terminal teardown.

use crossterm::event::{
    self, Event as CtEvent, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use idaptik_core::RunConfig;
use idaptik_core::scenario::event::{Event as SimEvent, LogLine};
use idaptik_core::scenario::{GhostLobbySim, TICK_DT, ghost_lobby, log_view};
use idaptik_tui::input::InputState;
use idaptik_tui::keymap::map_key;
use idaptik_tui::render;
use idaptik_tui::tutorial;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::time::{Duration, Instant};

/// Run the interactive TUI to completion (until the user quits).
///
/// The interactive run is always supervised (the WORKPLAN handoff): the sim
/// itself folds its event stream into the supervision state and applies the
/// team allocation to world coverage, so there is no caller-side supervisor.
pub fn run(cfg: RunConfig, seed: u32) -> io::Result<()> {
    let cfg = RunConfig {
        supervised: true,
        ..cfg
    };
    let mut sim = GhostLobbySim::new(ghost_lobby(), cfg, seed)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{e:?}")))?;
    let mut log: Vec<LogLine> = Vec::new();
    let mut tutor = tutorial::Tutor::new();
    // The brief comes before the first event: a newcomer should meet the
    // reason for the trip before the room's own chatter.
    log.extend(tutorial::brief(sim.definition(), 0, 0.0));
    let startup = sim.drain_events();
    ingest(&startup, &sim, &mut log, &mut tutor);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        );
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut sim, &mut log, &mut tutor, enhanced);

    if enhanced {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    sim: &mut GhostLobbySim,
    log: &mut Vec<LogLine>,
    tutor: &mut tutorial::Tutor,
    enhanced: bool,
) -> io::Result<()> {
    let mut input = InputState::with_keyboard_enhancement(enhanced);
    let mut hint: Option<String> = None;
    let mut acc = 0.0f64;
    let mut last = Instant::now();

    loop {
        terminal.draw(|f| render::draw(f, sim, log, hint.as_deref()))?;

        if event::poll(Duration::from_millis(16))?
            && let CtEvent::Key(key) = event::read()?
        {
            input.apply(map_key(key));
        }
        if input.quit {
            return Ok(());
        }
        if input.hint {
            hint = Some(contextual_hint(sim));
            input.hint = false;
        }

        let now = Instant::now();
        let dt = (now - last).as_secs_f64().min(0.25);
        last = now;
        acc += dt;
        while acc >= TICK_DT {
            let ti = input.sample();
            let events = sim.tick(&ti);
            if events
                .iter()
                .any(|e| matches!(e, SimEvent::Restarted { .. }))
            {
                log.clear();
                // A restart wipes the log, so it must re-teach the mission or
                // the retry loop loses the framing that made the first attempt
                // legible.
                log.extend(tutorial::brief(
                    sim.definition(),
                    sim.current_tick(),
                    sim.time(),
                ));
            }
            ingest(&events, sim, log, tutor);
            acc -= TICK_DT;
        }
    }
}

/// Convert freshly-emitted events into rendered log lines (a pure view).
fn ingest(
    events: &[SimEvent],
    sim: &GhostLobbySim,
    log: &mut Vec<LogLine>,
    tutor: &mut tutorial::Tutor,
) {
    let tick = sim.current_tick();
    let t = sim.time();
    for e in events {
        if let Some(line) = log_view(e, tick, t) {
            log.push(line);
        }
    }
    tutor.observe(events, tick, t, log);
    if log.len() > 500 {
        let drop = log.len() - 500;
        log.drain(0..drop);
    }
}

fn contextual_hint(sim: &GhostLobbySim) -> String {
    let s = sim.state();
    if !s.player.has_note {
        "The real lead is the boring kitchen note — hold E near it.".to_owned()
    } else if s.support < 0.4 {
        "Support is fraying — hide or ping cameras (1) before isolation fills.".to_owned()
    } else {
        "Reach the service exit (right edge) or reveal the laundry chute.".to_owned()
    }
}
