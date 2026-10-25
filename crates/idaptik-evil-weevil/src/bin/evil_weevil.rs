//! The Evil Weevil corpus runner.
//!
//! Runs the fixed adversarial corpus in-process (no sockets, no remote
//! systems), writes one redacted JSON report plus per-scenario replay
//! artifacts under `--out-dir`, prints a PASS/FAIL summary, and exits
//! non-zero if any scenario fails or crosses a bounded-test limit.
//!
//! Reproduction:
//!
//! ```sh
//! cargo run -p idaptik-evil-weevil -- --out-dir target/evil-weevil/run
//! ```

use clap::Parser;
use idaptik_evil_weevil::report::REPORT_SCHEMA;
use idaptik_evil_weevil::{CorpusReport, corpus};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "idaptik-evil-weevil",
    about = "Deterministic adversarial testing of IDApTIK multiplayer acceptance rules (ADR-0013)"
)]
struct Cli {
    /// Directory for the redacted corpus report and per-scenario replay
    /// artifacts. Created if missing.
    #[arg(long, default_value = "target/evil-weevil/latest")]
    out_dir: PathBuf,
    /// Run a single scenario by id instead of the whole corpus.
    #[arg(long)]
    scenario: Option<String>,
    /// Corpus seed. The default is fixed so runs are reproducible.
    #[arg(long, default_value_t = 777_001)]
    seed: u32,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let entries = corpus();

    let mut results = Vec::new();
    for scenario in &entries {
        let spec = scenario.spec();
        if let Some(filter) = &cli.scenario
            && filter != spec.id
        {
            continue;
        }
        eprintln!("== {} — {}", spec.id, spec.title);
        let result = scenario.run();
        for check in &result.checks {
            eprintln!(
                "   [{}] {} — {}",
                if check.passed { "PASS" } else { "FAIL" },
                check.name,
                check.detail
            );
        }
        eprintln!(
            "   {} (metrics: {:?})",
            if result.passed {
                "SCENARIO PASS"
            } else {
                "SCENARIO FAIL"
            },
            result.metrics
        );
        results.push(result);
    }

    if results.is_empty() {
        eprintln!(
            "no scenario matched {:?}; known ids:",
            cli.scenario.as_deref().unwrap_or("")
        );
        for scenario in &entries {
            eprintln!("  - {}", scenario.spec().id);
        }
        return ExitCode::from(2);
    }

    let report = CorpusReport::new(cli.seed, results);
    std::fs::create_dir_all(&cli.out_dir).unwrap_or_else(|e| {
        panic!("create {}: {e}", cli.out_dir.display());
    });

    let report_path = cli.out_dir.join("corpus-report.json");
    let report_json = serde_json::to_string_pretty(&report).expect("corpus report serialises");
    std::fs::write(&report_path, format!("{report_json}\n"))
        .unwrap_or_else(|e| panic!("write {}: {e}", report_path.display()));

    for result in &report.scenarios {
        let dir = cli.out_dir.join(&result.id);
        std::fs::create_dir_all(&dir).expect("scenario dir creates");
        let path = dir.join("result.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(result).expect("result serialises"),
        )
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    println!();
    println!("Evil Weevil corpus ({REPORT_SCHEMA})");
    println!("  run id:        {}", report.run_id);
    println!(
        "  scenarios:     {} passed, {} failed (of {})",
        report.totals.passed, report.totals.failed, report.totals.scenarios
    );
    println!(
        "  evidence:      {} protocol violations, {} transport faults",
        report.totals.protocol_violations, report.totals.transport_faults
    );
    println!(
        "  observer:      {} false positives, {} false negatives",
        report.totals.false_positives, report.totals.false_negatives
    );
    println!(
        "  limits:        peak queue depth {}, {} wire messages",
        report.totals.peak_queue_depth, report.totals.wire_messages
    );
    println!("  artifacts:     {}", cli.out_dir.display());

    if report.all_passed() {
        println!("PASS: every adversarial scenario held its host invariant");
        ExitCode::SUCCESS
    } else {
        println!("FAIL: one or more scenarios failed; inspect the artifacts above");
        ExitCode::from(1)
    }
}
