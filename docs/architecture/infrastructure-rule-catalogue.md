<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Infrastructure rule catalogue

**Status: design of record. Not implemented.** These rules are the specification
an implementation is written against; no rule id below exists in the code yet.
The evaluation model — phases, deltas, precedence, cascade bounds, events — is
in [`infrastructure-interactions.md`](infrastructure-interactions.md). This file
is the authority for *what each interaction does*.

## How to read a rule

Every rule fills the eleven properties the brief requires. They map onto
`RuleDef` (see the design doc, §2.4) as follows:

| Property here | Field in `RuleDef` / runtime |
|---|---|
| Preconditions & spatial scope | `when`, `scope` |
| Required access and actor capability | `access`, `actor` |
| Consumed / transformed quantities | `consume`, `produce`, staging deltas |
| Delay and duration | `delay_ticks`, `period_ticks` |
| Physical outcome | `effects` |
| Device / sensor readout | `readout` |
| Player-visible / audio cues | `cues` |
| Guard-observable evidence | `evidence` |
| Recovery and possible irreversible consequences | `recovery`, `irreversible` |
| Traceable causal events | `cause` (event family + chain) |
| Regression tests | `tests` |

Unless a rule says otherwise, `access: None` and `actor: []` mean it is an
environment rule, evaluated whether or not anyone is in the room.

**Rules never read an entity id or an `EntityKind`.** They read capabilities,
material properties and gas classes. Ids appear only in *authored instances*
(a level's `when` configuration), never in the ruleset. The
`renaming_every_entity_preserves_behaviour` test is what keeps that honest.

---

## 1. Ruleset tuning (`ruleset_version: 1`)

| Constant | Value | Meaning |
|---|---|---|
| `WET_THRESHOLD` | 350 ‰ | film thickness at which arcing becomes possible |
| `WET_RATE` | 250 ‰/s | wetting rate in contact with water |
| `DRY_RATE` | material `dries_permille_per_s` (12–90) | drying rate, only while de-energised |
| `ARC_PERIOD` | 30 ticks | arc cadence while conditions hold |
| `ARC_DAMAGE` | 4 ‰ | integrity lost per arc |
| `CONTACT_DEPTH` | 12 mm | depth at which a body completes a shock circuit |
| `SLIP_DEPTH` | 25 mm | depth at which movement noise and a cue begin |
| `SHOCK_PER_TICK` | 2 | dose while in a live shock field |
| `SHOCK_INCAP` | 300 | dose at which the actor is incapacitated (180 ticks) |
| `SHOCK_LETHAL` | 600 | dose at which the run fails |
| `IGNITION_DWELL` | 6 ticks | mixture must stay in band this long before ignition |
| `FIRE_INTENSITY` | 400 ‰ | intensity a gas ignition starts at |
| `FIRE_SPREAD_TICKS` | 30 | delay before fire spreads through a link |
| `FIRE_SPREAD_APERTURE` | 200 cm² | minimum opening a fire can pass |
| `FIRE_BURN_ML_PER_S` | 40 000 | gas consumed by fire |
| `WATER_LINK_K` | 0.01 | ml/s per (cm² × mm of head above the sill) |
| `GAS_LINK_K` | 0.05 | ml/s of passive species flow per cm² of open aperture |
| `GAS_EXTRACT` | fan-rated | an extractor removes the *species* at its rated flow |
| `OXYGEN_NOMINAL` | 210 ‰ | oxygen in authored air |
| `CONTACTOR_PULL_IN` | 6 ticks | motor start delay |

## 2. Vocabularies

### 2.1 Materials

| Material | Absorbs ‰ | Dries ‰/s | Wet film conducts | Spark class | Combustible ‰ | Permeable | Hardness |
|---|---|---|---|---|---|---|---|
| `steel` | 0 | 60 | yes | `Arc` | 0 | no | 6 |
| `copper` | 0 | 60 | yes | `Arc` | 0 | no | 3 |
| `pvc` | 0 | 80 | no | `None` | 120 | no | 2 |
| `concrete` | 220 | 25 | no | `Static` | 0 | yes | 9 |
| `drywall` | 400 | 30 | no | `None` | 80 | no | 2 |
| `carpet` | 700 | 12 | no | `None` | 260 | no | 1 |
| `rubber` | 0 | 90 | no | `None` | 200 | no | 2 |
| `cable-insulation` | 0 | 90 | no | `Arc` when severed | 140 | no | 1 |

An **ignition source** comes from one of two places, and nowhere else:

1. `Capability::Ignites { class }` on a device — a motor starting, a contactor
   closing, a heater glowing;
2. `MaterialDef::ignition` on a material being cut, struck or abraded — a cutter
   through a live cable.

A light switch's device has no `Ignites` capability and its housing is `pvc`.
That is why toggling it is not an explosion trigger, in this ruleset, for
`sable-2`, without any rule saying so.

### 2.2 Gas species

| Field | `sable-2` (level 2) | `solvent-n` (authoring example, unused) |
|---|---|---|
| density vs air | 1600 ‰ (sinks) | 1050 ‰ (sinks slowly) |
| `lfl` / `ufl` | 40 / 130 ‰ | 55 / 160 ‰ |
| ignition classes | `Inductive`, `Arc`, `HotSurface` | `Arc`, `HotSurface` |
| `oxygen_min` | 170 ‰ | 150 ‰ |
| `oxygen_per_litre` | 2 100 ‰ = 2.1 L O₂ per L burned | 1 000 ‰ = 1.0 L O₂ per L |
| `irritant` | 8 ‰ | 20 ‰ |
| `incapacitate` | 120 ‰ | 180 ‰ |
| `water` | `DisplacedOnly` | `Dissolves { coefficient_permille: 250 }` |

The second column exists to prove the first is data. The engine has no branch
for either. Species lighter than air are refused in v1 (`gases.unsupported_buoyancy`):
the layer model is written for gases that pool.

### 2.3 Cues

| Cue | Channel | Fires at | Notes |
|---|---|---|---|
| `WaterRunning` | audio, spatial | any leak > 0 | loop, location = leak |
| `WaterDrip` | audio | wet entity, rate by wetness | |
| `WaterSlosh` | audio | depth ≥ 25 mm | louder with movement |
| `WaterRise` | audio | depth rising, ≥ 25 mm | subtle, easy to miss |
| `Hiss` | audio | pressurised leak or escape | steam/gas |
| `PumpHum` | audio, spatial | `Moves` running | radius in links |
| `PumpCavitation` | audio | running while wet or starved | precedes damage |
| `VentHum` | audio, spatial | fan running, damper open | idle noise if damper shut |
| `VentStops` | audio | fan running → stopped | the loudest quiet in the game |
| `RelayClack` | audio | any actuator changes | also a fuse-box cue |
| `ArcCrackle` | audio + flash | each arc | short, bright, unmistakable |
| `BreakerTrip` | audio + HUD | energised → dead by protection | |
| `GasReek` | olfactory + HUD tint | concentration ≥ `irritant` | no direction, only presence |
| `GasAlarm` | audio + HUD | detector tripped | |
| `FireWhoomph` | audio + flash | ignition | once per fire |
| `Steam` | visual | hot + wet contact | |
| `WetFootprints` | visual | actor leaves water | decays |
| `IsolatorTag` | visual | lockout thrown | small, local, easy to miss |
| `CutterGrating` | audio | excavation in progress | loud, sustained |
| `Scraping` | audio | debris work, grate clearing | loud, sustained, local |

### 2.4 Evidence

| Tag | Channel | Decays | Visible from |
|---|---|---|---|
| `WetFloor` | Sight | 900 ticks | the compartment |
| `WaterUnderDoor` | Sight | 600 ticks | either side of the link |
| `SteamPlume` | Sight | 300 ticks | the compartment and one link |
| `PumpNoise` | Sound | 120 ticks | attenuated by `noise_permille` |
| `OpenIsolator` | Sight | never | the entity |
| `LockoutTag` | Sight | 1800 ticks | the entity |
| `BurntInsulation` | Sight + Smell | never | the entity |
| `GasAlarmPanel` | Sight | while active | the panel's compartment |
| `DamperMoved` | Sight | 900 ticks | the damper |
| `ForcedDoor` | Sight | never | the door |
| `FreshOpening` | Sight | never | the new link |
| `DebrisMoved` | Sight | 900 ticks | the grate |
| `AlarmLog` | Report | never | the guard radio |

### 2.5 Tools and actor capabilities

`Hands` (always), `Wrench`, `Cutter`, `InsulatedGloves`, `Respirator`,
`Flashlight`, `RepairKit`.

### 2.6 Material conditions

The closed `Condition` vocabulary in design §2.4 includes
`MaterialWaterFilmConductive(entity)`, encoded as
`{"kind":"material_water_film_conductive","entity":"self"}`. It resolves
the subject's `material` and is true exactly when
`MaterialDef.water_film_conductive` is true (the "Wet film conducts" column
in §2.1). It does not infer conductivity from the entity's kind or wetness.

---

## 3. WATER + ELECTRICITY

### WE-1 · `water-wets-surfaces`

`Template:` Wetting · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Entity's compartment has `depth_mm > entity.plinth_mm`; entity is not `sealed`. Scope: one entity, evaluated per entity. |
| **Access & actor** | `access: None`, `actor: []` — environment. |
| **Consumed / transformed** | Water is *not* consumed. `wetness += WET_RATE` per second up to 1000 ‰. |
| **Delay & duration** | Delay 0; continuous while in contact. |
| **Physical outcome** | The entity is wet. Nothing else happens yet. This is deliberately not a hazard. |
| **Device / sensor readout** | `wetness` appears on the plant panel as MOISTURE; depth appears on the room HUD. |
| **Player-visible / audio** | `WaterDrip` on the entity; `WaterSlosh` at ≥ 25 mm; `WetFootprints` when an actor leaves the volume. |
| **Guard-observable evidence** | `WetFloor` after 30 ticks of contact; `WaterUnderDoor` on any link whose sill is under water. |
| **Recovery & irreversible** | Drying (`WE-5`). Nothing irreversible. |
| **Causal events & tests** | Ambient `QuantityChanged`; threshold crossing emits load-bearing `StateChanged`. Tests: `water_on_dry_equipment_does_not_arc`, `a_puddle_below_contact_depth_is_not_lethal`. |

### WE-2 · `wet-arc`

`Template:` Hazard · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | `energised ∧ wetness ≥ 350 ‰ ∧ (integrity < 800 ‰ ∨ exposure = Exposed) ∧ material.water_film_conductive`. Scope: one entity. |
| **Access & actor** | Environment. |
| **Consumed / transformed** | `integrity −= ARC_DAMAGE` per arc; no circuit load change. |
| **Delay & duration** | First arc 15 ticks after the conditions first hold; then every `ARC_PERIOD` (30 ticks) while they hold. |
| **Physical outcome** | A hazard level 1 `Arc` exists at the entity. Each arc is an `IgnitionClass::Arc` event in the compartment, which gas (`EG-3`) may consume. |
| **Device / sensor readout** | Panel: ARCING; circuit load flickers; a `Senses(Integrity)` device would trip. |
| **Player-visible / audio** | `ArcCrackle` (bright, short, loud) + a brief light flash. `BreakerTrip` if a protection law trips. |
| **Guard-observable evidence** | `BurntInsulation` (permanent once charred); `ArcCrackle` is audible two links away. |
| **Recovery & irreversible** | De-energise (`WE-4`) and dry (`WE-5`) clears it. At `integrity 0` the entity is **destroyed** — permanent. |
| **Causal events & tests** | `HazardRaised{Arc}` → `QuantityChanged{integrity}` → possibly `Irreversible{EquipmentDestroyed}`; cause chains to the wetting. Tests: `damaged_wet_energised_equipment_arcs_on_cadence`, `water_on_dry_equipment_does_not_arc`. |

### WE-3 · `shock-field`

`Template:` Exposure · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Volume has `depth ≥ CONTACT_DEPTH` **and** at least one live `Arc` hazard in it **and** the water reaches that entity's plinth. Scope: the volume; effects on an actor standing in it. |
| **Access & actor** | Actor must be in the volume and touching the water (grounded). `InsulatedGloves` reduces dose to 0; `Rubber` footwear is not modelled (deliberately — one capability, not a footwear system). |
| **Consumed / transformed** | `shock_dose += 2` per tick; dose decays `−1`/tick out of contact. |
| **Delay & duration** | No delay; 2.5 s of contact to incapacitation. |
| **Physical outcome** | At 300 ‰ dose: incapacitated for 180 ticks (cannot move, can be grabbed). At 600: run failure `Electrocuted`. Below 300: a flinch, a cue, no injury. |
| **Device / sensor readout** | Actor dose on the HUD; a `Senses(Wetness)` device may warn. |
| **Player-visible / audio** | `ArcCrackle` nearby; a rising buzz; the screen edges close in at dose > 150. |
| **Guard-observable evidence** | None until it happens: a collapsed infiltrator is `Found` (ordinary guard collision), or `BurntInsulation` at the source. |
| **Recovery & irreversible** | Step out (dose decays). Incapacitation is temporary. Electrocution ends the run. |
| **Causal events & tests** | `ShockActor{dose}`, `HazardRaised{Shock}`; cause = the arc that made the field. Tests: `a_puddle_below_contact_depth_is_not_lethal`, `shallow_puddles_off_the_source_are_harmless`. |

### WE-4 · `protective-isolation`

`Template:` Control · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Entity has `LockOut { series }`; actor at it. Scope: the target circuit **or** a single entity. A circuit-targeted lockout de-energises every load on it (rank R0); an entity-targeted lockout removes only that device's supply and leaves the rest of the circuit live. Level 1 authors a single pump and keeps the floor running, so its isolator is entity-targeted — that difference is the whole distinction between protective isolation (A4) and cutting the feed (A5). |
| **Access & actor** | `access: Local(entity)`, `actor: [Hands]`. Remote operation **cannot** throw or lift it, ever. |
| **Consumed / transformed** | 30 ticks of the actor's time; no water, no power (throwing a breaker interrupts load). |
| **Delay & duration** | 30 ticks to throw, 30 to restore; latched indefinitely. |
| **Physical outcome** | The series target reads de-energised at rank R0. Remote start attempts are refused with `Denied{LockedOut}`. Drying becomes possible (`WE-5`). |
| **Device / sensor readout** | Circuit state on the panel reads ISOLATED; `PlantActuator.Active` reports false. |
| **Player-visible / audio** | `IsolatorTag`; a heavy clack; the hum of anything downstream stops (`VentStops` if a fan). |
| **Guard-observable evidence** | `OpenIsolator` (permanent), `LockoutTag` (30 minutes of game time). |
| **Recovery & irreversible** | Restore locally. Nothing irreversible. **This is the non-destructive lever**: it removes a hazard without destroying anything. |
| **Causal events & tests** | Load-bearing `Actuated{LockOut}`, `Denied{LockedOut}`; cause root = the player command. Tests: `isolation_prevents_remote_start`, `manual_isolation_beats_remote_and_emits_a_denial`. |

### WE-5 · `drying`

`Template:` Wetting (inverse) · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Entity is `de-energised` **and** its compartment depth is below its plinth. Scope: one entity. |
| **Access & actor** | Environment. |
| **Consumed / transformed** | `wetness −= material.dries_permille_per_s` per second. Absorbent materials also keep a residue: below `absorbs_permille` the rate halves. |
| **Delay & duration** | Continuous; steel dries in ~6 s, carpet in ~80 s. |
| **Physical outcome** | Wetness falls. Below `WET_THRESHOLD` arcing stops. |
| **Device / sensor readout** | MOISTURE falls on the panel. |
| **Player-visible / audio** | `WaterDrip` slows and stops. |
| **Guard-observable evidence** | `WetFloor` decays with the water, not with the entity. |
| **Recovery & irreversible** | The entity recovers fully. **Energised equipment does not dry**: that is what makes isolation protective rather than merely cautious. |
| **Causal events & tests** | Ambient `QuantityChanged`. Tests: `drying_requires_de_energisation`, `restoring_power_to_wet_equipment_arcs_after_delay_not_instantly`. |

### WE-6 · `wet-run-damage`

`Template:` Wetting + Hazard · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Entity `running ∧ wetness ≥ 350 ‰ ∧ material.water_film_conductive`. Scope: one entity. |
| **Access & actor** | Environment. |
| **Consumed / transformed** | `integrity −= 2 ‰/s` while running wet (in addition to arc damage). |
| **Delay & duration** | Continuous while running wet. |
| **Physical outcome** | The motor degrades audibly and then fails: `integrity 0` → **destroyed**, stops, load drops. A destroyed pump cannot be restarted by any player action. |
| **Device / sensor readout** | `PumpCavitation` cue; integrity falls; `PlantActuator.Active` goes false at failure. |
| **Player-visible / audio** | `PumpCavitation` (gravelly, wrong); then silence and a `RelayClack`. |
| **Guard-observable evidence** | `PumpNoise` while it runs; `BurntInsulation` and a dead pump afterwards. |
| **Recovery & irreversible** | Stop and dry *before* integrity reaches 0. **Irreversible: `EquipmentDestroyed`.** |
| **Causal events & tests** | `QuantityChanged{integrity}` → `Irreversible{EquipmentDestroyed}`. Tests: `running_a_wet_motor_destroys_it`. |

### WE-7 · `re-energise-wet`

`Template:` Energisation + Hazard · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | An entity transitions `de-energised → energised` while `wetness ≥ WET_THRESHOLD`. Scope: one entity. |
| **Access & actor** | Whoever restores the circuit (remote R4, manual R1, or control R3). |
| **Consumed / transformed** | Load draws; integrity at risk. |
| **Delay & duration** | `CONTACTOR_PULL_IN` (6 ticks) to pull in, then `WE-2`'s 15-tick onset. **There is a deliberate beat of about a third of a second between "it starts" and "it arcs".** |
| **Physical outcome** | The entity starts; if it is still wet, arcing begins on the `WE-2` schedule. If it is *dry enough*, nothing happens. Order of operations is the whole lesson. |
| **Device / sensor readout** | Panel shows RUNNING then ARCING; `ArcCrackle`. |
| **Player-visible / audio** | A start thunk, a hum that sounds wrong, then crackling. |
| **Guard-observable evidence** | `BurntInsulation`; `BreakerTrip` if protection trips. |
| **Recovery & irreversible** | Isolate again, dry, retry. Destroyed if it runs wet long enough. |
| **Causal events & tests** | `Actuated{energised}` → delayed `HazardRaised{Arc}` with the cause parenting the restore command. Tests: `restoring_power_to_wet_equipment_arcs_after_delay_not_instantly`. |

---

## 4. ELECTRICITY + GAS

### EG-1 · `powered-actuator`

`Template:` Energisation · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Entity has `Powered { circuit, load }`; circuit energised; no `LockOut` in series; for a `Gates` target, the target position is the one requested. Scope: one entity. |
| **Access & actor** | Whoever commands it: manual local (R1), controller (R3), remote (R4). |
| **Consumed / transformed** | Circuit load. A fan at 40 L/s draws 4 units; a pump 6 units; a detector 0.1 units (integer units, `4` and `1`). |
| **Delay & duration** | `CONTACTOR_PULL_IN` for rotating machinery; 0 for sensors. Latched until commanded otherwise. |
| **Physical outcome** | A fan moves gas, a valve gates flow, a detector samples, a pump moves water — per its own capability. This rule is only the precondition; it has no effect of its own. |
| **Device / sensor readout** | `PlantActuator.Active` = running truth; a live sensor reports truth. |
| **Player-visible / audio** | `RelayClack` then `VentHum` / `PumpHum`. |
| **Guard-observable evidence** | `PumpNoise`; plant-room equipment running. |
| **Recovery & irreversible** | Stop. Nothing irreversible. |
| **Causal events & tests** | `Actuated`. Tests: `unpowered_fan_stops_extraction_and_gas_accumulates`, `circuit_lockout_prevents_load_draw`. |

### EG-2 · `loss-of-extraction`

`Template:` Flow · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A gas source produces in a compartment; no running `Moves{Gas}` path out of it; scope = that compartment and everything reachable through open links. |
| **Access & actor** | Environment (or whoever stopped the fan). |
| **Consumed / transformed** | Gas `+rate` at the source; distribution per `WA-4`/`WA-6`. Extraction removes `−rate` of species where a fan runs. |
| **Delay & duration** | Continuous. |
| **Physical outcome** | Concentration rises until the source stops, the volume saturates, or ignition consumes it. Rooms fill **from the source outward**, driven by density and links, not uniformly. |
| **Device / sensor readout** | Concentration rises; a powered detector trips at its threshold; an unpowered one reports nothing (`EG-6`). |
| **Player-visible / audio** | `VentStops`; `GasReek` at `irritant`; the HUD tints. |
| **Guard-observable evidence** | `GasAlarmPanel` if the detector is live; otherwise only the guard's nose inside the compartment. |
| **Recovery & irreversible** | Restore extraction. Nothing irreversible by itself — but it is the setup for `EG-3`. |
| **Causal events & tests** | `QuantityChanged{gas}`; cause parents to the command that stopped the fan. Tests: `unpowered_fan_stops_extraction_and_gas_accumulates`. |

### EG-3 · `ignition`

`Template:` Hazard · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | In **one compartment**: species `g` present with `gas_permille ∈ [lfl, ufl]` continuously for `IGNITION_DWELL`; `oxygen ≥ g.oxygen_min`; and an ignition event whose class is in `g.ignition_classes`. Scope: that compartment only (no explosion through a wall; spread is `EG-4`). |
| **Access & actor** | Environment. The ignition *event* may come from a player action (starting a pump, cutting a cable). |
| **Consumed / transformed** | 10 % of the compartment's gas is consumed at ignition; the rest at `FIRE_BURN_ML_PER_S`; `oxygen −= gas burned × oxygen_per_litre_permille` (for `sable-2`, 2.1 L O₂ per L burned). |
| **Delay & duration** | Dwell 6 ticks (conditions must hold); then ignition is immediate. |
| **Physical outcome** | `SetFire`, `HazardRaised{Fire}`, and the source entity takes `−200 ‰` integrity. Gas below LFL or above UFL does not ignite — a rich mixture is as safe as an empty one, which is a real and legible property. |
| **Device / sensor readout** | Detector trips (if it survived); circuit load spikes then falls. |
| **Player-visible / audio** | `FireWhoomph`, a flash, then smoke and heat. |
| **Guard-observable evidence** | `GasAlarmPanel`, smoke sighted from the corridor, `BurntInsulation`, `AlarmLog`. |
| **Recovery & irreversible** | Extinguishing needs oxygen starvation (`EG-4`) or a suppression rule no level authors yet. **Irreversible: `GasIgnited`, and any equipment destroyed.** |
| **Causal events & tests** | `HazardRaised{IgnitionBand}` → `Actuated{class-event}` → `Irreversible{GasIgnited}`. Tests: `a_light_switch_does_not_ignite_gas`, `a_pump_start_ignites_gas_in_band`, `ignition_requires_the_authored_class`, `ignition_requires_dwell_not_a_single_tick`. |

### EG-4 · `combustion`

`Template:` Hazard + Flow · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A fire exists in a compartment. Scope: the compartment; spread to adjacent compartments through links. |
| **Access & actor** | Environment. |
| **Consumed / transformed** | Gas at 40 L/s; oxygen at `oxygen_per_litre_permille` of the gas burned (2.1 L O₂ per L for `sable-2`, so burning the riser's 96 L would take 202 L of its 504 L); combustible entities lose integrity at 10 ‰/s; `StructuralChar` after 30 ticks. |
| **Delay & duration** | Spread checked every 30 ticks; fire persists while gas > LFL **or** a combustible material is burning. |
| **Physical outcome** | Fire intensity 400 ‰, rising 20 ‰/s while fuel remains. Spread requires a link with `aperture ≥ 200 cm²` and a destination with `combustible > 0`. A sealed door stops it. Oxygen depletion (below 170 ‰) starves it. |
| **Device / sensor readout** | Detectors trip then die; circuits in the compartment fail at `StructuralChar`. |
| **Player-visible / audio** | Roaring, smoke, screen shake (or a static overlay with reduced motion). |
| **Guard-observable evidence** | `SteamPlume`, smoke, `AlarmLog`; a fire in a corridor denies traversal (`OnFire`). |
| **Recovery & irreversible** | Let it starve; no extinguisher rule in level 2. **Irreversible: `StructuralChar`, `FireSpread`, `EquipmentDestroyed`.** |
| **Causal events & tests** | `HazardRaised{Fire}` → `Irreversible{StructuralChar}` → `HazardRaised{Fire}` in the neighbour. Tests: `fire_consumes_the_species_and_spreads_by_combustibility`, `a_sealed_door_stops_fire_spread`. |

### EG-5 · `detector-alarm`

`Template:` Control · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Entity has `Senses{GasConcentration}` and `Alarms`; energised; not disabled. Scope: its compartment. |
| **Access & actor** | Whoever wires or spoofs it; the environment otherwise. |
| **Consumed / transformed** | None. |
| **Delay & duration** | Trip immediate at threshold; clear at `threshold − hysteresis`; `dwell_ticks` 120 before it can clear. |
| **Physical outcome** | `PlantSensor.Tripped` high for the duration; the alarm entity raises `Cue::GasAlarm` and `EvidenceTag::GasAlarmPanel`. |
| **Device / sensor readout** | Tripped; panel shows a concentration and a state. |
| **Player-visible / audio** | `GasAlarm`; the panel on the wall goes red. |
| **Guard-observable evidence** | `GasAlarmPanel` (while active), `AlarmLog` (permanent). |
| **Recovery & irreversible** | Clear by venting or sealing the source. Nothing irreversible. |
| **Causal events & tests** | `StateChanged{Tripped}` → `EvidenceEmitted`. Tests: `a_wired_sensor_starts_the_pump_one_hop_per_tick`, `detector_hysteresis_prevents_chatter`. |

### EG-6 · `loss-of-detection`

`Template:` Control (negative) · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A sensor's circuit is dead, the sensor is disabled by a wire, or a spoof is active. Scope: that sensor. |
| **Access & actor** | Whoever cut the power, wired the disable, or spoofed it. |
| **Consumed / transformed** | None. **Truth is unchanged**: the gas is still there. |
| **Delay & duration** | Immediate on loss; persists until restored. |
| **Physical outcome** | The reported value is 0 / stale. No alarm. The compartment continues to accumulate gas. |
| **Device / sensor readout** | Reports 0 ‰; the hacker's panel shows a clean room. |
| **Player-visible / audio** | **The absence of a cue is the cue.** The infiltrator's `GasReek` still fires — from truth, not from the sensor. |
| **Guard-observable evidence** | None until someone physically enters, or the alarm log shows a gap after the fact. |
| **Recovery & irreversible** | Restore power / remove the spoof. The consequences of the blind period are not reversible. |
| **Causal events & tests** | `Denied`/`StateChanged{sensor_dead}`. Tests: `a_spoofed_detector_misleads_the_controller_but_not_ignition`, `an_unpowered_detector_reports_nothing_while_gas_rises`. |

---

## 5. WATER + GAS / AIR

### WA-1 · `flood-obstructs-link`

`Template:` Flow · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A link with `fluid_block: AboveSill` whose mouth is submerged: the depth on its low side exceeds `sill_mm`. Scope: one link. Gate blocking applies independently (design §2.1). |
| **Access & actor** | Environment. |
| **Consumed / transformed** | Effective aperture becomes **0 for gas**. The water itself is not consumed, and **liquid flow is unchanged** — a submerged opening still weirs water, bounded by `capacity_ml_per_s`. Closing a duct to air must not turn it into a valve for water, or a flooded crawl would stop flooding. |
| **Delay & duration** | Immediate on crossing; persists until the depth falls. |
| **Physical outcome** | The duct stops carrying air. The compartment it served is no longer ventilated, even if the fan is running and the damper is open. |
| **Device / sensor readout** | No airflow on a `Senses(Flow)` device; concentration in the served compartment rises. |
| **Player-visible / audio** | `VentHum` continues but the draught at the grille stops; `WaterSlosh` in the duct. |
| **Guard-observable evidence** | `WaterUnderDoor` at the duct mouth. |
| **Recovery & irreversible** | Lower the water (`WA-6`). Nothing irreversible. |
| **Causal events & tests** | `StateChanged{link_blocked}`. Tests: `flood_closes_a_duct_without_blocking_traversal`, `flooded_duct_still_weirs_water`. |

### WA-2 · `flood-blocks-traversal`

`Template:` Flow (traversal predicate) · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A link with `clearance_mm > 0` and `actor: Passable` whose depth exceeds `clearance_mm`. Scope: one link, one actor. |
| **Access & actor** | The actor attempting traversal. |
| **Consumed / transformed** | None; the traversal is refused. |
| **Delay & duration** | Immediate. |
| **Physical outcome** | `TraversalDenied{Flooded}`. Fluid transport is **unaffected**: a flooded crawl may still carry water, and a dry crawl may carry no air. |
| **Device / sensor readout** | Depth on the HUD; the route indicator greys out. |
| **Player-visible / audio** | A refused step; sloshing at knee height; `WaterRise` underneath. |
| **Guard-observable evidence** | `WetFootprints` where the actor turned back. |
| **Recovery & irreversible** | Lower the water. Nothing irreversible. |
| **Causal events & tests** | `TraversalDenied{Flooded}`. Tests: `depth_above_clearance_blocks_traversal_without_closing_the_duct`, `a_flooded_duct_can_still_be_walked`. |

### WA-3 · `water-displaces-gas`

`Template:` Flow · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Water volume increases in a compartment holding gas. Scope: the compartment and the links open to it. |
| **Access & actor** | Environment. |
| **Consumed / transformed** | `gas_ml` is **not** destroyed. The displaced mixture carries `V × gas_ml / air_l` of the species into the neighbouring volume, where `V` is the water volume added. `Dissolves` species instead remove `coefficient × water_ml`, never more than present. |
| **Delay & duration** | Same tick as the water movement (one `WA-6` pass). |
| **Physical outcome** | The gas that was in the volume the water took ends up in the neighbouring compartment. A flood that takes 3 % of a room's volume moves at most 3 % of its gas: **the point is that it is not zero and it is not neutralisation.** |
| **Device / sensor readout** | Concentration falls here and rises there; both detectors report truth. The player-visible haze layer climbs the wall as the water rises, which is the perceptible part. |
| **Player-visible / audio** | `GasReek` moves with the gas; `WaterSlosh` where it rose. |
| **Guard-observable evidence** | The destination compartment's `GasAlarmPanel` or the guard's nose. |
| **Recovery & irreversible** | Vent the destination. Nothing irreversible. |
| **Causal events & tests** | `MoveGas{displaced}`. Tests: `water_displaces_gas_without_neutralising_it`, `a_dissolving_species_is_parameterised_not_hardcoded`. |

### WA-4 · `airflow-redistributes`

`Template:` Flow · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A `Moves{Gas}` entity is running and every link in its bound `path` passes gas: gates open, no submersion block (`WA-1`) or debris block. Scope: its `intake` and `discharge` compartments (design §2.2). |
| **Access & actor** | Whoever controls the fan: local, wired, or remote. |
| **Consumed / transformed** | Moves up to `rate_ml_per_s` of the species it draws — an inline transfer fan moves 1.5 L/s of gas, not 1.5 L/s of air. It cannot separate species; if two share a compartment, both move in proportion. |
| **Delay & duration** | Continuous while running. |
| **Physical outcome** | Directed flux. A reversible fan run backwards moves the same gas the other way. This is how "airflow changes distribution between compartments" is authored without a pressure model. |
| **Device / sensor readout** | Flow and direction on a `Senses(Flow)` device; concentrations change at both ends. |
| **Player-visible / audio** | `VentHum` with a directional note; draught cues near the grille. |
| **Guard-observable evidence** | `DamperMoved` if a damper was moved; otherwise nothing. |
| **Recovery & irreversible** | Reverse or stop. Nothing irreversible. |
| **Causal events & tests** | `MoveGas{directed}`. Tests: `airflow_moves_gas_between_compartments`, `reversing_the_fan_moves_the_gas_back`. |

### WA-5 · `extraction-clears`

`Template:` Flow · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Fan running into an `open_to_exterior` volume or a vent stack, gate open, intake unblocked. Scope: the intake compartment. |
| **Access & actor** | Whoever runs the fan. |
| **Consumed / transformed** | Removes up to `rate_ml_per_s` of the species to the exterior sink (`GAS_EXTRACT`). |
| **Delay & duration** | Continuous; 24 L at 4 L/s takes 6 s. |
| **Physical outcome** | Concentration falls to 0 at the fan's rate. The extraction path is the *only* thing that removes gas, unless a species dissolves. |
| **Device / sensor readout** | Concentration falls; detector clears after hysteresis. |
| **Player-visible / audio** | `VentHum` continues; the reek fades. |
| **Guard-observable evidence** | None. |
| **Recovery & irreversible** | N/A. |
| **Causal events & tests** | `QuantityChanged{gas↓}`. Tests: `extraction_clears_the_compartment`, `extraction_through_a_flooded_duct_does_nothing`. |

### WA-6 · `pressure-equalisation`

`Template:` Flow · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Any two compartments joined by an open, unblocked link. Includes a pump's discharge when `check_valve: false` and the pump is off. Scope: every link, every tick. |
| **Access & actor** | Environment. |
| **Consumed / transformed** | Water: `min(capacity_ml_per_s, aperture_cm2 × max(0, depth − sill) × WATER_LINK_K)` — a weir with a ceiling. Gas: `aperture_cm2 × GAS_LINK_K` of species (passive; slow). Both computed from the **sense snapshot**, applied antisymmetrically. A stopped pump with `check_valve: false` returns its `return_ml` discharge column to the intake. |
| **Delay & duration** | Per tick. |
| **Physical outcome** | Bounded equalisation. Opposing flows cancel exactly. A backflow through a stopped pump refills the sump and can undo an hour of pumping in a minute. |
| **Device / sensor readout** | Depth and concentration change on both sides. |
| **Player-visible / audio** | `WaterRunning` / gurgling; a slow rise where there should be none. |
| **Guard-observable evidence** | `WetFloor` at the destination. |
| **Recovery & irreversible** | N/A; drain or pump. |
| **Causal events & tests** | Ambient `MoveWater`/`MoveGas`. Tests: `opposing_flows_cancel_regardless_of_order`, `backflow_refills_the_sump_when_the_pump_is_off`. |

---

## 6. TUNNELS + UTILITIES

### TU-1 · `excavate-exposes`

`Template:` Breach · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Actor at a `Breaks { target, tool, hardness }` surface; tool rating ≥ hardness; the level authors an `ExposedUtility { entity, at, depth_mm }` behind it. Scope: the surface and the utility. |
| **Access & actor** | `access: Local(surface)`, `actor: [Hands, Cutter]`. |
| **Consumed / transformed** | Tool durability −1; actor stamina; time = `30 × hardness / tool_rating` ticks (a concrete wall is 9, a cutter is 6 → 45 ticks). |
| **Delay & duration** | The cut takes the time above; exposure is revealed at completion. |
| **Physical outcome** | The utility becomes visible and touchable (`SetTag(Exposed)`). **A live cable emits one `Arc` class event at first contact** — so exposing a cable in a gas atmosphere is itself a risk. A pressurised pipe hisses but holds. |
| **Device / sensor readout** | Circuit flicker for a live cable; pressure drop for a pipe; a `Senses(Pressure)` device would see it. |
| **Player-visible / audio** | `CutterGrating` throughout; `ArcCrackle` on a live cable; `Hiss` on a pipe. |
| **Guard-observable evidence** | `CutterGrating` (sound, attenuated); dust; the hole is `FreshOpening` only after `TU-2`. |
| **Recovery & irreversible** | The exposure itself is reversible with `RepairKit`. Nothing irreversible yet. |
| **Causal events & tests** | `StateChanged{exposed}`. Tests: `excavation_exposes_the_authored_utility`, `cutting_a_live_cable_arcs_once`. |

### TU-2 · `excavate-breach`

`Template:` Breach · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | `Breaks { target: Link(l) }` where the link is authored with `actor: Blocked, clearance_mm: 0`; actor present; tool rating ≥ hardness of the separating material. Scope: the wall, the two compartments. |
| **Access & actor** | `access: Local`, `actor: [Hands, Cutter]`. |
| **Consumed / transformed** | Tool durability −1; time `30 × hardness / tool_rating`. No fluids move during the cut. |
| **Delay & duration** | As above. |
| **Physical outcome** | `AlterLink(clearance = authored 900 mm)`. Two previously separate compartments are now connected: water, gas and actors all equilibrate through the new opening, immediately and permanently. |
| **Device / sensor readout** | Both compartments' depth and concentration converge. |
| **Player-visible / audio** | Dust, a change in room tone, `VentHum` becomes audible through the hole. |
| **Guard-observable evidence** | `FreshOpening` (permanent), noise during the cut. |
| **Recovery & irreversible** | **Irreversible: `StructuralBreach`** — the hole cannot be repaired in the level. |
| **Causal events & tests** | `StateChanged{clearance}` → `WA-6` flow. Tests: `breach_connects_previously_separate_compartments`. |

### TU-3 · `sever-utility`

`Template:` Breach · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Utility tagged `Exposed` (`TU-1`) and cut again. Scope: the utility, its circuit or its pipe run. |
| **Access & actor** | `access: Local`, `actor: [Hands, Cutter]`. Insulated gloves do **not** help: the tool is conductive and the actor is holding it. |
| **Consumed / transformed** | Cable: the circuit goes dead, load −. Pipe: `leak_ml_per_s` becomes 4000 (full bore). |
| **Delay & duration** | Immediate on the second cut. |
| **Physical outcome** | Dark circuit or a 4 L/s jet into the compartment. A severed live cable emits `Arc`; if gas is in band, `EG-3` may fire. |
| **Device / sensor readout** | Circuit dead; water depth rising fast. |
| **Player-visible / audio** | `ArcCrackle` or `Hiss`; darkness if lights were on that circuit. |
| **Guard-observable evidence** | `BurntInsulation`, `WaterUnderDoor`, and a `BreakerTrip` on the panel. |
| **Recovery & irreversible** | **Irreversible: `UtilitySevered`**; a `RepairKit` restores the circuit only if the level says so. Water is recoverable. |
| **Causal events & tests** | `Irreversible{UtilitySevered}` → `StateChanged{circuit_dead}` or `AddWater`. Tests: `cutting_exposed_cable_kills_the_authored_circuit`. |

### TU-4 · `water-enters-tunnel`

`Template:` Flow · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A tunnel/crawl link carries water (`WA-6`) into a compartment at lower `elevation`, or a severed pipe discharges into it. Scope: the tunnel and its destination. |
| **Access & actor** | Environment. |
| **Consumed / transformed** | Water redistributes. If the destination is sealed, its air is displaced (gas leaves through links, `WA-3`), and its depth rises. |
| **Delay & duration** | Per `WA-6` rate; a tunnel with a 900 cm² aperture at 100 mm head moves 900 ml/s. |
| **Physical outcome** | The tunnel becomes impassable when depth exceeds its `clearance_mm` (`WA-2`), long before the destination fills. |
| **Device / sensor readout** | Depth in the destination; a `Senses(WaterDepth)` device trips. |
| **Player-visible / audio** | `WaterRunning` moving through the tunnel; `WaterRise` at the far end — the player's first warning is usually the wrong room. |
| **Guard-observable evidence** | `WetFloor` in the destination, `WaterUnderDoor`. |
| **Recovery & irreversible** | Close the source; drain. Nothing irreversible. |
| **Causal events & tests** | `MoveWater` → `TraversalDenied{Flooded}`. Tests: `water_enters_a_tunnel_downhill`, `a_sealed_tunnel_displaces_its_air`. |

### TU-5 · `noise-and-vibration`

`Template:` Observation · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Any `Sounds` entity active, or a tool in use. Scope: the source compartment plus every link it can reach, attenuated. |
| **Access & actor** | Whoever made the noise. |
| **Consumed / transformed** | None. |
| **Delay & duration** | While the source runs. |
| **Physical outcome** | Noise level at distance = `level × Π((1000 − link.noise_permille)/1000)`. Evidence is emitted only where level ≥ the observation site's threshold. |
| **Device / sensor readout** | None. |
| **Player-visible / audio** | The cue itself, spatialised; the infiltrator hears it directly if close. |
| **Guard-observable evidence** | `PumpNoise`, `CutterGrating` as `Sound` evidence; the guard investigates the loudest reachable site. |
| **Recovery & irreversible** | Stop the noise. The evidence decays over 120 ticks, so the guard's response outlives the sound. |
| **Causal events & tests** | `EvidenceEmitted{Sound}`. Tests: `cutting_is_heard_through_links_by_attenuation`, `evidence_is_not_emitted_beyond_its_attenuation`. |

### TU-6 · `isolate-then-cut`

`Template:` composition (Breach + Control) · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A planned breach (`TU-2`) or exposure (`TU-1`) where the author has placed a utility behind the surface; the actor first isolates the circuit (`WE-4`) or closes the valve (`Gate`). Scope: the work site. |
| **Access & actor** | Local, with tool. The isolation must be local too — a remote cut is not a lockout and does not satisfy the safety condition. |
| **Consumed / transformed** | Two actions instead of one: 30 ticks isolation + the cut. |
| **Delay & duration** | Sequential; the isolation must precede the cut. |
| **Physical outcome** | **This is not a new mechanic.** It is `TU-1`/`TU-2` with the ignition/hazard preconditions removed by a prior `WE-4` or gate action. The cut is quiet, spark-free and dry, and the resulting route is safe to use. |
| **Device / sensor readout** | Circuit dead before the cut; no arc event; no water event. |
| **Player-visible / audio** | `CutterGrating` and nothing else. |
| **Guard-observable evidence** | `OpenIsolator` + `FreshOpening`; the *absence* of `BurntInsulation` is the mark of a careful intruder. |
| **Recovery & irreversible** | The breach is still irreversible; the utilities are not damaged. |
| **Causal events & tests** | `Actuated{LockOut}` precedes `StateChanged{clearance}`; no `HazardRaised` in between. Tests: `isolation_first_makes_the_breach_safe`, `cutting_without_isolation_can_arc`. |

---

## 7. CONTROL SIGNALS + PHYSICAL SYSTEMS

### CP-1 · `sensor-to-actuator`

`Template:` Control · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A `PlantSensor` device whose `Tripped` output is wired to a `PlantActuator` input, directly or through relays; both enabled. Scope: the device pair, one wire hop per tick. |
| **Access & actor** | Whoever laid the wire (hacker by bandwidth, infiltrator by busy ticks — ADR-0012 unchanged). |
| **Consumed / transformed** | Bandwidth or busy ticks per wire, per `SeatProfile`. |
| **Delay & duration** | Exactly one tick per wire hop; a sensor→relay→pump chain is 3 ticks including the plant's own step. |
| **Physical outcome** | The actuator's *request* flips; the plant then resolves it at R3 against power, gates and interlocks. A wire can ask; it cannot make water flow. |
| **Device / sensor readout** | `PlantActuator.Active` reports the outcome, not the request — the two differ when an interlock refuses. |
| **Player-visible / audio** | `RelayClack` at each hop; then the machine's own cue. |
| **Guard-observable evidence** | The machine's own evidence; the wire itself is `Wired` in the signal log. |
| **Recovery & irreversible** | Cut the wire. Nothing irreversible. |
| **Causal events & tests** | `SignalEvent::Switched` → `Actuated`. Tests: `a_wired_sensor_starts_the_pump_one_hop_per_tick`, `signal_hop_count_unchanged_with_infrastructure_enabled`. |

### CP-2 · `controller-regulation`

`Template:` Control · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | An authored `ControllerDef { id, reads, targets, law, hysteresis_permille, dwell_ticks, cooldown_ticks, rank }` (design doc §2.4); scope = the entities it targets. |
| **Access & actor** | Authored (the building's own automation) or a player-programmed law, if a level allows it. |
| **Consumed / transformed** | None. |
| **Delay & duration** | One decision per `cooldown_ticks`; a state change requires `dwell_ticks` of the new condition. |
| **Physical outcome** | Requests at R3. Hysteresis + dwell + cooldown are what stop a sensor/actuator pair chattering every tick. |
| **Device / sensor readout** | Regular, predictable cycling visible on the panel. |
| **Player-visible / audio** | Rhythmic machine cues; a player can learn the period and time an action to it. |
| **Guard-observable evidence** | Only the machine's evidence. |
| **Recovery & irreversible** | Disable the controller. Nothing irreversible. |
| **Causal events & tests** | `Actuated{controller}`. Tests: `a_feedback_loop_is_bounded_and_repeats_exactly`, `controller_dwell_prevents_chatter`. |

### CP-3 · `spoofed-readout`

`Template:` Control (deception) · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A spoof is active on a sensor's signal device (forged via the grounded network, as `MuteSensor` is today). Scope: that device's reported value. |
| **Access & actor** | The hacker, with network reach to the device's node. |
| **Consumed / transformed** | Trace/alert cost, as any other intrusion; no physical quantity changes. |
| **Delay & duration** | From the command until the spoof is removed or the device is power-cycled. |
| **Physical outcome** | **None.** The compartment's gas, depth and fire are exactly what they were. Only `ReportedValue` changes. |
| **Device / sensor readout** | Reports the spoofed value to controllers and to the hacker's panel. |
| **Player-visible / audio** | The hacker sees a clean room. The infiltrator smells gas. The two seats now disagree, and the only fix is talking to each other. |
| **Guard-observable evidence** | None from the spoof. After the fact, `AlarmLog` shows a suspiciously flat trace. |
| **Recovery & irreversible** | Remove the spoof. The consequences of the blind window are not reversible. |
| **Causal events & tests** | `StateChanged{spoof}` (network event); physics events continue with the truth. Tests: `a_spoofed_detector_misleads_the_controller_but_not_ignition`. |

### CP-4 · `manual-override`

`Template:` Control · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | A `LockOut` entity in series with a circuit or actuator; the actor is at it. Scope: the series target. |
| **Access & actor** | `Local` + `Hands`, as `WE-4`. This rule is the *precedence* half of `WE-4`. |
| **Consumed / transformed** | Actor time (30 ticks each way). |
| **Delay & duration** | Latched until physically restored. No timeout. |
| **Physical outcome** | R0 beats R1–R5. Remote attempts produce `Denied{LockedOut}` and cost nothing. Restoring the isolator does **not** re-run the refused commands: the remote request must be issued again. |
| **Device / sensor readout** | ISOLATED / LIVE on the panel. |
| **Player-visible / audio** | A denial cue at the hacker's end (a flat click, not an error buzz — the request reached the machine and was refused by it). |
| **Guard-observable evidence** | `OpenIsolator`, `LockoutTag`. |
| **Recovery & irreversible** | Restore locally. Nothing irreversible. |
| **Causal events & tests** | `Denied{LockedOut}` with the refused intent in the payload. Tests: `manual_isolation_beats_remote_and_emits_a_denial`, `restoring_an_isolator_does_not_replay_refused_commands`. |

### CP-5 · `interlock`

`Template:` Control · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | An authored interlock law (e.g. "no `Inductive` start while gas ≥ LFL"), reading a sensor. Scope: the targets it names. |
| **Access & actor** | Authored; the player can disable it only by disabling its sensor (which is what `CP-3` is for). |
| **Consumed / transformed** | None. |
| **Delay & duration** | Evaluated every tick at R2. |
| **Physical outcome** | Refuses the start with `Denied{Interlock}`. If the interlock's sensor is spoofed or dead, it refuses nothing — the interlock is only as good as its input. |
| **Device / sensor readout** | `PlantActuator.Active` stays false while the request is true: the panel shows REQUESTED / NOT RUNNING, which is itself information. |
| **Player-visible / audio** | `BreakerTrip` / a refusal thunk. |
| **Guard-observable evidence** | None. |
| **Recovery & irreversible** | Clear the condition or restore the sensor. Nothing irreversible. |
| **Causal events & tests** | `Denied{Interlock}`. Tests: `an_interlock_refuses_a_start_even_when_wired`, `a_spoofed_detector_defeats_the_interlock`. |

### CP-6 · `bounded-feedback`

`Template:` Control (invariant) · `Rule version:` 1 · `Levels:` 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | Any cycle sensor → controller/wire → actuator → sensor. Scope: the cycle. |
| **Access & actor** | N/A. |
| **Consumed / transformed** | N/A. |
| **Delay & duration** | Period ≥ number of wire hops + `dwell_ticks` + `cooldown_ticks`. One hop per tick is the floor; the plant adds no zero-cost loop. |
| **Physical outcome** | The loop oscillates within an authored band and repeats exactly. It cannot accelerate, because a wave cannot feed itself (`rules.no_self_schedule`). |
| **Device / sensor readout** | A repeating square wave at a computable period. |
| **Player-visible / audio** | A rhythm the player can act around. |
| **Guard-observable evidence** | A rhythmic machine is unremarkable; a machine cycling *faster than its design* would be evidence, and cannot happen. |
| **Recovery & irreversible** | N/A. |
| **Causal events & tests** | Repeats with identical tick deltas over 10 000 ticks. Tests: `a_feedback_loop_is_bounded_and_repeats_exactly`. |

### CP-7 · `remote-operation`

`Template:` Control · `Rule version:` 1 · `Levels:` 1, 2

| Field | Specification |
|---|---|
| **Preconditions & scope** | The hacker reaches the actuator's bound node (`netsim` vantage, pivot depth) and the bound signal device is accessible (ADR-0012 `Access`). Scope: one actuator. |
| **Access & actor** | `Remote(SignalId)`; the hacker's seat. |
| **Consumed / transformed** | Bandwidth for wiring; trace/alert for network intrusion, unchanged. |
| **Delay & duration** | Immediate as an intent (R4); the plant resolves it in the same tick, after the signal step. |
| **Physical outcome** | A request, not a fact. It can be refused by a lockout, an interlock or dead power, and every refusal is an event with a reason. |
| **Device / sensor readout** | Request state on the panel; running state from the plant. |
| **Player-visible / audio** | `RelayClack` (remote), plus the machine's own cue if it starts. |
| **Guard-observable evidence** | The network log (existing `NetHack`/`PivotOpened` events) and the machine's physical evidence. |
| **Recovery & irreversible** | Reverse the command. Nothing irreversible unless the action itself was. |
| **Causal events & tests** | `Denied{NoAccess, LockedOut, Interlock, Unpowered}`. Tests: `remote_operation_requires_reach`, `remote_start_is_refused_by_a_local_lockout`. |

---

## 8. Cross-cutting

### X-1 · `cascade-budget`

`Template:` Observation (invariant) · `Rule version:` 1

| Field | Specification |
|---|---|
| **Preconditions & scope** | Any wave reaching `MAX_WAVES_PER_TICK`, `MAX_DELTAS_PER_TICK`, `MAX_RULES_PER_WAVE` or `MAX_SCHEDULED_PER_TICK`. Scope: the tick. |
| **Access & actor** | N/A. |
| **Consumed / transformed** | N/A. |
| **Delay & duration** | Deferred intents run at the next tick's S2. |
| **Physical outcome** | No outcome is lost; some outcomes arrive a tick later. |
| **Device / sensor readout** | N/A. |
| **Player-visible / audio** | None (a one-tick delay on a busy frame is imperceptible). |
| **Guard-observable evidence** | None. |
| **Recovery & irreversible** | N/A. |
| **Causal events & tests** | One `CascadeDeferred{wave, count, roots}` per bound per tick. Tests: `every_produced_intent_is_accounted_for`, `deferral_changes_timing_not_outcome`. |

### X-2 · `causal-chain`

`Template:` Observation (invariant) · `Rule version:` 1

| Field | Specification |
|---|---|
| **Preconditions & scope** | Every effect that changes state. Scope: the run. |
| **Access & actor** | N/A. |
| **Consumed / transformed** | N/A. |
| **Delay & duration** | Allocated at scheduling time, monotonic, survives snapshot/restore. |
| **Physical outcome** | N/A — it is the record of the outcome. |
| **Device / sensor readout** | N/A. |
| **Player-visible / audio** | N/A. |
| **Guard-observable evidence** | The developer trace (`idaptik-infra-trace/1`), never the player's HUD. |
| **Recovery & irreversible** | Causes are never reused; a dropped cause is a bug (`MAX_LIVE_CAUSES` drops only closed causes). |
| **Causal events & tests** | Every event carries exactly one cause id. Tests: `every_effect_has_a_cause`, `cause_ids_are_stable_across_replay`. |

### X-3 · `evidence-emission`

`Template:` Observation · `Rule version:` 1

| Field | Specification |
|---|---|
| **Preconditions & scope** | Any rule with a non-empty `evidence` list whose thresholds hold. Scope: the evidence's `at` and its attenuation. |
| **Access & actor** | N/A. |
| **Consumed / transformed** | N/A. |
| **Delay & duration** | `latency_ticks`; decays over `decay_ticks`. |
| **Physical outcome** | N/A. |
| **Device / sensor readout** | N/A. |
| **Player-visible / audio** | The corresponding cue, which is usually but not always simultaneous. |
| **Guard-observable evidence** | The ledger observation; observed only from a site that can perceive the channel. |
| **Recovery & irreversible** | Evidence decays; permanent tags (`OpenIsolator`, `FreshOpening`) do not. |
| **Causal events & tests** | `EvidenceEmitted{tag, at, strength}`. Tests: `every_rule_emits_a_cue_and_an_evidence_item`, `evidence_is_not_emitted_beyond_its_attenuation`. |

### X-4 · `cue-emission`

`Template:` Observation · `Rule version:` 1

| Field | Specification |
|---|---|
| **Preconditions & scope** | Any rule with a non-empty `cues` list whose thresholds hold. Scope: the cue's location and radius. |
| **Access & actor** | N/A. |
| **Consumed / transformed** | N/A. |
| **Delay & duration** | `level` rises and falls with the underlying quantity; no cue fires below its threshold. |
| **Physical outcome** | N/A. |
| **Device / sensor readout** | N/A. |
| **Player-visible / audio** | The cue. This is the **only** channel through which a player learns about plant state that is not on a panel. |
| **Guard-observable evidence** | None directly; the cue is the player's channel, not the guard's. |
| **Recovery & irreversible** | N/A. |
| **Causal events & tests** | Cue records are ambient and coalesced. Tests: `every_rule_emits_a_cue_and_an_evidence_item`, `cue_below_threshold_is_silent`. |

---

## 9. Coverage of the brief

| Requested interaction | Rules | Level |
|---|---|---|
| Water reaching damaged powered equipment | WE-1, WE-2 | 1 |
| Protective isolation versus continuing operation | WE-4, WE-5, WE-6, CP-4 | 1 |
| Pumps losing power while water continues entering | EG-1, WA-6, WE-6 | 1 |
| Restored power while equipment remains wet | WE-7, WE-2, WE-5 | 1 |
| Power controlling fans, valves and detectors | EG-1, EG-5, EG-6, CP-1 | 2 |
| Loss of extraction changing gas distribution | EG-2, WA-4, WA-5 | 2 |
| Ignition only for an authored gas category and conditions | EG-3, EG-4 | 2 |
| Flooding obstructing an authored duct or access route | WA-1, WA-2 | 1, 2 |
| Airflow changing distribution between compartments | WA-4, WA-5, WA-6 | 2 |
| Separate rules for fluid transport and actor traversal | WA-1 vs WA-2 | 1, 2 |
| Water does not neutralise every gas | WA-3 | 2 |
| Excavation exposing or damaging a specific cable/pipe | TU-1, TU-3 | 2 |
| A new opening connecting previously separate compartments | TU-2, WA-6 | 2 |
| Water entering an authored tunnel | TU-4 | 2 |
| Noise/vibration revealing activity | TU-5, X-3 | 1, 2 |
| A safe alternative route via careful utility control | TU-6, WE-4, TU-2 | 2 |
| A sensor triggers a pump, valve, fan or alarm | CP-1, CP-2, EG-5 | 2 |
| A spoofed readout misleads a controller while truth persists | CP-3, EG-6 | 2 |
| Manual isolation overrides remote operation | CP-4, WE-4 | 1 |
| Feedback loops bounded and reproducible | CP-6, X-1 | 2 |
| Deterministic ordering for simultaneous actions | design doc §4.2, §5 | 1, 2 |
| Staged deltas against iteration-order bugs | design doc §5 | 1, 2 |
| Cascades bounded, nothing important dropped | X-1, design doc §6 | 1, 2 |
| No universal physics or chemistry | design doc §13 | — |
| No special outcome per pair of names | design doc §13, `renaming_every_entity_preserves_behaviour` | 1, 2 |

## 10. Authoring shape

One rule instance, to show what a level author (or UMS) actually writes. This is
the damaged-equipment branch of `WE-2`, in the wire form of design doc §2.4a —
note that it names **no entity**: the subjects come from the capability query
at load time. Integrity is integral permille, so `le 799` encodes strictly
below 800. The exposed-equipment branch uses `Tag(self, Exposed)` in place of
the integrity condition; both branches require the conductive-material
condition. The catalogue's full `WE-2` precondition remains the authority.

```json
{
  "id": "wet-arc",
  "version": 1,
  "title": "Damaged, energised, wet equipment arcs",
  "template": "hazard",
  "scope": { "kind": "entity" },
  "when": [
    { "kind": "energised", "circuit": "self" },
    { "kind": "wetness", "entity": "self", "op": "ge", "permille": 350 },
    { "kind": "integrity", "entity": "self", "op": "le", "permille": 799 },
    { "kind": "material_water_film_conductive", "entity": "self" }
  ],
  "access": "none",
  "actor": [],
  "consume": [],
  "produce": [],
  "delay_ticks": 15,
  "period_ticks": 30,
  "effects": [
    { "kind": "set_integrity", "entity": "self", "delta": -4 },
    { "kind": "raise_hazard", "hazard": "arc", "level": 1 },
    { "kind": "ignition", "class": "arc" }
  ],
  "readout": [
    { "quantity": "wetness" },
    { "quantity": "integrity" }
  ],
  "cues": [{ "cue": "arc-crackle", "level": 900 }],
  "evidence": [{ "tag": "burnt-insulation", "channel": "sight", "strength": 1000 }],
  "recovery": { "kind": "de_energise_and_dry" },
  "irreversible": ["equipment_destroyed"],
  "cause": "hazard.arc",
  "tests": [
    "damaged_wet_energised_equipment_arcs_on_cadence",
    "water_on_dry_equipment_does_not_arc"
  ]
}
```

`"self"` resolves to the subject entity, so the clause names no entity and no
circuit either — `energised` on `self` reads the entity's own `Powered`
circuit. The same rule runs unchanged on a pump, a fan, a door motor or a
server, in any level, under any name.

### 10.1 How a level cites rules

A level's `rules` array is a list of **ids from this catalogue**, in
`rule_rank` order, and it is complete: every rule whose preconditions can arise
in the level is cited, even if the walkthrough never triggers it. The level docs
repeat their citation list in the header so the prose and the JSON can be
diffed against each other. `X-1`–`X-4` are never cited; they are engine
machinery that is always active.

The two test levels cite:

| Level | Rules cited |
|---|---|
| 1 | `WE-1`–`WE-7`, `EG-1`, `WA-1`, `WA-2`, `WA-6`, `CP-4`, `CP-7`, `TU-5` |
| 2 | level 1's list, plus `EG-2`–`EG-6`, `WA-3`–`WA-5`, `TU-1`–`TU-4`, `TU-6`, `CP-1`–`CP-3`, `CP-5`, `CP-6` |

Level 2 exercises `WE-1`–`WE-7` too, because it inherits level 1's flooded
plant; a level does not stop needing a rule because its walkthrough is about
something else.
