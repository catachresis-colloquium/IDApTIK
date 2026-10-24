# ADR-0013: Water, gas and airflow are three coarse building systems, not a fluid solver

- Status: Proposed
- Date: 2026-10-24
- Relates to: ADR-0004 (deterministic event-sourced sim), ADR-0012 (signal
  network), `docs/design/water-gas-airflow.md`, `docs/level-design.md`

## Context

The building runtime knows rooms, portals, locks, circuits and zones. The
grounded network knows reach and power cascade. Neither knows anything about
the *medium* a room is full of. Level design wants flooded stairwells, haze in a
corridor, a ventilation duct that can be crawled, a scent trail a guard could
follow, and equipment that dies when the water reaches it.

Two tempting shortcuts are wrong for this game:

1. **One "environment" scalar per room.** Cheap, but it erases the very
   distinctions the design brief asks for: water is not gas, a duct is not a
   door, and a puddle does not conduct electricity because it is wet.
2. **A fluid solver.** Physically satisfying, but unreadable, non-deterministic
   in practice, and impossible to author: a level designer cannot reason about
   Navier–Stokes, and a player cannot learn causality from it.

## Decision

Add water, air and gas as **three distinct, coarse, integer, deterministic
systems** in `crates/idaptik-core/src/scenario/infrastructure/`, interoperating
through a small declared interface.

| Piece | Module | Contract |
|---|---|---|
| Definition + validator | `infrastructure/mod.rs`, `water.rs`, `air.rs` | `InfrastructureDefinition`, format `idaptik-infrastructure/1`, `load_infrastructure` (the UMS seam) |
| Runtime | `infrastructure/sim.rs` | `InfrastructureSim::apply` / `step`; snapshot `idaptik-infrastructure-snapshot/1` |
| Fixtures | `infrastructure/fixtures.rs` | water-only, ventilation-only, then combined |
| Tests | `crates/idaptik-core/tests/infrastructure.rs` | conservation, causality, coupling, snapshot replay, denials |
| Design | `docs/design/water-gas-airflow.md` | the model declaration, rules, cues and recoveries |

### The model

- **Compartments** are building rooms (`"floor/room"`) or authored exteriors
  (`"exterior/<name>"`). Exteriors are reservoirs, not rooms.
- **Water** is volume in rooms plus an explicit reservoir. Pipes are *capacities*
  in a directed graph; water inside a pipe is not modelled. Valves gate pipes;
  outlets and leaks add to rooms; drains remove to an authored exterior; open
  openings spill downhill.
- **Air** is a fixed volume per room plus a pressure scalar. Ducts move air only
  while an authored fan runs; dampers scale it; filters capture a bounded
  fraction of each species; openings equalise passively.
- **Gas** species are fictional categories (`Haze`, `Hush`, `Ember`) with
  authored per-mille thresholds. Only `Ember` is flammable; only `Ember` ignites,
  and only at `Critical`.
- **Scent and noise** are traces carried by the same air movement, so
  redirecting a duct redirects the information as well as the hazard. They decay.
- **Traversability is a separate channel** on every opening. A duct can carry
  air without being crawlable, and a flooded room can be impassable while a dry
  duct beside it is not.
- **Every hazard has a stable cue** (`Puddle`, `Spray`, `Hiss`, `Haze`,
  `PressureWhine`, `FilterLoaded`, `Ignition`, `Gurgle`) and a defined recovery
  or consequence.

### Non-negotiables encoded in the runtime

- All quantities are `u64`. There are no floats, no RNG, no wall clock.
- Every transfer is clamped by source availability, destination free capacity
  and route capacity; `take_*` returns what was actually taken.
- Every removal is a ledger entry (`drained_to_exterior`,
  `spilled_to_exterior`, `species_captured`, `species_combusted`,
  `species_absorbed`, `traces_decayed`, exterior crossings).
- Every source is authored. Nothing is created by the tick.
- `model_declaration()` travels in every snapshot with an explicit
  `not_simulated` list, so the payload cannot be mistaken for physics.

### What is deliberately absent

Full fluid dynamics, pressure waves, pipe fill time, temperature, buoyancy,
real concentrations, toxicity, sound propagation and smell perception. Each is
listed in the declaration rather than left to the reader.

## Consequences

- A designer authors abstract units and per-mille thresholds; playtests tune
  them. They are not engineering figures and must never be documented as such.
- A player can learn one ladder (`Clear → Trace → Warning → Critical`) and one
  cue vocabulary for all three systems.
- Water and air remain testable apart: a water-only definition carries no air
  state at all, and the fixtures are delivered in that order.
- The systems meet in exactly three places (conductive-water trips, absorption
  and dissolved transport, equipment controlling a drain). New coupling needs a
  new decision here, not a side channel.
- The building layer still owns traversal and locks: an opening's `traversal`
  never changes when a door is locked, and a locked door still passes media.
- Hosting the runtime in a scenario, the Nickel authoring schema, and the Bevy
  cues are **(proposed)** follow-ups; see `docs/design/water-gas-airflow.md`
  §14.
