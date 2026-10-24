//! Headless runner: expand a script, simulate to the end, print one JSON blob
//! (`event_log` + `debrief` + `final_snapshot`) to stdout. No TTY required.

use crate::config;
use crate::script::{ScriptFile, expand};
use idaptik_core::Debrief;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::signal::ghost_lobby_wiring::ghost_lobby_wired;
use idaptik_core::scenario::{
    GhostLobbySim, RuntimeSnapshot, ScenarioDefinition, ghost_lobby, ghost_lobby_rooftop,
    pump_house,
};
use serde::Serialize;
use std::path::Path;

/// The wire format tag of [`HeadlessOutput`].
pub const HEADLESS_FORMAT: &str = "idaptik-ghost-lobby-headless-v1";

/// The headless stdout payload — the determinism artifact.
///
/// Public (with public fields) so a networked seat (`idaptik-net`) can emit the
/// *same* blob: the loopback gate compares seat outputs to each other and to
/// this runner's output byte-for-byte.
#[derive(Serialize)]
pub struct HeadlessOutput {
    pub format: &'static str,
    pub event_log: Vec<Event>,
    pub debrief: Option<Debrief>,
    pub final_snapshot: RuntimeSnapshot,
}

/// The floor a script names. Absent (or `"ghost-lobby"`) is the canonical
/// floor, so every existing script — and every committed fixture — runs
/// exactly what it always ran; the other names reach the wired, rooftop and
/// challenge-set floors. An unknown name is an error, never a silent
/// fallback: a script that asks for a floor must get that floor.
pub fn scenario_for(script: &ScriptFile) -> Result<ScenarioDefinition, String> {
    match script.scenario.as_deref() {
        None | Some("ghost-lobby") => Ok(ghost_lobby()),
        Some("ghost-lobby-wired") => Ok(ghost_lobby_wired()),
        Some("ghost-lobby-rooftop") => Ok(ghost_lobby_rooftop()),
        Some("pump-house") => Ok(pump_house()),
        Some(other) => Err(format!(
            "unknown scenario {other:?} (one of: ghost-lobby, ghost-lobby-wired, ghost-lobby-rooftop, pump-house)"
        )),
    }
}

/// Build a sim from a script's config and seed.
pub fn build(script: &ScriptFile) -> Result<GhostLobbySim, String> {
    let diff = config::parse_difficulty(&script.difficulty)?;
    let cfg = config::run_config(diff, script.reduced_motion, script.supervised);
    GhostLobbySim::new(scenario_for(script)?, cfg, script.seed)
        .map_err(|e| format!("invalid scenario: {e:?}"))
}

/// Simulate a script to its end (or `max_ticks`), returning the sim and the full
/// event log (including the startup events).
pub fn simulate(script: &ScriptFile) -> Result<(GhostLobbySim, Vec<Event>), String> {
    let mut sim = build(script)?;
    let mut log = sim.drain_events();
    for input in expand(script) {
        if sim.is_ended() {
            break;
        }
        log.extend(sim.tick(&input));
    }
    Ok((sim, log))
}

/// Load, run and print a headless script.
pub fn run(path: &Path) -> Result<(), String> {
    let script = load(path)?;
    let (sim, log) = simulate(&script)?;
    let out = HeadlessOutput {
        format: HEADLESS_FORMAT,
        debrief: sim.debrief().cloned(),
        final_snapshot: sim.snapshot(),
        event_log: log,
    };
    let json = serde_json::to_string_pretty(&out).map_err(|e| format!("serialize: {e}"))?;
    println!("{json}");
    Ok(())
}

/// Read and parse a script file.
pub fn load(path: &Path) -> Result<ScriptFile, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parse script {}: {e}", path.display()))
}
