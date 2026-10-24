<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Infrastructure interactions — the design of record

**Status: design of record. Not implemented.** There is no
`scenario/infrastructure` module, no `SystemInfrastructure`, no
`SignalKind::PlantActuator`, and none of the test names below exist in the
tree. This document specifies behaviour precisely so that the implementation
can be written against it and checked. Where it states something about the code
that *does* exist, the file and function are named; where it states something
about the layer being designed, the word is *must*.

| Claim | Status |
|---|---|
| The tick, system order and signal propagation described in §1 | existing code, cited |
| The data model, rules, ordering and cascade bounds in §2–§7 | proposed |
| The tests in §15 | specified, not written |
| The two test levels | specified in `infrastructure-level-1.md`, `infrastructure-level-2.md` |

Navigation: rules live in
[`infrastructure-rule-catalogue.md`](infrastructure-rule-catalogue.md); the
decision is [ADR-0013](../adr/0013-infrastructure-interaction-rules.md). The
rule catalogue is the authority for *what each interaction does*; this document
is the authority for *how any interaction is evaluated*.

---

## 1. Where this sits

### 1.1 Three networks, three questions

| Layer | Question it answers | Module | Status |
|---|---|---|---|
| `netsim` | *Who can reach what?* | `crates/idaptik-core/src/netsim/` | live |
| `signal` | *What happens when they get there?* (boolean, wired) | `scenario/signal/` | live (ADR-0012) |
| **infrastructure** | *What does the building physically do?* | `scenario/infrastructure/` | **this design** |
| `building` | *What space is it all in?* | `scenario/building/` | live |

The three stay distinct. A door is reachable over `netsim`, commandable over
`signal`, and physically a portal with a threshold, a seal and a motor. None of
those facts is duplicated into another layer: the infrastructure layer *binds*
to the others by id (`EntityDef.node`, `EntityDef.signal`, `gate: Portal(…)`).

### 1.2 The tick as it exists today

`crates/idaptik-bevy/src/driver.rs` pins `Time::<Fixed>::from_hz(60.0)` and runs
the sim in `FixedUpdate`. `GhostLobbySim::tick` (`scenario/sim/mod.rs`) is:

1. **P0** pause and restart immediates.
2. **P2** every other immediate, at the pre-increment `t` (uplink, pivot, SSH,
   NetHack, `Command::Signal`, test hooks).
3. **P3** `update(input)`: `t += TICK_DT; tick += 1`, then the systems in the
   fixed order of `scenario/sim/systems.rs`:
   `timers → signal → player → (vantage) → interactions → usb → vacuum →
   cameras → intel → support → behaviour → billy → collisions → objectives`,
   then POST: crisis timer, lockdown, trace.

`TICK_DT` is `1.0 / 60.0` and time is **accumulated**, never recomputed as
`tick / 60.0`. No system draws RNG except the reset roll.

The signal step has three properties this design must not disturb
(`scenario/signal/sim.rs::Run::step_body`):

- outputs are read from **state at the start of the tick**;
- inputs then change state, so **one wire is crossed per tick**;
- edge inputs fire once on the rising edge; relay inputs are level; a door
  `pulse` holds for `ROUTE_DURATION`.

`system_signal` runs immediately after `system_timers`, deliberately: "a lock
lands before anybody moves this tick".

### 1.3 The building host today

`BuildingSim` (`scenario/building/sim.rs`) is command-only: `apply(cmd)` and no
tick. A `Traverse` adds `portal.travel_time` to `clock` instantly and may deny
with `UnknownPortal` / `NotAdjacent` / `Locked` / `Unpowered`. Circuits are
`BTreeMap<CircuitId, bool>`; a portal is impassable when its circuit is cut.

The plant needs a clock, so the building host gains one:

```
BuildingSim::apply(cmd)          — unchanged entry point; queued as this tick's intent
BuildingSim::tick(&BuildingTick) — new: 60 Hz, B1..B3 below, returns Vec<BuildingEvent>
```

### 1.4 The insertion point

```
GhostLobbySim::update                       BuildingSim::tick
  system_timers                               B1  fold queued commands   (immediates)
  system_signal        ◄── reads world        B2  signal.step(readout)   ◄── reads world
  system_infrastructure ◄── NEW               B3  infrastructure.step    ◄── NEW
  system_player                               B4  portal/traversal checks read committed state
  … unchanged …
```

Three reasons, in priority order:

1. **Control before consequence, consequence before bodies.** The signal step is
   the players' expressed intent; the plant is what the building does about it;
   actors then move through the world the plant has just committed. A rising
   flood denies a traversal in the tick its depth crosses the sill, not one tick
   later.
2. **No new signal hop.** `signal_readout()` is computed at the top of
   `system_signal`. A plant commit at tick *T* is therefore read by the signal
   network at *T+1*, which is exactly the existing one-hop-per-tick cost. If the
   plant ran *before* the signal step, a sensor could reach an actuator in zero
   ticks and the propagation contract would be quietly changed.
3. **Nothing downstream reorders.** `system_player` and everything after it
   already read committed state; adding a reader of committed state ahead of
   them cannot change their order relative to each other.

The plant draws no RNG, so the RNG draw sequence — pinned by `rng_vectors` — is
untouched by its presence.

---

## 2. The model

Four data layers, one runtime. All of it hangs off
`BuildingDefinition.infra: Option<InfrastructureDefinition>` (`#[serde(default,
skip_serializing_if = "Option::is_none")]`), the same optional-sub-network shape
`ScenarioDefinition.signal` already uses.

Payload format: **`idaptik-infrastructure/1`**.

### 2.1 Layer A — compartments and links

A **compartment** is the unit of fluid bookkeeping. It is anchored to a building
room for cues, evidence and actor presence, but it is not the room: a duct run,
a sump or a riser shaft can be its own compartment inside one room.

```rust
// Illustrative shape; not yet compiled.
pub struct CompartmentDef {
    pub id: VolumeId,
    pub name: String,
    pub room: Option<RoomRef>,      // spatial anchor; None only when `exterior`
    pub air_l: u32,                 // free volume, litres
    pub floor_cm2: u32,             // depth_mm = water_ml * 10 / floor_cm2
    pub elevation: i8,              // -3..+3, used only for density ordering
    pub open_to_exterior: bool,     // a sealed room is a lethal room; author it
    pub exterior: bool,             // a sink (outside, sewer); holds no depth
    pub infiltration_permille: u16, // make-up air a fan may draw, 0 = sealed
    pub gas_init: BTreeMap<GasId, u32>,  // pure gas ml, at t=0
}
```

A **link** is a connection with *two independent behaviours*:

```rust
pub struct FluidLinkDef {
    pub id: LinkId,
    pub a: VolumeId,
    pub b: VolumeId,
    pub reach: LinkReach,        // Opening | Duct | Seep | Grate | VentStack
    pub aperture_cm2: u32,       // FLUID conductance
    pub capacity_ml_per_s: u32,  // rated flow; a link never exceeds it
    pub sill_mm: u32,            // fluid passes only above this depth on side `a`
    pub clearance_mm: u32,       // ACTOR clearance; 0 = actors never pass
    pub actor: ActorPassage,     // Passable | Blocked | NeedsGate
    pub gate: Option<GateRef>,   // Portal(PortalId) | Entity(EntityId)
    pub fluid_block: FluidBlock, // Never | AboveSill | WhenGateShut | WhenDebris
    pub noise_permille: u16,     // attenuation for cues/evidence crossing it
}
```

Water crosses a link at

```
flow_ml_per_s = min(capacity_ml_per_s,
                    aperture_cm2 × max(0, depth_mm − sill_mm) × WATER_LINK_K)
```

which is a weir with a ceiling: nothing flows until the water is over the sill,
the flow grows with the head above it, and it never exceeds the link's rating.

The two numbers do different jobs and are easy to confuse:

- `sill_mm` is a **liquid** quantity, and it is what `WA-1` also reads. Once the
  depth on the link's low side is above the sill, the opening is submerged, so
  the **gas path closes** — an opening full of water carries no air. Liquid is a
  different matter: a submerged opening still weirs water through it, subject to
  the rating. `WA-1` therefore closes a duct to air without being a valve for
  water. If a level wants a duct that carries air but no liquid, it authors
  `capacity_ml_per_s` to the trickle it should carry (a raised collar, a
  trapped mouth, a rated orifice) and the weir formula respects it. Level 2's
  `duct-low` does exactly this with a 50 ml/s rating.
- `capacity_ml_per_s` is the rating, and it binds in both directions; a link
  rated at 50 ml/s cannot become a drain because the head is large.

`aperture_cm2` and `clearance_mm` are the whole of the "fluid transport is not
traversal" rule. They are separate numbers on the same link, and the rules that
read them are separate rules (`WA-1` reads the aperture; `WA-2` reads
`clearance_mm`). Nothing infers one from the other.

Where a link mirrors a building portal, `gate: Portal(id)`: opening the door
opens the fluid path, and the door's own state is the only truth. The plant
never keeps a second copy.

### 2.2 Layer B — entities and typed capabilities

An entity is a physical thing with a position and a **capability set**. The
rules never read `kind`; `kind` exists for rendering, validation defaults and
the editor. That property is tested (`renaming_every_entity_preserves_behaviour`,
§15).

```rust
pub struct EntityDef {
    pub id: EntityId,
    pub name: String,
    pub kind: EntityKind,           // presentational; not read by any rule
    pub compartment: VolumeId,
    pub material: MaterialId,
    pub node: Option<String>,       // grounded-network node id (reach)
    pub signal: Option<SignalId>,   // signal-network device id (wiring)
    pub caps: Vec<Capability>,
    pub init: EntityInit,
}
```

The closed capability set (v1):

| Capability | Parameters | What it means mechanically |
|---|---|---|
| `Powered` | `circuit`, `load` | runs only while the circuit is energised |
| `Moves` | `medium`, `rate_ml_per_s`, `check_valve`, `return_ml`, `reversible` | transports a medium while running |
| `Gates` | `flow: Entity \| Link`, `positions` | position gates another entity's flow or a link's aperture |
| `Stores` | `medium`, `capacity_ml`, `accepts_ml_per_s`, `outlet` | holds a medium; accepts at a rate; may discharge to an outlet |
| `Carries` | `medium`, `pressurised`, `leak_ml_per_s` | a conduit; leaks at an authored rate |
| `Senses` | `quantity`, `threshold`, `hysteresis_permille`, `dwell_ticks` | reports a quantity crossing a threshold, after holding it |
| `Sounds` | `cue`, `level`, `radius_links` | emits attenuation-limited noise |
| `Ignites` | `class: IgnitionClass` | *may* produce a spark by its class when switching or damaged |
| `LockOut` | `series: Circuit \| Entity` | a local, physical disconnection with absolute precedence |
| `Breaks` | `target: Entity \| Link \| Portal`, `tool`, `hardness` | excavation or forced release: expose or sever a specific authored thing |
| `Alarms` | `cue`, `evidence` | raises an alarm and emits evidence |
| `Heats` | `delta_mk_per_s` | raises fire intensity / air temperature (fire only) |

`Senses.quantity` is closed: `WaterDepth`, `Wetness`, `GasConcentration(GasId)`,
`Pressure`, `Flow`, `GatePosition`, `Integrity`.

`IgnitionClass` is closed: `None`, `Static`, `Inductive`, `Arc`, `HotSurface`.
A light switch is `None`. A motor, contactor or pump start is `Inductive`. A
damaged, wet conductor is `Arc`. A heater or a fire is `HotSurface`. That single
authored property is why "any electrical toggle explodes" cannot happen.

### 2.3 Layer C — materials and gas species (authored, closed)

```rust
pub struct MaterialDef {
    pub id: MaterialId,
    pub absorbs_permille: u16,        // max wetness retained; 0 sheds water
    pub dries_permille_per_s: u16,
    pub water_film_conductive: bool,  // does a wet film on THIS material carry current?
    pub ignition: IgnitionClass,      // class it emits when it makes/breaks a circuit
    pub combustible_permille: u16,    // 0 = never sustains fire
    pub permeable: bool,              // passes seepage
    pub hardness: u8,                 // excavation resistance 0..10
}
```

```rust
pub struct GasDef {
    pub id: GasId,
    pub name: String,
    pub density_vs_air_permille: i16, // 1000 = neutral; >1000 sinks
    pub lfl_permille: u16,            // lower flammable limit
    pub ufl_permille: u16,
    pub ignition_classes: Vec<IgnitionClass>, // sources that can light THIS gas
    pub oxygen_min_permille: u16,
    pub oxygen_per_litre_permille: u16,       // ml of O2 consumed per ml of gas burned;
                                              // 2100 = 2.1 L O2 per L gas
    pub irritant_permille: u16,       // cue threshold
    pub incapacitate_permille: u16,   // actor dose threshold
    pub water: GasWaterRule,          // None | DisplacedOnly | Dissolves { coefficient_permille }
}
```

`GasWaterRule` is the *entire* water/gas chemistry of the game. Level 2's
fictional category `sable-2` declares `DisplacedOnly`; a later level may author
`Dissolves` for a different species and no engine code changes. Nothing
"neutralises" a gas unless the species says so.

The three fictional gases the levels need are authored per level, not in the
engine. The engine knows only the six fields above.

### 2.4 Layer D — rules

```rust
pub struct RuleDef {
    pub id: RuleId,
    pub version: u16,              // outcome-changing edits bump this
    pub title: String,
    pub template: Template,        // the closed mechanics set, §2.5
    pub scope: Scope,              // Entity | Volume | Link | Actor | Level
    pub when: Vec<Condition>,      // ALL must hold
    pub access: AccessReq,
    pub actor: Vec<ActorCapability>,
    pub consume: Vec<Amount>,
    pub produce: Vec<Amount>,
    pub delay_ticks: u32,
    pub period_ticks: Option<u32>,
    pub effects: Vec<Effect>,
    pub readout: Vec<ReadoutRef>,
    pub cues: Vec<CueRef>,
    pub evidence: Vec<EvidenceRef>,
    pub recovery: Recovery,
    pub irreversible: Vec<Irreversible>,
    pub cause: CausalTag,
    pub tests: Vec<TestId>,
    pub rate: Option<RateLimit>,
}
```

The container, the whole authored layer in one value:

```rust
pub struct InfrastructureDefinition {
    pub format: InfraFormat,              // "idaptik-infrastructure/1"
    pub ruleset_version: u16,
    pub materials: Vec<MaterialDef>,      // may reference the base set by id
    pub gases: Vec<GasDef>,               // authored per level; engine knows none
    pub compartments: Vec<CompartmentDef>,
    pub links: Vec<FluidLinkDef>,
    pub entities: Vec<EntityDef>,
    #[serde(default)]
    pub controllers: Vec<ControllerDef>,  // empty for a level with no control law
    pub rules: Vec<RuleId>,               // cited catalogue ids, in rule_rank order
    #[serde(default)]
    pub budgets: CascadeBudgets,          // §6 defaults when absent
}

pub struct ControllerDef {
    pub id: ControllerId,
    pub reads: Option<SignalId>,      // a bound sensor device, or None (law only)
    pub targets: Vec<EntityId>,
    pub law: ControllerLaw,           // threshold(s) on the quantity it reads
    pub hysteresis_permille: u16,
    pub dwell_ticks: u32,             // minimum run before the law may reverse it
    pub cooldown_ticks: u32,          // minimum off before it may re-assert
    pub rank: OriginRank,             // always R3 Control in v1
}
```

`rules` is a list of ids, never rule bodies, and the order is the `rule_rank`
used by the S2 sort. A level may cite only ids that exist in the catalogue at
the same `ruleset_version`; an unknown id is a load error, not a silent skip.
Two conventions follow from this:

- **every rule whose preconditions can arise in a level must be cited**, whether
  or not the level's walkthrough reaches it. A level does not get to forget
  that water conducts;
- the four cross-cutting rules `X-1`–`X-4` are **always active and are never
  cited**. They are the engine's own budget, trace and cue machinery, not
  content a level opts into.

`Condition` and `Effect` are **closed** enums. There is no expression language,
no scripting hook and no escape hatch: a condition the engine does not have is a
rule the level cannot express, which is a feature (it is a reviewable surface)
and not a bug.

Conditions v1:

```
Energised(circuit) | NotEnergised(circuit)
GatePosition(entity, Position)
WaterDepth(volume, Ge(n)) | Wetness(entity, Ge(n)) | Integrity(entity, Le(n))
GasIn(volume, gas, Ge(n) | Lt(n) | Between(lo, hi))
ActorIn(volume) | ActorTouching(entity) | ActorHas(capability)
FireIn(volume, Ge(n)) | LinkBlocked(link) | PortalOpen(portal)
Tag(entity, tag)                 // authored latch, e.g. "regulator shut"
```

Effects v1:

```
AddWater(volume, ml)             MoveWater(from, to, ml)
Wet(entity, permille)            Dry(entity, permille)
SetGate(entity, position)        SetPower(circuit, on, as_lockout)
SetIntegrity(entity, delta)      Destroy(entity)
MoveGas(from, to, gas, ml)       ConsumeGas(volume, gas, ml)   ProduceGas(volume, gas, ml)
SetFire(volume, intensity)       Extinguish(volume)
AlterLink(link, aperture_delta | clearance_delta)
SetTag(entity, tag)              ShockActor(dose)              Alarm(entity)
RaiseHazard(kind, level)         Ignition(class)
```

`RaiseHazard` and `Ignition` are the two effects only a `Hazard`-template rule
may use, and they are what make §3.3 step 6 a *candidate* list rather than a
verdict: step 6 tells the rules which subjects are worth considering
(`energised ∧ wet ∧ damaged` is an arc candidate), and the `T3` rule decides
whether a hazard exists and how hard it is. Nothing else may raise a hazard, so
a hazard in the state always has a rule and a cause behind it.

### 2.4a JSON encoding

The Rust shapes above are illustrative; the wire form is what a level file
contains and what validation reads, so the tags are pinned here. Discriminants
are snake_case in a `"kind"` field, and `"self"` is accepted wherever an entity
or a circuit is expected. Comparison operators are one field `op` with
`ge | le | between`, and the right-hand side is a named unit field
(`mm`, `permille`, `ml`), never an anonymous number.

| `Condition` | JSON |
|---|---|
| `Energised(c)` | `{"kind":"energised","circuit":"M"}` |
| `NotEnergised(c)` | `{"kind":"not_energised","circuit":"N"}` |
| `GatePosition(e, p)` | `{"kind":"gate_position","entity":"dmp-x1","position":"shut"}` |
| `WaterDepth(v, Ge)` | `{"kind":"water_depth","volume":"plant","op":"ge","mm":180}` |
| `Wetness(e, Ge)` | `{"kind":"wetness","entity":"pump-p1","op":"ge","permille":350}` |
| `Integrity(e, Le)` | `{"kind":"integrity","entity":"pump-p1","op":"le","permille":800}` |
| `GasIn(v, g, cmp)` | `{"kind":"gas","volume":"riser","gas":"sable-2","op":"ge","permille":40}` |
| `ActorIn(v)` | `{"kind":"actor_in","volume":"plant"}` |
| `ActorTouching(e)` | `{"kind":"actor_touching","entity":"valve-v1"}` |
| `ActorHas(cap)` | `{"kind":"actor_has","cap":"waders"}` |
| `FireIn(v, Ge)` | `{"kind":"fire","volume":"plant","op":"ge","permille":100}` |
| `LinkBlocked(l)` | `{"kind":"link_blocked","link":"vent-stack"}` |
| `PortalOpen(p)` | `{"kind":"portal_open","portal":"door-d5"}` |
| `Tag(e, t)` | `{"kind":"tag","entity":"regulator-r1","tag":"sealed"}` |

`between` carries `lo` and `hi` instead of a single value:
`{"kind":"gas",…,"op":"between","lo":40,"hi":130}`.

| `Effect` | JSON |
|---|---|
| `AddWater(v, ml)` | `{"kind":"add_water","volume":"plant","ml":500}` |
| `MoveWater(a,b,ml)` | `{"kind":"move_water","from":"plant","to":"lobby","ml":500}` |
| `Wet(e, ‰)` | `{"kind":"wet","entity":"pump-p1","permille":250}` |
| `Dry(e, ‰)` | `{"kind":"dry","entity":"pump-p1","permille":250}` |
| `SetGate(e, p)` | `{"kind":"set_gate","entity":"dmp-x1","position":"shut"}` |
| `SetPower(c, on, lock)` | `{"kind":"set_power","circuit":"M","on":false,"as_lockout":true}` |
| `SetIntegrity(e, d)` | `{"kind":"set_integrity","entity":"self","delta":-4}` |
| `Destroy(e)` | `{"kind":"destroy","entity":"self"}` |
| `MoveGas(a,b,g,ml)` | `{"kind":"move_gas","from":"riser","to":"plant","gas":"sable-2","ml":100}` |
| `ConsumeGas(v,g,ml)` | `{"kind":"consume_gas","volume":"riser","gas":"sable-2","ml":100}` |
| `ProduceGas(v,g,ml)` | `{"kind":"produce_gas","volume":"plant","gas":"sable-2","ml":100}` |
| `SetFire(v, ‰)` | `{"kind":"set_fire","volume":"plant","permille":400}` |
| `Extinguish(v)` | `{"kind":"extinguish","volume":"plant"}` |
| `AlterLink(l, d)` | `{"kind":"alter_link","link":"crawl-c1","aperture_delta":900}` |
| `SetTag(e,t)` | `{"kind":"set_tag","entity":"regulator-r1","tag":"sealed"}` |
| `ShockActor(d)` | `{"kind":"shock","dose":2}` |
| `Alarm(e)` | `{"kind":"alarm","entity":"alarm-a1"}` |
| `RaiseHazard(k,l)` | `{"kind":"raise_hazard","hazard":"arc","level":1}` |
| `Ignition(class)` | `{"kind":"ignition","class":"arc"}` |

The rule in §10 of the catalogue and the level files are written in exactly
this form. There is **no** gate for it yet: the examples are parsed as JSON at
review time and the check belongs with the loader tests in §15. Do not cite one
until it exists.

### 2.5 The eight templates

Everything in the catalogue is one of eight mechanics. If a proposed rule is not
one of these, it is a new mechanic and needs the same review a new system would
get.

| # | Template | What it does |
|---|---|---|
| T1 | Wetting | moves water onto a surface or into a store; tracks wetness decay |
| T2 | Energisation | resolves circuit power through series gates and sources |
| T3 | Hazard | a source plus conditions produce a periodic hazard (arc, fire onset) |
| T4 | Exposure | an actor in a hazard field accumulates a dose |
| T5 | Flow | bounded transfer of a medium between volumes or along a conduit |
| T6 | Breach | material integrity changes: expose, sever, or open a new link |
| T7 | Control | sense → decide → actuate, through the precedence ladder |
| T8 | Observation | cue, evidence and causal emission from any of the above |

### 2.6 Layer E — runtime state

```rust
pub struct InfrastructureState {
    pub ruleset_version: u16,
    pub tick: u64,
    pub volumes: Vec<VolumeState>,   // water_ml, gas_ml per GasId, fire, oxygen
    pub entities: Vec<EntityState>,  // position, energised, wetness, integrity, tags
    pub links: Vec<LinkState>,       // breached, blockage
    pub actors: Vec<ActorExposure>,  // shock dose, gas dose, contact
    pub hazards: Vec<HazardState>,   // kind, subject, level, since_tick, dwell
    pub scheduled: BTreeMap<(u64, u32), ScheduledEffect>,
    pub open_causes: Vec<Cause>,     // bounded; see §7
    pub next_cause: u32,
    pub deferred: Vec<StagedIntent>, // carried into next tick, never dropped
}
```

Every field is serde. `ruleset_version` is in the snapshot; a restore across a
ruleset change is a typed refusal, not a reinterpretation (§12).

---

## 3. Quantities, units and determinism

### 3.1 Integer units

| Quantity | Unit | Notes |
|---|---|---|
| water | millilitres (`u64`) | depth derived: `water_ml * 10 / floor_cm2` mm (1 ml = 1 cm³) |
| gas | ml (amount) and ‰ (concentration) | `permille = gas_ml / air_l` (the ml/L ratio *is* the permille) |
| wetness | ‰ of saturation, 0..1000 | |
| integrity | ‰, 0..1000 | 0 = destroyed |
| fire | ‰ intensity, 0..1000 | |
| oxygen | ‰ of air volume | starts at 210 |
| power | `bool` per circuit, `u16` load | |
| time | ticks (`u32`), 60 Hz | seconds only for display |
| dose | shock/gas, 0..1000 | |
| noise | 0..1000, attenuated per link | |

**Why integers.** A float sum depends on evaluation order near the rounding
edge; an integer sum does not. The evidence ledger already made this decision
for mass (`Mass = uint32 in 0..10_000`, `docs/architecture/
evidence-ledger-design.adoc`). The plant follows it. Floats are confined to the
existing scenario math (`TICK_DT`, camera sweeps) and to display conversion.

### 3.1a Gas is a species amount, not a well-mixed atmosphere

This is the one abstraction worth stating outright, because three separate
gameplay claims rest on it:

- a compartment holds `gas_ml` of each species, where `gas_ml` is **pure gas**;
  the concentration a detector or a player reads is `gas_ml / air_l`;
- an extractor removes the **species** at its rated flow — a 4 L/s fan clears
  4 L of gas per second from the compartment it serves, because its intake sits
  in the enriched layer. This is why extraction is worth doing at all;
- a passive link carries `aperture_cm2 × GAS_LINK_K` ml/s, which is slow. In a
  three-minute level, gas does not diffuse through a building; it is *moved* by
  fans, by doors, and by water displacing it.

There is no pressure field, no partial-pressure solver and no well-mixed
atmosphere. A link may still be closed to gas because water covers its opening
(`WA-1`) — that is a binary obstruction, not a fluid model.

Buoyant species (`density_vs_air_permille < 1000`) are **not supported in v1**;
validation refuses them (`gases.unsupported_buoyancy`). Neutral and sinking
species are the whole of the v1 vocabulary, and `sable-2` is sinking.

### 3.1b Displacement is conservation, not chemistry

When water takes volume `V` in a compartment, it displaces the same volume of
*mixture*, which carries `V × gas_ml / air_l` of the species into the
neighbouring volume. Nothing is destroyed. A flood that takes 3 % of a room's
volume can move at most 3 % of its gas, and the `WA-3` test asserts exactly
that invariant rather than a dramatic number.

### 3.2 No RNG, no wall clock

The layer draws **zero** random numbers. Arc cadence is a counter, not a roll.
Fire spread is a scheduled deterministic effect. Two runs of the same command
stream produce the same plant state, byte for byte, on any target.

### 3.3 Derived values, one fixed order

Derived quantities are recomputed once per commit in this order, because later
ones read earlier ones:

```
1. energy      circuit energised → entity energised (series gates, lockouts)
2. wetness     water contact → wetness (per material)
3. depth       water_ml → depth_mm per volume
4. concentration  gas_ml / air_l → permille per (volume, gas)
5. oxygen      burned gas → oxygen_permille
6. hazard      energised ∧ wet ∧ damaged → arc candidate;
               concentration ∈ [LFL,UFL] ∧ source → ignition candidate;
               depth ∧ conductive source → shock field
7. traversal   depth vs clearance; gate position; fire presence
8. readout     sensor values, HUD values, network-reported values
```

No step reads a value it has not already produced, and no step reads a value
from a later step.

---

## 4. The tick

### 4.1 Five phases

| Phase | Reads | Writes | Determinism device |
|---|---|---|---|
| **S1 sense** | committed state, host facts | nothing | one immutable `Sense` snapshot per wave |
| **S2 intent** | `Sense`, commands, controller laws | intent list | sorted by `(rank, rule_rank, subject_index, field)` |
| **S3 resolve** | intent list | staged deltas | total order; at most one winner per `(subject, field)` |
| **S4 commit** | staged deltas | state | sum per `(subject, quantity)`, clamp once, apply in ascending index |
| **S5 aftermath** | committed state | schedule, cues, evidence, causes | `BTreeMap<(due_tick, seq)>` |

S1–S5 is one **wave**. A wave's hazards can schedule effects for the next wave
or a later tick; waves are bounded (§6).

### 4.2 The precedence ladder

Every intent carries an origin rank. Conflicts are resolved by rank, then by
`(rule_rank, subject_index, field)`. There is no other tie-break.

| Rank | Origin | Example | Loses to |
|---|---|---|---|
| **R0** | `LockOut` | a thrown isolator, a padlocked valve, a physical disconnection | nothing |
| **R1** | `Manual` | an actor throws a switch or closes a valve this tick | R0 |
| **R2** | `Interlock` | "no ignition source while gas ≥ LFL", "pump off while dry-protection trips" | R0–R1 |
| **R3** | `Control` | a wired sensor→actuator chain, a controller law | R0–R2 |
| **R4** | `Remote` | the hacker actuating a plant device over the network | R0–R3 |
| **R5** | `Default` | the authored resting state | everything |

Two properties follow by construction, not by special case:

- **Manual isolation overrides remote operation** (`CP-4`): a lockout is R0, a
  remote command is R4. A remote request against a locked-out entity is
  *refused with an event*, never silently ignored.
- **An interlock outranks automation** (`CP-5`): a safety law refuses a start
  even when a wire asks for it. Whether the interlock is *right* depends on the
  sensor it reads — which is what makes `CP-3` (spoofing) dangerous.

The ladder is about **who wins a conflict**, not about **what is true**. A rule
that computes a physical outcome (arc, ignition, shock) reads the committed
state, never a controller's reported value. §8.

### 4.3 Worked example — four things at once

Level 1, tick 3600 (t = 60.0 s). Plant depth is 224 L = **140 mm**; sill 180 mm;
pump plinth 150 mm. Riser gas is 0. `feed-m` is energised. Four things land in
the same tick:

| # | Input | Origin |
|---|---|---|
| 1 | infiltrator throws `iso-m` (local isolator) | R0 |
| 2 | hacker `Enable`s `pump-p1` over the signal network | R4 |
| 3 | `gd-1` reports 20 ‰ in level 2 | R2 (alarm interlock) |
| 4 | the leak keeps entering | environment (no conflict) |

**S1** reads: `depth_plant = 140`, `wet_pump = 0`, `energised(M) = true`,
`iso_m = closed`, `signal(sig-pump) = off`, `gas_riser = 0`.

**S2** produces, in this order:

| rank | rule | subject | field | value |
|---|---|---|---|---|
| 0 | `WE-4` | `pump-p1` | `energised` | **false** (lockout) |
| 4 | `CP-7` | `pump-p1` | `running` | true |
| — | `WA-6` | `plant` | `water_ml` | +800 |
| — | `WE-1` | `pump-p1` | `wetness` | 0 (below plinth) |

**S3** resolves `pump-p1.energised`: R0 wins, R4 is refused. `pump-p1.running`
is moot once energisation is false; it resolves to `false` with
`reason: unpowered`, and the refusal is one event, not silence.

**S4** commits: `water_ml += 800` (leak), pump stays dead and dry, circuit M
stays energised but the pump draws nothing.

**S5** emits, in this order (sorted by rank then subject):

```jsonl
{"event":"infra.denied","tick":3600,"rule":"WE-4","subject":"pump-p1",
 "requested":{"by":"hacker","field":"running"},"reason":"locked-out","cause":41}
{"event":"infra.actuated","tick":3600,"rule":"CP-4","subject":"iso-m",
 "field":"position","before":"closed","after":"open","cause":42}
{"event":"infra.quantity","tick":3600,"volume":"plant","field":"water_ml",
 "before":223200,"after":224000,"cause":43}
```

Cause tree as the developer trace prints it:

```
cause#41  remote start refused            ← CP-7 / WE-4
cause#42  isolator thrown (infiltrator)   root
cause#43  leak inflow                     ← environment (Cylinder/Leak source)
```

The player perceives: the pump never starts; the hum they expected does not
come; a small metal clack from the isolator. The guard, one floor up, perceives
nothing this tick — the denial is silent *to the guard* by design, and the
isolator's evidence (`LockoutTag`) only exists if someone looks.

---

## 5. Staged deltas: why iteration order cannot matter

**Claim.** For any two schedules of the same set of intents within one wave,
the committed state is identical.

**Argument.**

1. Intents are values; they are not applied as they are produced. S2 only
   appends to a `Vec`.
2. Conflicts are decided by a **total order** on `(rank, rule_rank,
   subject_index, field)`. Reordering the input cannot change the winner,
   because the ordering key does not include input position.
3. Every non-conflicting effect is a delta `(subject, quantity, i64)`. Commits
   group by `(subject, quantity)`, **sum in `i64`**, clamp once, and write
   once. Integer addition is commutative and associative. Clamping after the
   sum (rather than per delta) is what makes this true — clamping per delta
   would make the result depend on order.
4. Flow is antisymmetric by construction: `MoveWater(a, b, x)` stages `-x` at
   `a` and `+x` at `b`, and both are computed from the **S1 snapshot**. Two
   opposing flows therefore cancel exactly, and a chain `a→b→c` cannot
   "double-spend" water within one tick, because `b`'s balance is computed from
   its pre-tick volume.
5. **Sibling flows are allocated, not summed.** The snapshot alone does *not*
   prevent two outgoing transfers from overdrawing one source: if a compartment
   holds 100 ml and two links each stage 100 ml, summing and clamping leaves the
   source at 0 while crediting 200 ml to the destinations — 100 ml invented.
   S3 therefore allocates before S4 credits. For each `(subject, medium)` with
   total requested outflow `R` and available `A`:

   ```
   if R <= A:            allocated_i = request_i
   else:                 allocated_i = floor(request_i * A / R)
                         r = A - Σ allocated_i
                         give 1 ml each to the first r intents in total order
   ```

   The destination is credited with `allocated_i`, never with `request_i`, and
   the scaled effect is what S4 commits. Hamilton largest-remainder with the
   total order as tie-break: deterministic, order-independent, and
   **conservative to the millilitre**. The same routine allocates gas species,
   which is why a fan and a leak competing for one compartment cannot both take
   the whole amount.
6. Derived values are recomputed in the fixed order of §3.3, after the commit,
   from committed quantities only.

**Consequences.** Entity iteration order is an authoring detail, not a
behavioural one. This is the same discipline `building/graph.rs` already uses
(`BTreeMap`/`BTreeSet` throughout, "iteration order can never leak into
results"), extended to numeric state.

**Tests.** `permuted_intents_yield_identical_committed_state`,
`opposing_flows_cancel_regardless_of_order`,
`clamp_applies_once_after_the_sum`,
`concurrent_outflows_from_one_source_conserve_the_source`,
`definition_order_permutation_preserves_outcomes` (§15).

---

## 6. Cascades: bounded, deferred, never dropped

### 6.1 Bounds

| Bound | Default | On exhaustion |
|---|---|---|
| `MAX_WAVES_PER_TICK` | 3 | remaining intents go to `deferred` |
| `MAX_DELTAS_PER_TICK` | 512 | remainder deferred |
| `MAX_RULES_PER_WAVE` | 256 | remainder deferred |
| `MAX_SCHEDULED_PER_TICK` | 128 | remainder deferred |
| `MAX_LIVE_CAUSES` | 64 | oldest *closed* causes dropped, never open ones |

Deferred intents are carried in `InfrastructureState.deferred`, sorted by the
same key, and applied at the start of the next tick's S2 with `wave = 0` and
`origin_deferred = true`. They do not lose their cause id.

### 6.2 The no-silent-drop invariant

> Every intent that is produced is either committed, deferred, or refused with
> an event. There is no fourth outcome.

Enforced by construction: S3 has three exit paths and each writes to exactly one
of `commit_set`, `deferred`, `refusals`. A debug assertion in tests
(`every_produced_intent_is_accounted_for`) counts them.

Each exhausted bound emits **one** `CascadeDeferred { wave, deferred, roots }`
event per tick per bound, not one per intent, so the log cannot be flooded by
the failure mode itself.

### 6.3 Why this is not a licence to be slow

A three-wave cap is not a performance fudge; it is a semantic statement: **no
rule may depend on more than three causal steps within one 16.6 ms frame.** A
level that needs more is a level whose interactions should be spread over time,
which is what delay and duration are for. The bound is authored data
(`InfraTuning`), and changing it is a ruleset version bump.

### 6.4 Self-scheduling

A rule may not schedule itself for the same tick. Attempting it is a
`SelfScheduled` validation error at load time, so an oscillation cannot be
smuggled past the wave cap. Legitimate periodic rules use `period_ticks`, which
is scheduled for `tick + period`, not the same tick.

---

## 7. Events, causes and the developer trace

### 7.1 Event families

| Family | Class | Example |
|---|---|---|
| `RuleFired` | ambient | a rule matched and staged a delta |
| `QuantityChanged` | ambient | `water_ml 223200 → 224000` |
| `StateChanged` | load-bearing | position, energised, integrity, wetness crossing a threshold |
| `Actuated` | load-bearing | pump started, damper closed, isolator thrown |
| `HazardRaised` / `HazardCleared` | load-bearing | arc, fire, shock field, gas at LFL |
| `Denied` | load-bearing | remote start against a lockout |
| `Irreversible` | load-bearing | equipment destroyed, door mechanism broken, gas ignited |
| `CascadeDeferred` | load-bearing | budget exhausted |
| `ReadoutSampled` | ambient | sensor value per tick, coalesced |
| `EvidenceEmitted` | load-bearing | guard-observable item created |

**Load-bearing** events are never coalesced, never throttled and never dropped.
**Ambient** events are coalesced per tick per `(rule, subject, field)` into one
record carrying `count`, `first_tick`, `last_tick`. If `MAX_EVENTS_PER_TICK`
would be exceeded, ambient events are dropped first, and a single
`EventBudgetExceeded { dropped_ambient }` is emitted — so the developer can see
that the log is partial. Load-bearing events alone cannot exceed the cap,
because they are bounded by `MAX_RULES_PER_WAVE`.

### 7.2 Cause ids

A cause is allocated at scheduling time, from a monotonic counter carried in
state (so it survives snapshot/restore), in the deterministic order of §4.1.

```rust
pub struct Cause {
    pub id: u32,
    pub parent: Option<u32>,
    pub root: RootCause,   // PlayerCommand | Controller | Environment | Fire | Sensor
    pub rule: RuleId,
    pub tick: u64,
}
```

Causes form a tree, not a graph: a cause has at most one parent, and an effect
carries exactly one cause. This makes the developer view a printable tree and
makes "why did this happen?" a bounded walk.

### 7.3 The developer trace

`idaptik-infra-trace/1` is one JSON object per line, emitted only when tracing
is enabled (developer builds, `--headless --trace`). It is a *view*: the same
facts, in the order they were produced.

```json
{"format":"idaptik-infra-trace/1","tick":3600,"wave":0,"phase":"resolve",
 "rule":{"id":"WE-4","version":1},"subject":{"kind":"entity","id":"pump-p1"},
 "cause":42,"decision":"refuse","reason":"locked-out",
 "intent":{"origin":"remote","field":"running","value":true},
 "sense":{"depth_plant_mm":140,"wet_pump":0,"energised_m":true},
 "commit":null}
```

The trace is what the level documents quote in §6 of each level file. It is not
a player surface and it is not replicated.

---

## 8. Readouts: reported values versus truth

This is the section that makes `CP-3` honest.

| Reader | Sees | Can be fooled? |
|---|---|---|
| physics (arc, ignition, shock, flow) | committed state | **no** |
| controller / interlock | the *reported* value of the sensor it reads | yes, if spoofed |
| hacker's panel | network-reported values | yes — that is the deception |
| infiltrator's senses | cues derived from truth, attenuated by space | no, but noisy and local |
| guard | evidence at observation sites, delayed by travel | no, but never omniscient |

A **spoof** (`CP-3`) rewrites the reported value only:

```rust
pub struct ReportedValue { pub entity: EntityId, pub quantity: SenseQuantity, pub value: u32 }
pub struct SpoofState { pub signal: SignalId, pub reported: u32, pub until_tick: u64 }
```

The controller acts on `ReportedValue`; the hazard rule reads `InfraState`. A
spoofed gas detector can therefore talk a controller into starting a pump in a
90 ‰ atmosphere. The ignition rule does not care what the controller believes.
That is the whole point: the deception is real, the physics is not negotiable.

The two seats can *disagree* — hacker panel says 0 ‰, infiltrator's nose says
otherwise — and resolving that disagreement by voice is gameplay, not a bug.

---

## 9. Cues (players) and evidence (guards)

### 9.1 Cues

```rust
pub struct CueRef { pub cue: CueId, pub level: u16, pub at: VolumeId, pub when: CueWhen }
```

`CueId` is a closed tag list — `WaterRunning`, `WaterDrip`, `WaterSlosh`,
`WaterRise`, `Hiss`, `PumpHum`, `PumpCavitation`, `VentHum`, `VentStops`,
`RelayClack`, `ArcCrackle`, `BreakerTrip`, `GasReek`, `GasAlarm`, `FireWhoomph`,
`Scraping`, `Steam`, `WetFootprints`, `IsolatorTag`, `CutterGrating`. The frontend binds a
tag to audio/visual assets; the engine never names a file.

Cues are **thresholded**: a cue appears only when the underlying quantity
crosses the authored level, so a puddle does not announce itself before it is
deep enough to matter, and a leak is audible before it is visible.

### 9.2 Evidence

```rust
pub struct EvidenceRef {
    pub tag: EvidenceTag,        // WetFloor | WaterUnderDoor | SteamPlume | PumpNoise |
                                 // OpenIsolator | LockoutTag | BurntInsulation |
                                 // GasAlarmPanel | DamperMoved | ForcedDoor | FreshOpening |
                                 // DebrisMoved
    pub at: VolumeId,
    pub channel: Channel,        // Sight | Sound | Smell | Report
    pub strength: u16,
    pub decay_ticks: u32,
    pub latency_ticks: u32,
}
```

Evidence enters the existing observer-relative ledger
(`scenario/vsm.rs`, `docs/architecture/evidence-ledger-design.adoc`) as an
observation at a place and a time. The guard's belief is updated only from
evidence he could have perceived: same compartment, or a sound that crossed a
link with enough `noise_permille`. The engine never hands the guard truth.

---

## 10. Integration with the building runtime

### 10.1 Definition

```rust
pub struct BuildingDefinition {
    // … unchanged fields …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalNetworkDef>,     // optional, as ADR-0012 anticipated
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infra: Option<InfrastructureDefinition>,
}
```

Both are skipped when absent, so `exchange_house.json`, the ghost-lobby goldens
and every recorded replay serialise byte-identically. This is the precedent
`ScenarioDefinition.signal` set and the reason the canonical floor's golden did
not move when ADR-0012 landed.

### 10.2 Host contract

The runtime is host-agnostic, like `SignalSim`:

```rust
impl InfrastructureSim {
    pub fn new(def: &InfrastructureDefinition) -> Result<Self, Vec<InfraValidationError>>;
    pub fn apply(&mut self, op: &InfraOp, access: &InfraAccess) -> Vec<InfraEvent>;
    pub fn step(&mut self, input: &InfraInput) -> Vec<InfraEvent>;
}

pub struct InfraInput {
    pub power: BTreeMap<CircuitId, bool>,          // from netsim effects
    pub portals: BTreeMap<PortalId, bool>,         // open?
    pub signals: BTreeMap<SignalId, SignalFact>,   // request latch + reported value
    pub spoofs: BTreeMap<SignalId, u32>,           // reported overrides
    pub actor: ActorPresence,                      // volume, reach, capabilities
}
```

`InfraAccess` is the same shape as `signal::Access`, carrying which entities each
seat can touch, computed by the host from vantage and room — one access model,
not two.

The wire form is `Command::Infrastructure { op: InfraOp }`, index-addressed
(`EntityIndex`, like `NetNodeIndex` and `SignalIndex`) so that `Command` stays
`Copy` and a renamed entity cannot break a recorded script. It is folded as an
immediate at P2, exactly as `Command::Signal` is, and it carries the issuing
seat so the origin rank (R1 local, R4 remote) is known without asking the
transport.

### 10.3 Portals: traversal versus fluid

`BuildingSim`'s traversal check gains two deny reasons:

```rust
pub enum BuildingDenyReason {
    UnknownPortal, NotAdjacent, Locked, Unpowered,
    Flooded,   // WA-2: depth > link clearance
    OnFire,    // EG-4: fire in the destination compartment
}
```

Fluid is *not* a deny reason for fluid: `WA-1` closes an aperture without
touching traversal, and `WA-2` blocks traversal without touching the aperture.
A flooded duct that stops carrying air may still be walkable — and a walkable
crawl may be carrying no air at all.

### 10.4 Netsim and actuation

The plant reads power; it does not write reach. `Effect::PowerCut` and
`Effect::DevicePowerLost` from `netsim::apply_actuation` are folded into
`InfraInput.power` by the host. Plant actuation from the hacker arrives over the
signal network (§11), not as a new `Actuation` variant — `Actuation` stays a
closed, `Copy` enum whose job is reach, exactly as ADR-0012 argued.

---

## 11. The signal network: propagation preserved

### 11.1 Invariants that do not change

| Invariant | Where it lives |
|---|---|
| outputs are read from state at tick start | `signal/sim.rs::Run::step_body` step 1 |
| a signal crosses at most one wire per tick | same, step 2 |
| edge inputs fire once on the rising edge; relays are level | same, step 3 |
| relay feedback loops are well-defined at one hop/tick | same |
| a door pulse holds for `door_pulse_ticks` | same |
| access is per seat, per device | `signal/sim.rs::Access`, `sim/signal.rs::signal_access` |
| wiring is charged in bandwidth or busy ticks | `SeatProfile` |

None of these functions changes. The propagation contract is not up for
renegotiation.

### 11.2 The one additive change

The plant needs two new *device roles* on the wire, because overloading `Light`
or `Sensor` would make plant semantics depend on light semantics — exactly the
kind of aliasing the brief forbids.

| Kind | Inputs (Edge) | Host outputs | Mirrors |
|---|---|---|---|
| `PlantActuator` | `Enable`, `Disable`, `Toggle`, `Open`, `Close` | `Active` | a light's "commanded" latch |
| `PlantSensor` | `Enable`, `Disable` | `Tripped` | a sensor's enable latch |

`outputs_of` gains two arms:

```rust
SignalKind::PlantActuator => set(OutPort::Active,  host(OutPort::Active)),
SignalKind::PlantSensor   => set(OutPort::Tripped, s.active && host(OutPort::Tripped)),
```

The deliberate asymmetry, and why the two arms differ:

- for a `PlantActuator`, the device carries **two different fields with two
  different meanings**: `DeviceState.active` is the wire-latched *request*, and
  `OutPort::Active` is the plant's *report* of what the machine is actually
  doing. The report is host truth only, exactly like `Guard`, because a machine
  that a manual local switch started is running whether or not a wire asked for
  it. The gap between request and report is where interlocks, lockouts and
  spoofing live, and it is visible to the player as REQUESTED / NOT RUNNING;
- for a `PlantSensor`, `DeviceState.active` is the enable latch and
  `OutPort::Tripped` is the reported threshold crossing — identical to the
  existing `Sensor` arm, because a disabled sensor genuinely reports nothing.

`SignalState::initial` treats `PlantSensor` as initially active, like `Sensor`;
`PlantActuator` starts inactive, like a light that is off.

### 11.3 Hop-count proof

| Chain | Ticks | Why |
|---|---|---|
| physical change → sensor `Tripped` | **+1** | plant commits in S4; the readout is computed at the top of the *next* signal step |
| sensor output → wire → relay input | same tick as the report | existing one-hop-per-tick rule |
| relay output → wire → actuator input | +1 | same, because the relay's output is read at tick start |
| actuator input → plant actuation | same tick as the input change | `system_infrastructure` runs after `system_signal` |
| plant actuation → sensor readout | +1 | readout is taken at the top of the next signal step |

So a sensor→relay→pump chain costs exactly three ticks, and it would cost the
same three ticks without the plant. The alternative ordering (plant before
signal) would make it two. That is the change §11 refuses.

### 11.4 Format and the fallback

`SIGNAL_FORMAT` becomes `idaptik-signal/2`; `load_signal_network` accepts `v1`
and `v2`, and a `v1` document produces a byte-identical golden event log (the
two new kinds cannot appear in it). Existing `SignalKind` JSON is unchanged:
adding variants to a serde enum is additive for data that does not use them.

**If the maintainers refuse the format bump**, the fallback is: plant entities
are commanded only by `InfraOp` (direct, seat-checked) and by controller laws
inside the plant, and sensor→actuator automation is expressed as controller
laws rather than as wires. The plant then works, but the hacker loses the
"rewire the plant" verb and `CP-1` degrades to `CP-2`. That is a real loss, which
is why the additive change is the recommendation and the fallback is documented
rather than assumed.

---

## 12. Versioning and compatibility

| Surface | Tag | Rule |
|---|---|---|
| infrastructure definition | `idaptik-infrastructure/1` | loader gate; unknown format refused |
| ruleset | `InfrastructureDef.ruleset_version` | any outcome-changing edit bumps it |
| individual rule | `RuleDef.version` | bump on any edit to `when`, `effects`, `delay`, `recovery` |
| snapshot | `InfrastructureState.ruleset_version` + per-rule versions | restore across a mismatch is refused |
| trace | `idaptik-infra-trace/1` | view only; no compatibility promise |
| signal | `idaptik-signal/2` | v1 accepted unchanged (§11.4) |
| building | `idaptik-scenario/1` | unchanged; `infra` and `signal` are optional |

Refusal is a typed `InfraValidationError::RulesetMismatch { found, expected }`,
not a silent reinterpretation. A replay whose rules changed under it is a bug
report, not a mystery.

---

## 13. Anti-goals

- **No fluid dynamics.** No pressure field, no Navier–Stokes, no per-cell mass
  transport. 1-D transfers between authored volumes at authored rates.
- **No chemistry.** The only gas↔water behaviour is the species' `GasWaterRule`.
  The only gas↔fire behaviour is `Ignition` plus `Combustion` consuming the
  authored species. There is no reaction table.
- **No thermodynamics.** Fire is one scalar per compartment. Smoke is a scalar.
  Heat is a rate on fire.
- **No per-pair special cases.** Rules match capabilities, materials and gas
  classes. A rule that reads an entity id or an `EntityKind` fails review and
  fails the rename test.
- **No second access model.** The plant uses the signal layer's seat access
  shape.
- **No RNG, no wall clock, no floats in replicated state.**
- **No new signal hop, and no change to edge/level semantics.**
- **No outcome that exists only in a log.** Every rule names a cue and an
  evidence item; if it cannot, it is a debug rule and is marked as such.

---

## 14. Authoring validation

`InfrastructureDefinition::validate` returns named checks, in the shape of
`BuildingDefinition::validate` and `ScenarioDefinition::validate`:

| Check | Rejects |
|---|---|
| `compartments.unique_ids` | duplicate `VolumeId` |
| `compartments.room_anchor` | a compartment with `room: None` that is not `exterior: true` |
| `compartments.rooms_disjoint` | two compartments claiming the same room without `sub_volume: true` |
| `links.resolve` | a link to a missing compartment |
| `links.gate_resolves` | `GateRef` naming a missing portal or entity |
| `links.actor_and_aperture` | `actor: Passable` with `clearance_mm: 0` |
| `entities.resolve` | entity in a missing compartment, or a missing material |
| `entities.capability_targets` | `Gates`/`Breaks` naming a missing target |
| `entities.signal_role` | a sensor bound to `PlantActuator` or vice versa |
| `entities.node_resolves` | a `node` the grounded graph lacks |
| `entities.plinth_within_volume` | a plinth above the compartment's flood capacity |
| `circuits.resolve` | `Powered` naming a circuit the building lacks |
| `gases.resolve` | a rule naming a missing species |
| `gases.unsupported_buoyancy` | a species with `density_vs_air_permille < 1000` (v1) |
| `materials.resolve` | a rule or entity naming a missing material |
| `controllers.resolve` | a controller naming a missing entity or signal |
| `controllers.law_matches_reading` | a threshold law on a quantity the sensor does not report |
| `rules.unique_ids` | duplicate `RuleId` |
| `rules.resolve` | a level citing an id the build's catalogue does not carry |
| `rules.cited_if_reachable` | a rule whose preconditions can arise in the level but which the level did not cite (§10.1 of the catalogue) |
| `rules.conditions_resolve` | a condition naming a missing subject |
| `rules.no_self_schedule` | a rule scheduling itself in the same tick |
| `rules.tests_present` | a rule with an empty `tests` list (the brief requires a regression test per interaction, and this makes that checkable) |
| `rules.irreversible_has_recovery` | an irreversible effect with no stated recovery path |
| `objectives.reachable` | a level with no successful route when every rule is evaluated |

The last one is a reachability *search over rules*, not a special case: it asks
whether some command sequence reaches the objective without an irreversible
failure. `infrastructure-level-1.md` §5 enumerates the routes the search is
expected to find.

---

## 15. Test strategy

### 15.1 Shape

Tests live in `crates/idaptik-core/tests/infrastructure.rs` (behaviour) and
`crates/idaptik-core/src/scenario/infrastructure/sim.rs` (unit invariants), in
the style of `tests/signal_network.rs`: build a level, drive a command stream,
assert on state *and* the event log.

The harness must let a test set state directly (`sim.state_mut()`) for the same
reason `ghost_lobby` tests do: to test one rule in isolation without scripting a
minute of play.

### 15.2 Index

| Group | Tests |
|---|---|
| determinism | `replay_determinism_with_infrastructure`, `permuted_intents_yield_identical_committed_state`, `opposing_flows_cancel_regardless_of_order`, `clamp_applies_once_after_the_sum`, `concurrent_outflows_from_one_source_conserve_the_source`, `definition_order_permutation_preserves_outcomes`, `snapshot_restore_mid_cascade_is_exact`, `ruleset_mismatch_is_refused` |
| cascade | `every_produced_intent_is_accounted_for`, `cascade_deferral_emits_one_event`, `deferred_intent_keeps_its_cause`, `deferral_changes_timing_not_outcome`, `self_scheduling_is_a_load_error` |
| water/electric | `water_on_dry_equipment_does_not_arc`, `a_puddle_below_contact_depth_is_not_lethal`, `damaged_wet_energised_equipment_arcs_on_cadence`, `isolation_prevents_remote_start`, `drying_requires_de_energisation`, `running_a_wet_motor_destroys_it`, `restoring_power_to_wet_equipment_arcs_after_delay_not_instantly` |
| electricity/gas | `unpowered_fan_stops_extraction_and_gas_accumulates`, `a_light_switch_does_not_ignite_gas`, `a_pump_start_ignites_gas_in_band`, `ignition_requires_the_authored_class`, `ignition_requires_dwell_not_a_single_tick`, `fire_consumes_the_species_and_spreads_by_combustibility` |
| water/gas/air | `flood_closes_a_duct_without_blocking_traversal`, `flooded_duct_still_weirs_water`, `depth_above_clearance_blocks_traversal_without_closing_the_duct`, `water_displaces_gas_without_neutralising_it`, `a_dissolving_species_is_parameterised_not_hardcoded`, `airflow_moves_gas_between_compartments` |
| tunnels | `excavation_exposes_the_authored_utility`, `cutting_exposed_cable_kills_the_authored_circuit`, `breach_connects_previously_separate_compartments`, `water_enters_a_tunnel_downhill`, `cutting_is_heard_through_links_by_attenuation`, `isolation_first_makes_the_breach_safe` |
| control | `a_wired_sensor_starts_the_pump_one_hop_per_tick`, `a_spoofed_detector_misleads_the_controller_but_not_ignition`, `manual_isolation_beats_remote_and_emits_a_denial`, `an_interlock_refuses_a_start_even_when_wired`, `a_feedback_loop_is_bounded_and_repeats_exactly` |
| observability | `every_rule_emits_a_cue_and_an_evidence_item`, `evidence_is_not_emitted_beyond_its_attenuation`, `load_bearing_events_are_never_coalesced` |
| anti-hardcoding | `renaming_every_entity_preserves_behaviour`, `no_rule_reads_entity_kind` |
| signal preservation | `signal_hop_count_unchanged_with_infrastructure_enabled`, `plant_state_reaches_the_signal_network_next_tick_only`, `v1_signal_definition_golden_is_byte_identical`, `plant_actuator_request_and_report_are_distinct` |
| compatibility | `building_json_without_infra_is_byte_identical`, `scenario_snapshot_v3_still_restores`, `no_rng_draws_from_the_infrastructure_layer` |

Each test name appears in at least one rule's `tests` field in the catalogue.
The inverse is also gated: `rules.tests_present` (§14) refuses a rule with none.

---

## 16. Open questions

1. **Netplay.** The plant is deterministic and RNG-free, so the relay needs no
   change beyond routing `Command::Infrastructure` by seat, as ADR-0012 §"Not
   decided here" already notes for `Signal`. Verify with the loopback gate
   before the first networked level.
2. **Nickel schema.** `config/infrastructure-schema.ncl` plus `bad_*.ncl`
   fixtures, mirroring `config/scenario-schema.ncl`. Not designed here.
3. **TUI.** A plant view needs a compact rendering of compartments, quantities
   and hazard state. The developer trace is the first consumer; the TUI overlay
   is a follow-up.
4. **Guard model depth.** Evidence is specified at the interface; how a guard
   ranks `PumpNoise` against `WetFloor` belongs to the existing VSM/ledger work,
   not to this layer.
5. **Sub-volumes.** A duct modelled as its own compartment (rather than a link
   with an aperture) is allowed by the model; whether it earns its complexity is
   a level-authoring question. Level 2 does not need it.
6. **Repair.** `Recovery::NeedsTool` is specified but no level uses it. Ship it
   when a level needs it, not before.
