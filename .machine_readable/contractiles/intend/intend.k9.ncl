K9!
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 IDApTIK contributors
{
  pedigree = {
    schema_version = "1.0.0",
    component_type = "contractile:intend",
    contractile_verb = "intend",
    semantics = "Non-gating north-star, committed next-actions, and horizon aspirations.",
    tier = 'Kennel,
    authority = 'reporting,
    security = {
      leash = 'Kennel,
      trust_level = "declaration-only",
      allow_network = false,
      allow_filesystem_write = false,
      allow_subprocess = false,
      capabilities = [],
    },
    metadata = {
      name = "idaptik-intend",
      version = "1.0.0",
      description = "Separates current commitments from wishes and implementation evidence.",
      author = "IDApTIK contributors",
      paired_runner = "intend.ncl",
    },
    side_effects = [],
  },
  declaration = {
    schema_version = "1.0.0",
    verb = "intend",
    authority = 'reporting,
    north_star = "Ship an asymmetric infiltration game where the hacker solves a real, legible network and the infiltrator moves through a physical space whose layer bites back.",
    anti_purpose = [
      "Do not become a game engine consumed by unrelated third-party games.",
      "Do not trade deterministic replay for renderer or network convenience.",
      "Do not describe planned deployment modes as working runtime features.",
      "Do not turn the hacker into a disembodied support role or the game into shared-control co-op.",
    ],
    if_in_doubt = ["Ask before changing the engine license, an ADR decision, the Rust-Elixir wire format, or gameplay-truth determinism in crates/idaptik-core."],
    committed = [
      { id = "envelope-playable-slice", status = "in-progress", description = "Deliver the Envelope minimum game with movement and basic hacker interactions.", evidence = "README roadmap and crates/idaptik-core/src/scenario/" },
      { id = "two-player-windowed-slice", status = "declared", description = "Deliver the Gossamer-window and Burble-transport two-player slice.", evidence = "docs/adr/0006-gossamer-host-and-burble-transport.md" },
      { id = "session-relay", status = "in-progress", description = "Maintain the Elixir Bandit/Phoenix relay over the typed command/event boundary; simulation authority remains topology-specific.", evidence = "ADR-0002, ADR-0005, ADR-0006, and ADR-0013" },
      { id = "config-schema-coverage", status = "in-progress", description = "Validate Nickel scenarios and ensure invalid fixtures fail the configuration gate.", evidence = "config/ and just config-check" },
    ],
    horizon = [
      { id = "renderer-decision", horizon = "near", status = "achieved", description = "Select Bevy and retire the alternative renderer.", evidence = "ADR-0008" },
      { id = "wire-format-settled", horizon = "mid", status = "open", description = "Choose the Rust-Elixir wire encoding and persistence strategy.", evidence = "open decision in IDApTIK_chora.deed" },
      { id = "adaptive-security-ai", horizon = "mid", status = "declared", description = "Consider security countermeasures that react to hacker interference.", evidence = "README roadmap" },
      { id = "full-experience", horizon = "far", status = "declared", description = "Deliver adaptive security, emergent tension, and the complete multiplayer experience.", evidence = "README roadmap" },
      { id = "direct-peer-transport", horizon = "far", status = "planned", description = "Implement direct peer transport and deterministic recovery without a required central service.", evidence = "ADR-0013" },
      { id = "federated-routing", horizon = "far", status = "planned", description = "Implement advisory federation discovery without transferring session authority.", evidence = "ADR-0013" },
    ],
  },
}
