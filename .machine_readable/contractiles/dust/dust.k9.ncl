K9!
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 IDApTIK contributors
{
  pedigree = {
    schema_version = "1.0.0",
    component_type = "contractile:dust",
    contractile_verb = "dust",
    semantics = "Exnovation register, repository hygiene, and recovery pointers.",
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
      name = "idaptik-dust",
      version = "1.0.0",
      description = "Advisory hygiene checks and explicit record of retired metadata mechanisms.",
      author = "IDApTIK contributors",
      paired_runner = "dust.ncl",
    },
    side_effects = [],
  },
  declaration = {
    schema_version = "1.0.0",
    verb = "dust",
    authority = 'advisory,
    hygiene = [
      { id = "no-build-artifacts-committed", severity = "info", description = "Rust and Elixir build output is not committed.", tolerance = "No tracked target/ or _build/ output.", evidence = "git ls-files target _build" },
      { id = "no-editor-cruft", severity = "info", description = "No stray editor backups or platform metadata are present.", tolerance = "No .orig, *~, or .DS_Store files outside generated build trees.", evidence = "repository tree" },
      { id = "no-duplicate-root-metadata", severity = "warning", description = "Project state is not duplicated as root STATE, META, or ECOSYSTEM files.", tolerance = "The chora deed is the sole descriptive project-state source.", evidence = "IDApTIK_chora.deed" },
      { id = "no-generated-k9-output", severity = "info", description = "No unreviewed generated K9 build output is committed.", tolerance = "Only source components and runner files are tracked.", evidence = ".machine_readable/contractiles/" },
      { id = "legacy-layout-retired", severity = "critical", description = "No legacy A2ML files, wrapper manifest, or obsolete readers remain active.", tolerance = "No .a2ml files or k9iser.toml remain in the repository.", evidence = "tests/machine_readable_migration_test.sh" },
    ],
    retirements = [
      { id = "retire-a2ml-metadata", status = "completed", disposition = "Classified descriptive state into the one project deed, policies into six K9 components, and instructions into root coordination.k9; removed legacy files and readers.", evidence = "tests/machine_readable_migration_test.sh and docs/standards-conformance.md" },
      { id = "retire-k9iser-wrapper", status = "completed", disposition = "Removed the TOML wrapper, generator/build recipes, and stale manifest references; K9 Nickel components use the pinned standards validator.", evidence = "scripts/machine_readable_check.sh and .machine_readable/contractiles/" },
      { id = "retire-inline-launcher-metadata", status = "completed", disposition = "Removed the unused embedded metadata block from launcher.sh; launch behavior remains in scripts and documented recovery guides.", evidence = "launcher.sh and docs/man/idaptik-launcher.1" },
      { id = "retire-split-descriptiles", status = "completed", disposition = "Consolidated project facts into IDApTIK_chora.deed instead of maintaining one file per metadata family.", evidence = "IDApTIK_chora.deed" },
      { id = "retain-copy-mode-guidance", status = "retained", disposition = "Keep existing WSLg Copy Mode diagnosis and recovery instructions in player-facing sources; do not duplicate them here.", evidence = "docs/MULTIPLAYER-TROUBLESHOOTING.md and tests/runtime_doctor_test.sh" },
    ],
  },
}
