# Handover — accessible physical tampering testbed (2026-10-25)

Status: **closed — inspection complete, no gameplay code written, handover
delivered.** This note exists because the authoring sessions for this request
kept running out of context. It says what was found in the tree, what the
request still needs, and what the next session should do first. Everything below
is either cited to a file or marked **proposed**.

The audit was taken at `aa28895` (2026-10-25, morning) and merged as PR #180.
`main` then moved a long way in the same day. **§0a is the postscript that
reconciles this note with `63b4364`** — read it before trusting any line below,
and treat §2–§5 as the record of what was true when the audit was taken, not as
a description of the tree you are looking at.

Read it with `AGENTS.md` open. That file's invariants ("gameplay truth lives in
`idaptik-core`", "the session layer is not in this repository", "no Python/Ruby",
"checks must be able to fail") are not repeated here, only assumed.

## 0. State of the branch when written

| Fact | Value |
|---|---|
| Branch | `arena/0639e4ae-idaptik` (merged to `main` as PR #180; not the branch you are on) |
| HEAD then | `aa28895` — "fix(ci): unblock fixtures-and-must past the documentation-claims gate (#161)" |
| Working tree | clean before this note; this note was the only change |
| Behind `origin/main` then | **1 commit.** `9680a7f` = PR #164, *docs: design the interacting building infrastructure* |
| Rust toolchain in the authoring sandbox | **absent** in every session of this thread (`cargo`, `rustc`, `just`, `nickel` all not on PATH) |

Two consequences the next session must absorb before touching code:

1. **Nothing in this session was compiled or run.** Every "implemented" claim
   below was read out of source (and, for unmerged work, out of fetched remote
   branches). No test was executed. Treat this as a reading of the tree, not a
   verification of it.
2. **Merge `origin/main` first.** `docs/architecture/building-infrastructure-design.md`
   (864 lines, from #164) is the most relevant existing design document for this
   request and it was *not* in this checkout — it was on `main`, one commit ahead.
   Its §5 interaction matrix and §8 truth/readout/knowledge planes are the
   scaffold this feature should reuse rather than reinvent. (This is done: the
   design document is in the tree now.)

## 0a. Postscript — the same day, against `63b4364`

`main` advanced from `aa28895` to `63b4364` in the course of 2026-10-25. Most of
what this note listed as a dependency, a blocker or a warning has changed state.
What follows is the delta, each line checked against the tree at `63b4364`.

**Landed, and it makes this work smaller:**

- **The power plane exists.** `scenario/power/` (`mod.rs`, `sim.rs`,
  `test_floor.rs`) is ADR-0016: `SourceDef`/`FeedDef`/`LoadDef`/`BreakerDef`,
  the one-line arbitration rule `operating = commanded AND energized AND intact
  AND not interlocked AND warmed up`, and `power::sim::OffReason`
  (`sim.rs:260`) reporting the *first failed rung* as a readable phrase —
  `Damaged`, `Disconnected`, `Interlock { label }`, `Unavailable { feed }`,
  `Isolated { breaker }`, `Unpowered { feed }`, `Restarting { ticks }`,
  `NoCommand`. That is the vocabulary Slice A needed, so **do not design a
  second one**. Hosted by the sim as `sim/power.rs`.
- **Water, gas and air exist.** `scenario/infrastructure/` (ADR-0013) — but see
  the wiring caveat below.
- **Moletaire is hosted.** `Command::Mole` (`command.rs:172`), `MoleOp`
  (`command.rs:122`), `MoleHostState` on `RuntimeState` (`state.rs:383`),
  `sim/mole.rs`, and the sim owns a private `mole: Option<MoletaireSim>`
  (`sim/mod.rs:83`), built by `build_mole()`. This **corrects §2.5 and §2.9's
  premise**: the companion is no longer a seam waiting for a host.
- **Evil Weevil exists**, twice: `crates/idaptik-evil-weevil` (the sans-IO
  adversarial corpus, ADR-0017) and `crates/idaptik-weevil` (the host adapter
  over the kernel's C ABI, plus a headless `idaptik-weevil-replay` binary). See
  `docs/dev-notes/weevil-integration.md`.
- **Durable storage exists.** `crates/idaptik-persist` (Tier 1, `redb`) — a store
  *beside* the tick for command log, checkpoints and replay-safe resume; it is on
  the `just test-ghost` list precisely so a run that fails to replay is noticed.
- **The ADR-number collision this note warned about is resolved.** The register
  is now: 0012 signal network, 0013 water/gas/air, 0014 consequences as
  observations, 0015 deployment topologies, 0016 electrical, 0017 Evil Weevil,
  0018 infrastructure interaction rules. The next free number is **0019**.
- **Every PR in §4 merged.** #162→#181, #163→#177, #164→#164, #165→#165,
  #166→#166, #167→#167, plus #175 (persist), #184 (guard brain) and #173 (the
  DEED/K9 metadata migration). #165's disposition was argued in
  `docs/reviews/2026-10-25-pr165-vs-main.md`; its design documents were kept
  beside #167's code rather than folded into it. The "work in flight" framing in
  §4 is now history: keep it for what each PR contained, not for what is pending.
- **The metadata migration.** `.machine_readable/descriptiles/STATE.a2ml` is
  retired. Project state now lives in `IDApTIK_chora.deed`, contractile policy in
  six K9 components under `.machine_readable/contractiles/`, and agent invariants
  in `coordination.k9`. Any reference to `STATE.a2ml` in this note is a reference
  to a file that no longer exists — including the one in the §2.5 table.

**Still true, and now blocking rather than pre-emptive:**

- **The script vocabulary is still the gap in §2.8, but narrower than written.**
  `script.rs::press_command` gained a `mole:*` family (`mole:dig`,
  `mole:sabotage`, `mole:synth:<index>`, `mole:move:<x>[:under]`,
  `script.rs:102`). There is still **no token for `Command::Signal`,
  `Command::Power`, `NetSsh` or `NetHack`.** M0's script-token task in §6 stands,
  and now has a precedent to copy.
- **No panel, tamper or serviceability code exists.** `grep -ri 'PanelOpened|
  PanelClosed'` is empty; the builtin immutability rule is still one comparison,
  now at `signal/sim.rs:530` (`if self.state.links[i].builtin { return
  Err(SignalDenyReason::Hardwired) }`) — the line reference in §2.9 and §5
  ("~515") is stale, the rule is unchanged. So M1 is untouched and still the
  right first step.
- **No device revision on any command.** Unchanged.
- **The wiring caveat, and it is the big one.** `scenario/infrastructure/`
  and `scenario/power/` are in the tree, but power is not driven on any shipped
  floor and the infrastructure runtime is not called from the game tick — see
  open issue **#192** ("the off-reason reaches no player, and no shipped floor is
  powered") and `docs/reviews/2026-10-25-pr165-vs-main.md` ("no
  `system_infrastructure`, no signal bridge, no `BuildingRuntime` call site").
  **A tamper slice that assumes a live power simulation would be built on a
  module nobody runs.** M2's first task is therefore "give the corridor a powered
  floor", not "use the existing power".
- **Open, and they touch this work:** issue #192 above; #196 (electrical id
  domains + current-base Rust integration checks); #197 (PR #186 recovery
  integrity); #198 (branch pruning, owner-approved); #191/#188 (EW-06 assumes an
  observation Billy cannot make). Read #192, #196 and #191 before adding state
  to `scenario/power/`.

**One correction to the request's own vocabulary, worth keeping:** the request's
"distinguish removed fuse / protective trip / disconnected supply / broken load"
maps almost exactly onto `OffReason`'s eight rungs. Use them; do not add a
parallel fault enum.

## 1. What the request is, in one paragraph

A small playable maintenance-corridor testbed where a player physically
approaches a device, opens its panel with an appropriate tool, makes one small
understandable change, closes the panel, and then learns what happened by playing
normally — while NPCs observe, misinterpret, gossip, and repair on their own
partial knowledge. It must not become a technician's exam or a separate hacking
minigame, and it must keep the existing separation between network reach,
control signals, electrical power, physical access, and actor knowledge.

First deliverable: one corridor, a switch and light, a controlled door, a camera,
a serviceable panel, two NPCs of different familiarity and competence, and five
scripted demonstration beats (unnoticed change, visible fault, competent repair,
misleading inference, later correction).

## 2. Implemented vs. absent — the six inspection areas

The request's "FIRST INSPECT" list, answered from source. Status vocabulary:
**implemented** (code + tests in this repo), **partial** (exists but not the
thing the request needs), **absent** (no code).

### 2.1 Netsim reach and device actuation — implemented, reused as-is

- `crates/idaptik-core/src/netsim/` — `GroundedGraph` (segments, nodes, DNS,
  vantages), segment-access reachability, ping/traceroute, `AgentSession` with
  pivot stack and trace clock, `Effect` + power cascade over `deps`.
- `crates/idaptik-core/src/scenario/floor_graph.rs` — the *derived* per-floor
  network: a `SmartDoor` node per door, `Light` per room, `Camera` per camera,
  the vacuum, one `Inside` vantage per room, plus an authored backbone.
  Commitment stated in its docs: *only the backbone is authored; every fixture is
  derived*, so the graph cannot drift from the definition.
- Actuation per node lives in `netsim/graph.rs` (`Actuation`), so cut-power,
  disengage-lock and valve-style verbs already have a typed home.

**Relevance:** the "remote must not remove a physical fuse" rule is already
structurally true — reach only picks which `Actuation`s are offered. Do not add a
second actuation path for panels.

### 2.2 Scenario/signal definitions and the deterministic runtime — implemented

- **Definition-as-data + fixed 60 Hz tick.** `GhostLobbySim` in
  `scenario/sim/mod.rs`; `TICK_DT` in `scenario/mathf.rs`; `fold()` in
  `scenario/command.rs` collapses an ordered `Command` stream into one
  `TickInput` (immediates before systems). Run = pure function of
  `(definition, config, seed, command stream)`.
- **The wire API.** `Command` (`scenario/command.rs:115`), tagged
  `{"cmd": ...}`, with round-trip tests *in the enum's own test module* — a new
  variant that does not survive the wire is not a command. `Event`
  (`scenario/event.rs`), 60+ variants, tagged `{"event": ...}`;
  `Event::Signal { signal }` at `event.rs:73` already carries signal-network
  events into the canonical log.
- **Snapshots.** `scenario/snapshot.rs`, format
  `idaptik-ghost-lobby-runtime-v3`; `RuntimeSnapshot` carries definition, config,
  seed, tick, RNG, state, paused, validation.
- **Signal network (ADR-0012), fully landed.** `scenario/signal/mod.rs`:
  `SignalKind` with a fixed port table (`outputs()`/`inputs()`), `LinkDef`
  (`mod.rs:215`, with `builtin: bool`), `SeatProfile` (`mod.rs:241` — bandwidth
  vs `busy_ticks`, `may_reprogram`, `may_wire_hacker_only`), wire `budget`
  (`mod.rs:294`). `scenario/signal/sim.rs`: `SignalCommand` (wire / cut / invert
  / reprogram, `sim.rs:114`), `SignalDenyReason` (`sim.rs:154`), `apply()` at
  `sim.rs:371`, `step()` at `sim.rs:376`, snapshot `idaptik-signal-snapshot/1`.
  Tick law: outputs are read at tick start, inputs applied after, so a signal
  crosses exactly one wire per tick.
- **Host integration.** `scenario/sim/signal.rs`: `signal_access()` (line 38)
  computes per-seat `Access`; `signal_readout()` (line 87) turns world facts into
  host-owned outputs; busy hands are `RuntimeState::signal_busy`
  (`state.rs:291`), charged in `sim/signal.rs:206` and denying with
  `SignalDenyReason::Busy` (`sim/signal.rs:183`).

### 2.3 Physical building circuits and portals — implemented, but a *separate* runtime

`scenario/building/` has `BuildingDefinition` (floors with `FloorKind::{Standard,
Billy, Service}`, rooms, `PortalDef` of kind Door/Stair/Lift/Ladder/Vent with
`travel_time`, optional `lock { controller }`, optional `circuit`; lifts must
declare a circuit), `CircuitDef`, `ZoneDef`, Dijkstra reachability
(`building/graph.rs`), `BuildingSim` (`building/sim.rs`) with
`TraversalDenied { UnknownPortal | NotAdjacent | Locked | Unpowered }`, and the
canonical 4-floor `exchange_house()` fixture with committed goldens.

**The catch, stated in #164 §1.2:** `BuildingSim` and `GhostLobbySim` are *two
runtimes sharing a definition projection*. A building that embeds the Ghost Lobby
has two independently derived networks. There is no single tick that plays
building rooms, scenario actors, power and signals together. This request wants a
corridor that is simultaneously a physical space, a power circuit, a signal path
and an NPC-perception problem, so **the runtime question is the first design
decision, not an implementation detail.**

Also relevant: `PortalKind::Vent`/`Ladder` exist and Moletaire's depth model
exists, but no fabric/duct model exists (see #164 §1.3) — so "Moletaire reaches
service spaces" is currently a *route* concept only.

### 2.4 Bevy wiring, screen interaction, art rules — implemented; panel UI absent

- `crates/idaptik-bevy/src/driver.rs` steps the sim at 60 Hz in `FixedUpdate`.
  `scene.rs` renders the side-on 2.5D cross-section; `SceneMap::row_lift` stacks
  floor rows (`ROW_STACK = 440.0`). `tests/parity.rs` proves a scripted Bevy app
  emits exactly the events a headless run does.
- `wiring.rs` is the only existing "panel" surface, and it is **screen work**:
  drawn on a console's schematic, mouse gestures become `Command::Signal`, colour
  follows the seat law, hardwired runs are grey, untouchable devices are dimmed.
  Its module docs cite **art rule 5**: floating UI stays off the infiltrator's
  world.
- `net_view.rs` is the grounded-network view; clicks become `NetSsh`/`NetHack`.
- `keymap.rs` has movement/interact (E), hacker digits 1–4, P/G/X pivots, C
  overlay, N net view, Tab pause, R restart. Nothing for panels, because there
  are no panels.

**So:** the request's art rule ("local panel interaction belongs to the physical
device; remote work belongs on a character screen") is already the repo's rule —
`wiring.rs` is the remote half and there is *no* physical-device half yet. The
new work is the world-side panel interaction and a local detail view, and it must
not become a floating building overlay.

### 2.5 Inventory, tools, interaction, busy time — mostly **absent**

| Thing the request assumes | Reality |
|---|---|
| Inventory | No inventory system in `idaptik-core`. `PlayerState` carries `has_note`, `has_usb` (`state.rs:48`), nothing else. |
| Tools (screwdriver, keys) | None. No tool items, no tool-gated interaction anywhere. |
| Moletaire's equipment | Exists as a *single-slot* enum, seven items (`companion/equipment.rs`): Skateboard, Glider, BatteringRam, Beacon, Rucksack, FlashCamera, PlasmaCutter. No screwdriver, no fuse. Five-slot coprocessor bay in `companion/coprocessors.rs`. |
| Moletaire wired into play | Was **not** wired when this audit was taken — the status source said `tested-seam`. **Changed on 2026-10-25:** the companion is now hosted (`Command::Mole`, `MoleHostState`, `sim/mole.rs`) — see §0a. Its `MoleCommand::SabotageCable` / `MoleEvent::CableSabotaged { cable_id: String }` still carry an **opaque string no circuit knows** (#164 §1.2), and that part has not changed. |
| Busy / occupied hands | Exists only for wiring: `signal_busy` + `Disallow Busy`. Also `PlayerState::interaction_progress` and `lockpick` for the note and front door. There is no general "work in progress on a device" object, no interruption model, no "someone else is working here" model. |

**Implication:** fuses "as identifiable components/items where the inventory
system supports this" cannot be taken literally today — there is no inventory to
support it. Either (a) add a minimal typed component/carry slot to
`PlayerState`, or (b) keep components *in the device* and model only remove /
carry-in-hand / replace with no general inventory. (b) is smaller and matches
"keep costs proportionate"; (a) is the honest reading of the request but is its
own slice. **This is decision D2 below.**

### 2.6 Camera perception and displayed feeds — implemented, and closer to the request than expected

- Sweep and detection: sinusoidal sweep per camera (`CAM_SWEEP_*`), detection
  gated in `sim/systems.rs` (~line 1235) on room match, `stale`, looped,
  dead-node, and signal-blindness.
- **Looping already exists as a hacker uplink, not as a panel mod.**
  `RuntimeState::camera_looped: Vec<f64>` (`state.rs:269`) holds remaining seconds
  (`CAM_LOOP_DUR = 4.0`, `constants.rs:209`), set by
  `Effect::CameraLooped` in `scenario/agents.rs:124`, and a looped camera cannot
  flag anyone (`systems.rs:1245`). The comment there is exactly the separation
  the request asks for: *"a looped camera is showing the operator yesterday's
  corridor, and an unpowered one is showing nothing at all."*
- Displayed feed: `scenario/console.rs` — `ScreenView`/`ScreenDevice`, per-camera
  status `OFF` / `LOOPED {n}s` / `STALE FEED` / `LIVE`
  (`sim/console.rs:306`), gated by console capability. `Intel` (`console.rs:46`)
  holds building-logged fixes with a time; **the module law is "people are never
  shown directly."** Bevy draws the cone, and dims/invalidates it when looped
  (`cones.rs:168`).

**Gap against Slice B:** there is no *recording* object and no *recorded source*
requirement — the existing loop is a fixed 4-second duration with no origin, no
age, no provenance. There is also no camera orientation state to redirect (sweep
is a pure function of time and the authored range), and no health/status report
separate from `dead_nodes`. All of that is new, but it lands on top of a
well-formed detection/display pipeline, which is why Slice B is cheap *after*
the tick-integration question in §2.3 is answered.

### 2.7 Actor interests, evidence, familiarity, communication — partial

- **Interests.** `scenario/actor/mod.rs:82` `InterestProfile` — per-object
  attention with a `value_signal` economy (`ObjectClass`, `Leakage`), shared
  arithmetic in `actor/belief.rs`, archetypes + ordered `Modifier`s as data in
  `actor/actors.json` (Billy is the default archetype, projected from
  `constants.rs`).
- **Hypotheses and evidence.** `scenario/vsm.rs`: `Hypothesis`,
  `HypothesisLedger` (`vsm.rs:77`), `EvidenceLedger` (`vsm.rs:90`) —
  Dempster–Shafer-style fixed-point masses, plus an opt-in supervisor
  (`RunConfig::supervised`, `RuntimeState::supervision`) that allocates team
  attention and bends Billy's patrol band. Gated by
  `TeamAttentionAllocated` / `CoverageRetargeted` announce-once events.
- **Communication.** One event: `Event::BossCalled { reported }`. Billy calls his
  boss once. There is no channel model, no recipient model, no failure-to-deliver.
- **Familiarity.** No per-location or per-device familiarity. Competence is 29
  scalar archetype stats; nothing distinguishes "this is my kitchen" from "this is
  somebody's office."

**Gap:** the request's five separate stages (observe → notice → interpret →
choose → communicate) collapse today into "Billy's interest meter crosses a
threshold and he forms a belief about note-vs-USB". Familiarity, workload,
opportunity-to-observe, expectations, tool authority, and received-from-others
are all absent. The request explicitly says *not* to use one intelligence number
— so this is new state, but it should be **new fields on the existing archetype
and evidence planes**, not a replacement cognitive system.

### 2.8 Multiplayer, validation, replay, snapshots — implemented; device revision absent

- Relay is external (`metadatastatician/burble`, pinned); this repo holds the
  client and the sans-IO lockstep core. `idaptik-net/src/envelope.rs`:
  `seat_for()` (line 69) routes every command variant by seat — **including
  `Command::Signal`, via `op.seat()`** — `scripted_sender()` (line 94), relay
  `seq` dedupe, `at` lockstep scheduling, watermark commits, `Resync` (snapshot +
  log + committed-but-unexecuted), `NET_PROTO = 2`.
- Validation today is *actor + access + skill + budget + busy*, all inside
  `SignalSim::apply`. **There is no device revision/generation number on any
  command** (`grep -ri revision` in `idaptik-core` returns only prose). So
  "validate the current device revision when a modification completes" is new
  state, and it is the mechanism that must arbitrate simultaneous player/NPC work.
- Replay: `idaptik-tui/src/headless.rs` (script → JSON `event_log` + `debrief` +
  `final_snapshot`) and `replay.rs` (run twice, compare, optionally match a
  recorded log).
- **Sharp gap:** the script vocabulary (`idaptik-tui/src/script.rs`,
  `press_command`) had **no token for `Command::Signal`, `NetSsh` or `NetHack`.**
  Headless replay therefore cannot exercise wiring, let alone panel work. Any new
  panel command needs a script token, or the "headless replay and meaningful
  tests" deliverable is blocked on it. **Partly fixed on 2026-10-25:** a `mole:*`
  token family landed (`script.rs:102`); signal, power and net verbs still have
  none. See §0a.

### 2.9 The immutability rule the request asks about — found

> "Do not silently make all existing builtin signal links removable. Inspect the
> current immutability rule and introduce explicit physical
> serviceability/tampering permissions with backwards-compatible defaults."

The rule is exactly one field: `LinkDef::builtin: bool`
(`scenario/signal/mod.rs:215`), enforced in `SignalSim::cut` —
`if self.state.links[i].builtin { return Err(SignalDenyReason::Hardwired) }`
(`sim.rs`, ~line 515). `builtin` is set `true` only by the fixture's
`hardwired()` helper (`signal/ghost_lobby_wiring.rs:52`) and `false` for
player-laid wires (`sim.rs:106`).

So the extension is small and well-shaped: add an authored
`serviceable: bool` / `tamper_permission` on the link (default **false**, so
every existing fixture keeps byte-identical behaviour and every committed golden
still passes), and let a panel-level command cut a serviceable builtin. Two
things to keep straight:

- "Serviceable" is a property of the *installation*, not of the actor. Access,
  tool and competence are separate checks (as `Access` + `SeatProfile` already
  are). Do not fold them into one flag.
- Restoring a cut builtin is **not** the inverse of cutting it: the request wants
  "restore service" and "restore exact original wiring" to be different outcomes,
  and repairs must not erase evidence or knowledge.

## 3. Load-bearing rules any implementation must satisfy

Read off the tree, in the order they will bite:

1. **New state must be serde-defaulted and absent-when-empty**, or every
   committed golden and byte-identical fixture breaks. The established pattern is
   `#[serde(default, skip_serializing_if = "…")]` — see `RuntimeState::signal`
   (`state.rs:279`), `camera_looped`, `signal_busy`, `using_console`, `intel`.
   The test gates that enforce this are `golden_reset`, `snapshot_equivalence`,
   `replay_determinism`, `json_roundtrip`.
2. **Every new `Command` variant must round-trip on the wire**, with a test in
   `command.rs`'s own module, and must be added to
   `idaptik-net/src/envelope.rs::seat_for` and (if scriptable)
   `script.rs::press_command`. `idaptik-bevy/tests/parity.rs` will catch a
   frontend that disagrees.
3. **Thin frontend.** No render-side state feeds simulation. Panel UI emits
   commands and reads readouts; nothing else.
4. **Determinism.** `BTreeMap`/`BTreeSet` or sorted `Vec` everywhere iterated;
   no RNG except the seeded stream; no wall clock; integer or fixed-point for any
   confidence/mass value that must replay exactly (see `vsm.rs` basis points).
5. **Three planes, never mixed** (#164 §8): *truth* (sim state, written only by
   systems and validated command application), *readouts* (pure projection,
   computed on demand, never stored), *knowledge* (`Intel` fixes, belief meters,
   session logs — the only plane anything may falsify). The developer view the
   request requires is literally a three-column view of these planes; do not
   invent a fourth.
6. **Art rule 5 / seat law.** Physical panel work is world-side; remote work is
   screen-side; no floating omniscient overlay by default.
7. **Toolchain discipline.** No Python/Ruby/JS anywhere, including scripts and
   workflows. Commits are `git commit -s` (DCO). Do not edit
   `docs/PROJECT-ASSURANCE-PROFILE.adoc` labels as a side effect.
8. **Checks that can fail.** `just test-ghost`, `just config-scenario-check`,
   `scripts/doc_claims_check.sh`, and `scripts/must_check.sh` (a probe whose tool
   is missing **fails** — so `must_check.sh` will report failures in a sandbox
   without cargo, by design).

## 4. Work in flight that this feature had to reconcile with

*Historical. When this audit was taken, five PRs touching the same ground were
open and were fetched and read. All of them merged the same day (see §0a). The
table is kept because it records what each contained at the time, which is what
the reconciliation argument was built on.*

| PR | Branch | What it actually contains | Relation to this request |
|---|---|---|---|
| [#162](https://github.com/metadatastician/IDApTIK/pull/162) | `arena/e86a742d-idaptik` | Electrical infrastructure in core: `SourceDef`/`FeedDef`/`LoadDef`/`BreakerDef`, one rule `operating = commanded AND energized AND intact AND not interlocked AND warmed up`, off-reasons rendered as one sentence | **The power plane this request needs.** "Distinguish removed fuse / protective trip / disconnected supply / broken load" is nearly this PR's `Condition` + breaker vocabulary |
| [#163](https://github.com/metadatastician/IDApTIK/pull/163) | `arena/9befe5c4-idaptik` | ADR-0013 *consequences as observations*; `scenario/infra.rs` (water zones, alarms incl. a tamper circuit whose trip **is** watched-node power loss), a hosted Moletaire, `Command::Mole`, building audit, `tests/infra_observations.rs` + `tests/challenge_set.rs` | **Closest existing work.** Its observation-only actor perception is the rule this request restates. WIP, CI used as compile loop, mergeable UNKNOWN |
| [#164](https://github.com/metadatastician/IDApTIK/pull/164) | — | **MERGED** (`9680a7f`): `docs/architecture/building-infrastructure-design.md`, `NodeId` identity plan, interaction matrix, tick contract, three test levels, staged plan | The design of record to extend, not duplicate |
| [#165](https://github.com/metadatastician/IDApTIK/pull/165) | `arena/df59c446-idaptik` | Docs only: **another** ADR-0013 (`0018-infrastructure-interaction-rules.md`), `infrastructure-interactions.md`, a 32-rule catalogue, two levels ("Sump", "Sable"), M0 acceptance tests | Explicitly says no Rust exists and no test named in it exists |
| [#166](https://github.com/metadatastician/IDApTIK/pull/166) | `arena/cb226ff1-idaptik` | `idaptik-evil-weevil` crate: five deterministic adversarial scenarios over session/movement/epoch/reconnect/observation rules, four permission-separated realms | The request's "integrate Evil Weevil only where its adapter exists" — **it does not exist on `main`**; it is this PR. ADR-0013 too |
| [#167](https://github.com/metadatastician/IDApTIK/pull/167) | `arena/a7c1a66b-idaptik` | Water/gas/air runtime in `scenario/infrastructure/` | Out of scope for this request beyond noting it claims the same module namespace |

**Two warnings** — both were accurate when written; the first has since been
resolved.

1. **Three unmerged PRs each claimed ADR-0013** (#162 title, #163
   `0013-infrastructure-consequences-as-observations.md`, #165
   `0018-infrastructure-interaction-rules.md`, #166 title). `docs/adr/README.md`
   says "take the next number" and the next free number was 0013. **Resolved on
   2026-10-25** by renumbering: 0013 water/gas/air, 0014 consequences as
   observations, 0015 deployment topologies, 0016 electrical, 0017 Evil Weevil,
   0018 infrastructure interaction rules. The next free number is **0019**, and
   `IDApTIK_chora.deed` carries the register that must be updated with it.
2. **Do not stack a 5,000-line WIP branch under this feature.** #162/#163/#165/
   #167 all rewrite or claim `scenario/` module space and are BLOCKED/UNKNOWN
   for merge. Build the tamper slice on `main` + #164's design doc, and reuse
   #162's power vocabulary and #163's observation rule *as references* when they
   merge.

## 5. Gap analysis against the request, in one table

*The "Today" column is as of the audit (`aa28895`). §0a lists everything that
moved afterwards; the verdicts below survived it except where §0a says so.*

| Request | Today | Verdict |
|---|---|---|
| 6-step interaction sequence (approach → inspect → open → modify → close → observe) | Only step 4 partially, and only for player-laid wires from a console screen | **absent** |
| Opening a panel ≠ changing internals; opening leaves evidence | No panel object, no open/closed state, no evidence of opening | **absent** |
| Understandable actions: disconnect/reconnect, move between terminals, fuse, setting, attach mod, restore known config | wire / cut / invert / reprogram exist as ops; the rest absent; "restore a known previous configuration" needs recorded provenance that nothing has | **partial** |
| Context-sensitive descriptions, local diagrams, progressive detail | `wiring.rs` draws ports on a screen schematic; no local panel detail view | **partial** |
| Keyboard/controller access, cues beyond colour | keymap is keyboard-only; `wiring.rs` uses colour for seat law and dimming | **partial** |
| Physical access vs tools vs time vs competence vs network reach as separate requirements | `Access` (reach) + `SeatProfile` (skill/busy) are separate; tools do not exist; physical access is "which room you stand in" | **partial** |
| Remote cannot remove a physical fuse; screwdriver ≠ network access | Structurally true by construction (no cross-path exists) — worth a test that keeps it true | **implicit** |
| Serviceability permission with backwards-compatible default | `builtin` is absolute; one-line rule at `sim.rs` ~515 | **designed, unimplemented** |
| Slice A: switch + light, door control; separated layers (physical state / outgoing signal / requested state / power+fault / actual state) | Signal layer exists; power/fault/actual-state split is #162's; switch-as-physical-object absent | **absent** |
| Slice B: camera orientation within authored limits; loop/replay from a **recorded source**, with origin and age in truth | loop exists as a 4 s uplink effect with no origin/age; no orientation state; no recording | **partial / wrong shape** |
| Slice C: internal telephony exchange, routes, calls, delivery outcomes affecting NPC comms | `Event::BossCalled { reported }` only | **absent** |
| Slice D: fuses as components, selective isolation, four distinct failure appearances | `CircuitDef` + `dead_nodes`; no fuses, no breaker | **absent** |
| NPC stages observe → notice → interpret → choose → communicate, with familiarity/workload/opportunity/expectations/tools/authority/hearsay | Interest meters + DS evidence ledger + one boss call | **absent as staged model** |
| Noticing ≠ accusing; repair ≠ erasing knowledge | Nothing to violate yet — keep it that way | **n/a** |
| Repair and counterplay with real time/resources, no magic config restore | no repair behaviour at all | **absent** |
| Provenance-bearing hypotheses about the player's plan; report de-duplication; belief revision | `HypothesisLedger` + `EvidenceLedger` machinery exists but is fed by Ghost Lobby events and answers note-vs-USB | **partial infrastructure, different question** |
| All modifications = validated, versioned commands/events; no direct state mutation | true of all existing commands; **no device revision** | **partial** |
| No global undo that rewinds NPC knowledge | no undo machinery exists | **n/a** |
| Developer view: truth vs readouts vs beliefs | `RunConfig::supervised` telemetry is thin; WORKPLAN line "Expose developer telemetry in TUI/replay output" is still unchecked | **absent** |
| Headless replay + meaningful tests | headless/replay exist; **script vocabulary cannot express `Signal`/`NetSsh`/`NetHack`** | **blocked on one small change** |

## 6. Proposed plan for the next session

Ordered so that each step is independently mergeable and the corridor becomes
playable as early as possible. **Proposed**, not agreed. Definitions of done are
written so a check can fail.

*Progress since: M0's first bullet is done (merge upstream — five PRs landed);
M1 is untouched; M2's power task changed shape because the power module exists
but is not yet driven by any shipped floor (see §0a and issue #192).*

### M0 — reconcile and unblock (half a session)

- ~~Merge `origin/main` into the session branch.~~ Done on 2026-10-25.
- Read `docs/architecture/building-infrastructure-design.md` §5 and §8; decide
  whether the corridor lives in `GhostLobbySim` (scenario) or in a new
  consolidated runtime (#164 §7.2 sketches one). **Recommendation: build the
  slice in `GhostLobbySim`'s world with the signal network as the control plane,
  and do not introduce a third runtime.** The corridor is one floor; the
  consolidating runtime is a separate, larger decision (#164 §13 defers it).
- Extend `idaptik-tui/src/script.rs::press_command` with tokens for
  `Command::Signal` (e.g. `signal.cut.<from>.<port>.<to>.<port>`) and the net
  verbs. **Done when** a headless fixture exercises a cut and a re-lay and
  reproduces byte-identically twice, with the existing four fixtures unchanged.

### M1 — the panel as an object, and the permission that makes it serviceable

New typed state, all defaulted-absent so goldens are untouched:

- `PanelDef` / `PanelState`: stable component id, installation id
  (`SignalId`/node join), `access_method` (tool tag or "unlatched"),
  `serviceable: bool` and per-terminal tamper permission, open/closed, revision
  counter, per-component installed/removed state.
- `Command::Panel { op }` mapped in `seat_for`; events `PanelOpened`,
  `PanelClosed`, `ComponentChanged { revision }`, `TamperedDenied { reason }`.
- Extend the builtin rule: `LinkDef` gains `serviceable` (default `false`),
  `SignalDenyReason` gains a panel-path variant distinct from `Hardwired`, so the
  two denials stay legible and testable separately.
- **Done when** (a) every existing fixture is byte-identical, (b) a test proves a
  cut is refused on a non-serviceable builtin *from a panel* and allowed on a
  serviceable one, (c) opening a panel with nothing changed emits an
  open/close event pair and no state change, and (d) a stale-revision command is
  refused.

### M2 — Slice A: switch/light and door control with the five layers separated

Model the layers explicitly, since the request's whole point is that they are not
one value:

| Layer | Home |
|---|---|
| switch physical position | panel/device state (`truth`) |
| outgoing control signal | `SignalState` wire level |
| receiving device's *requested* state | signal-driven input latch |
| power and fault | `scenario/power/`'s `OffReason` (`sim.rs:260`) — its eight rungs are exactly the distinctions the request asks for |
| actual physical state | sim truth the renderer draws |

The UI rule from the request — *"must not claim a door opened if its actuator
could not operate"* — is a readout rule: the panel shows *requested*, the world
shows *actual*, and the discrepancy is where the player learns something. The
sentence generator already exists (`OffReason::phrase()`); reuse it rather than
writing a second one.

**The live caveat (issue #192):** `scenario/power/` is not driven by any shipped
floor, and `OffReason` reaches no player yet. A corridor that wants a light to go
out *because the feed is dead* has to be the first floor that actually powers
one, or the slice will be demonstrating a sentence nobody prints.

- **Done when** the corridor's switch drives its light through one wire, power
  removal makes the light *not* light while the switch still shows *on*, a
  conflicting command (two sources, one input) resolves by a documented rule
  and emits a typed event, and all of it replays.

### M3 — the NPC pipeline, staged (this is the heart, not an add-on)

Five separate steps, each with its own state, so the demonstration beats fall out:

1. **observe** — reuse the existing perception (sight line, hearing threshold,
   room boundary). Nothing new sees through walls.
2. **notice** — per actor × per installation `familiarity` (high at home,
   localised in a big building) × `workload` × `expectation`; a threshold, not a
   dice roll.
3. **interpret** — composes *reason for the anomaly* from `Hypothesis` masses;
   may be wrong; carries provenance. De-duplicate reports by originating event id
   before they reach the ledger (`EvidenceLedger` currently accepts what it is
   given).
4. **choose response** — ignore / retry the control / close the panel / restore
   the connection they understand / replace a compatible missing part if they
   have one / call maintenance / isolate the device / post a guard / take another
   route. Each is a real action with a duration.
5. **communicate** — the channel model that Slice C will fill in. Until then, one
   channel: face-to-face. An actor who has not been told does not know.

- **Done when** the corridor's two NPCs (one high-familiarity, one
  high-technical-competence but new here) produce different observable behaviour
  from the *same* fault, no NPC ever names the player as the cause of a fault,
  and a completed repair leaves the repairing NPC's knowledge intact (a test that
  asserts the knowledge record survives the repair).

### M4 — developer view (small, high leverage, do it early)

Three panes over one tick: **truth** / **readouts** / **knowledge**, plus the
hypothesis table with masses and provenance ids. TUI first — it already has
`headless`, `replay`, `export` and the WORKPLAN item is open for exactly this.
This is also how the demonstration beats get eyeballed before any Bevy UI exists.

### M5 — Slice B (camera): orientation, recording, loop, provenance

- Orientation state bounded by authored mechanical limits; readable as a
  direction, not a slider to a number.
- A **recording** object with `started_at`, `source_camera`, `frames` duration,
  `age`; the loop mod requires one, and the panel says which recording it would
  replay and how old it is.
- Truth records loop origin + age; NPCs do **not** get that automatically.
- Define affected paths explicitly (feed-to-viewer yes; independent sensor,
  recording path and alarm no).
- **Done when** looping a feed leaves detection, alarms and a second independent
  sensor untouched (a differential test, per #164 §8's compromised-sensor
  pattern), and an inconsistency is discoverable by comparing two observations.

### M6 — Slices C and D (telephony, fuse box)

Deferred by the request itself: "Implement that slice before adding the telephone
exchange and fuse box." Keep them out of M0–M5.

## 7. Draft device/action/observation matrix — Slice A (proposed)

The request asks for a device/action/observation matrix as a deliverable. This is
a first cut to argue with, scoped to Slice A. Columns are deliberately the ones
the request's rules need; "evidence" is *what a witness could later know*, not
what the player did.

| # | Component | Action | Truth changed | Immediate readout | Evidence (provenance) | Who can | Cost | Deliberately not claimed |
|---|---|---|---|---|---|---|---|---|
| A1 | panel cover | open | `open = true`, revision+1 | cover open on the wall | `PanelOpened { by, installation }` — visible to anyone in the room | infiltrator in reach; mole if service-side; hacker **cannot** | time; hands busy; exposed | that anything inside changed |
| A2 | switch (labelled pair) | disconnect | wire absent | light stops following the switch; switch still moves | `ComponentChanged` + a *missing link* record | panel opener with serviceable link | time | that the light failed *because* of the switch |
| A3 | switch → light | reconnect at the correct compatible terminal | wire restored | light follows the switch again | inverse provenance, **new** timestamp — not a rewind | panel opener | time | restoration of the *original* wiring if a different terminal was used |
| A4 | door-control terminal | move the connection to the door's `Pulse` input | requested state changes for `PULSE_TICKS` | door opens then closes | the door's own `Open`/`Closed` host output | panel opener | time | that the door's actuator can operate — power is separate |
| A5 | door actuator | (power removed upstream) | actual state cannot change | requested "open", actual "closed" | `DoorHeld` denied / off-reason sentence | hacker (reach), NPC | — | that the door is open |
| A6 | fuse (Slice D preview) | remove | circuit de-energised | the load's room darkens | `FuseRemoved { circuit }` | panel opener with matching fuse | time | that it was sabotage rather than a trip |
| A7 | any | close panel | `open = false` | cover shut | `PanelClosed` | same | negligible | erasure of A1..A6 evidence |

Cross-cutting rows the matrix needs once NPCs are in (M3): *observe* (who was in
the room, with what line of sight), *hear* (panel work is quiet; a removed fuse is
not), *infer* (a hypothesis about intent, carrying its supporting event ids, and
allowed to be wrong), *repair* (restores service, never the exact previous
configuration, and never deletes knowledge), *report* (a fault can be reported by
someone who cannot diagnose or fix it).

## 8. The corridor's five demonstration beats, and what produces each

The first deliverable names five beats. Each is a *test* first and a scenario
fixture second.

| Beat | Mechanism | Assertion |
|---|---|---|
| 1. A modification that goes unnoticed initially | A2/A4 on a low-traffic installation, no witness in room, no downstream anomaly | no NPC observation references it within N ticks; truth did change |
| 2. A visible fault noticed without diagnosis | A5 power removal making the light dead while the switch reads "on" | the resident NPC forms an *anomaly* observation and **no** cause hypothesis; a visitor in the same room does not |
| 3. A competent inspection and repair | NPC with tools + competence opens the panel, restores A3 at a compatible terminal, closes it | service restored; the NPC's knowledge retains both the anomaly and the discovery; **no** claim of restoring the original wiring |
| 4. A misleading inference about the player's route | selective failure pattern (e.g. camera feed + a door hold) supports "someone wants the east stair unwatched" | a hypothesis exists with the supporting ids; **wrong** relative to the scripted route; the player was never named |
| 5. A later correction from new evidence | a contradictory observation (a fix in a room the hypothesis excluded) arrives through an available channel | old mass decreases, new hypothesis dominates, and the earlier report is *not* double-counted |

Beat 5 is the one that will expose a real defect in `EvidenceLedger` if one
exists, because the ledger currently has no report-deduplication and no
channel-gated spread. Check that before building on it.

## 9. Open questions that need an owner ruling, not an agent guess

- **D1 — runtime.** Corridor inside `GhostLobbySim`, or wait for #164 §7.2's
  consolidated runtime? *(Recommendation above: `GhostLobbySim`.)*
- **D2 — components.** Minimal typed carry slots on `PlayerState`, or
  components stay in the device and "carrying" is a single hands slot?
- **D3 — tools.** Where do tools come from? There is no inventory and Moletaire's
  seven equipment items have no screwdriver. Options: add a `Tool` item class to
  `PlayerState` (small), reuse Moletaire equipment (needs the mole hosted first),
  or make access method a *capability* on the actor archetype rather than an item
  (cheapest, least "physical").
- **D4 — merge order for #162/#163/#165/#167.** This feature's power vocabulary
  and observation rule both want to be #162's and #163's, not new ones. If those
  land first, this work gets smaller. If they stall, this feature has to
  restate a small version of both — which is the thing to avoid duplicating.
- **D5 — ADR number.** Free after #162/#163/#165/#166 settle.
- **D6 — panels for the hacker.** The request says the hacker alters "only
  genuinely reachable and remotely controllable functions". Is a *panel* ever
  remotely viewable (a maintenance console that shows a panel's state) or is the
  hacker strictly limited to the actuations the node exposes? The former is a nice
  information asymmetry for the two-seat game; the latter is simpler and truer to
  "a screwdriver must not grant network access."

## 10. Things that will waste the next session's time (learned the hard way)

1. **There is no Rust toolchain in the sandbox.** You cannot compile. Two
   precedents exist for using CI as the compile loop (#163's temporary
   `arena-feedback.yml` digest workflow, removed before merge; #166's note), and
   both are explicitly temporary. Budget for it, and do not claim a test passed
   that you did not run — this repo's whole `DOCUMENTATION-CLAIMS.md` culture is
   about exactly that.
2. **`scripts/must_check.sh` fails without a toolchain by design** ("an absent
   toolchain is a failure, never a skip"). Do not "fix" that.
3. **`scripts/doc_claims_check.sh`** will fail a document that carries a 64-hex
   digest no gate knows about, or a workflow step name that claims a file it does
   not touch. This note carries no digests for that reason.
4. **`git commit -s`** — DCO. And `just fmt` / `clippy -D warnings` are gated;
   hand-written Rust that misses rustfmt will fail CI, not review.
5. **Goldens.** `ghost_lobby_wiring.json`, `exchange_house` JSON, `actors.json`,
   the TUI fixtures and the session-relay fixtures are compared byte-for-byte.
   New optional state must be absent-when-default, and any golden that *must*
   change needs its ignored regeneration test run and the change justified in the
   PR body.
6. **`scenario/infrastructure/` and `scenario/power/` now exist.** Do not create
   a second plant or a second power model beside them, and do not assume either
   one runs: neither is called from the game tick on a shipped floor (#192).
7. **The branch name is fixed per session** (`arena/<id>-idaptik`) and each
   session is tracked by its own; work continues on the session's branch and PRs
   open from it. This note's branch, `arena/0639e4ae-idaptik`, merged as #180 and
   is not the branch you are on.
8. **The metadata files are DEED/K9 now.** There is no `STATE.a2ml`, no
   `.machine_readable/descriptiles/`, and no `k9iser.toml`; `just
   machine-readable-check` and `tests/machine_readable_migration_test.sh` both
   fail if one reappears. Project state goes in `IDApTIK_chora.deed`.

## 11. One-line summary for whoever picks this up

The building's *nervous system* (signals, reach, power, snapshots, lockstep) is
built and disciplined; the building's *skin* — panels you can open, parts you can
swap, and people who notice — does not exist yet. Build the skin in this order:
panel-as-object with a serviceability permission (M1), the five-layer switch/door
slice (M2), the staged NPC notice-and-repair pipeline (M3), the developer
truth/readout/belief view (M4), then cameras, telephony and fuses. Keep the
existing three planes and the `builtin` rule; extend them, do not replace them.

## 12. Closing position of this thread

Nothing in this request was implemented. What the thread produced is the audit
(§2), the gap table (§5), a staged plan with failing-test definitions of done
(§6), a first device/action/observation matrix (§7), the five demonstration beats
as assertions (§8), and six questions that need an owner ruling before code
(§9) — plus §0a, which turns the audit into a starting point against the tree as
it actually stands.

The honest next step is small: give one corridor a powered floor, then M1's
panel object with the serviceability permission. Everything after that is
additive.

