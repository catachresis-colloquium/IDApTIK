//! The three Exchange-House-style JSON export surfaces: definition, runtime
//! snapshot, and after-action debrief.

use idaptik_core::RunConfig;
use idaptik_core::scenario::{GhostLobbySim, ghost_lobby};
use idaptik_tui::headless::{load, simulate};
use std::path::Path;

/// Which export surface to print.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ExportKind {
    Definition,
    Snapshot,
    Debrief,
}

/// Print the requested export surface. If `script` is given it is run first so
/// the snapshot/debrief reflect the finished run.
pub fn run(
    kind: ExportKind,
    script: Option<&Path>,
    cfg: RunConfig,
    seed: u32,
) -> Result<(), String> {
    let sim = match script {
        Some(p) => {
            let s = load(p)?;
            simulate(&s)?.0
        }
        None => {
            let mut sim = GhostLobbySim::new(ghost_lobby(), cfg, seed)
                .map_err(|e| format!("invalid scenario: {e:?}"))?;
            let _ = sim.drain_events();
            sim
        }
    };
    let json = match kind {
        ExportKind::Definition => serde_json::to_string_pretty(&sim.definition_export()),
        ExportKind::Snapshot => serde_json::to_string_pretty(&sim.snapshot()),
        ExportKind::Debrief => serde_json::to_string_pretty(&sim.debrief_export()),
    }
    .map_err(|e| format!("serialize: {e}"))?;
    println!("{json}");
    Ok(())
}
