# Architecture

IDApTIK is a polyglot game monorepo. **Rust/Crusoe owns gameplay truth** (an
engine-agnostic, deterministic, event-sourced simulation); **multiplayer
session relaying lives in burble** (the estate's gaming communication platform,
estate ruling 2026-08-04); frontends are thin drivers over the same typed
`Command`/`Event` wire.

For the reasoning behind each choice, see the ADRs in [`docs/adr/`](docs/adr/).
The proposed cross-repository epistemic state-machine seam is documented in
[`architecture/esm-contract-and-repo-boundaries.md`](architecture/esm-contract-and-repo-boundaries.md).

## Directory structure

```
.
├── crates/            # Rust/Crusoe Cargo workspace (edition 2024, AGPL-3.0-or-later)
│   ├── idaptik-core/  #   engine-agnostic sim: netsim + scenario + Moletaire companion
│   ├── idaptik-ffi/   #   C ABI surface (JSON in / JSON out) over the core
│   ├── idaptik-tui/   #   ratatui/crossterm frontend + --headless/--replay/--export verifier
│   ├── idaptik-bevy/  #   selected Bevy side-on 2.5D frontend (ADR-0008)
│   ├── idaptik-net/   #   Phoenix Channels client over burble game-session fabric (ADR-0006)
│   └── idaptik-evil-weevil/  # sans-IO deterministic adversarial testing adapter (ADR-0013)
├── config/            # Nickel typed config: scenario-schema.ncl + fixtures (incl. bad_*.ncl)
├── docs/adr/          # Architecture Decision Records
├── fixtures/          # cross-language wire fixtures
├── scripts/           # bootstrap.sh, doctor.sh, install-idris2.sh
├── LICENSES/          # full SPDX license texts (REUSE)
└── justfile           # task runner (build/test/lint/config gates)
```

## The core (`crates/idaptik-core`)

The authoritative simulation. It is **engine-agnostic** (no rendering or I/O),
**deterministic**, and **event-sourced**: a run is a pure function of
`(definition, config, seed, command stream)` producing an `Event` log and a
`Debrief` (ADR-0004). `#![forbid(unsafe_code)]`.

- `netsim/` — the grounded network model (devices, addressing, DNS, routing,
  reachability, sessions) that the hack is played against.
- `scenario/` — definition-as-data scenarios (Ghost Lobby, the Exchange House
  building runtime as floors), the actor archetype + modifier registry, and the
  tick/command/event/snapshot machinery.
- `companion/` — Moletaire, the deterministic companion (Rust port).

Determinism is enforced by tests: `replay_determinism`, `snapshot_equivalence`,
`rng_vectors`, `no_panic_fuzz`, and the negative Nickel fixtures.

### Versioned package boundary

IDApTIK publishes and owns `contracts/idaptik/v1`. The loader in
`idaptik-core::package` deserializes the real `ScenarioDefinition`, applies the
game's semantic validation, executes the scheduled commands, snapshots,
restores and compares the replay tail. UMS consumes this contract but is never
a runtime or UI dependency. See
[`architecture/ums-package-contract.md`](architecture/ums-package-contract.md)
and ADR-0007.

## The FFI boundary (`crates/idaptik-ffi`)

A C ABI that drives the core over a **JSON-in/JSON-out** surface
(`idap_ghost_lobby_new/tick_json/snapshot_json/free`, `idap_demo_network`, …).
Panics are contained with `catch_unwind`; errors surface as `{"error": …}`
sentinels. The generated header (`include/idaptik.h`) is committed and
regenerated with `just ffi-header`. An in-process-vs-ABI parity test guards
equivalence. Intended host bridge is Zig (ADR-0001).

## Frontends

`idaptik-tui` is the reference frontend and headless verifier.
`idaptik-bevy` is the selected side-on 2.5D graphical frontend. Both are thin
drivers: they read input, push `Command`s, and render the `Event`/snapshot
stream. The core never depends on them (ADR-0003 and ADR-0008).

## Multiplayer (burble game-session fabric)

Multiplayer session relaying lives in the **burble** repository
(`metadatastician/burble/server/lib/burble_web/channels/game_channel.ex`),
which is the estate's designated gaming communication platform (estate ruling
2026-08-04). The `crates/idaptik-net` crate adapts Burble's pinned
`burble-client` Phoenix implementation to IDApTIK's transport/error seams and
connects to `game:<id>` channels over `/voice/websocket` (ADR-0006). The relay
is **transport-agnostic** and holds no gameplay authority
— it forwards; the core (run on each client) decides (ADR-0005).

The `idaptik-evil-weevil` crate (ADR-0013) exercises these acceptance rules
adversarially: a sans-IO, deterministic five-scenario corpus drives raw,
unsanitised frames through the unchanged production validators, with four
permission-separated realms (adversary, harness, read-only defensive
observer, operator-only local enforcement). It opens no sockets; the relay
rules are modelled in-process and separately confirmed against a throwaway
local burble. See `docs/security/evil-weevil-corpus.md`.

## Build & toolchain

`just` orchestrates `cargo` (Rust), `mix` (Elixir), and `nickel` (config).
Rust/Crusoe toolchains are pinned via `mise.toml` and `rust-toolchain.toml`. Key gates:

- `just test-ghost` — `cargo test` (core/tui/ffi/net/evil-weevil) +
  `cargo clippy -D warnings` + `cargo fmt --check`.
- `just evil-weevil-check` — the ADR-0013 offline adversarial corpus:
  tests, clippy, redacted JSON artifacts, secret-shape and byte-identical
  determinism gates. `just evil-weevil-relay-check` adds the optional
  throwaway-burble leg (needs Elixir + a sibling burble checkout).
- `just config-scenario-check` — exports the Nickel scenario, round-trips it
  through the Rust validator, and asserts every `bad_*.ncl` fixture is rejected.

## Design principles

- **One source of truth.** The core is authoritative; everything else observes.
- **Determinism first.** Same inputs ⇒ same `Event` log ⇒ replayable, testable.
- **Definition-as-data.** Scenarios, actors, and buildings are data, not code.
- **Transport-agnostic multiplayer.** Swapping the transport must not change the
  game (a plain socket substitutes for the real transport in tests).

---

*See the ADRs in `docs/adr/` for the decisions behind this structure.*
