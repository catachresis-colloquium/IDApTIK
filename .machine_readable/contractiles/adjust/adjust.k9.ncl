K9!
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 IDApTIK contributors
{
  pedigree = {
    schema_version = "1.0.0",
    component_type = "contractile:adjust",
    contractile_verb = "adjust",
    semantics = "Advisory drift review with explicit tolerance bands and corrective actions.",
    tier = 'Kennel,
    authority = 'advisory,
    security = {
      leash = 'Kennel,
      trust_level = "declaration-only",
      allow_network = false,
      allow_filesystem_write = false,
      allow_subprocess = false,
      capabilities = [],
    },
    metadata = {
      name = "idaptik-adjust",
      version = "1.0.0",
      description = "Advisory architecture, documentation, licensing, and toolchain drift clauses.",
      author = "IDApTIK contributors",
      paired_runner = "adjust.ncl",
    },
    side_effects = [],
  },
  declaration = {
    schema_version = "1.0.0",
    verb = "adjust",
    authority = 'advisory,
    clauses = [
      { id = "core-engine-isolation", severity = "advisory", description = "The gameplay core remains independent of rendering engines.", tolerance = "Zero Bevy or Fyrox references in idaptik-core dependencies.", corrective = "Move rendering-coupled code into frontend crates; see ADR-0003 and ADR-0008.", evidence = "Cargo manifests, docs/adr/0003-engine-strategy.md, and docs/adr/0008-select-bevy-and-retire-fyrox.md" },
      { id = "determinism-drift", severity = "advisory", description = "Gameplay truth remains deterministic for replay and export.", tolerance = "No unseeded RNG or wall-clock reads in simulation paths; preserve deterministic iteration.", corrective = "Route randomness through seeded scenario RNG and add replay coverage.", evidence = "ADR-0001, ADR-0004, and core tests" },
      { id = "state-freshness", severity = "advisory", description = "The canonical deed tracks material project phase and component-status changes.", tolerance = "Refresh descriptive state when implementation or CI evidence materially changes.", corrective = "Update IDApTIK_chora.deed from README, source, ADRs, and workflow evidence.", evidence = "IDApTIK_chora.deed" },
      { id = "adr-coverage", severity = "advisory", description = "Settled architectural decisions are recorded as ADRs.", tolerance = "Every settled open decision is backed by an accepted ADR; unresolved items remain marked open.", corrective = "Write or update the relevant record under docs/adr/.", evidence = "README roadmap and deed decision-register" },
      { id = "license-layer-consistency", severity = "advisory", description = "Code, content, and governance license layers remain correctly separated.", tolerance = "No source header crosses the engine/content license boundary.", corrective = "Restore the SPDX identifier for the file's actual layer; request approval for license changes.", evidence = "LICENSE, LICENSES/, and source headers" },
      { id = "spdx-header-consistency", severity = "advisory", description = "Committed source and machine-readable files carry the correct SPDX header.", tolerance = "Zero source files are missing their layer-appropriate SPDX-License-Identifier.", corrective = "Add the matching SPDX header and re-run the format checks.", evidence = "source tree and conformance checks" },
      { id = "pin-sync", severity = "advisory", description = "mise and Rust toolchain pins stay aligned with CI.", tolerance = "CI tool versions match mise.toml and rust-toolchain.toml.", corrective = "Reconcile workflow setup with the committed pins.", evidence = "mise.toml, rust-toolchain.toml, and CI workflows" },
      { id = "deployment-evidence-honesty", severity = "critical", description = "Do not promote contract-only deployment modes or GSA integration to implemented status without runtime and test evidence.", tolerance = "Deed status agrees with source and CI evidence.", corrective = "Keep peer transport, hosted authority, federation routing, and GSA adapter marked unimplemented until proven.", evidence = "ADR-0015, crates/idaptik-net/src/deployment.rs, and deployment tests" },
    ],
  },
}
