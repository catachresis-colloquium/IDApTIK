K9!
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 IDApTIK contributors
{
  pedigree = {
    schema_version = "1.0.0",
    component_type = "contractile:trust",
    contractile_verb = "trust",
    semantics = "Trust boundaries, integrity, provenance, and approval requirements.",
    tier = 'Kennel,
    authority = 'blocking,
    security = {
      leash = 'Kennel,
      trust_level = "declaration-only",
      allow_network = false,
      allow_filesystem_write = false,
      allow_subprocess = false,
      capabilities = [],
    },
    metadata = {
      name = "idaptik-trust",
      version = "1.0.0",
      description = "Blocking trust checks and approval boundaries for source, dependencies, and release changes.",
      author = "IDApTIK contributors",
      paired_runner = "trust.ncl",
    },
    side_effects = [],
  },
  declaration = {
    schema_version = "1.0.0",
    verb = "trust",
    authority = 'blocking,
    current_trust_level = "standard",
    trust_levels = ["maximal", "standard", "restricted", "minimal"],
    permissions = ["read", "build", "test", "format"],
    approval_required = ["license changes", "ADR decisions", "Rust-Elixir wire-format changes", "gameplay-truth determinism changes", "sensitive paths in CODEOWNERS"],
    clauses = [
      { id = "no-secrets-committed", severity = "critical", description = "Do not commit credential files or production secrets.", scope = "repository tree and Git history", evidence = "secret scanner and pre-push hook" },
      { id = "no-private-keys", severity = "critical", description = "Do not commit private-key files, including PEM, KEY, id_rsa, or id_ed25519 files.", scope = "tracked repository files", evidence = "repository secret scan" },
      { id = "no-tokens-in-source", severity = "critical", description = "Do not hardcode API keys, secrets, tokens, or passwords in source.", scope = "Rust, Elixir, Zig, and Idris2 source", evidence = "secret scanner and code review" },
      { id = "dco-signoff", severity = "warning", description = "Contributions should carry a DCO 1.1 Signed-off-by trailer.", scope = "commit messages", evidence = "git log -1 --format=%b" },
      { id = "engine-license-intact", severity = "critical", description = "Engine code remains AGPL-3.0-or-later; content remains CC-BY-SA-4.0 and governance material remains separately licensed.", scope = "repository license layers", evidence = "Cargo.toml, LICENSE, LICENSES/, and SPDX headers" },
      { id = "trust-deny-areas", severity = "advisory", description = "Sensitive areas require explicit human approval before modification.", scope = "LICENSE, LICENSES/, docs/adr/, .machine_readable/contractiles/, .github/CODEOWNERS, and crates/idaptik-core", evidence = ".github/CODEOWNERS and coordination.k9" },
      { id = "dependency-lock-integrity", severity = "critical", description = "Keep dependency lockfiles and supply-chain policy aligned with reviewed manifests.", scope = "Cargo.lock and deny.toml", evidence = "supply-chain CI gate" },
      { id = "authority-is-not-discovery", severity = "critical", description = "Discovery, clustering, and cache replication do not grant simulation authority or provide consensus.", scope = "multiplayer and federation", evidence = "ADR-0013 and deployment tests" },
    ],
  },
}
