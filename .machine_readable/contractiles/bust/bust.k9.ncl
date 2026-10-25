K9!
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 IDApTIK contributors
{
  pedigree = {
    schema_version = "1.0.0",
    component_type = "contractile:bust",
    contractile_verb = "bust",
    semantics = "Failure drills and recovery expectations for fragile runtime boundaries.",
    tier = 'Kennel,
    authority = 'alarm,
    security = {
      leash = 'Kennel,
      trust_level = "declaration-only",
      allow_network = false,
      allow_filesystem_write = false,
      allow_subprocess = false,
      capabilities = [],
    },
    metadata = {
      name = "idaptik-bust",
      version = "1.0.0",
      description = "Failure scenarios and tested recovery routes; drills are not daily gates.",
      author = "IDApTIK contributors",
      paired_runner = "bust.ncl",
    },
    side_effects = [],
  },
  declaration = {
    schema_version = "1.0.0",
    verb = "bust",
    authority = 'alarm,
    drills = [
      { id = "config-schema-violation", injection = "Use a negative scenario fixture that violates the Nickel schema.", expected = "Export or validation fails with a typed error; no invalid package reaches gameplay.", recovery = "Correct the source and rerun just config-check.", evidence = "config/scenario-fixtures/bad_*.ncl" },
      { id = "toolchain-missing", injection = "Start from a fresh machine without one of the pinned Zig, Elixir, Rust, or just toolchains.", expected = "The doctor reports the missing tool; required gates fail rather than skip.", recovery = "Run just setup and then just doctor.", evidence = "mise.toml, rust-toolchain.toml, and just doctor" },
      { id = "frontend-core-desync", injection = "Change a frontend adapter so its behavior diverges from gameplay truth.", expected = "Frontend parity tests detect the mismatch.", recovery = "Restore the adapter boundary and rerun the Bevy parity suite.", evidence = "ADR-0003, ADR-0008, and frontend tests" },
      { id = "determinism-break", injection = "Introduce nondeterminism into scenario simulation or replay.", expected = "Replay or deterministic core tests fail on the divergent result.", recovery = "Restore seeded deterministic inputs and rerun core/replay tests.", evidence = "ADR-0004 and core tests" },
      { id = "relay-loss", injection = "Stop or disconnect the configured Burble relay during a two-seat session.", expected = "Report peer loss or missing watermark; do not silently substitute a session authority.", recovery = "Rejoin and resynchronize using the existing relay recovery path.", evidence = "just loopback-check and ADR-0006" },
      { id = "wslg-copy-mode", injection = "Break the WSLg Copy Mode launch path on a Windows host.", expected = "Runtime diagnostics identify the failure and point to the tested fallback.", recovery = "Follow existing troubleshooting and launcher recovery guidance; do not duplicate it in this component.", evidence = "docs/MULTIPLAYER-TROUBLESHOOTING.md, docs/player-launch-flow.html, docs/man/idaptik-launcher.1, and tests/runtime_doctor_test.sh" },
      { id = "authority-partition", injection = "Make the configured hosted or federated session authority unreachable.", expected = "Fail closed for that session; never promote a directory peer, cache, or GSA client.", recovery = "Restore the same authority and recover only from its matching durable epoch.", evidence = "deployment tests and ADR-0014" },
      { id = "coordination-format-drift", injection = "Change the coordination version, order, header, indentation, or DEED reference in a fixture.", expected = "The coordination structural guard rejects the file.", recovery = "Restore the v1.2.0 structure and rerun just machine-readable-check.", evidence = "scripts/coordination_k9_check.sh" },
    ],
  },
}
