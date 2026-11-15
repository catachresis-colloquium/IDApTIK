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
//!
//! `--emit-mind-trace PATH`, given with `--emit-epistemic`, runs the guard
//! through the ABI 1.1 mind (`ee_tick_mind`) and also writes one
//! `enaction-mind-trace/1` document, whose `caused_by` ids name events of the
//! runner output written beside it (see `idaptik_weevil::mind_trace`).

use idaptik_weevil::epistemic::runner_output;
use idaptik_weevil::mind_trace::documents;
use idaptik_weevil::profile::ProfileSet;
use idaptik_weevil::replay::{ReplayTrace, decision_identity, replay_equal};
use idaptik_weevil::{Result, WeevilError, run_corridor};
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
    let mut emit_mind_trace: Option<String> = None;

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
            "--emit-mind-trace" => {
                let Some(path) = args.get(i + 1).cloned() else {
                    eprintln!("--emit-mind-trace needs a path");
                    std::process::exit(2);
                };
                emit_mind_trace = Some(path);
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
            "unknown profile '{profile_id}'; known: cautious-sentry, aggressive-pursuer, equipment-specialist, decoy-lure, the-grudge, the-grudge-control, the-flinch, the-flinch-control, the-stalker, the-stalker-control, caught, caught-control, false-exposure, false-exposure-control"
        );
        std::process::exit(2);
    };

    // The mind trace cites events of the runner output of the SAME run, so the
    // two documents are only ever written together.
    if emit_mind_trace.is_some() && emit_epistemic.is_none() {
        eprintln!("--emit-mind-trace needs --emit-epistemic: both documents come from one run");
        std::process::exit(2);
    }
    let mind = emit_mind_trace.is_some();

    // A second run, same configuration: the reproducible half of the check.
    let run = run_corridor(ticks, profile, seed, mind)?;
    let again = run_corridor(ticks, profile, seed, mind)?;
    if let Err(m) = replay_equal(&run.trace, &again.trace) {
        eprintln!("NOT REPRODUCIBLE: {m}");
        std::process::exit(1);
    }
    if run.mind != again.mind {
        eprintln!("NOT REPRODUCIBLE: the mind records differ");
        std::process::exit(1);
    }
    let first = &run.trace;

    if let Some(path) = &check {
        let raw = fs::read_to_string(path).map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        let stored: ReplayTrace =
            serde_json::from_str(&raw).map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        match replay_equal(first, &stored) {
            Ok(()) => println!("replay matches {path}"),
            Err(m) => {
                eprintln!("replay does NOT match {path}: {m}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    print_summary(first);
    if let Some(path) = out {
        let json = serde_json::to_vec_pretty(first)
            .map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        fs::write(&path, json).map_err(|e| WeevilError::ReplayParse(e.to_string()))?;
        println!("wrote {path}");
    }
    let write = |path: &str, bytes: Vec<u8>| -> Result<()> {
        fs::write(path, bytes).map_err(|e| WeevilError::Epistemic(e.to_string()))?;
        println!("wrote {path}");
        Ok(())
    };
    match (emit_epistemic, emit_mind_trace) {
        (Some(epi), Some(mind_path)) => {
            let (runner, mind) = documents(&run)?;
            write(&epi, runner.to_canonical_bytes()?)?;
            write(&mind_path, mind.to_canonical_bytes()?)?;
        }
        (Some(epi), None) => write(
            &epi,
            runner_output(first, &run.truth)?.to_canonical_bytes()?,
        )?,
        _ => {}
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
