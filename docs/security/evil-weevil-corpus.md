# Evil Weevil corpus — methodology, results, and deployment-mode gaps

- Date: 2026-10-24 (updated 2026-11-01: EW-06 observation premises and attribution)
- Corpus version: `2` (`CORPUS_VERSION` in `crates/idaptik-evil-weevil/src/scenarios/mod.rs`)
- Report schema: `idaptik-evil-weevil-corpus-v1`
- Decision record: [ADR-0017](../adr/0017-evil-weevil-adversarial-testing-adapter.md)
- Code: `crates/idaptik-evil-weevil/` (sans-IO, `#![forbid(unsafe_code)]`)

This document is the runbook for the first, fixed adversarial corpus. It
states, per scenario, the principal and permitted operations, the protocol
invariant under test, the exact bounded input sequence, the expected
rejection/acceptance/recovery, and the observable result. Every scenario is
deterministic: no wall clock, no socket, no RNG outside the seeded
`Mulberry32` used only to schedule in-memory jitter. Two runs produce
byte-identical reports, and the offline gate enforces that with `cmp`.

## 1. Method and permission separation

Evil Weevil proposes; it never applies. A proposal reaches production code
only through this chain:

1. `agent::EvilWeevil::propose` emits an `Intent` holding a **raw,
   unsanitised** wire payload (`wire.rs` is the only module allowed to forge
   frames the typed client cannot express).
2. `policy::TestPolicy` confirms the adversary holds only
   `ProposeBoundedIntent`; the harness identity alone may deliver and feed
   validators. There is no bypass permission.
3. `relay_model::RelayModel` applies the documented burble relay rules
   offline (role table, strictly-increasing `seq`).
4. Anything relayed goes through the unchanged production path:
   `idaptik_net::envelope::decode_command`,
   `idaptik_net::lockstep::LockstepCore`, or the real
   `idaptik_core::scenario::GhostLobbySim`.
5. Outcomes become validated `telemetry::TelemetryEvent`s; the read-only
   `observer::DefensiveObserver` classifies them and recommends only;
   `enforce::EnforcementPolicy` decides any **local** response, and only the
   operator principal may select it.

The four realms (adversary, harness, observer, operator) are asserted
non-interchangeable in both unit tests and
`tests/regression.rs::permission_realms_are_not_interchangeable`.

### Bounds and minimal evidence

Hard caps (`src/limits.rs`): 500 lockstep steps, 512 wire messages, queue
depth 64 (the observed peak is recorded and the run fails at the cap), 32
intents per scenario. The hand-written sequences stay far smaller (no
scenario proposes more than 8 intents). There is no availability testing, no
scanning, and no remote address anywhere in the crate. Artifacts are JSON
written by the corpus binary and pass through recursive redaction
(`report::redact`, seven sensitive key names plus `Bearer` and
credential-URL patterns); all identities are fixed dummy strings of the form
`ew-dummy-<seat>-<nnnn>`. A planted-secret unit test proves the redactor
actually fires.

## 2. The six scenarios

### EW-01 — stale or duplicate session messages (`EW-01-stale-duplicate-messages`)

- **Principal / ops:** `ew-dummy-hacker-0001`, adversary; `ProposeBoundedIntent` only.
- **Invariant:** no frame at or below a committed watermark can re-enter the
  lockstep input set; duplicate `seq` envelopes are dropped, not relayed.
- **Bounded sequence (7 declared inputs):** commit through tick 5; a tick-4
  `NetSsh` with fresh `seq`; the same envelope replayed verbatim; a frame
  with a lower `seq`; a tick-6 frame exactly at the frontier; a command
  object missing `at`; an unknown tag `WarpToRoom`.
- **Expected / observed results (all demonstrated against production):**
  - `stale-frame-rejected-by-lockstep` — `on_peer_command` refuses the tick-4
    frame ("arrived after its commit").
  - `duplicate-envelope-dropped`, `lower-seq-dropped` — the relay model
    acks-and-drops both with `stale_or_duplicate`; a `RelayDropped`
    telemetry fact is recorded, never escalated.
  - `frontier-frame-accepted` — refusals are scoped, not blanket.
  - `missing-at-rejected`, `unknown-tag-rejected` — decoder failures
    (`Undecodable`), so the sim never sees the frames.
  - `no-execution-from-refused-frames` — the core's step counter stays at 0.
  - `relay-accounting` — exactly relayed 2, dropped 2, rejected 0.

### EW-02 — impossible movement and invalid target claims (`EW-02-impossible-movement-invalid-targets`)

- **Principal / ops:** `ew-dummy-hacker-0002`; `ProposeBoundedIntent` only.
- **Invariant:** the wire cannot author positions; typed-but-impossible
  intents are no-ops or typed denials that leave state coherent, and
  untyped frames never reach the sim.
- **Bounded sequence (6):** `NetSsh` and `NetHack` against
  `NetNodeIndex(graph.nodes.len() + 100)`; a cold camera `Uplink`; a cold
  `Pivot(GridJump)`; holding right for 30 ticks against the world edge; a
  forged `TeleportToRoom` tag.
- **Expected / observed:**
  - `out-of-graph-netssh-is-a-noop`, `out-of-graph-nethack-is-a-noop` —
    post-tick state equals a quiet tick and no `PivotOpened`/`NetHackAction`
    event is emitted; no panic.
  - `cold-uplink-denied-no-route` — `Event::UplinkDenied { reason:
    DenyReason::NoRoute }`.
  - `cold-grid-pivot-denied` — `Event::PivotDenied`; the ISP hop is a
    precondition.
  - `movement-clamped-inside-the-world` — player x stays finite within
    `WORLD_WIDTH` (1260.0); movement is a held-button intent, not a position
    claim.
  - `position-claim-verb-does-not-exist` — the forged tag is
    `Undecodable`; the `Command` enum has no position variant.
  - `gameplay-denials-stay-benign` — the four gameplay outcomes classify
    benign; the single violation in the trace is the forged wire tag,
    demonstrating the fault/hostility boundary rather than blurring it.

### EW-03 — conflicting session and authority epochs (`EW-03-conflicting-epochs`)

- **Principal / ops:** `ew-dummy-infiltrator-0003`; `ProposeBoundedIntent` only.
- **Invariant:** only seats agreeing on the full run epoch (protocol, seed,
  difficulty, reduced-motion, tick budget) may lockstep; resync authority
  cannot be self-issued; nothing schedules past the run.
- **Bounded sequence (8):** four `net:hello` frames each mutating exactly one
  epoch field (seed, `max_ticks`, `NET_PROTO+1`, difficulty `"operator"`),
  deserialised through the real `Hello` serde path; a same-seat
  `adopt_resync`; a commit and a command at `max_ticks`; a rejoin hello for a
  seat with no run; an unknown-game relay join.
- **Expected / observed:**
  - `baseline-hellos-compatible`, then `epoch-conflict:{seed,max_ticks,proto,
    difficulty}` all refused by `Hello::compatible_with`.
  - `same-seat-resync-rejected` — "same seat on both sides".
  - `past-run-commit-rejected`, `past-run-command-rejected` — "past the run".
  - `rejoin-without-run-refused` — the live-driver predicate (`peer.rejoin`
    meeting a fresh seat) is pinned offline; the socket path is covered by
    loopback run 4.
  - `unknown-game-join-refused` — the relay model's outer epoch boundary.
- **Authority note:** epoch here means agreement on the run configuration,
  not administrative power; no principal can redefine another seat's epoch.

### EW-04 — disconnect/reconnect and obsolete prior-session messages (`EW-04-reconnect-obsolete-messages`)

- **Principal / ops:** `ew-dummy-hacker-0004`; `ProposeBoundedIntent` only.
- **Invariant:** uncommitted commands never happened; a replaced session
  cannot inject frames below the committed watermark; resync reconstructs
  the reference run exactly.
- **Bounded sequence (7):** lockstep to step 8; an orphan hacker command
  whose commit never arrives; `on_peer_lost`; the survivor's `make_resync`;
  a fresh hacker `adopt_resync`; an obsolete command one tick below the
  survivor's watermark; an old-session hello carrying a different seed;
  drive to completion.
- **Expected / observed:**
  - `orphan-pruned-on-loss` — `your_pending` in the resync is empty.
  - `resync-carries-committed-history` — full event log and a snapshot at or
    before the death step.
  - `resync-adopted-by-fresh-seat` — the replacement core reconstructs
    mid-run.
  - `obsolete-prior-session-frame-rejected` — stale-after-commit from the
    production validator.
  - `old-session-hello-epoch-refused` — seed mismatch cannot join the
    recovered epoch.
  - `survivor-matches-reference`, `rejoined-seat-matches-reference` — both
    full event logs serialize identically to the headless reference.
- The same death/resync path over a real burble relay is exercised by
  `scripts/loopback_check.sh` run 4; this scenario adds the adversarial
  obsolete-frame and old-epoch checks.

### EW-05 — delayed/withheld observations: faults versus hostility (`EW-05-delayed-withheld-observations`)

- **Principal / ops:** `ew-dummy-infiltrator-0005`; `ProposeBoundedIntent` only.
- **Invariant:** transport pathologies classify as faults, never hostility;
  recovery preserves the deterministic run; enforcement is local and
  operator-gated.
- **Bounded sequence (5 phases):** 14 rounds of seeded 1–3 frame jitter
  (nothing lost); four rounds withholding every frame; release and catch-up;
  one genuine stale-after-commit frame as the contrast case; the enforcement
  boundary demonstration.
- **Expected / observed:**
  - `delay-is-pacing-only` — both seats' event logs equal the headless
    reference; only timing changed.
  - `withholding-trips-loss-predicate` — `blocked_on_peer()` (the same
    predicate the live socket driver uses) becomes true; faults are recorded
    as `WithheldWatermark`/`SilenceGraceExpired`.
  - `withheld-run-recovers-to-reference` — after release, both logs again
    match byte-for-byte; recovery is recorded, not assumed.
  - `contrast-violation-rejected` — the one real protocol violation is
    rejected by the host.
  - `faults-never-flagged-hostile` — every fault index stays outside the
    violation set; the trace contains both faults and violations and the
    observer separates them.
  - `strongest-recommendation-is-review` — the ceiling of observer power is
    `FlagForOperatorReview`.
  - `observe-only-never-acts` — the default policy logs everything and acts
    on nothing.
  - `enforcement-alphabet-is-local-only` — an exhaustive match over
    `EnforcementAction`; a future remote variant would fail to compile.
  - `non-operator-cannot-configure:*` — harness, observer and adversary are
    all refused `local_protective`.
  - `operator-may-end-only-the-local-session` — termination is explicit,
    local, and operator-only.

### EW-06 — infrastructure and companion orders: seat rules, the relay gap, and the observation trail (`EW-06-infrastructure-orders-and-observations`)

- **Principal / ops:** `ew-dummy-hacker-0001`; `ProposeBoundedIntent` only.
- **Invariant:** the infrastructure verbs (`Command::Mole` from ADR-0014,
  electrical `Command::Power`) obey the same acceptance rules as every other
  frame — the relay role table fails closed on tags it does not carry, the
  watermark rejects stale orders, only an accepted order moves the sim — and
  the consequences come back through the canonical log *as observations*,
  never as a leak of the hidden order.
- **Bounded sequence (7 declared inputs):** commit the infiltrator watermark
  through tick 1; propose a `Mole` synth and a `Power` inspect from the hacker
  seat through the model relay; submit the same `Mole` synth at frontier
  tick 2 on the infiltrator peer path, direct to the production validator;
  schedule `ForceCrisis` and a silent underground `MoleOp::MoveTo` at tick 2,
  then the scene synth at tick 81; commit through tick 85 and advance in
  sampling rounds; finally replay a `Mole` order below the watermark.
- **Observation premises (issue #191):** the shared live-seat fixture is
  pointed at `pump-house`, with its pause/extract tail removed and horizon
  raised. A shadow lockstep run replays the same seed, script, frontier
  synth probe and crisis frame to find Billy's actual position at tick 81;
  it does not assume the crisis assignment is where he remains. The mole
  target is the centre of the intersection between Billy's hearing radius
  and the mole's reachable travel band, with a corridor-width check. The
  production run then checks the predicted position, actual source distance
  and intensity against Billy's own archetype stats. No hearing or phase
  gate is widened, and no simulation state is injected.
- **Expected / observed:**
  - `mole-order-refused-at-relay` / `power-order-refused-at-relay` — the
    pinned burble profile carries neither tag; both refusals fail closed.
    `Power` additionally carries its seat per-op, which a tag-keyed table
    cannot route without splitting the verb.
  - `frontier-mole-order-accepted` / `scene-frame-accepted-at-*` — the
    production lockstep validator accepts the peer-seat frames.
  - `mole-order-applied-by-sim` / `no-denial-for-legitimate-order` — a typed
    `SynthSoundPlayed` event and its canonical diversion are sampled after
    command tick 81, not after the earlier acceptance probe; no `MoleDenied`.
  - `guard-on-shift-when-the-synth-plays`,
    `shadow-position-matches-billy-at-synth`, and
    `diversion-source-within-billys-hearing` — the observation premises hold
    in the production run, not only in the shadow prediction.
  - `diversion-announcement-source-unambiguous` — the diversion is the only
    audible `UnusualNoise` observation in its room on that tick. The event
    carries kind + room and the watcher deduplicates on that key, so a
    coincident noise in the same room cannot be credited to the synth.
  - `guard-observed-the-diversion` — exactly one matching
    `GuardObserved(UnusualNoise)` event is emitted on the scene synth tick.
  - `perception-contract-lists-the-noise` — `sim.actor_observations()` at
    Billy's position lists a noise matching the diversion's source position
    and intensity. The free `infra::observations()` function is not a
    substitute when called with empty `extra_noise`: companion noise enters
    through the sim method.
  - `only-the-scene-diversion-announced` — the acceptance probe cannot
    supply the witness for the later order.
  - `stale-mole-order-rejected` — below the watermark the new verbs are
    refused like any other.
  - `relay-accounting` — two refusals, zero relays, zero drops.
- **Negative controls:** `tests/infra_orders.rs` replays the original quiet,
  480-unit setup and asserts silence; forces a crisis while still out of
  range and asserts that presence alone is insufficient; then walks the mole
  to Billy's seeded snack position and asserts that the sim method carries
  the audible diversion while the free function with empty `extra_noise`
  does not. A third test pins the source-specific corpus checks by name.


## 3. Results, defects, and the honesty boundary

### What is demonstrated

All six scenarios pass against the real production validators in this
repository. The corpus, regression and dedicated EW-06 observation tests,
plus the crate's unit tests, pin the guarantees. The corpus report records,
per run: pass/fail per named check, full telemetry, observer classifications
with measured false-positive/false-negative counts (zero/zero corpus-wide),
queue-depth and wire-message peaks, and redaction counts.

### Demonstrated cross-repository gaps

**Burble's idaptik profile lacks `Signal`, `Mole` and `Power`.** The client
routing table handles these verbs, but the pinned burble `command_roles()`
carries none of those tags, so the live relay refuses them as unknown
commands. Evidence: `RelayModel::client_tags_missing_from_relay()` returns
exactly `["Signal", "Mole", "Power"]` under a regression test; EW-06 drives
the `Mole` and `Power` refusals, and the real-relay leg covers `Signal`.
`Power` also needs per-op seat routing rather than a single tag-level seat.
If burble adds an entry, the pin fails and the finding must be re-evaluated
rather than silently aging. These are cross-repository gaps, not defects in
the host validator.

### Hypotheses and predicate-only coverage (not claimed as demonstrated end-to-end)

- The fresh-seat rejoin-hello refusal (EW-03 input 8) is demonstrated at the
  predicate level offline; the async `idaptik_net::live` socket path is
  covered indirectly by loopback run 4's resync sequence, not by a dedicated
  hostile-rejoin socket test.
- Anything in §4's gap table: no claim is made about unimplemented modes.
- wasm32 behaviour of any native Evil Weevil ABI: out of scope and
  untested; the SNIF guest remains a pure numeric/buffer DSP kernel.

## 4. Deployment-mode authority, recovery, and gap table

The task asks the corpus to reason about four deployment modes. Only the
first exists; the rest are recorded gaps, never simulated.

| Mode | Implemented? | Authority rule | Recovery rule | Evil Weevil coverage |
|---|---|---|---|---|
| Single rendezvous relay (burble `game:` lane, no game-server-admin) | Yes | Relay enforces role table + `seq` only; it never parses command bodies and holds no sim state. Seats independently validate epochs (`Hello`) and schedules (`LockstepCore`); every seat runs the same deterministic sim and compares digests. | Peer-to-peer `net:resync` snapshot hand-off; the survivor prunes uncommitted peer commands on loss, the replacement reconstructs from snapshot + full event log. | **Demonstrated**: all six scenarios offline; relay rules additionally confirmed against a throwaway local burble by `evil_weevil_relay_check.sh`; resync over the real relay by `loopback_check.sh` run 4. |
| Direct P2P game transport | No | Hypothesis: epoch and watermark rules would move entirely to the seats with no relay de-duplication, so `seq` handling needs a transport-supplied ordering substitute. | Hypothesis: the existing resync payload is transport-agnostic and could survive, but nothing implements the signalling. | **Gap.** No code, no test; recorded here only. |
| Federated / multi-server | No | Hypothesis: cross-server authority would need an epoch-ownership rule that does not exist in any crate. | Hypothesis: undefined; snapshot hand-off has no cross-federation path today. | **Gap.** |
| Single authoritative game server | No | Hypothesis: the server would become the validator the local sim currently is; every invariant in §2 would need re-derivation against its acceptance surface. | Hypothesis: server-driven rejoin state; unbuilt. | **Gap.** ADR-0005 option 1 remains a recorded option, not a system. |
| Any mode with game-server-admin present | Not integrated | The `game-server-admin` repository is design-only/quarantined relative to this client; no principal in this corpus holds administrative power, and no permission alphabet grants it. | Not defined. | **Gap.** Every scenario explicitly records `game_server_admin_present = false`. |

Every scenario result serialises its `topology` (`SingleRendezvousRelay`) and
`game_server_admin_present` flag, and `tests/corpus.rs` asserts no scenario
silently claims another mode.

## 5. Reproduction

### Offline gate (Rust only; no sockets, no Elixir)

```sh
just evil-weevil-check
# equivalent:
bash scripts/evil_weevil_check.sh
```

It runs the crate's unit and integration tests, `clippy --all-targets -D
warnings`, the corpus binary, artifact-existence and secret-shape gates, and
a byte-identical determinism comparison across two runs. A missing cargo
fails the gate rather than skipping it.

Run the corpus directly and keep the artifacts:

```sh
cargo run -p idaptik-evil-weevil -- --out-dir target/evil-weevil/latest --seed 777001
# one scenario:
cargo run -p idaptik-evil-weevil -- --scenario EW-04-reconnect-obsolete-messages
```

Outputs: `corpus-report.json` (schema `idaptik-evil-weevil-corpus-v1`,
deterministic run id `ew-corpus-v2-seed-777001`) and one
`<scenario-id>/result.json` per scenario containing the exact proposed
intents with their raw wire payloads (the replay input side), the named
checks, observed effects, validated telemetry, observer report, metrics and
notes. The declared `replay_artifact` field in every scenario spec names
this same `result.json`, and the offline gate fails if any per-scenario
artifact is missing.

### Real-relay leg (optional; needs Elixir and a local burble checkout)

```sh
just evil-weevil-relay-check
# equivalent, with burble cloned beside this repository:
IDAPTIK_BURBLE_DIR=../burble bash scripts/evil_weevil_relay_check.sh
```

It builds the throwaway burble in `MIX_ENV=test` on its hard-coded isolated
test port (4002, loopback only), runs the three `#[ignore]` tests in
`crates/idaptik-net/tests/evil_weevil_relay.rs` with guest dummy identities
and a bounded frame count, and tears the relay down via an exit trap.
Missing mix, cargo, or burble fails the gate rather than skipping it. This
leg is deliberately not part of `just test-ghost`.
