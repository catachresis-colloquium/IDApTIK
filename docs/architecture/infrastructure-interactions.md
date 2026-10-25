# Infrastructure interactions — unimplemented design

> **Status: not implemented.** This document specifies a rule layer that the
> code does not contain. What shipped in `crates/idaptik-core/src/scenario/
> infrastructure/` is a different design: three fixed-order integer systems
> with scalar `water_conductance` / `air_conductance` per opening, hard-coded
> rule order, and no authored `RuleDef`, capability set or precedence ladder.
> See `infrastructure-implementation-status.md` for the identifier-by-identifier
> comparison, and ADR-0018 for the decision this would implement.
>
> Read this as a **backlog**, not a description of the engine.

**Status: design of record. Not implemented.** There is no
`SystemInfrastructure`, no `SignalKind::PlantActuator`, and none of the test
names below exist in the tree. This document specifies behaviour precisely so
that the implementation can be written against it and checked. Where it states
something about the code that *does* exist, the file and function are named;
where it states something about the layer being designed, the word is *must*.

> **Correction (2026-10-25):** a `scenario/infrastructure/` module *now exists*,
> but it is not this design — it is ADR-0013's water/gas/air runtime
> (`CompartmentDef`, `OpeningDef`, `WaterNetworkDef`, `AirNetworkDef`,
> `InfrastructureCommand`), and it is not yet called from the game tick. The
> reconciliation between the two is argued in
> [`docs/reviews/2026-10-25-pr165-vs-main.md`](../reviews/2026-10-25-pr165-vs-main.md).
> What remains unimplemented is precisely this document's layer: capability-typed
> rules, `AccessReq`, actuator precedence and spoof detection.




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



### 2.1 Geometry (not implemented)

The shipped `OpeningDef` carries a scalar `water_conductance` and
`air_conductance` per opening: flow is whatever the number says, so a level
author can make a puddle deep in a cupboard and shallow in a basement. This
section specified the alternative the levels were designed against — flow and
depth derived from room size and aperture, so "the hall is ankle-deep" is a
consequence of the room rather than a number someone picked.

```
depth_mm      = water_ml * 10 / floor_cm2

flow_ml_per_s = min(capacity_ml_per_s,
                    aperture_cm2 × max(0, depth_mm − sill_mm) × WATER_LINK_K)
```

That is a weir with a ceiling: nothing flows until the water is over the sill,
the flow grows with the head above it, and it never exceeds the link's rating.

- `sill_mm` is a **liquid** quantity and is what rule `WA-1` reads. Once the
  depth on the link's low side exceeds it, the opening is submerged; actor
  passage is a different question, governed by `clearance_mm` alone.
- `aperture_cm2` and `clearance_mm` are the whole of "fluid transport is not
  traversal": `WA-1` reads the aperture, `WA-2` reads the clearance, and
  nothing infers one from the other.
- A compartment is anchored to a building room for cues, evidence and actor
  presence, but it is not the room: a duct run, sump or riser shaft can be its
  own compartment inside one room.
- `full_depth_mm = air_l × 10 000 / floor_cm2` is the depth at which a
  compartment of that volume is full; future acceptance tests would use it as
  the bound a run may never exceed.

None of this exists in code. `full_depth_mm`, `aperture_cm2` and `weir` have
zero occurrences under `crates/`.


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
| `Moves` | `medium`, `intake: VolumeId`, `discharge: VolumeId`, `path: Vec<LinkId>`, `rate_ml_per_s`, `check_valve`, `return_ml`, `reversible` | transports the medium between its typed endpoints along its authored path while running |
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

Every mover declares its intake and discharge compartments (including exterior
sinks). `path` lists intervening links in intake-to-discharge order; an empty
path is a dedicated mover conduit, with no passive-link gate to inherit.
Non-empty paths must connect the endpoints and obey each link's gates,
medium-specific blocking and capacity. Reversing swaps intake and discharge
and reverses the path. The entity's location alone never selects its route.

Thus `pump-p1` has Water endpoints `plant → sewer`, `path: []`: its dedicated
discharge works while the separate `grate-d1` drain remains blocked. `fan-f1`
has Gas endpoints `riser → outside`, `path: [vent-stack]`; `fan-f2` has Gas
endpoints `riser → plant`, `path: [pen-1]` (reversible).

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
MaterialWaterFilmConductive(entity) // true iff the entity's material conducts a wet film
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
| `MaterialWaterFilmConductive(e)` | `{"kind":"material_water_film_conductive","entity":"self"}` |
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
| `infra.format` | a payload whose `format` is not `idaptik-infrastructure/1` |
| `compartments.unique_ids` | duplicate `VolumeId` |
| `compartments.room_anchor` | a compartment with `room: None` that is not `exterior: true` |
| `compartments.rooms_disjoint` | two compartments claiming the same room without `sub_volume: true` |
| `links.resolve` | a link to a missing compartment |
| `links.gate_resolves` | `GateRef` naming a missing portal or entity |
| `links.actor_and_aperture` | `actor: Passable` with `clearance_mm: 0` |
| `entities.resolve` | entity in a missing compartment, or a missing material |
| `entities.capability_targets` | `Gates`/`Breaks` naming a missing target |
| `entities.mover_routes` | `Moves` naming a missing endpoint/link, a disconnected or misordered path, or a medium the route cannot carry |
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
| `rules.cited_if_reachable` | **deferred to M1** — requires evaluating preconditions, which needs `InfrastructureSim`; a rule whose preconditions can arise in the level but which the level did not cite (§10.1 of the catalogue) |
| `rules.conditions_resolve` | a condition naming a missing subject |
| `rules.no_self_schedule` | a rule scheduling itself in the same tick |
| `rules.tests_present` | a rule with an empty `tests` list (the brief requires a regression test per interaction, and this makes that checkable) |
| `rules.irreversible_has_recovery` | an irreversible effect with no stated recovery path |
| `objectives.reachable` | **deferred to M1** — a reachability search over rules, so it needs the evaluator. A level with no successful route when every rule is evaluated |

The last one is a reachability *search over rules*, not a special case: it asks
whether some command sequence reaches the objective without an irreversible
failure. `infrastructure-level-1.md` §5 enumerates the routes the search is
expected to find.

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
