<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Water, gas and airflow as building systems

Status: the runtime, definition format, three fixtures and their tests are in
`crates/idaptik-core/src/scenario/infrastructure/` and
`crates/idaptik-core/tests/infrastructure.rs`. The Nickel authoring schema is
**(proposed)** and not written yet.

This document is the design of record for the three systems. It states what is
modelled, what is deliberately qualitative, and how a player is meant to read
and undo what the model does.

- [1. Scope and delivery order](#1-scope-and-delivery-order)
- [2. What already existed and is reused](#2-what-already-existed-and-is-reused)
- [3. What this model is not](#3-what-this-model-is-not)
- [4. Shared vocabulary](#4-shared-vocabulary)
- [5. Water](#5-water)
- [6. Air, gas, scent and noise](#6-air-gas-scent-and-noise)
- [7. The declared coupling between the systems](#7-the-declared-coupling-between-the-systems)
- [8. Traversability is not flow](#8-traversability-is-not-flow)
- [9. Accounting, bounds and non-negativity](#9-accounting-bounds-and-non-negativity)
- [10. The authoring and player surface](#10-the-authoring-and-player-surface)
- [11. Hazards: cue, recovery, consequence](#11-hazards-cue-recovery-consequence)
- [12. Designing for experimentation](#12-designing-for-experimentation)
- [13. Fixtures and tests](#13-fixtures-and-tests)
- [14. Integration points and next steps](#14-integration-points-and-next-steps)
- [15. Open questions](#15-open-questions)

## 1. Scope and delivery order

The goal is three systems that are **distinct, interoperable and readable**:

| System | Owns | Does not own |
|---|---|---|
| Water | reservoirs, pipes as capacities, valves, outlets, leaks, drains, room accumulation, spill between rooms | pressure waves, pipe fill time, pipe freezing, water quality |
| Air | a fixed air volume per room, a pressure scalar, ducts, fans, dampers, filters | turbulence, buoyancy, temperature, real fan curves |
| Gas | fictional species carried by air, sources, capture, combustion, absorption | chemistry, real concentrations, toxicity modelling |
| Traces | scent and noise carried by the same air movement | sound propagation, smell perception |

Delivery order was fixed by the design brief and is reflected in
`fixtures::all()`: **water alone first**, then **ventilation alone**, then the
**combined** fixture. A combined model that hides which half is failing is
worse than two models a designer can test separately.

## 2. What already existed and is reused

The building layer already had the right concepts; the infrastructure layer
imports them rather than inventing a parallel world:

| Existing concept | Where | How infrastructure uses it |
|---|---|---|
| Rooms as `RoomRef { floor, room }` | `scenario/ids.rs`, `building/mod.rs` | A compartment id is `"<floor>/<room>"`; `CompartmentId::as_room()` converts back |
| Portals and their locks | `building/mod.rs` (`PortalDef`, `PortalKind`) | An opening may name the portal that owns its **traversal**; the lock stays the building's business |
| Circuits and zones | `building/mod.rs` (`CircuitDef`, `ZoneDef`) | Fans and equipment name the circuit that powers them; `sync_circuit` is the seam |
| Power-cut cascade | `netsim::apply_actuation` | A dead circuit stalls a fan and de-powers equipment, without a second power model |
| Named validation checks with typed errors | `BuildingValidationReport` | `InfrastructureValidationError` and `validate_against_building` |
| Definition-as-data + snapshot + typed command/event | `scenario/sim`, `signal/` | Same shape: `idaptik-infrastructure/1`, `InfrastructureSim::{new, restore, apply, step, snapshot}` |
| Determinism rules | ADR-0004, ADR-0012 | Integers only, no RNG, no wall clock, `skip_serializing_if` on optional sections |

Nothing that already existed was changed to make this fit: a scenario or
building that does not carry an `infrastructure` section stays byte-identical.

## 3. What this model is not

The model is **qualitative**. `model_declaration()` travels inside every
snapshot so a UI, a replay tool or a modder cannot mistake it for physics:

- `class: "qualitative-game-model"`;
- a one-paragraph statement;
- an explicit `not_simulated` list.

Deliberately not simulated:

1. fluid dynamics, pressure waves, and pipe fill/empty time;
2. real gas concentrations, toxicity, explosibility limits or temperatures;
3. gas buoyancy, stratification or temperature-driven airflow;
4. sound propagation and smell perception (traces move, they are not heard);
5. water quality, corrosion, or electrical conduction through anything but
   authored conductivity;
6. pipe and duct wall leakage other than the authored fixtures;
7. weather, or any exterior process beyond the authored boundary values.

All quantities are abstract **units**, pressures are per-mille around
`PRESSURE_BASE = 1000`, concentrations are per-mille of room air, and
conductivity is per-mille of `MAX_CONDUCTIVITY = 1000`. None of these numbers
is a safety threshold or engineering advice.

## 4. Shared vocabulary

### Compartments

A compartment is either a `Room` in the building or an authored **exterior**
boundary. Exteriors are named `exterior/<name>`, carry no body, hold pressure at
`PRESSURE_BASE`, accept unlimited water, and record every crossing in a ledger.
They are how "the storm drain", "the roof" and "the yard" exist without
becoming rooms.

### Openings

An `OpeningDef` has **three independent channels**, and this independence is the
core of the design:

| Channel | Field | Meaning |
|---|---|---|
| Traversal | `traversal`, optional `portal` | People can pass while it is open. Locks never stop media |
| Air | `air_conductance` | Air — and everything carried in it — equalises pressure |
| Water | `water_conductance` | Standing water spills between the compartments |

A door with traversal and air conductance but no water conductance is a normal
door on a raised threshold. A grille with air but no traversal is a transfer
grille. A hatch with water and traversal is a flooded stairwell. The three
channels are never derived from one another.

### Tiers, cues and hazards

`Tier { Clear, Trace, Warning, Critical }` is shared by hazards and cues so the
UI has one colour ladder. Every hazard has one stable cue:

| `HazardKind` | Cue |
|---|---|
| `Water` | `Puddle` |
| `Gas(species)` | the species' authored cue (`Haze`, `Ignition`, `Hiss`) |
| `Visibility` | `Haze` |
| `Pressure` | `PressureWhine` |

`HazardChanged` is emitted only when a tier **changes**, in either direction, so
a log reads as "the room got worse" / "the room is clear again" rather than as a
per-tick number.

## 5. Water

### Entities

- **Reservoirs** — the only authored water sources: `capacity`, `initial`,
  `supply_per_tick`, `conductivity`. An explicit source, never conjured.
- **Junctions** and **pipes** — pipes are directed **capacities** in a graph
  (`capacity` units/tick). Water inside a pipe is not modelled; a pipe is a
  route, not a store.
- **Valves** — one per pipe, open/closed. A pipe carries nothing while its valve
  is shut. Two valves on one pipe is a validation error.
- **Outlets** — a room fixture that releases up to `rate` units/tick into its
  compartment while open.
- **Leaks** — an unplanned outlet, patchable; `start_patched` lets the author
  decide whether the player inherits a problem or causes one.
- **Drains** — a room sink with `capacity` units/tick into an authored
  **exterior** target only. A drain that targets a room is rejected.

### Per-tick rules (fixed order)

```
drains  →  spill through openings  →  reservoir supply
```

1. **Drains.** For every clear drain whose equipment is available
   (`drain_available`), take up to `capacity` units from its room and add them to
   `drained_to_exterior`. Dissolved gas leaves with the water
   (`species_drained`).
2. **Spill.** For every open opening with water conductance, if the higher side
   holds more than `spill_units`, move `min(conductance, excess above spill,
   half the difference, destination free capacity)` units downhill. Into an
   exterior, the amount is unlimited and recorded as `spilled_to_exterior`.
3. **Supply.** Each reservoir with water remaining is run through an
   Edmonds–Karp max-flow over the pipe graph:

   ```
   source = reservoir
   sink capacity of an outlet = min(rate, room free capacity)
   sink capacity of a leak   = min(rate, room free capacity), if unpatched
   edge capacity             = pipe.capacity, 0 if its valve is shut
   ```

   The solution writes `outlet_flow` / `leak_flow` and places the water in the
   destination rooms. `supply_per_tick` caps how much a reservoir may push in
   one tick; `reservoir_remaining` is the tank it draws from. Explicit recharge
   is reserved (`WaterLedger::recharged` stays zero) because a refilling tank is
   a new source the author should have to declare.

Nothing else moves water. There is no "water appears because a room is dry".

### Bounds

- Every transfer is clamped by **available source matter**, **destination free
  capacity** and **route capacity**. `WaterState::free_capacity` is the single
  place that answers the second question.
- `initial` above `capacity` is a validation error, and no process refills a
  reservoir implicitly.
- `take_water` returns what was actually taken (plus the dissolved gas that came
  with it), so a caller cannot debit more than the body holds.
- `add` refuses to exceed `water_capacity` of the compartment.

### Ledger

`WaterLedger { initial_reservoir, recharged, drained_to_exterior,
spilled_to_exterior }`, and `WaterState::balanced()` asserts

```
initial_reservoir + recharged
  == water_in_reservoirs + water_in_compartments
   + drained_to_exterior + spilled_to_exterior
```

### Stylised parameters (`WaterTuning::default()`)

| Field | Default | Meaning |
|---|---|---|
| `trace_units` | 1 | any standing water is visible |
| `warning_units` | 150 | a serious puddle |
| `critical_units` | 400 | a flooded room |
| `spill_units` | 40 | depth below which water does not cross an opening |
| `traversal_block_tier` | `Critical` | the tier at which `traversal_blocked` becomes true |

These are game numbers chosen for readability, not litres.

## 6. Air, gas, scent and noise

### Entities

- **Air body per room** — fixed `air` volume plus a pressure scalar
  `PRESSURE_BASE * air / volume` (per-mille). Air can be moved between rooms or
  to/from an exterior, never created.
- **Ducts** — a link between two compartments with `capacity`, optional
  `crawlable` and a hatch. A duct is **not** traversable just because air moves
  through it.
- **Fans** — at most one per duct; `flow`, optional circuit, `start_on`,
  `reversible` with a start direction. Flow is `min(duct.capacity, fan.flow)`
  scaled by the damper.
- **Dampers** — one per duct, position 0–100, scaling flow proportionally.
- **Filters** — one per duct, `efficiency` per-mille and `capacity`. Load is
  tracked **per species**, so a filter can be spent on haze and still pass
  ember.
- **Gas sources** — a finite reservoir of one species in one room, released up to
  `release_per_tick`. Being finite is the point: a source is a budget the player
  can spend, empty or close.
- **Trace sources** — finite scent/noise emitters in a room.
- **Equipment** — a powered fixture in a room with an optional controlled target
  (a fan or a drain), `spark_when_live`, an optional `trip { at,
  conductivity_min }`, and `start_tripped`.

### Species

`GasClass { Haze, Hush, Ember }` are **fictional gameplay categories**:

| Class | Behaviour | Cue |
|---|---|---|
| `Haze` | obscures visibility when it reaches a tier | `Haze` |
| `Hush` | a calmant-like category; no extra physics | `Hiss` |
| `Ember` | the only flammable category; ignites at `Critical` | `Ignition` |

Not every gas is flammable, by construction: `GasClass::flammable()` is true for
`Ember` only. Solubility is per species, so not every gas dissolves in water.
Thresholds are authored per species in per-mille of room air, and must be
strictly ordered (`trace < warning < critical`).

### Per-tick rules (fixed order inside `air_step`)

```
release sources → ducts (fans) → openings (equalisation) → trace decay → combustion
then: absorption by standing water, then hazard diffing
```

1. **Sources** release up to `release_per_tick`, bounded by their remaining
   reservoir, into their room's body; the ledger records
   `species_released` / `traces_released` and `GasSourceEmptied` fires once.
2. **Ducts.** For each duct with a running fan:
   - unpowered → stall `Unpowered`;
   - damper position 0 → stall `DamperClosed`;
   - flow is clamped by the source room's lower pressure bound
     (`SourceStarved`) and the destination's upper bound (`TargetFull`);
   - the moved air carries species and traces **proportionally**;
   - the filter, if any, captures `efficiency` per-mille of **every** species in
     the moved air, bounded by its remaining capacity; traces pass through
     unfiltered, and `FilterSaturated` fires once when the load reaches capacity.
3. **Openings.** Two rooms at different pressures exchange air through
   `air_conductance`, clamped exactly as in a duct. Exteriors hold pressure at
   `PRESSURE_BASE`, so a pressurised room leaks to the outside and a starved room
   draws from it — both bounded and both recorded.
4. **Trace decay.** Scent and noise decay by `trace_decay_per_mille`
   (`AirTuning::default() = 20`, i.e. 2%/tick), with at least one unit removed
   from a non-zero trace so it eventually disappears.
5. **Combustion.** For each un-tripped, powered, `spark_when_live` fixture: every
   flammable species **at `Critical`** in that room is destroyed, recorded in
   `species_combusted`, announced as `Combustion` plus a `Cue { Ignition,
   Critical }`, and every other un-tripped fixture in the room trips with
   `TripCause::Ignition`.
6. **Absorption.** Standing water absorbs each species at
   `water_units * solubility / 1000` units per tick, up to what is actually in
   the air; the gas becomes `dissolved` in the water and travels with it when it
   drains, spills or is pumped.

### Air accounting

`AirLedger` records `species_released`, `species_combusted`, `species_captured`,
`species_absorbed`, `species_drained`, `species_spilled`, `traces_released`,
`traces_decayed`, `air_to_exterior`, `air_from_exterior`. `air_balanced()`
checks the air carrier; the species rows are per-species budgets of the same
shape.

## 7. The declared coupling between the systems

The systems meet in exactly three named places, and nowhere else:

1. **Conductive water trips equipment.** `settle_equipment_trips` reads the
   water tier and conductivity of an equipment's room and trips it if the
   authored `trip` threshold is met. Water does not know what a fan is; it only
   knows a room's tier and conductivity.
2. **Water absorbs and carries gas.** `absorb_step` lets standing water take
   soluble species; drains and spills carry the dissolved gas out of the
   building, recorded per species.
3. **Equipment can be the water's own way out.** A `Drain` target means a pump
   can be tripped by the water it is standing in — a designed trap, not an
   emergent surprise.

That is the whole interface. There is no shared solver, no shared units, and no
path by which a duct reads a pipe.

## 8. Traversability is not flow

A duct can carry 100 units of air per tick and still be uncrawlable; a flooded
room can be impassable while a dry duct beside it is passable. The runtime
reports the two facts separately and never derives one from the other:

- `passable_openings()` lists openings that are open **and** `traversal`;
- `crawlable_ducts()` lists ducts that are `crawlable` **and** whose hatch is
  open;
- `CompartmentConditions::traversal_blocked` is set by
  `water_blocks_traversal`, using `WaterTuning::traversal_block_tier`.

The building layer owns locks. An opening's `traversal` never changes when a
door is locked, and a locked door still passes air and water: locks are a
permission, not a seal.

## 9. Accounting, bounds and non-negativity

| Guarantee | Mechanism |
|---|---|
| No negative quantity | Every field is `u64`; debits go through `take_water` / `take` helpers that return what was taken |
| No accidental material creation | Sources are authored reservoirs; every addition is written to a ledger entry |
| No silent loss | Every removal is a ledger entry: drain, spill, capture, combustion, absorption, decay, exterior crossing |
| No over-capacity | `free_capacity` and `air` bounds clamp every transfer |
| No runaway sums | `MAX_QUANTITY = 1_000_000_000` per authored quantity; `saturating_*` on every accumulation |
| No unexplained change | `InfrastructureSim::conservation_report()` returns machine-readable lines, and `accounting().balanced` is a single boolean |

`conservation_report()` and `WaterState::balanced()` / `AirState::air_balanced()`
are exercised by `crates/idaptik-core/tests/infrastructure.rs`; the same test
file asserts that every fixture round-trips through serde unchanged.

## 10. The authoring and player surface

### Commands

| Command | Verb for the player | Reversible? |
|---|---|---|
| `SetValve { valve, open }` | open/close a stopcock | yes |
| `SetOutlet { outlet, open }` | open/close a tap or standpipe | yes |
| `PatchLeak { leak }` | patch a leak | no (the patch is the fix) |
| `ClearDrain { drain }` | clear a clogged drain | no (clearing is the fix) |
| `SetDamper { damper, position }` | throttle a duct | yes |
| `SetFan { fan, on }` | run/stop a fan | yes |
| `SetFanDirection { fan, direction }` | reverse a reversible fan | yes |
| `ReplaceFilter { filter }` | swap a loaded filter | yes, but the load is disposed |
| `ResetEquipment { equipment }` | reset a tripped fixture | yes |
| `SetHatch { duct, open }` | open a crawl hatch | yes |
| `SetGasSource { source, on }` | open/close a canister | yes |
| `SetTraceSource { source, on }` | open/close a scent or noise source | yes |

`sync_opening` mirrors a building door's open state; `sync_circuit` mirrors a
power cut. Commands that cannot apply return `Denied { reason }` rather than
being silently ignored: `UnknownFixture`, `NotReversible`, `NotCrawlable`.

### Non-destructive interactions the model is designed to afford

- **Drain a route.** Clear a drain, patch the leak feeding it, and a flooded
  corridor becomes passable again.
- **Redirect airflow.** Reverse a fan or close a damper to move haze, scent or
  noise away from a guard, or to starve a fire of fuel by extracting it.
- **Clear visibility.** Run the extract fan to bring a hazy room below its
  warning tier, and watch the `Haze` hazard return to `Clear`.
- **Move a cue.** Scent and noise ride the same air; a redirected duct relocates
  the hint as well as the hazard.
- **Restore equipment.** Reset a tripped fixture once its room is dry; the fan
  comes back only if the circuit is live.
- **Create alternative access.** Open a crawlable duct's hatch or an authored
  hatch opening; neither is implied by the medium flow.

## 11. Hazards: cue, recovery, consequence

Every hazard has a cue the player can perceive before it is fatal, a recovery
verb, and a consequence if ignored.

| Hazard | Cue | Recovery | Consequence |
|---|---|---|---|
| Standing water | `Puddle` at `Trace`, deeper at `Warning` | clear drain, patch leak, close valve | `Critical` blocks traversal; conductive water trips equipment |
| Pressurised spray | `Spray` cue from the outlet/leak source | close valve, patch leak | room fills, spill spreads to neighbours |
| Haze | `Haze`, visibility tier | run extract fan, open a grille | obscured room: visibility hazard masks the room |
| Ember | `Ignition` on combustion | close the gas source, vent the room, reset equipment | instant combustion, equipment tripped, cue and event |
| Hush | `Hiss` | ventilate | fictional calmant effect is a host-side consumer of the tier |
| Pressure | `PressureWhine` | open a damper/door, run or stop a fan | stall (`SourceStarved`/`TargetFull`), unreadable airflow |
| Loaded filter | `FilterSaturated` event (`FilterLoaded` cue for the UI) | replace filter | gas stops being captured and spreads |
| Tripped equipment | `EquipmentTripped` event + `TripCause` | `ResetEquipment` when the room is safe | fan or pump stays dead, cascade stalls |

Note what "consequence" means here: the model never kills anyone itself. It
publishes a tier, a cue and a typed event; the host decides what a tier does to
the player, Billy, or the score. That keeps the hazard vocabulary reusable and
keeps the sim deterministic.

## 12. Designing for experimentation

1. **Causality is readable.** Every state change is either a command, a named
   flow with a ledger entry, or a typed event. There are no hidden timers and no
   RNG.
2. **Every action is reversible or explicitly a fix.** Closing a valve undoes
   the outlet; patching a leak and clearing a drain are one-way repairs, and
   that is legible.
3. **Failure is informative.** A stalled fan says why (`Unpowered`,
   `DamperClosed`, `SourceStarved`, `TargetFull`); a denied command says which
   id was unknown.
4. **Hazards degrade rather than jump.** Tiers are `Clear → Trace → Warning →
   Critical`, and `HazardChanged` fires on every crossing so a UI can teach the
   scale.
5. **Nothing is one bad tick from disaster.** In the water fixture, the player
   can flood the store for ten ticks and still recover: the warning arrives
   before `traversal_blocked`.
6. **Systems are testable apart.** A water-only definition has no air state at
   all (`Option` sections, skipped in serde), and vice versa.

## 13. Fixtures and tests

`crates/idaptik-core/src/scenario/infrastructure/fixtures.rs` authors three
definitions with the same helpers:

| Fixture | Proves |
|---|---|
| `minimal_water` | reservoir → junction → valve → outlet; a patchable leak; a clogged drain to an exterior; spill through a door |
| `minimal_ventilation` | duct + reversible fan + damper + filter; haze and ember species; finite gas and trace sources; a sparking fixture with a trip |
| `combined` | both systems on one plan; power cut stalls the fan; water trips the starter; soluble `hush` is absorbed by standing water |

`crates/idaptik-core/tests/infrastructure.rs` asserts: each fixture validates,
each round-trips through the wire format, water is conserved and causal,
ventilation clears and filter loads, ember ignites only while equipment is
live, the combined fixture couples through the trip, snapshots replay
identically, and unknown ids are denied rather than ignored.

The runtime itself is `InfrastructureSim::{new, restore, apply, step, snapshot,
conditions, all_conditions, duct_flow, passable_openings, crawlable_ducts,
accounting, conservation_report}`; the definition loader is
`load_infrastructure`.

## 14. Integration points and next steps

### Hosted in a scenario (intended)

The `signal` layer shows the pattern: an optional `infrastructure` section on the
scenario definition and an optional infrastructure state on the runtime state,
both `skip_serializing_if`, so existing goldens and replays stay byte-identical.
Each tick would call `InfrastructureSim::step` after the existing timers and feed
`HazardChanged` into the event log. **(proposed)** — not wired yet.

### Building link (intended)

`validate_against_building` already checks compartments against rooms, openings
against portals and circuits against the building. Two follow-ups are needed for
a full link: `PortalDef` has no aperture/conductance field, so an authored
`OpeningDef` currently has to restate its portal; and a door's lock state is
mirrored manually via `sync_opening`.

### Nickel authoring schema (proposed)

`config/infrastructure-schema.ncl` would mirror the serde shapes and the
semantic checks, in the style of `config/scenario-schema.ncl`, with `bad_*.ncl`
negatives for: a room with no body, a drain targeting a room, two valves on one
pipe, an unreachable outlet, duplicate ids, thresholds out of order, a damper
above 100, and a filter on a nonexistent duct.

### Presentation (intended)

The Bevy frontend can draw `CompartmentConditions::cues` directly: a puddle
decal, a haze overlay, a hiss audio loop, a pressure whine, a filter-loaded icon.
None of this is required for the model to be correct.

## 15. Open questions

1. **Door aperture.** Should `PortalDef` grow an `air_conductance` /
   `water_conductance` pair so a door does not have to be restated, or should
   the infrastructure section own all media and the building own all traversal?
   The second is cleaner conceptually; the first is less authoring work.
2. **Hush's effect.** Its cue and tiers exist; what a `Critical` hush tier does
   to a player is a host decision not yet made.
3. **Scent and noise as evidence.** The model carries them, but no guard
   perception reads them yet.
4. **Multi-floor spill.** Water currently spills only through authored
   openings, so a stairwell must be an opening rather than a geometry rule. This
   is a deliberate simplification; whether it survives contact with level design
   is open.
5. **Tuning ranges.** The defaults are first guesses; they should be re-derived
   from playtests before they appear in level design docs.
