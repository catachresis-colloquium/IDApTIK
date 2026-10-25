# Design: interacting building infrastructure

Status: **proposal / design snapshot** — written 2026-10-24; source additions landed 2026-10-25
Scope: `crates/idaptik-core` (truth), `crates/idaptik-bevy` (views), `crates/idaptik-net` (contracts), `contracts/idaptik/v1` (UMS boundary)

**Current-state note (2026-10-25):** power and water/gas/air subsystems now have
source and tests in `scenario/power/`, `scenario/sim/power.rs`, and
`scenario/infrastructure/`. This proposal predates those additions. Section 1
is the source-backed status summary; sections 4–9 and 12 describe target
integration/design, not a current code map or roadmap. The root deed is the
canonical descriptive status record.

This design makes the Exchange House building a place where **control wiring,
power, pipes, ducts, physical routes and excavatable fabric interact
consistently**, without merging the systems into one untyped graph, without
duplicating the world inside each subsystem, and without a wholesale rewrite.

Every claim about the current state below cites the source it was read from.
"Implemented" means Rust source with tests in this repository. Design
documents, ADR context sections, and the archived prototypes are cited only
as design intent, never as implemented gameplay.

---

## 1. Source-backed inventory

### 1.1 Implemented (source + tests in this repo)

| System | Location | What exists, precisely |
|---|---|---|
| Deterministic scenario sim | `crates/idaptik-core/src/scenario/sim/mod.rs`, `sim/systems.rs` | `GhostLobbySim`: 60 Hz fixed tick (`TICK_DT`, `scenario/mathf.rs`), immediates applied before systems, twelve systems in a load-bearing order, post-phase checks (crisis timer, lockdown, trace). Event-sourced: every transition is a typed `Event` (`scenario/event.rs`). Panic-free, seeded `Mulberry32` (`scenario/rng.rs`). |
| Scenario definition | `scenario/definition.rs` | `ScenarioDefinition`: one floor of contiguous rooms on a 1-D walk line, doors on room boundaries, cameras with sinusoidal sweeps, hide spots, props, objectives, difficulty presets, tuning, scoring. Optional `signal` network (ADR-0012), optional `exterior` (rooftop, `scenario/rooftop.rs`), optional consoles. Exchange-House-style named validation checks (`ValidationReport`). |
| Grounded network | `crates/idaptik-core/src/netsim/` | `GroundedGraph` (segments, nodes, DNS, vantages, `graph.rs`); segment-access-matrix reachability (`reach.rs`, `access.rs`); ping/traceroute (`route.rs`); `AgentSession` with pivot stack, trace clock, logs (`session.rs`); `Effect` + power cascade over `deps` with a seen-set that terminates on cycles (`effect.rs`); `Actuation` per node (`graph.rs`). |
| Floor derivation | `scenario/floor_graph.rs` | `floor_graph(def)`: the *derived* network of a scenario floor — one segment per room, a `SmartDoor` node per door (`door_node_id`), `Light` per room (`light_node_id`), `Camera` per camera on the shared `security` segment (`camera_node_id`), the vacuum, `Inside` vantage per room, the authored backbone (substation → `feed`) embedded from `ghost_lobby_floor.json`. Commitment: *only the backbone is authored; every fixture is derived, so the floor graph cannot drift from the definition.* Address capacity caps: `MAX_CAMERAS`, `MAX_DOORS_PER_ROOM` (host-octet arithmetic, non-panicking). |
| Building runtime | `scenario/building/` | `BuildingDefinition` (`mod.rs`): floors (Standard/Billy/Service, exactly one Billy floor), rooms, `PortalDef` (Door/Stair/Lift/Ladder/Vent, `travel_time`, optional `lock { controller }`, optional `circuit`; lifts must declare a circuit), `CircuitDef { id, name, source, zone }`, `ZoneDef` (dotted /24 subnet, one `netsim` segment each). Named-check validation report + typed errors; `load_building` format-gates `idaptik-scenario/1`. `graph.rs`: Dijkstra `shortest_path` / `reachable_rooms` over rooms+portals, `BTreeMap`/`BTreeSet` throughout for order determinism. `network.rs`: `building_network(def)` derives a `GroundedGraph` — one segment per zone, a `PowerStation` feed node per circuit (`Actuation::CutPower`), a `Lock` controller node per locked portal (`Actuation::DisengageLock`, `deps` on its feed). `sim.rs`: `BuildingSim` — pure function of (definition, command stream); commands `Traverse(PortalId)` / `Actuate(String)`; events `Entered` / `TraversalDenied { UnknownPortal | NotAdjacent | Locked | Unpowered }` / `Unlocked` / `PowerCut` / `ActuationFailed`; full restorable snapshot (`idaptik-building-runtime-v1`); five export surfaces; no RNG, no wall clock. |
| Electrical power | `scenario/power/`, `scenario/sim/power.rs`, `scenario/state.rs` | Typed source/feed/load/backup/breaker/hazard model with deterministic arbitration, commands, events, snapshots, and a power test floor; power state and commands are integrated into `GhostLobbySim`. |
| Water / gas / air infrastructure | `scenario/infrastructure/{mod,water,air,sim}.rs`, `infrastructure/fixtures.rs` | Qualitative integer-only water and airflow runtime; fictional gas species ride in air, with declared coupling, validation against a building definition, snapshots, accounting ledgers, fixtures, and tests. It is a separate runtime, not yet the consolidated building/scenario tick. |
| Canonical building + goldens | `scenario/building/exchange_house.rs` | `exchange_house()`: 4 floors (service −1, exchange 0, office 1, ghost-lobby 2 as the Billy floor with the full embedded scenario), 12 authored portals + projected scenario doors, 2 circuits, 3 zones; `EXCHANGE_HOUSE_JSON` + `EXCHANGE_HOUSE_RUN_JSON` committed goldens, round-trip tested, regenerated by ignored tests. `tests/building_runtime.rs` covers validation, reachability, exports, the UMS seam, and a Ghost-Lobby-as-floor ending. |
| Signal network (control wiring) | `scenario/signal/` | ADR-0012, landed. `SignalNetworkDef` (`mod.rs`, format `idaptik-signal/1`): `SignalDeviceDef` (kind with a fixed port table; **`node: Option<String>` joins a device to its grounded-network node; `room: Option<String>` joins it to a room**), `LinkDef` (output→input, invertible, `builtin` hardwired wires), per-seat `SeatProfile` (bandwidth vs busy-ticks; re-program and `hacker_only` gates), budget. `sim.rs`: `SignalSim::apply(cmd, access)` / `step(readout)`; outputs read at tick start, inputs applied after — a signal crosses exactly one wire per tick; edge vs level ports; relay gates (And/Or/Xor/Nand/Nor/Latch); snapshot `idaptik-signal-snapshot/1`. `sim/signal.rs` (host): `signal_access()` (infiltrator = room they stand in + doors in arm's reach + console-reachable; hacker = pivot-reachable or standing-on), `signal_readout()` (world facts → host-owned outputs), application of locks/holds/darkness/vacuum routing. `Command::Signal { op }` on the wire with index-addressed `SignalOp` (`command.rs`); netplay seat routing already in `idaptik-net/src/envelope.rs::seat_for`. |
| Observation / console screens | `scenario/console.rs` | Consoles show a pure function of reach × capability × intel: `Intel { infiltrator: Option<Fix>, billy: Option<Fix>, last_room }` (building-logged observations: camera sightings, door passages), `Estimate` (radius grows with age; only if the fixer is still in reach), `ScreenView`/`ScreenDevice`. Module law: *"People are never shown directly."* |
| VSM supervision | `scenario/vsm.rs`, `sim/mod.rs` | Opt-in deterministic supervisor: event fold → attention allocation → coverage target bending Billy's patrol; announce-once events. Always on in the TUI, byte-identical off elsewhere. |
| Actors as data | `scenario/actor/` | `ActorArchetype` (29 stats) + `Modifier` composition; Billy projected from the same constants; `belief` engine shared. |
| Companion Moletaire | `crates/idaptik-core/src/companion/` | Deterministic dt-stepped `MoletaireSim` (11-state machine, tunneling with depth 0..1, hunger gravity model, equipment, 5-slot coprocessor bay with ladders, chiptune pattern data). Typed `MoleCommand` in / `MoleEvent` out; single seeded RNG with documented draw order; snapshot `idaptik-moletaire-runtime-v1`; pure `view()`/`body_rect()` projections. Notable seams already typed: `MoleCommand::SabotageCable`, `MoleEvent::CableSabotaged { cable_id: String }`, `TrapTriggered { x, y }`, `VibrationDetected`, `UndergroundScanComplete`, `SynthSoundPlayed`. |
| Package boundary | `crates/idaptik-core/src/package.rs`, `contracts/idaptik/v1/` | `idaptik-package/v1` envelope (ADR-0007): format/contract version, game/profile compatibility, scenario id, 9-term taxonomy, actor bindings, scheduled commands, deterministic seed, snapshot format, event guarantees; loader then runs the real `ScenarioDefinition::validate`. `contracts/idaptik/v1/fixtures/ghost-lobby-scenario.json` is the authoritative fixture, compiled into the game. |
| Frontend views | `crates/idaptik-bevy/src/` | `driver.rs`: `SimDriverPlugin` steps the sim at 60 Hz in `FixedUpdate`, folds `CommandQueue` into `TickInput`. `scene.rs`: side-on 2.5D cross-section, `SceneMap::row_lift` stacks floor rows (`ROW_STACK = 440.0`) — the multi-floor seam, already mapped. `wiring.rs`: the signal overlay drawn on a console screen; mouse gestures become `Command::Signal`. `net_view.rs`: grounded-graph view; clicks become `NetSsh`/`NetHack`. `interp.rs`: double-buffered pose interpolation between ticks. **`tests/parity.rs`: a headless Bevy app stepping a scripted stream must emit exactly the events a plain headless run emits** — the invariant that makes Bevy a renderer, never a second simulation. |
| Multiplayer | `crates/idaptik-net/src/` | Relay-only burble session (ADR-0005/0006); byte-preserving `command`/`event` relays; `seq` dedupe; `at` lockstep scheduling; delay-lockstep with per-tick `net:commit` watermarks, loss-by-silence, and `Resync` (snapshot + full event log + committed-but-unexecuted commands) — proven in-memory against the headless reference byte-for-byte (`lockstep.rs` tests). `NET_PROTO = 2`. |
| Typed config authoring | `config/*.ncl`, `justfile` | Nickel schemas mirroring the Rust types with deliberate strictness deltas (closed records); `just config-check` / `config-scenario-check` gates exports and round-trips; negative fixtures under `config/scenario-fixtures`. |

### 1.2 Partially integrated

| Item | Evidence | Gap, precisely |
|---|---|---|
| Building ↔ scenario floor | `FloorDef::from_scenario`, `scenario_floor_portals`, validation `scenario.rooms_match` (`building/mod.rs`) | Definition-level only. `BuildingSim` and `GhostLobbySim` are **two separate runtimes** that share a definition projection. No single sim plays the building's rooms while the embedded scenario floor runs Billy, cameras, signal and network. |
| Building network ↔ scenario network | `building_network` (`building/network.rs`) vs `floor_graph` (`floor_graph.rs`) | Both derive `GroundedGraph`s, but independently and for different sims. A building that embeds the Ghost Lobby has two derived networks; nothing unifies them (the exchange-house network carries only feeds + lock controllers — no door/light/camera nodes). |
| Moletaire ↔ world | `companion/mod.rs` "Integration seam — not yet wired into `GhostLobbySim`"; `MoleEvent` doc notes the archive game loop's routing; `IDApTIK_chora.deed` component-status entry for `Moletaire` | The mole is a standalone sim. Its `CableSabotaged` event carries an **opaque string** no building circuit knows; its tunneling ignores any building fabric; nothing hosts it inside a tick. |
| Netplay ↔ signal | `seat_for` handles `Command::Signal` by seat (`envelope.rs`) | Client side done. The relay's role table lives in the external burble repo (`Burble.Games.Idaptik`); ADR-0012 "Not decided here" item 1 is the open half. |
| Netplay ↔ building | — | Lockstep runs `GhostLobbySim` only. Building commands have no netplay path. |
| UMS ↔ building | `load_building` seam + the `idaptik-edit/1` contract declared UMS-side (building/mod.rs module docs); UMS main currently carries an older edit-script verb set (`add_character`, `add_device`, `add_dog`, `add_drone`, `add_guard`, `add_item`, `add_npc`, `add_zone`, `set_mission`, `set_physical` in `schemas/edit-script.schema.json` of catachresis-colloquium/universal-modding-studio) | Contract v1 *does not carry building packages* (ums-package-contract.md: "A future contract version may publish building packages separately; v1 does not pretend that migration or loader exists"). The floor/room/portal/circuit/zone edit verbs are the UMS-side PR, not yet on main. |

### 1.3 Not yet integrated / not implemented

| Item | Current status |
|---|---|
| Water / gas / air integrated with the multi-floor building tick | Separate deterministic subsystem exists in `scenario/infrastructure/`; it is not yet composed with `BuildingSim`, traversal, and the scenario floor in one runtime/snapshot. The older claim that no subsystem code existed is superseded. |
| Excavatable building fabric (walls, fill, foundations) | Not implemented. `PortalKind::Vent`/`Ladder` and Moletaire's depth model are adjacent seams, not a model of the fabric between rooms. |
| Consolidated multi-floor runtime (building + scenario + infrastructure + fabric + mole in one tick) | Not implemented; `level-design.md` describes the intended synthesis, while `ROW_STACK` is render-side only. |
| Gossamer host + burble desktop window | ADR-0006: design-accepted, deferred. |

### 1.4 Unknown (needs verification outside this repo, or is an open question)

| Item | Status |
|---|---|
| Burble relay role table coverage for `Signal` and for any building commands | External repo (pinned rev in `Cargo.toml`); verify at netplay stage. |
| Exact final verb set of the UMS `idaptik-edit/1` building verbs | UMS PR #2 (referenced in `building/mod.rs` docs); UMS main currently shows the older verb set. |
| DEED publication and local conformance status | This repository's migration completed 2026-10-25. The pinned normative ABNF is v1.2.0, while the accompanying informative grammar spec remains DRAFT/not ratified; `just machine-readable-check` uses the pinned DEED/K9 tools. See §10. |
| Fail-secure vs fail-safe for lock controllers on dead circuits | Today `BuildingSim` simply denies actuation of a dead controller; the *traversal* rule is "locked while locked". A dead lock controller therefore fails **secure** (stays locked) by construction — this design makes that the documented, tested rule (§7.5). |

---

## 2. Design principles (what this design must not break)

1. **Preserve the existing distinctions.** Network reach (who can get to what — `netsim`), signal wiring (what happens when they get there, and when — `signal`), electrical infrastructure (power — circuits + `deps` cascade), water/gas/air (each medium its own small state), traversal (where actors can go — rooms + portals), fabric (what can be opened, damaged, excavated). Each keeps its own definition fields, its own state fields, its own events.
2. **One physical object, one stable identity, explicit references.** A door, a valve, a feed, a vent is *one object* that several systems reference by the same id — never several objects that happen to look alike. No mapping tables, no string surgery, no per-subsystem copies of the world.
3. **Derived, not duplicated.** The grounded network is already derived from the definition (`floor_graph`, `building_network`). New systems join the same pattern: definitions live once in `BuildingDefinition`; the grounded graph is derived once at construction; nothing stores a second copy of topology.
4. **Coarse, playable, bounded.** Rooms and portals for actors; a small cell grid only where excavation actually plays (underground); edges between rooms for pipes/ducts; no pressure, flow-rate or structural-engineering simulation. Hard capacity caps fail validation, not the tick.
5. **Three planes, one direction.** Truth (sim state) → readouts (pure functions of truth) → knowledge (what actors have learned). Hacking a sensor rewrites knowledge and readouts; it never writes a truth field.
6. **Additive, golden-pinned, no rewrite.** Every new definition field is optional and `skip_serializing_if`-absent (the signal/exterior/console precedent), so every existing golden, replay and wire payload stays byte-identical until a stage deliberately changes something.

---

## 3. The identity problem, stated concretely

A physical object participates in several systems at once. Today:

| Object | Systems it is in | Current join mechanism |
|---|---|---|
| Scenario door | traversal (`doors`), network (`door-<id>` node), signal (`SignalKind::Door` device) | node id string minted by `floor_graph::door_node_id`; `SignalDeviceDef.node` |
| Room lights | room property (`lit`), network (`light-<room>` node), signal (Light device) | `light_node_id(room)`; `SignalDeviceDef.node` |
| Camera | detection, network (`cam-<i>` node), signal (Camera device), intel source | `camera_node_id(i)`; `SignalDeviceDef.node` |
| Circuit feed | power, network node, (intended) cable target | `CircuitDef.source` (free string), `Node.deps` |
| Portal lock | traversal (deny rule), network node, (intended) signal target | `PortalDef.lock.controller` (free string) |
| Medium line valve | (new) medium network, network node, signal device, fabric (the duct cell it guards) | **does not exist yet** |
| Fabric cell | (new) excavation, traversal (breach portals), media (duct/pipe integrity) | **does not exist yet** |
| Mole's cable target | (new) companion action, power/medium truth | `MoleEvent::CableSabotaged { cable_id: String }` — **opaque** |

Two weaknesses are visible:

- The join keys are free `String`s. Nothing stops `CircuitDef.source` from
  naming a node the derived graph does not carry; nothing stops the mole's
  `cable_id` from naming a cable that is not a circuit feed. Validation
  today checks non-empty and unique — not *resolves against the derived
  graph*.
- Two new physical objects (valves, fabric cells) have no identity at all,
  which is where the water/gas/air and excavation systems would otherwise
  grow their own private ids — exactly the duplication this design must
  avoid.

The fix is small and follows the house pattern (`scenario/ids.rs`):
**serde-transparent string newtypes, resolved to indices once at
construction.**

---

## 4. Minimal types and module ownership

### 4.1 Identity (stage 0 — hardening, no behaviour change)

```rust
// scenario/ids.rs — added, all serde-transparent like their siblings
pub struct NodeId(pub String);        // a grounded-network node (door, light, feed, valve, …)
pub struct MediumId(pub String);
pub struct MediumLineId(pub String);
```

**One derived graph for the consolidated runtime.** Today the building and
the scenario floor derive two separate `GroundedGraph`s (§1.2). The
consolidated runtime (stage 1) derives **one**, in `building/network.rs`
which already owns `building_network`: floor fixtures (doors, lights,
cameras, vacuum, vantages — reusing `floor_graph`'s minting functions, not
re-inventing them) + building fixtures (feeds, lock controllers) + plant
and valve nodes (new, §4.2), over floor segments (`auto-<room>`) and zone
segments (zones). Standalone `BuildingSim` keeps its zones-only graph for
backward compatibility; nothing is rewritten, the consolidated derivation
is an added function.

`NodeId` replaces the free strings at the four existing join sites
(`PortalLock.controller`, `CircuitDef.source`, `SignalDeviceDef.node`,
`BuildingCommand::Actuate`) and one companion site
(`MoleEvent::CableSabotaged.cable`). Being `#[serde(transparent)]`, the wire
tags, JSON goldens and every replay are **byte-identical** — this is a
compile-time tightening, not a format change.

Target rules (to be enforced by validation and represented in K9 policy contractiles; the praxis deed in §10 is only a historical sketch):

- **Minted ids** come from exactly one derivation module per floor
  (`floor_graph::door_node_id` etc.). **Authored ids** are the join keys.
  Cross-system references may name authored ids only; a reference that does
  not resolve against the *derived* `GroundedGraph` fails validation
  (`nodes.resolve` check family) instead of degrading to a silent no-op at
  runtime.
- `IdIndex` gains `portals`, `circuits`, `zones`, `mediums`, `medium_lines`,
  `fabric_cells` — resolved once at construction, exactly as rooms/doors
  already are (lookup maps stay `HashMap`; nothing iterates them, preserving
  the determinism rule that iteration happens over `BTree*`).

### 4.2 Target medium integration (water / gas / air) — original proposed model

Coarse model: a medium is **a source node, a circuit it draws from, and a set
of lines between rooms**. No pressures, no flow rates, no in-room routing.

```rust
pub enum MediumKind { Water, Gas, Air }          // serde snake_case, like SignalKind

pub struct MediumDef {
    pub id: MediumId,
    pub name: String,
    pub kind: MediumKind,
    pub source: NodeId,              // boiler / tank / compressor node in the derived graph
    pub circuit: Option<CircuitId>,  // pump/boiler/compressor power; None = unpowered-plant levels
    pub lines: Vec<MediumLineDef>,
}

pub struct MediumLineDef {
    pub id: MediumLineId,
    pub from: RoomRef,
    pub to: RoomRef,
    pub valve: Option<NodeId>,       // a valve node on this line (hacker-reachable, wirable)
    /// The valve's starting state (a line with no valve is always open).
    #[serde(default = "crate::scenario::building::medium::start_open")]
    pub start_open: bool,
    pub cell: Option<CellKey>,       // the duct/pipe cell the line passes through
}
```

New `DeviceKind`s (appended, wire tags stable, mirroring the UMS-parity
append precedent in `device.rs`): `Boiler`, `Tank`, `Compressor`, `Valve`.
Validation: a medium source node must exist in the derived graph, be one of
the plant kinds, and sit on its circuit's zone segment (or have no circuit);
a valve node must be `Valve` kind; a line's `cell`, if present, must exist
and be a `Duct` cell.

Runtime (in `BuildingState`, one place):

```rust
pub struct MediumState {
    pub pressurized: BTreeMap<MediumId, bool>,   // false when its circuit is cut
    pub open: BTreeMap<MediumLineId, bool>,      // valve states (default: as authored)
    pub severed: BTreeSet<MediumLineId>,         // broken by excavation or sabotage
    pub leaks: BTreeMap<RoomRef, LeakState>,     // room currently leaking, and from which line
}
pub struct LeakState { pub line: MediumLineId, pub since: f64 }
```

**Flow is derived, never stored**: a line flows iff its medium is
pressurized ∧ the line is open ∧ not severed; a room is *served* iff a path
of flowing lines from the source reaches it (BFS over lines — bounded, §7).
A **leak** exists exactly when a flowing line is severed: the leak is in the
room the line's cell sits in (or `from` for a valveless line); a leak stops
when flow stops. This one rule gives every medium a consistent, readable
behaviour without any hydraulics.

Per-medium gameplay effect (all coarse, all tunable, all evented):

| Medium | Effect while a room is served / leaking |
|---|---|
| Water | Leak: small alert rise + `LEAK` console status. (Optional per-level slowdown factor, default off.) |
| Gas | Leak: alert rise + Billy drawn to the room (his existing `Investigate` mode) + `LEAK` console status + gas-sensor readouts trip. |
| Air | Service: a `Vent` portal whose endpoint rooms are both served by the same air line travels at `travel_time * air_vent_factor` (default 0.5) — *the duct is alive*. Air off: vents are just slow shafts. |

New `Actuation` variant (appended): `ToggleValve` — actuating a valve node
flips its line's `open` state. New `Effect` variants:
`ValveToggled(String)`, `LeakStarted(RoomRef)`, `LeakStopped(RoomRef)`,
`LineSevered(MediumLineId)` — routed through `apply_effects`-style handlers
that write `MediumState` (truth) and emit events. (`ValveToggled` is what
makes a valve openable by net hack as well as by wire — the stage-3 gate
asserts both paths appear as their own systems' events.)

### 4.3 Fabric (excavatable building fabric) — `scenario/building/fabric.rs` (new)

Coarse model: **one cell grid per building** over the world width, with
depth bands. This is the *only* place spatial cells are used — actors move
between room compartments, the mole moves through cells, everything else
lives on edges or nodes.

```rust
pub struct CellKey { pub col: u16, pub band: u16 }        // Ord, BTree-friendly

pub struct FabricDef {
    pub cell_w: f64,                // canvas units per column
    pub columns: u16,               // cap 64
    pub bands: Vec<FloorId>,        // each depth band belongs to one floor's undercroft; cap 8
    pub cells: BTreeMap<CellKey, CellMaterial>,  // only non-Void cells are authored
    pub dig_time: f64,              // seconds to fully excavate one non-Foundational cell
}

pub enum CellMaterial { Fill, Wall, Duct, Foundation }    // Void is the absence of an entry
```

Rules: `cell_w * columns` spans the world width; room boundaries of every
floor align with column boundaries (validated — this is what makes breach
portals well-defined); `Foundation` is indestructible; only `Wall` cells sit
on room boundaries; only `Duct` cells may be referenced by a medium line.

Runtime:

```rust
pub struct FabricState {
    pub dug: BTreeMap<CellKey, f64>,     // seconds dug, toward `dig_time`
    pub breached: BTreeSet<PortalId>,    // derived breach portals currently open
}
```

Settlement (one per tick, §7): a cell whose `dug` reaches `dig_time` becomes
Void, and then exactly one of:

- `Wall` cell under floor `F` between columns `c-1`/`c` → a **breach portal**
  appears between the two rooms of `F` that share that boundary. Its id is
  derived, not authored: `breach-{F}-{c}`. Breach portals are `Door`-kind,
  no lock, no circuit — they are holes, and they stay open.
- `Duct` cell → every line passing through it is `severed` (events follow
  from the flow rule in §4.2).
- `Fill` cell → nothing but a mole passage.
- `Foundation` → the attempt is denied (`FabricDenied { reason: Foundation }`),
  the cell keeps its state, the mole resists (its existing
  hunger-resistance episode is the natural presentation hook).

Traversal interplay: a breach portal is passable like any door portal; it is
*added to the physical graph at runtime* by union (the derived graph is
rebuilt lazily for path queries only — Dijkstra runs on demand, never per
tick, §7).

### 4.4 Mole coupling — `companion/` + `scenario/building/`

No new mole types. The host (the building runtime, stage 4) does three
things, all through the already-typed boundary:

1. **Cable targets are node ids.** The host picks the `SabotageCable`
   target as the `NodeId` of a circuit feed, medium source or valve (the
   only three node kinds a cable can be). `MoleEvent::CableSabotaged`
   becomes `{ cable: NodeId }`; the handler maps node → effect:
   feed → `PowerCut` cascade (existing `apply_actuation`), medium source →
   `pressurized = false`, valve → line `severed`. One event, one identity,
   no string guessing.
2. **Fabric is the mole's tunnel.** The mole's (x, depth) maps to
   (column, band) through the same `cell_w`/`bands` constants; it moves
   only through Void cells (the runtime denies `MoveTo` targets in solid
   cells instead of letting the sim disagree with the fabric).
3. **Coprocessors are knowledge, not truth.** `VibrationDetected` feeds the
   hacker's view (the archive seam says so already); `UndergroundScanComplete`
   drives the mole's overlay. Neither writes a truth field.

### 4.5 Commands and events (additions)

```rust
// building/sim.rs — BuildingCommand gains:
SeverLine(MediumLineId),      // sabotage verb (mole or infiltrator wrench)
Excavate(CellKey),            // one dig-time quantum of work on a cell
// Actuate is hardened to Actuate(NodeId) in stage 0 — a valve is actuated
// through exactly this path, via its Actuation::ToggleValve. No separate
// verb: one actuation path for every grounded node, as today.

// BuildingEvent gains:
LeakStarted { room: RoomRef, line: MediumLineId, kind: MediumKind },
LeakStopped { room: RoomRef },
LineSevered { line: MediumLineId },
Breached { portal: PortalId, cell: CellKey },
FabricDenied { cell: CellKey, reason: FabricDenyReason },   // Foundation | UnknownCell | NotDiggable
MediumDead { medium: MediumId },                            // its circuit was cut
CableCut { cable: NodeId, effect: CableEffect },            // what the chewed cable was
// ActuationFailed gains `reason: ActuationDenyReason` (UnknownNode |
// Unpowered | AlreadyInState) with #[serde(default)] — old event logs
// still decode; the log line finally tells a dead controller apart from a
// locked one.
```

All serde-tagged like their siblings; `log_view` gains lines for the new
events (the player must be able to *read* a leak and a breach); unknown ids
are deny-events, never panics (the fuzz test drives every command with
garbage ids — keep that property).

**Reach is a seat concern, not a building concern.** Standalone
`BuildingSim::Actuate` stays the raw building primitive (as today — no
vantage, no session). In the consolidated runtime the *seat-facing* command
applies the netsim reach gate first (`can_reach` from the agent's active
vantage, the existing rule) and only then calls the primitive — a hacker
who cannot reach `valve-0` from where they stand gets a denial, not an
actuation. This keeps `netsim` the single owner of "who can get to what".

### 4.6 Module ownership

| Concern | Owner | Why |
|---|---|---|
| Building structure (floors, rooms, portals, circuits, zones, mediums, fabric) | `scenario/building/mod.rs` (definitions) + new `medium.rs` / `fabric.rs` | one `BuildingDefinition`, one validation report |
| Grounded network derivation | `floor_graph.rs` (scenario floors), `building/network.rs` (building) | already the derivation modules; new: plant/valve nodes derived from `MediumDef`/`CircuitDef` (same file that already mints feeds) |
| Reach, pivots, trace | `netsim/` | untouched except appended `Actuation::ToggleValve` |
| Control wiring | `scenario/signal/` | untouched except appended `SignalKind::Valve` (inputs Open/Close/Toggle, output Active) — the valve is a wirable device with the *same* `node` join as every other device |
| Traversal & physical paths | `scenario/building/graph.rs` | unchanged API; breach portals unioned in at query time |
| Power & media truth | `BuildingState` (one struct, one snapshot) | no per-subsystem state copies |
| Mole | `companion/` | host integrates; the mole module never learns about the building |
| Observation (Intel, screens, readouts) | `scenario/console.rs` + new pure `view` functions | read plane only |
| Rendering, debug overlays | `idaptik-bevy/` | pure views of `view()` output; the parity test is the contract |
| Lockstep, resync | `idaptik-net/` | building commands join `Command` (and `seat_for`) when the consolidated runtime lands |
| UMS boundary | `package.rs`, `contracts/idaptik/v1/` | contract v2 (§9); the game keeps owning the vocabulary |
| Metadata & policy | `IDApTIK_chora.deed` (descriptive state), `.machine_readable/contractiles/` (K9 policy), root `coordination.k9` (instructions) | estate convention, not game code |

**Explicit non-goals:** no `ECS`; no graph crate; no `World` struct that
unifies systems; no per-subsystem `HashMap` world copies; no pressure/flow
simulation; no 2-D physics for media.

---

## 5. Interaction matrix

Rows act, columns receive. Cells name the **typed relationship** doing the
coupling — this is the whole contract. A dash means "no coupling; if you
want one, it needs a row here first."

| acts ↓ / affected → | Traversal (rooms+portals) | Power (circuits) | Water | Gas | Air | Signal wiring | Network reach | Fabric | Knowledge (Intel/belief/screens) |
|---|---|---|---|---|---|---|---|---|---|
| **Traversal** | — (self: actor position) | reads `is_powered(circuit)` for portal deny rule | reads served-room flags (optional slowdown) | reads leak rooms (alert, Billy draw) | reads served pairs for vent `travel_time` factor | emits `Readout` facts (door open, camera tripped) | — | **breach portals** appear/disappear the fabric settles | emits passage Fixes to `Intel` |
| **Power** | de-powers lifts/ports (Unpowered deny) | — (self: `powered` map, cascade over `deps`) | source dies → `pressurized=false` | same | same | — | — | — | console reads circuit state |
| **Water** | (optional: wet-floor slowdown, default off) | draws its circuit | — (self: line/leak state) | — | — | valve is a Signal device (Open/Close/Toggle) | valve node is hacker-reachable | line may be severed by a dug `Duct` cell | `LEAK` status; no direct people-view |
| **Gas** | Billy drawn to leak room (Investigate) | draws its circuit | — | — (self) | — | same valve join | same valve join | same duct join | leak raises alert; gas sensors trip (readout) |
| **Air** | vent portal speed factor while both ends served | draws its circuit | — | — | — (self) | same valve join | same valve join | same duct join | fan noise (optional, default off) |
| **Signal wiring** | locks/holds doors, darkens rooms (existing application) | — (a wire cannot re-power; it asks) | opens/closes valves | same | same | — (self: wires, relays, budgets) | reach decides *who may wire* (`Access`) | — | a compromised terminal's `Compromised` output is knowledge, not truth |
| **Network reach** | — | cutting a feed cascades (`apply_actuation`) | valve toggle (`ToggleValve`) | same | same | reach + `Access` per seat (who may wire, who may actuate) | — (self: segments, pivots, trace) | — | pivot/trace logs are actor knowledge |
| **Fabric** | **breach portals** (derived id `breach-{F}-{c}`) | — | severs lines through dug `Duct` cells | same | same | — | — | — (self: `dug` progress) | the mole's scan overlay; nothing else |
| **Mole (companion)** | occupies cells; traps (existing `TrapTriggered`) | **`CableSabotaged { cable: NodeId }`** → feed cut / source death / line sever | — | — | — | — | — | tunnels Void cells only; denied in solid | vibration & scan readings → hacker/mole view only |
| **Sensors / observation** | reads door passage (Fix source) | reads circuit state (console) | reads leak state (console) | reads leak + gas sensors | reads air state | reads device states (`ScreenDevice.status`) | reach gates what a console shows | reads breach state | **writes here, and only here** — including *false* writes from compromised sensors |

Reading the matrix:

- Every off-diagonal coupling is either an **id join** (NodeId/CircuitId/
  RoomRef/MediumLineId/CellKey), a **derived query** (flow, served rooms,
  vent factor) or a **one-way read** (readouts, knowledge). Nothing cycles.
- Power is the only *cascade* (existing, `deps` + seen-set). Media and
  fabric are *derived* (flow, served, breach) — they cannot cascade
  indefinitely because the derivation is a single pass over bounded lines.
- Knowledge is the only column anything may *falsify*, and the only thing
  falsification may touch.

---

## 6. Which quantities live where (spatial model decision)

| Quantity | Primitive | Rationale |
|---|---|---|
| Where actors (infiltrator, Billy) can be | **Room compartments + portals** | The existing building model; one-dimensional inside a scenario floor (the scenario keeps its `x`-line). Coarse enough to read at a glance, exact enough to be a graph. |
| Where the mole can be | **Fabric cells** (columns × bands) | The mole is the only actor that moves through the fabric; it is the one system that needs a grid. One grid per building, caps 64 × 8. |
| Pipes / ducts / signal wiring | **Edges between rooms** (medium lines; signal links) | No in-room geometry. A line says "room A to room B, optionally through cell C, optionally via valve V". A signal wire says "device output to device input". Both are topological; the renderer draws them, the sim never routes around obstacles. |
| Power | **Membership** (circuit → set of nodes via `deps`) | Power has no geometry today and none needs it; the cascade is a graph walk, not a spatial one. |
| Network reach | **Segments** (zones → `netsim` segments) | Existing, proven; no geometry. |
| Detection (cameras, sensors) | **Room + 1-D `x`** inside scenario floors (existing sweep formulas) | Keep the floor's own model; a building-level sensor readout is room-granular (`Intel` is already room + fix-granular). |

Deliberately not modeled: in-room pipe routing, ceiling vs wall placement,
per-pipe pressure, wall thickness by material beyond the four materials,
multi-actor cell occupancy beyond "the mole is a cell, actors are rooms".

---

## 7. Tick / update contract

### 7.1 The existing contracts (keep)

- `GhostLobbySim::tick`: 60 Hz; P0 pause/restart; P2 immediates (uplink,
  pivot, net verbs, signal, test hooks) at pre-increment `t`; the twelve
  systems in their fixed order; post-phase crisis/lockdown/trace checks.
  The vantage follows the body *after* movement, *before* anything reads it.
- `SignalSim::step(readout)`: outputs sampled at tick start, inputs applied
  after → exactly one wire-hop per tick; feedback loops through relays are
  well-defined by construction.
- `BuildingSim::apply`: command-driven, no clock except traversal time.
- `MoletaireSim::tick(dt, edibles)`: dt-stepped, own RNG stream.

### 7.2 The consolidated `BuildingRuntime` tick (lands as a skeleton in
stage 1; composes, does not replace)

The consolidated runtime owns a `BuildingDefinition`, the **one** derived
`GroundedGraph` (§4.1), the embedded scenario floor's `GhostLobbySim`
state, and the building `BuildingState` (which carries `MediumState` and
`FabricState`). Steps 3–5 are added by stages 1–2; the skeleton ships with
step 3 (fabric) and the reach-gated seat commands, and its no-behaviour
change is proven by event-identity against the standalone sims it composes.
One 60 Hz tick:

```
PRE   0  pause / restart          (existing P0 semantics)
      1  immediates, stream order (typed commands; each resolves once via IdIndex;
                                        unknown id → deny event; budget: all of the
                                        stream, no re-queue, no retry)
UPDATE
      2  timers                    (pulses, holds, locks, cooldowns, dig progress += TICK_DT
                                        for every cell currently being dug)
      3  fabric settle             (cells that finished dig → Void → breach appears or
                                        duct line severs; events; bounded: only cells
                                        with dug > 0 are touched)
      4  power settle              (new cuts since last tick → cascade walk (seen-set,
                                        one pass) → dead nodes; events; bounded O(nodes))
      5  medium flow               (pressurized := circuit powered; flow/served derived
                                        by one BFS per medium; leaks derived from
                                        (flowing ∧ severed); events on change only)
      6  signal step               (readout from truth; one wire-hop; apply locks/holds/
                                        darkness/valves; the scenario floor's own
                                        system_signal runs at the same point)
      7  traversal                 (actors' in-flight portal traversals complete; deny
                                        checks happen at command time, here completion)
      8  observation               (cameras/sensors/valves → Intel + screen readouts;
                                        compromised-sensor overrides apply HERE, writing
                                        knowledge only)
      9  behaviour                 (Billy FSM + belief; agents; the mole's tick(dt) with
                                        fabric-masked MoveTo; traps)
     10  objectives / outcome
POST  11  crisis timer, lockdown, trace   (existing post semantics)
```

Ordering notes (each is load-bearing, in the same style as `systems.rs`):

- Fabric settles **before** power and flow: a breach or sever this tick is
  visible to every system that reads it this tick; a duct cut this tick stops
  flow this tick.
- Power settles **before** flow: a dead pump cannot serve this tick.
- Flow **before** signal: the valve's `Active` output reflects this tick's
  flow, and a wired valve command applied as an immediate lands in step 5.
- Signal **before** traversal: a lock lands before anyone moves (existing
  rule, kept).
- Observation **after** movement and before behaviour: Billy's belief reads
  the fixes that exist this tick; a false fix from a compromised sensor
  enters at the same gate a true one does — the system cannot tell the
  difference, and that is the point.
- The mole ticks inside behaviour with `dt = TICK_DT`; its two RNG draws
  (eat roll, starving wander) keep their documented order; its tunnel is
  masked by the fabric (solid cell → denied `MoveTo`, evented).
- The mole's cable cut completes in step 9, so its `PowerCut` cascade lands
  in step 4 of the *next* tick — the same one-tick latency discipline as
  the signal net, not an exception to it. Test level 3 asserts the exact
  two-tick spacing.

### 7.3 Bounded work (contract, tested)

| Work | Bound |
|---|---|
| Immediates per tick | the input stream; each O(index) via `IdIndex`; no re-queue |
| Fabric settle | O(cells with `dug > 0`) ≤ commands issued |
| Power cascade | O(nodes) one pass, seen-set (existing) |
| Medium flow | O(mediums × lines) per tick, lines ≤ 8/medium |
| Signal step | O(devices + wires) (existing) |
| Dijkstra / reachability | **on demand only** (route queries, validation) — never per tick |
| Cameras / belief | existing per-floor bounds |

Caps (validation fails, tick cannot): columns ≤ 64, bands ≤ 8, cells ≤ 512
authored non-Void, portals ≤ 256, rooms ≤ 64, grounded nodes ≤ 512 (the
host-octet arithmetic already caps derived addressing at 254 per subnet),
signal devices ≤ 128, player wires ≤ 96 (budget field), mediums ≤ 8, lines
≤ 8 per medium.

### 7.4 Determinism rules (unchanged, extended to the new systems)

- One seeded `Mulberry32` per run; draw order documented in the module docs
  (reset rolls first, then mole draws, then floor draws) — the new systems
  **draw nothing** (flow, flow-settlement and breach are pure functions of
  state; a leak is not a roll).
- Iteration over `BTreeMap`/`BTreeSet` only; `HashMap` is for lookup
  (`IdIndex`) and is never iterated.
- Floats: `libm` / `total_cmp` where comparisons matter (the existing
  `mathf.rs` stance); dig progress accumulates `TICK_DT`, never wall time.
- A run is a pure function of (definition, config, seed, command stream);
  the event log is the artifact; parity tests (headless vs TUI vs Bevy vs
  netplay seats) extend to building commands the moment they exist.

### 7.5 Fail-secure, documented

A lock controller on a dead circuit cannot be actuated (`BuildingSim`
already refuses the actuation); a locked portal therefore **stays locked**
while its controller is dead. The two denials must be *readable apart*:

- traversing the portal → `TraversalDenied { reason: Locked }` (unchanged —
  the door is locked, full stop);
- actuating the dead controller → `ActuationFailed { reason: Unpowered }`
  (new reason, §4.5) — the log says the controller has no power, so the
  player knows the lock is not going to open from this side, and can go
  find the circuit.

Tested in the plant-room level (§11.1). The alternative (fail-safe: a dead
lock opens) is a gameplay decision for its own ADR; the default is the safe
one.

---

## 8. World truth / device readouts / actor knowledge

| Plane | Lives in | Examples | May be written by |
|---|---|---|---|
| **Truth** | `BuildingState` + embedded `RuntimeState` + `SignalState` + `MediumState` + `FabricState`, one snapshot | door open/closed, circuit powered, line flowing/severed, leak present, cell material, valve open, Billy's mode | the sim's systems (fixed order) and validated command application only |
| **Readouts** | pure functions, computed on demand, never stored | `signal_readout()`, `ScreenView`/`ScreenDevice.status`, `medium_readout()` (served/leaking per room), Bevy `view()` output, the console's `Estimate` | nobody — they are projections; a stale one is a bug, a *false* one is the next plane |
| **Knowledge** | `Intel` (Fix/Estimate), Billy belief meters, `AgentSession.logs`, mole coprocessor readings, `VibrationReading` | "the camera saw someone in office at t", "Billy believes it was the note", "the mole's scanner sees 2 objects ahead" | the observation system (step 8) — **including compromised sensors**, which write *false* fixes/estimates/readings here and nowhere else |

The compromised-sensor pattern, made a contract:

1. A compromised camera/terminal/sensor is **truth that the sensor node is
   compromised** (existing `Terminal` `Compromised` output; a `dead_nodes`
   / signal-disabled state for cameras) plus a *policy* (authored or
   wirable) about what it reports while compromised.
2. Step 8 emits its Fix/estimate **through the override**: the override may
   relocate, age, or drop the fix, and may add fixes for people who are not
   there.
3. The only assertions the sim makes: the override ran in step 8; it wrote
   only knowledge fields; truth (door, power, flow, cell, valve) is
   byte-equal to the run without the compromise on every other field.

This is testable as a differential: same seed, same commands, one run with a
compromised sensor — assert truth-equality on all non-knowledge fields and
knowledge-inequality on the planted lies. Test level 2 (§11.2) ships this
assertion.

---

## 9. Versioning and validated UMS authoring surfaces

### 9.1 Runtime formats — no migration

- `BuildingDefinition` stays format `idaptik-scenario/1`. New top-level
  fields `mediums: Vec<MediumDef>` and `fabric: Option<FabricDef>` are
  `#[serde(default, skip_serializing_if = ...)]` — every committed golden
  (`EXCHANGE_HOUSE_JSON`, `EXCHANGE_HOUSE_RUN_JSON`, contract fixtures)
  stays byte-identical. A format bump to `/2` is reserved for the day an
  *existing* field changes meaning; nothing in this design needs it.
- Snapshot: `idaptik-building-runtime-v1` keeps restoring (new state fields
  are optional-with-default, as `signal` already does on the scenario
  snapshot). The consolidated `BuildingRuntime` (stage 1) gets its own tag
  (e.g. `idaptik-building-runtime-v2`) because it composes a scenario
  snapshot inside it — same precedent as `EXPORT_FORMAT` embedding
  `SNAPSHOT_FORMAT`.
- `Command`/`Event` additions are serde-tagged enums: existing tags are
  untouched, new variants append. The netplay `seat_for` function is the
  forcing function (it must not compile with an unassigned seat) — building
  commands get seats when they land (stage 5): `Excavate`/`SeverLine` are
  the infiltrator's physical verbs; building `Actuate` (valves included) is
  the hacker's seat, exactly as `NetHack` is.
- The wire contract for UMS (`contracts/idaptik/v1`) is untouched by
  stages 0–4. **Contract v2** (stage 5) adds a building payload slot:
  envelope carries `building: BuildingDefinition` (the real Rust type, as
  v1 carries `scenario`), taxonomy gains `medium_line`, `fabric_cell`,
  `valve_device`; the loader runs the extended validation; the UMS-side
  `idaptik-edit/1` verbs (`add_medium`, `add_line`, `set_valve`,
  `add_fabric_cell`, `set_material`, …) mirror the Rust shapes 1:1 — the
  game owns the vocabulary (ADR-0007), UMS owns the editor.

### 9.2 Authoring surfaces

| Surface | Format | Gate |
|---|---|---|
| Building payloads (UMS DLC) | versioned JSON (`idaptik-scenario/1` + optional fields) | `load_building`: format tag → full named-check validation report (extended with `mediums.*` / `fabric.*` check families) |
| Typed level config (hand-authored floors) | Nickel (`config/*.ncl`) | `just config-check`: closed-record schemas mirroring the Rust types, negative fixtures |
| Golden fixtures | committed pretty JSON, `include_str!` | round-trip tests + ignored regeneration tests (existing pattern) |
| Edit scripts (UMS) | `idaptik-edit/1` JSON | UMS-side schema; the game's loader is the final gate — an edit script that produces an invalid building is refused by `load_building` |
| Descriptive metadata, policy contractiles, and coordination instructions | `IDApTIK_chora.deed`, `.machine_readable/contractiles/`, and root `coordination.k9` | pinned DEED/K9/coordination conformance gates |

---

## 10. Current machine-readable metadata boundary

IDApTIK keeps three machine-readable roles separate:

- `IDApTIK_chora.deed` records descriptive repository state and evidence pointers.
- `.machine_readable/contractiles/` contains K9 policy components and their paired Nickel runner descriptors.
- Root `coordination.k9` contains agent-coordination instructions; `0-AI-MANIFEST.deed` is the pointer-only startup deed.

DEED is the current descriptive repo-metadata format and the successor to A2ML, which is retired and must not be extended. A2ML is mentioned below only as historical migration context. The actual standard, as of this writing:

- **Normative grammar:** `deed.abnf` **v1.2.0**, `hyperpolymath/standards`
  `1-formats/deed/spec/abnf/deed.abnf` (the ABNF wins over the informative
  EBNF). S-expression surface: one top-level form, `;;`-SPDX header lines,
  `:keyword value` fields, named clauses, values are string / symbol /
  integer / `#t`/`#f` / `#u5"…"` / `#u8"domain:name"` / quoted / list. No
  `[section]`, no `key = value`, tabs invalid.
- **Four document forms**, dispatched by filename stem: `estate_chora.deed`
  → `estate-deed` (the estate vocabulary); `*_chora.deed` (stem ≠ `estate`)
  → `repo-deed` (what a repo *is*: a record); `0-AI-MANIFEST.deed` →
  `repo-deed` (the canonical machine front door); `*_praxis.deed` →
  `praxis-deed` (what a tool *does*: rules with explicit `:priority`).
- **Identity:** `repo-deed` requires `:canonical-name`, `:repo-uuid` (UUID8
  in domain `gv-clade-index`, name `github.com/<owner>/<repo>`; UUID5 legal
  until re-mint) and `:beholding-chora` (UUID5 — vocabulary is declared in
  the estate chora and *nowhere else*). `praxis-deed` requires
  `:canonical-name` and `:beholding-chora`; rule precedence is carried by
  `:priority` integers, never by file position. `:schema-version` STRING
  exactly once per form.
- **Status, stated honestly:** the pinned normative DEED ABNF is v1.2.0;
  the accompanying informative grammar spec remains DRAFT and not ratified.
  IDApTIK's migration completed on 2026-10-25 (the wider estate campaign is
  tracked by standards#837). Validation uses the pinned
  `1-formats/deed/tools/deed_lint.js` v1.2.0; `deed_lint.py` is frozen at
  ABNF v1.0.0 and does not parse `#u8`, so it is not the repository's gate.

### What uses DEED here (and what never does)

| Artifact | Form | Why DEED is right |
|---|---|---|
| Descriptive repository record: identity, component routing, implementation status, and evidence pointers | `repo-deed` (`IDApTIK_chora.deed`) | DEED is the estate's current record-of-what-a-repository-is format; this is the single project deed |
| Repository policy and contractiles | K9 components under `.machine_readable/contractiles/`, paired with Nickel runner descriptors | K9 holds the active policy clauses and probes; the pinned contract and validator provide conformance |
| Agent coordination instructions | Root `coordination.k9` | The coordination protocol is a separate format, not a DEED or K9 component |
| Historical building-authoring policy sketch | `idaptik-building-authoring_praxis.deed` in this directory | Non-authoritative design example only; active policy must live in the K9 contractiles |
| — | — | **Not DEED:** building/scenario payloads (versioned JSON, golden-pinned, serde-validated), runtime state and snapshots (serde, format-tagged), Nickel level config (typed authoring with its own gate), the `Command`/`Event` wire (serde enums), and any in-game content. Inventing a deed form for those would be exactly the "invented syntax for unrelated content" this section exists to forbid. |

### Historical DEED examples (non-authoritative)

`docs/architecture/deed-examples/idaptik-building-authoring_praxis.deed` is a
pre-migration design example retained for historical context. It is not active
policy: repository policy belongs in the K9 contractiles. The live root deed
is authoritative for descriptive state; the pinned v1.2.0 `deed_lint.js` gate
is documented in `docs/standards-conformance.md`. The repo-deed snippet below
is likewise illustrative and may lag current source:

```
;; SPDX-FileCopyrightText: 2026 IDApTIK contributors
;; SPDX-License-Identifier: CC-BY-SA-4.0
(repo-deed
  :schema-version "1.0.0"
  :canonical-name "IDApTIK"
  :repo-uuid #u8"gv-clade-index:github.com/catachresis-colloquium/IDApTIK"
  :beholding-chora #u5"estate/chora"

  (identity :owner "catachresis-colloquium" :primary-forge github)

  (routing
    :building-truth "crates/idaptik-core/src/scenario/building/"
    :grounded-network "crates/idaptik-core/src/netsim/"
    :signal-wiring "crates/idaptik-core/src/scenario/signal/"
    :floor-derivation "crates/idaptik-core/src/scenario/floor_graph.rs"
    :companion "crates/idaptik-core/src/companion/"
    :views "crates/idaptik-bevy/src/"
    :lockstep "crates/idaptik-net/"
    :ums-contract "contracts/idaptik/v1/")

  (status
    :evidence-source "IDApTIK_chora.deed"
    :building-systems "Historical design-sketch status only; consult the current root deed and source-backed inventory above."))
```

Migration status: completed for this repository on 2026-10-25. The live
layout is `IDApTIK_chora.deed` for descriptive state, six K9 component/runner
pairs in `.machine_readable/contractiles/` for policy, and root
`coordination.k9` for agent instructions. They are repository metadata, not
game content, and nothing in the simulation reads them.

---

## 11. Three small test levels

Each level is a Rust authoring module (the `exchange_house.rs` pattern), a
committed golden JSON (round-trip tested), a scripted command run, and
assertions on the event sequence and final truth. Each embeds exactly one
Billy floor (validation requires it) using the sanctioned
`FloorDef::from_scenario` projection of `ghost_lobby()`.

### 11.1 "The Plant Room" — power × gas × fabric

The smallest building that lets every pipe-and-brick interaction bite.

- **Floors:** `hall` (0): `lobby`, `boiler-room`; `service` (−1): `crawl`;
  `ghost-lobby` (1, Billy): the embedded scenario.
- **Portals:** `p-front` (lobby→boiler-room, Door, 2 s); `p-hatch`
  (lobby→crawl, Ladder, 5 s); `p-vent` (crawl→boiler-room, Vent, 10 s,
  lock `lock-vent` on circuit `plant`).
- **Circuits/zones:** `plant` (source `feed-plant`, zone `ot`,
  subnet `10.61.30.` covering service + boiler rooms).
- **Medium (gas):** source `boiler-0` (`Boiler` node) on circuit `plant`;
  one line `gas-main`: boiler-room → hall, valve `valve-0`, passing through
  duct cell `(col 4, band hall)`.
- **Fabric:** 8 columns × 2 bands (`[service, hall]`); cell `(4, hall)` is
  `Duct`; cell `(2, service)` is `Wall`; columns 0 and 7 are `Foundation`
  in band `service`; the rest `Fill`.
- **Script and assertions (in order):** (the level runs on the consolidated
  runtime of stage 2, so seat commands carry the reach gate — the hacker
  acts from the van, pivoted to the `ot` zone's bridge node.)
  1. Mole digs the duct cell (three `Excavate`s over `dig_time`) →
     `LineSevered { gas-main }` then `LeakStarted { hall }` (flow was on).
     Truth: gas line severed; leak in hall.
  2. Alert rises in hall; the Billy floor's Billy forms an `Investigate`
     toward it (existing mode, new fuel).
  3. Hacker actuates `valve-0` (the `Actuate` path on a `ToggleValve`
     node) — flow stops — `LeakStopped { hall }`. **A closed valve stops a
     leak; only a severed *flowing* line leaks.**
  4. Hacker cuts `feed-plant` → `PowerCut { plant }` +
     `MediumDead { gas }`; the `lock-vent` controller dies. Now:
     `Actuate(lock-vent)` → `ActuationFailed { reason: Unpowered }`, and
     `Traverse(p-vent)` → `TraversalDenied { reason: Locked }` (fail-secure,
     §7.5) — the door is *still* locked, and the two denials read apart.
  5. Mole digs the wall cell `(2, service)` → `Breached {
     portal: breach-service-2 }`; the breach portal is now in the physical
     graph (a route query finds it); `Traverse` through it works.
  6. Mole digs `(0, service)` → `FabricDenied { reason: Foundation }`;
     the cell is unchanged.
  7. Console readout for the OT zone shows `LEAK` in step 1–2 and clear in
     step 3 (readout plane), while truth (severed set) is unchanged by the
     readout.
- **Invariants asserted:** snapshot round-trips; two runs, byte-identical
  event logs; no truth field other than the ones named above changed.

### 11.2 "The Laundry Riser" — signal × air × knowledge

The wiring puzzle, and the compromised-sensor differential.

- **Floors:** `service` (−1): `riser-shaft`; `ghost-lobby` (1, Billy) — the
  embedded scenario, with its `laundry` room.
- **Air:** source `compressor-0` (`Compressor`) on circuit `hvac`
  (source `feed-hvac`, zone `ot`); one line `air-riser`: riser-shaft →
  laundry, valve `valve-vent` (authored **closed**), no cell (a structural
  riser — not excavatable).
- **Portal:** `p-laundry-vent` (riser-shaft → laundry, Vent, 14 s; with
  air serving both ends, 7 s via `air_vent_factor = 0.5`).
- **Signal:** the `ghost_lobby_wired` fixture network plus the valve as a
  device (`SignalKind::Valve`, node `valve-vent`, room laundry) — the same
  `NodeId` the air line's valve field names: one physical valve, joined
  three ways.
- **Script and assertions:**
  1. Air is pressurized but the valve is closed — no flow; the
     infiltrator's first riser traversal enters at 14 s.
  2. Hacker pivots onto the `bms-bridge` terminal (existing `Pivot`/`NetSsh`
     — standing on it is what drives `Terminal`'s `Compromised` output,
     `sim/signal.rs`); a player-laid chain (2 of the fixture's 3-wire
     budget) — `bms-bridge.Compromised` → relay (Latch) →
     `valve-vent.Open` — fires one tick after the compromise (the
     one-wire-hop-per-tick contract); `Switched { valve-vent, Active, on }`
     then the line flows and both vent ends are served;
     `Traverse(p-laundry-vent)` on the second trip enters at 7 s.
  3. Cut `hvac` → the compressor dies (`MediumDead { air }`) → the vent is
     14 s again next trip (flow is derived every tick, never remembered).
  4. **Compromised camera:** the laundry camera is wired off *and* a
     compromise override plants a door-passage fix in `office` while the
     infiltrator stands in `laundry`. Assert: `Intel.infiltrator.room ==
     office` (knowledge), the console screen says office (readout), the
     infiltrator's truth position is laundry, and *every truth field is
     equal to the un-compromised twin run* (the differential of §8).
- **Invariants asserted:** valve open/close is a *wiring* effect (a
  `SignalEvent`), the flow change is a *medium* effect (a `BuildingEvent`),
  and the two never appear as one fused event — the systems stay distinct
  in the log too.

### 11.3 "Exchange House, All Systems" — the canonical building, full stack

`exchange_house()` extended, proving the identities hold at building scale.

- **Additions to the canonical definition:** water (`tank-0` in
  `service/plant`, circuit `service`, line plant→lobby), gas (`boiler-0` in
  `service/plant`, circuit `service`, line plant→tellers through duct cell
  `(3, service)`), air (`compressor-0` in `service/plant`, circuit `main`,
  line plant→office); fabric over the service band with the gas duct cell,
  a wall cell between plant and crawl, and foundations at the edges; the
  mole hosted at `service/plant`; the ghost-lobby floor carries the wired
  signal fixture.
- **Script (summary):**
  1. Mole: `MoveTo` under the teller duct cell; `SabotageCable` with
     target **`feed-main`** — the mole event's `cable` id *is* the
     circuit's `source` id (the stable-identity join, asserted literally:
     `event.cable == def.circuit("main").source`).
  2. Handler: `CableCut { cable: feed-main, effect: PowerCut }` on tick N;
     the cascade — `PowerLost { nodes }` — lands in step 4 of tick N+1 (the
     one-tick latency rule, §7.2; the level asserts the exact spacing). The
     lifts are now `Unpowered` (denied, as in the canonical script); the
     teller lock controller dies *before* anyone unlocks it, so `p-tellers`
     stays sealed (fail-secure changes the canonical ending — a *new*
     scripted ending through `p-vent-tellers`, whose controller is on the
     still-live `service` circuit).
  3. Mole digs the gas duct cell → `LineSevered` + `LeakStarted { tellers }`
     → gas alert on the Billy floor; the Billy floor's run proceeds under
     it (belief, patrol, extraction) — the embedded scenario and the
     building's systems share one tick.
  4. Water line valve closed by the infiltrator sitting at the plant
     console and *wiring* a wire into `valve-0.Close` (the wired-console
     path of `signal_access` — the console screen itself stays look-only,
     as the existing model says) while the tank is pressurized: no leak
     (nothing severed), console `OK`.
- **Invariants asserted:** full snapshot (building + embedded scenario +
  medium + fabric + mole) round-trips and restores to a tick-equal sim;
  the event log is byte-identical across two runs and across headless and
  Bevy parity runs; the identity joins (mole cable → circuit source; valve
  node → medium line → signal device, all the same `NodeId`) are asserted
  as equalities, not by string coincidence.

---

## 12. Historical staged implementation plan

This plan was written before the power and infrastructure source additions
landed on 2026-10-25. It is **not the current roadmap**; the stage table records
the original design intent and has not been re-audited item-by-item. Current
facts belong in the source-backed inventory above and `IDApTIK_chora.deed`.
The remaining multi-runtime integration and fabric work are still gaps.

The original plan aimed to avoid changing existing golden bytes except by
deliberate regeneration; each stage was intended to be additive and independently revertible.

| Stage | Content | Ships with | Gate (definition of done) |
|---|---|---|---|
| **0 — Identity hardening** | `NodeId`/`MediumId`/`MediumLineId` newtypes at the five existing join sites (serde-transparent); minting centralised (only `floor_graph` / `building::network` format node ids); `IdIndex` extensions; validation check family `nodes.resolve` (every authored node reference resolves against the *derived* graph); appended `DeviceKind`s (`Boiler`, `Tank`, `Compressor`, `Valve`) and `Actuation::ToggleValve` (no behaviour yet) | nothing gameplay-wise | all committed goldens byte-identical (round-trip tests prove it); wire tags unchanged (serde-transparent tests, mirroring `ums_parity_kinds_have_stable_wire_tags`); CI green; fuzz test still passes with garbage ids |
| **1 — Fabric + consolidated tick skeleton** | `FabricDef`/`FabricState`, `CellKey`, materials, `Excavate`/`FabricDenied`, breach derivation (`breach-{F}-{c}`), Dijkstra on demand with unioned breaches, validation (`fabric.*` family: bounds, alignment, foundation, caps); **the `BuildingRuntime` tick (§7.2) lands here as the skeleton** — the §7.1 systems it composes already exist; the skeleton adds fabric settle (step 3) and the reach-gated seat commands, and it hosts the embedded scenario floor from day one; Bevy debug overlay (hatched cells, dig progress, breach markers — pure `view()`), TUI debug line | a standalone fabric level (crawl + breach + foundation, no media) | fabric assertions pass; no-behaviour-change proof: (a) the canonical exchange-house script run through the skeleton is event-identical to standalone `BuildingSim`, and (b) a scripted floor run through the skeleton is event-identical to standalone `GhostLobbySim`; goldens unchanged; bounds test: 512-cell definition validates, 513 fails |
| **2 — Media** | `MediumDef`/`MediumLineDef`/`MediumState`, derived flow + served + leak, `SeverLine`/`LineSevered`/`Leak*`/`MediumDead`, plant/valve nodes in the consolidated derivation, console `LEAK` statuses, per-medium effects (gas alert + Billy draw, vent factor, water optional), validation (`mediums.*` family) | **test level 1 "The Plant Room"** (complete, §11.1) | all level-1 assertions incl. fail-secure and leak-stops-on-closure; two-run byte-identical logs; no cascade longer than one BFS per medium |
| **3 — Valve as signal device + knowledge seam** | `SignalKind::Valve` (ports, snake_case wire tag appended), valve device joins `MediumLineDef.valve` by the same `NodeId`; compromised-sensor override in step 8 (policy: authored `compromised_report` on sensor nodes), differential test harness | **test level 2 "The Laundry Riser"** (§11.2) | valve opens by *wire* (SignalEvent) and by *net hack* (Effect) — both in the log as distinct systems; vent factor measured at 14 s / 7 s; the differential proves truth-equality / knowledge-inequality |
| **4 — Mole hosting** | mole hosted at 60 Hz (dt = TICK_DT) inside the consolidated tick's behaviour step; cable target = `NodeId` with the three legal kinds; fabric-masked `MoveTo`; vibration/scan routed to knowledge only | **test level 3 "Exchange House, All Systems"** (§11.3) | the mole's chewed cable *is* `feed-main` by id-equality; cascade + leak + breach + Billy floor in one tick; full snapshot round-trip |
| **5 — Boundaries** | building commands on the `Command` wire with seats (`seat_for` forced); resync carries the consolidated snapshot; contract **v2** (building payload slot, 3 new taxonomy terms); UMS `idaptik-edit/1` extended verbs (UMS-side PR, game-side loader gate); Bevy debug projections complete (medium edges coloured by kind, fabric grid, truth-vs-readout toggle); **DEED surfaces** (repo chora updated + `idaptik-building-authoring_praxis.deed` linted into CI, §10); burble role table verified/extended | the three levels as a CI suite (headless + Bevy parity + netplay loopback) | parity tests pass for building scripts; loopback seats byte-equal to headless; UMS round-trip of a building package (the UMS-side counterpart of ADR-0007); DEED files lint-clean under the estate-pinned linter |

Sequencing logic: 0 makes the joins type-safe so 1–4 never need a string
fix-up; the consolidated tick lands at 1 *as a skeleton* because media
(stage 2) and the valve join (stage 3) need a real tick to settle in, and
the skeleton's no-behaviour-change proof keeps that landing cheap; 2 before
3 because a leak needs flow before a valve is worth wiring; 3 before 4
because the mole's cable legality includes valves only once the valve join
exists; 5 last because it is the only stage that changes what netplay can
carry and what UMS can package — both are boundary work that should land
once the inside is stable.

---

## 13. Not decided here

1. Fail-safe locks (dead controller opens the door) — needs its own ADR and
   a design pass on how a player *tells* a dead lock from a locked one
   without omniscience.
2. Water's physical effects (slowdown, short-circuiting wiring) — deferred
   to a gameplay ADR; the default is readout-and-alert only.
3. Whether mediums can carry *actors* (a gas-main crawl) — the current
   model says no; vent portals are the traversal, the medium is the
   property.
4. Multi-mole / companion packs — the host seam is single-mole; the
   companion module already has `MoleParams.id` for the day this changes.
5. The DEED `praxis` rule sub-language (`:when`/`:then`) — the spec marks it
   provisional; the examples here use only the shipped `:priority`/`:text`
   rule shape.
