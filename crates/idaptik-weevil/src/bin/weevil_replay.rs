//! Headless replay driver for the Evil Weevil adapter.
//!
//! Runs a bounded trace of the kernel driving a deterministic corridor host,
//! prints a summary, and can write or check a JSON replay. This is the
//! reproducible, headless half of the first milestone: no window, no game, no
//! server — just the kernel, the adapter and a recorded decision sequence.
//!
//! ```text
//! idaptik-weevil-replay --ticks 200 --profile cautious-sentry --seed 7
//! idaptik-weevil-replay --ticks 200 --profile aggressive-pursuer --seed 7 --out /tmp/a.json
//! idaptik-weevil-replay --ticks 200 --profile aggressive-pursuer --seed 7 --check /tmp/a.json
//! idaptik-weevil-replay --ticks 200 --profile decoy-lure --seed 7 --emit-epistemic /tmp/e.json
//! ```
//!
//! `--emit-epistemic PATH` writes the guard's beliefs as one
//! `enaction-runner-output/1` document in canonical bytes (see
//! `idaptik_weevil::epistemic`). It is written only after the two runs below
//! agree, which is what its `esm/replay-deterministic` guarantee claims.

use idaptik_weevil::epistemic::runner_output;
use idaptik_weevil::profile::ProfileSet;
use idaptik_weevil::replay::{ReplayTrace, decision_identity, replay_equal};
use idaptik_weevil::{Result, WeevilError, run_trace, run_trace_with_truth};
use std::collections::BTreeMap;
use std::fs;

fn main() {
    if let Err(e) = run() {
        eprintln!("weevil-replay: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut ticks: u64 = 200;
    let mut profile_id = "cautious-sentry".to_owned();
    let mut seed: u64 = 7;
    let mut out: Option<String> = None;
    let mut check: Option<String> = None;
    let mut emit_epistemic: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--ticks" => {
                ticks = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(ticks);
                i += 2;
            }
            "--profile" => {
                profile_id = args
                    .get(i + 1)
                    .cloned()
                    .unwrap_or_else(|| profile_id.clone());
                i += 2;
            }
            "--seed" => {
                seed = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(seed);
                i += 2;
            }
            "--out" => {
                out = args.get(i + 1).cloned();
                i += 2;
            }
            "--check" => {
                check = args.get(i + 1).cloned();
                i += 2;
            }
            "--emit-epistemic" => {
                let Some(path) = args.get(i + 1).cloned() else {
                    eprintln!("--emit-epistemic needs a path");
                    std::process::exit(2);
                };
                emit_epistemic = Some(path);
                i += 2;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    let set = ProfileSet::canonical();
    let Some(profile) = set.get(&profile_id) else {
        eprintln!(
            "unknown profile '{profile_id}'; known: cautious-sentry, aggressive-pursuer, equipment-specialist, decoy-lure"
        );
        std::process::exit(2);
    };

    // A second run, same configuration: the reproducible half of the check.
    let (first, truth) = run_trace_with_truth(ticks, profile, seed)?;
    let second = run_trace(ticks, profile, seed)?;
    if let Err(m) = replay_equal(&first, &second) {
        eprintln!("NOT REPRODUCIBLE: {m}");
        std::process::exit(1);
    }

    if let Some(path) = &check {
        let raw = fs::read_to_string(path).map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        let stored: ReplayTrace =
            serde_json::from_str(&raw).map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        match replay_equal(&first, &stored) {
            Ok(()) => println!("replay matches {path}"),
            Err(m) => {
                eprintln!("replay does NOT match {path}: {m}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    print_summary(&first);
    if let Some(path) = out {
        let json = serde_json::to_vec_pretty(&first)
            .map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        fs::write(&path, json).map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        println!("wrote {path}");
    }
    if let Some(path) = emit_epistemic {
        let bytes = runner_output(&first, &truth)?.to_canonical_bytes()?;
        fs::write(&path, bytes).map_err(|e| WeevilError::Epistemic(e.to_string()))?;
        println!("wrote {path}");
    }
    Ok(())
}

fn print_summary(t: &ReplayTrace) {
    let mut actions: BTreeMap<String, u64> = BTreeMap::new();
    let mut from_memory = 0u64;
    let mut degraded = 0u64;
    let mut investigate = 0u64;
    let mut blocked = 0u64;
    let mut rejected = 0u64;
    for e in &t.entries {
        *actions.entry(e.intent.action.clone()).or_insert(0) += 1;
        if e.intent.from_memory {
            from_memory += 1;
        }
        if e.intent.degraded {
            degraded += 1;
        }
        if e.intent.action == "INVESTIGATE" {
            investigate += 1;
        }
        if e.applied.blocked {
            blocked += 1;
        }
        if !e.accepted {
            rejected += 1;
        }
    }
    println!(
        "kernel      : {}.{}.{}",
        t.header.kernel_version.0, t.header.kernel_version.1, t.header.kernel_version.2
    );
    println!(
        "abi         : major {} fingerprint {}",
        t.header.abi_major, t.header.abi_fingerprint
    );
    println!(
        "profile     : {} v{}",
        t.header.profile_id, t.header.profile_version
    );
    println!("scenario    : {}", t.header.scenario_id);
    println!("ticks       : {}", t.len());
    println!("digest      : {}", decision_identity(t));
    println!("actions     : {:?}", actions);
    println!("from_memory : {from_memory}");
    println!("investigate : {investigate}");
    println!("degraded    : {degraded}");
    println!("blocked     : {blocked}");
    println!("rejected    : {rejected}");
}
