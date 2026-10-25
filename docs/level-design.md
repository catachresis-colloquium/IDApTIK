# Level design integration — 2.5D side-on, multi-floor, UMS-wired

This is the synthesis view: how the pieces that landed on `main` combine into one
coherent level-design picture — a **2.5D side-on** cross-section, a **multi-floor
Exchange House** building, and procedural **wiring driven via UMS**. It is a map of
the seams, not new design; each section links the module that owns the truth.

## The spine: definition-as-data over a deterministic sim

Everything below hangs off one decision (ADR-0004): a level is **data**, and the
simulation is a **pure, event-sourced function** of `(definition, config, seed,
command stream)`. The ordered `Event` stream is the canonical artifact; the same
`Command`/`Event` wire is spoken by every frontend and by the multiplayer relay.

Consequences that make the rest of this document possible:

- Every layer — layout, building, network, actors, companion — is **serde
  definition-as-data** with a committed golden JSON and a round-trip test. Content
  can therefore be authored (or generated) *outside* the engine and loaded in.
- Determinism means the renderer, the TUI, and the relay are all **thin views**
  over the same run — none of them can disagree with gameplay truth.

## 2.5D side-on layout

`crates/idaptik-bevy/src/scene.rs` renders the *side-on 2.5D cross-section of the
building — Gunpoint-like, flat-shaded quads for readability over art*. Scenario
coordinates are canvas-style (x right, y **down**, floor line at `world.floor`);
`SceneMap` converts to Bevy's y-up. Rooms are one horizontal row of flat quads,
laid out purely from the room definitions; doors slide up out of the walk line,
and camera view-cones are drawn from the sim's exact sweep formula
(`CAM_SWEEP_W`/`CAM_SWEEP_A`) so the picture can never disagree with detection.

The **multi-floor seam is already in the mapping**: `SceneMap::row_lift` stacks
additional floor rows vertically (`ROW_STACK = 440.0`) *"so the coming multi-floor
Exchange House building runtime slots in as extra rows without touching the
mapping."* The renderer is a pure view: `SimDriverPlugin` steps `GhostLobbySim` at
`SIM_HZ = 60` in `FixedUpdate`, folds a `CommandQueue` into a `TickInput`, and all
sync systems read `sim.state()`/`sim.definition()` (ADR-0003/0006).

## The reference floor: Ghost Lobby

`crates/idaptik-core/src/scenario/ghost_lobby.rs` — *Envelope 001 — Ghost Lobby*
(`scenario_id: envelope-001-ghost-lobby`, wire tag `idaptik-ghost-lobby-v2`). A
single-floor asymmetric infiltration:

- **Roles.** *Infiltrator* (moves through five rooms), *Hacker* (remote support —
  opens doors, cuts lights), and **Billy**, a guard NPC whose belief meter
  *interprets player behaviour* to decide what was stolen.
- **Objectives** (`ObjectiveKind`): `Note` (secure the contact note),
  `Misdirect` (convince Billy it was the drive), `Exit` (service exit / laundry
  chute). The win is **misdirection**, not mere escape.
- **World.** Five contiguous rooms tiling `x ∈ [20, 1260)` on the floor line
  `y = 585`: `kitchen`, `hall`, `office` (the lit "USB trap"), `laundry`, `exit`;
  doors D1–D4 on interior boundaries; cameras (laundry's is `stale` and never
  detects). The constants are dual-homed — `pub const` source of truth projected
  into the definition, with a `json_roundtrip` test asserting fidelity.

### The played opening: from the roof

`crates/idaptik-core/src/scenario/rooftop.rs` builds the floor the windowed
game plays (`ghost_lobby_rooftop`): the wired Ghost Lobby with an optional
`ExteriorDef` attached. The infiltrator starts on the **roof**, with two ways in:

- the **rope** tied off at the roof's front edge: abseil down the facade to the
  street, then through the **front door** (`D0`, on the kitchen's street wall).
  It is locked: the infiltrator picks it from the street (`PICK_TIME`, with a
  little noise), or the hacker opens it like any other door;
- the **hatch** over the laundry: quiet to reach, but it drops you in at the
  far end of the floor, and the landing is heard.

Outside (roof, rope or street) no camera sees the infiltrator, Billy cannot
see or grab them through the outer wall, and support is the exterior's own
(the van is in plain sight). Billy's arrival clock waits until they are inside.
The body's place (`Floor` / `Roof` / `Rope`) and a picked lock are serialised
only when set, and `exterior` only when present, so the canonical floor's
golden data and every replay are unchanged.

The frontend frames each seat by the **space** its character is in, not by the
level or the character: a room close (three screen pixels per art pixel), the
outside — street, facade, roof — wide (two), the inside of the van closest
(five).

## Multi-floor: the Exchange House building runtime

`crates/idaptik-core/src/scenario/building/` generalises a scenario into *one floor
of a data-defined building* (`BuildingDefinition`, format `idaptik-scenario/1`):

- **`FloorDef`** — `level: i32` (storey; ground = 0, service negative), a
  `FloorKind` of `Standard` / `Billy` / `Service`, its `rooms`, and an optional
  embedded `scenario: ScenarioDefinition`. Validation enforces **exactly one
  `Billy` floor** per building.
- **`PortalDef`** — `Door` / `Stair` / `Lift` / `Ladder` / `Vent`, bidirectional,
  with a `travel_time`, an optional `lock { controller }` (a network node
  disengages it), and an optional `circuit` (an unpowered portal can't be
  traversed; **lifts must declare a circuit**).
- **`CircuitDef`** — a power feed keyed to a `zone`; cutting its `source`
  de-powers every portal and controller drawing from it. It is the building-level
  precedent for the scenario-level power network below: same idea (one feed, one
  source, one set of loads), one boolean instead of a model.
- **`ZoneDef`** — a named region with a dotted `/24 subnet`, mapped **1:1 to a
  netsim segment**.

**"Ghost Lobby as one floor"** is literal: `FloorDef::from_scenario` embeds a whole
`ScenarioDefinition`, and validation (`scenario.rooms_match`) enforces that the
floor's rooms mirror the scenario's exactly; `scenario_floor_portals()` projects
the scenario's *interior* doors into building portals (a door on the world's outer
edge projects nothing — it is the scenario's own extraction surface). In
`exchange_house.json` the stack is `service` (−1: plant, crawl), `exchange`
(0: lobby/hall/tellers), `office` (1: landing/bullpen/manager), `ghost-lobby`
(2: `kind: Billy`, the full embedded scenario + four interior door-portals). Entry
is `exchange/lobby`; portals include lifts on circuit `main`, stairs, a service
ladder, and vents (one into the ghost-lobby laundry).

`BuildingSim` is again a pure function of `(definition, command stream)` — commands
`Traverse(PortalId)` / `Actuate(String)`; events `Entered` / `TraversalDenied` /
`Unlocked` / `PowerCut` / `ActuationFailed`, with deny reasons
`UnknownPortal` / `NotAdjacent` / `Locked` / `Unpowered`. It exports five surfaces
(definition, runtime snapshot, a legacy flat level config, an after-action
debrief, and the combined export), and a deterministic physical graph
(`reachable_rooms()`, Dijkstra `shortest_path()`).

## The wiring and plumbing: the grounded network

`crates/idaptik-core/src/netsim/` is *one vantage-keyed graph running from the wider
internet down to the devices in the room the infiltrator is standing in*. This is
the "wiring/plumbing" a level contains, and most of it is **derived, not authored**
(`scenario/floor_graph.rs`):

- **Devices** (`device.rs`, `DeviceKind`): Laptop, Router, Server, IotCamera,
  Terminal, PowerStation, Ups, Firewall, SmartDoor, Camera, Lock, Elevator, Light,
  Sensor, Substation — each with an ordered `SecurityLevel`
  (Open < Weak < Medium < Strong) and an `Actuation` (HoldDoor, DisengageLock,
  LoopCamera, CallElevator, KillLights, CutPower, MuteSensor, RunVacuum, …).
- **Derivation.** Only the network *backbone* is hand-authored (in
  `ghost_lobby_floor.json` / `config/ghost_lobby_floor.ncl`); every fixture is
  derived from the scenario `def`: each door → a weak `SmartDoor`, each room → a
  `Light`, each camera → a `Camera` on the shared `security` segment, the vacuum →
  a `Sensor`. Every fixture draws from a building `feed` fed by a `substation`, so
  **cutting the substation cascades to the whole floor** (test
  `cutting_the_substation_cascades_to_every_fixture`).
- **Vantages** give the two seats different attack surfaces: the hacker plays from
  a `Van` (higher reach, lower physical risk); an infiltrator standing in a room
  gets an `Inside` vantage on that room's segment (short reach, high risk). A level
  offers distinct lines — a one-hop **building line** vs a two-hop **upstream power
  line** to the substation.

This grounded-network fusion is what took the scenario wire format to `v2`.

## Actors as data ("Billy as data")

`crates/idaptik-core/src/scenario/actor/` ports *enemies as data*
(`idaptik-actors/1`). An `ActorArchetype` is 29 scalar FSM `stats` (`StatId`) plus
per-object `InterestProfile`s; a `Modifier` is an ordered list of `StatOp`
(`Add`/`Mul`/`Set`) that `compose()` applies deterministically (unknown ids are a
typed `ComposeError`, never a panic). **Billy** is projected from the same
`scenario::constants` numbers, expressed as data; his interests encode the decoy
economy (`note` = `Objective` signal 0.9; `usb` = `Decoy` signal 0.75). The default
registry ships Billy plus canonical `veteran` and `skittish` modifiers — the unit
from which UMS actor packs extend.

## The companion seam: Moletaire

`crates/idaptik-core/src/companion/` is Moletaire, the robotic-mole companion
(`idaptik-moletaire/1`), a deterministic port (typed `MoleCommand`/`MoleEvent`, one
seeded `Mulberry32` stream): depth-based movement, a gravity-model `hunger` system
that periodically resists the controller, seven equipment slots, and a five-slot
**coprocessor** augment bay (AudioSynthesiser, PathOptimiser, SignalProcessor,
VibrationAnalyser, StabilisationCore, each on a four-rung Stock→MK-III ladder).

**Status — seam, not yet wired.** Moletaire is deliberately standalone: a hosting
floor would drive it *through the typed boundary only* (`SynthSoundPlayed` → dogs
within 250px investigate; `VibrationDetected` → the hacker's view;
`UndergroundScanComplete` → a scan overlay). It is landed and test-covered as a
module, but **not yet integrated into `GhostLobbySim`** — treat it as a ready seam.

## The UMS seam: where procedural content plugs in

UMS (the Unified Modding Studio) is **not a code dependency** here — it is a
content/DLC **load seam**. Procedural generation lives *above* this crate; the
crate owns the format gate and validation. Three parallel, version-gated entry
points:

| Payload | Entry point | Format |
|---------|-------------|--------|
| Building / scenario-definition | `scenario/building/mod.rs::load_building` | `idaptik-scenario/1` |
| Actor pack | `scenario/actor/mod.rs::load_actor_pack` | `idaptik-actors/1` |
| Companion | companion definition-as-data | `idaptik-moletaire/1` |

Each `load_*` **format-gates on `payload.format` then validates** before the runtime
trusts the content. The building model is also the ground truth for an
`idaptik-edit/1` **edit-script contract** (metadatastician/universal-modding-studio) whose verbs
(add/move/remove floors, rooms, portals, circuits, zones) target exactly these ids
and shapes: *"the ids and shapes here are that contract's ground truth."* There is
no in-repo `dlc-manifest` or procedural generator — that is UMS-side, and it feeds
these validated load points.

## Multiplayer and host window

- **Relay (ADR-0005, landed).** The Elixir `SessionChannel` is **relay-only**: it
  fans byte-preserving `command`/`event` wire enums between the two seats and reads
  only the routing tag. Determinism (ADR-0004) makes lockstep sound for this
  cooperative vertical slice, so it does not need a host authority; this does
  not rule out hosted/federated authorities specified in ADR-0014.
- **Host + transport (ADR-0006, design-accepted, deferred).** The plan wraps the
  Bevy frontend in the **gossamer** desktop shell (a webview host, *not* Tauri) and
  carries the Phoenix socket over **burble** transport for the two-player windowed
  slice. This is the churniest integration glue and lands **last**; it is not yet
  implemented.

## What is live vs. what is a seam

| System | Status |
|--------|--------|
| Deterministic definition-as-data sim (Ghost Lobby) | **Live**, test-covered |
| Multi-floor Exchange House building runtime | **Live**, test-covered |
| 2.5D side-on renderer (multi-floor row-stacking ready) | **Live** |
| Grounded network / derived fixtures / power cascade | **Live**, test-covered |
| Actor archetype + modifier registry ("Billy as data") | **Live**, test-covered |
| Session relay (Elixir, relay-only lockstep) | **Live** |
| Moletaire companion | Landed module — **seam, not yet wired into the sim** |
| UMS content load (`load_building` / `load_actor_pack`) | **Live** load points; generator is UMS-side |
| Signal network (ADR-0012) | **Live** in the sim (opt-in floor), Bevy wiring overlay; netplay pending burble |
| Electrical infrastructure (ADR-0013) | **Live** in the sim (opt-in floor, test floor only); no frontend overlay, netplay pending burble |
| gossamer host + burble transport (ADR-0006) | **Design-accepted, deferred** |

## See also

- ADR-0013 — electrical infrastructure: sources, feeds, loads, protection, backup
  and exposed power (`docs/electrical-infrastructure.md`)
- ADR-0004 — deterministic, event-sourced, definition-as-data scenario sim
- ADR-0005 — session relay topology
- ADR-0006 — gossamer host window + burble transport (design)
- `docs/scenarios/ghost-lobby.md` — the prose model of the reference floor
- Modules: `scenario/building/`, `scenario/actor/`, `netsim/`, `companion/`
