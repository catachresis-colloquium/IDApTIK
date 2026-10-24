# ADR-0013: Evil Weevil — a separate, deterministic adversarial testing adapter

- Status: Proposed
- Date: 2026-10-24
- Relates to: ADR-0005 (session relay topology), ADR-0006 (gossamer host and
  burble transport), ADR-0012 (signal network), `docs/security/evil-weevil-corpus.md`

## Context

IDApTIK's multiplayer acceptance rules live in three production surfaces:

1. the envelope/lockstep validators in `idaptik-net`
   (`decode_command`, `LockstepCore::on_peer_command`, `on_peer_commit`,
   `adopt_resync`, the `Hello::compatible_with` epoch predicate);
2. the deterministic simulation in `idaptik-core` (`GhostLobbySim`), which
   copes with typed-but-impossible gameplay intents as no-ops or typed
   denials;
3. the burble `GameChannel` relay (external repository, pinned at
   `ef6756cf1591406d434c5c04c3c018e935836e4c` in CI), which enforces the role
   table and per-seat `seq` de-duplication.

We need adversarial evidence that those rules hold against deliberately
hostile input — replays, duplicates, impossible claims, epoch conflicts,
obsolete prior-session frames, and withheld observations — without turning
the test instrument itself into a hostile system. The in-game enemy utility
policy is gameplay AI, **not** a security policy, and an LLM adversary adds
non-determinism without adding coverage at this stage.

## Decision

Add a **separate crate**, `crates/idaptik-evil-weevil`, as a sans-IO,
`#![forbid(unsafe_code)]` adversarial testing adapter. It never opens a
socket, runs a shell command, resolves a remote address, or touches a
database; every endpoint is an in-memory model and every identity a
disposable dummy.

### Fixed corpus first; adaptive selection later

The first corpus is five bounded, deterministic scenarios (see
`docs/security/evil-weevil-corpus.md` for the per-scenario contract and
results). There is no adaptive planner and no LLM. Adaptive action selection
is deferred until the fixed corpus proves reproducible coverage; the agent
(`agent::EvilWeevil`) is a small explicit state machine whose whole proposal
vocabulary is the `IntentKind` enum.

### Four permission realms, never interchangeable

| Realm | Principal | May | May never |
|---|---|---|---|
| Adversary | `PrincipalKind::Adversary` | propose bounded intents from a scenario's fixed vocabulary | touch the sim, deliver frames, read telemetry, change policy |
| Test harness | `PrincipalKind::TestHarness` | authorise attempts, own the in-memory wire, feed the **real** production validators, record outcomes | mutate sim state directly, relax a production check, grant permissions |
| Defensive observer | `PrincipalKind::DefensiveObserver` | read validated telemetry, classify, recommend — read-only | send anything, end a session, enforce |
| Host operator | `PrincipalKind::HostOperator` | hold the enforcement policy, take local protective action | ban or retaliate against a remote machine (no such permission exists) |

There is deliberately no `BypassHostValidation` permission. Evil Weevil
proposes; the harness decides whether an *attempt* is authorised inside the
scenario; the unchanged production handler independently decides whether the
frame is acceptable. Tests are never routed through a client helper that
sanitises the invalid condition under test — raw frames are constructed only
in `wire.rs` and fed through the same `decode_command` /
`LockstepCore` / `GhostLobbySim` paths production traffic uses.

### Faults are never hostility; recommendations never retaliate

The observer consumes only host-validated telemetry. Packet loss, delay,
silence, reconnect duplicates and divergence are `FaultKind` facts with no
intent attribution, and are classified benign/fault by construction; only a
frame the host's own rules refuse is a violation. False positives and false
negatives are measured on every run and fail the scenario if non-zero. The
strongest recommendation is `FlagForOperatorReview`. The enforcement alphabet
(`EnforcementAction`) contains no remote ban, blocklist, or packet-injection
variant, and even local protection requires the explicit operator principal;
the default mode is observe-only.

### Bounded by construction

Hard caps in `limits.rs` (500 steps, 512 wire messages, queue depth 64, 32
intents per scenario) fail the run the moment they are crossed. There is no
wall clock in the sans-IO core — "timeouts" are logical step budgets — so
every run is reproducible, and the offline gate proves byte-identical reports
across two runs.

### The relay is modelled, then separately confirmed

`relay_model.rs` re-states the documented burble channel rules offline so the
corpus needs no BEAM process. It is explicitly **not** the relay and holds no
relay authority. A cross-repository check
(`scripts/evil_weevil_relay_check.sh`, three `#[ignore]` socket tests in
`crates/idaptik-net/tests/evil_weevil_relay.rs`) runs against a throwaway
local burble on its isolated test port, loopback only, with guest dummy
identities. Divergence between model and channel is a defect in the model,
and the model's profile table is pinned against the burble source by a
regression test.

## Consequences

- Multiplayer host invariants now have regression tests driven by prohibited
  proposed actions, not only happy-path scripted seats.
- One demonstrated cross-repository defect is pinned rather than hidden:
  burble's idaptik profile has no `Signal` entry (ADR-0012 verbs exist
  client-side but the relay refuses them as unknown commands). The pin fails
  loudly if burble adds the entry, so the finding cannot silently stale.
- Unimplemented deployment modes (P2P game lanes, federation, authoritative
  host, game-server-admin) are recorded as gaps in the security doc, never
  simulated. Only the single rendezvous relay topology is exercised.
- The offline gate (`just evil-weevil-check`) needs only the Rust toolchain
  and fails hard if it is missing; the relay leg
  (`just evil-weevil-relay-check`) additionally needs Elixir and a local
  burble checkout and is not part of the default test gate.
- No Elixir or `server/` tree is added to this repository, per `AGENTS.md`;
  session-layer change remains the burble repository's concern.
- Out of scope, explicitly: native/FFI and wasm32 execution of any Evil
  Weevil ABI (the SNIF guest stays a pure numeric/buffer DSP kernel),
  Dragonfly or any hosted coordination dependency, LMDB beyond local
  replay/index data, availability testing, scanning, and traffic against any
  system that is not the isolated local test environment.
