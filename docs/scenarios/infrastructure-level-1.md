<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Infrastructure test level 1 — "Sump"

**Status: design of record. Not implemented, not authored, not loadable.**

A service-level (floor −1) proving ground for the electricity↔water rules. It
exists to answer one question in play: *can you get past a flooding plant room
without wrecking it — and what does wrecking it cost?*

- Floor: Exchange House, `service` (−1). Entry `svc/riser`.
- Rules cited: `WE-1`–`WE-7`, `EG-1`, `WA-1`, `WA-2`, `WA-6`, `CP-4`, `CP-7`,
  `TU-5` — the same list as the JSON's `rules` array in §10.1 of the
  [catalogue](../architecture/infrastructure-rule-catalogue.md). `X-1`–`X-4`
  are always active and are never cited.
- Objectives: take the maintenance manifest from `terminal-svc` in the lobby,
  leave by the stair or the lift.
- Quality, not just success: the debrief counts `equipment_destroyed`,
  `irreversible`, and permanent evidence tags.

---

## 1. The floor

```
                         floor −1 (service)
        ┌──────────────┐        D5         ┌──────────────┐
        │              │  ┌───────────┐    │              │
 stair  │    riser     ├──┤  door D5  ├────┤     lobby    ├── LIFT (M) → 0
  S1 ───┤  2 400 L air │  └───────────┘    │   5 000 L    │
        │  gas source  │                   │  terminal-svc│
        │  (level 2)   │                   │  socket-l2   │
        └──────┬───────┘                   └──────┬───────┘
          ladder L1                          crawl C1 (sill 180 mm,
               │                              clearance 180 mm, 2.2 s)
        ┌──────┴─────────────────────────────────┐│
        │               plant                     │
        │  1.6 m² bunded, 3 800 L air             │
        │  pump-p1 (plinth 150)   grate-d1        │
        │  valve-v1               iso-m           │
        │  pipe-spr (leak 800 ml/s)               │
        └─────────────────────────────────────────┘
```

`C1` is a **link**, not a room: the crawl is a 2.2 s traversal and a fluid path
with two independent numbers (`clearance_mm` for actors, `aperture_cm2` for
water and air).

### 1.1 Compartments

| Id | Room | Air (L) | Floor (cm²) | Elevation | Notes |
|---|---|---|---|---|---|
| `plant` | `svc/plant` | 3 800 | 16 000 | −1 | bunded; 1 mm of water = 1.6 L |
| `riser` | `svc/riser` | 2 400 | 8 000 | −1 | vertical shaft; gas source in level 2 |
| `lobby` | `svc/lobby` | 5 000 | 20 000 | −1 | objective room |
| `sewer` | — | — | — | −3 | exterior sink: `exterior: true`, `open_to_exterior` |
| `outside` | — | — | — | 0 | exterior sink for the vent stack (level 2) |

### 1.2 Links

| Id | Between | Aperture (cm²) | Sill (mm) | Clearance (mm) | Gate | Fluid block |
|---|---|---|---|---|---|---|
| `crawl-c1` | plant ↔ lobby | 2 000 (cap 4 000) | 180 | 180 | — | `AboveSill` |
| `grate-d1` | plant → sewer | 1 200 (cap 10 000) | 0 | 0 | entity `grate-d1` | `Never` (gate still blocks) |
| `door-d5` | riser ↔ lobby | 0 shut, 400 open (cap 2 000) | 30 | 2 000 | portal `door-d5` | `Never` (gate still blocks) |
| `ladder-l1` | riser ↔ plant | 0 (cap 0) | — | 900 | — | never (a ladder is not a duct) |

`door-d5` carries fluid *and* actors. That single authored fact is why closing
it can save the lobby from a flood — and why opening it can lose the lobby to
one.

---

## 2. Authored things

### 2.1 Entities

| Id | Kind (presentational) | Compartment | Material | Capabilities | Initial |
|---|---|---|---|---|---|
| `iso-m` | isolator | plant | `steel` | `LockOut { series: pump-p1 }`, `Sounds` | closed |
| `pump-p1` | pump | plant | `steel` | `Powered { M, 6 }`, `Moves { Water, intake: plant, discharge: sewer, path: [], 6 000 ml/s, check_valve: false, return_ml: 20 000 }`, `Ignites { Inductive }`, `Sounds { PumpHum, 700 }` | off, `integrity 620`, plinth 150 mm |
| `valve-v1` | valve | plant | `steel` | `Gates { pipe-spr }` | open |
| `pipe-spr` | pipe | plant | `copper` | `Carries { Water, pressurised, leak 800 ml/s }` | live |
| `grate-d1` | floor drain | plant | `steel` | `Stores { Water, 900 000 ml, accepts 10 000 ml/s, outlet: sewer }` | **blocked by debris** (its position gates `grate-d1`) |
| `socket-l2` | wall socket | lobby | `copper` | `Powered { M, 1 }`, `Ignites { Arc }` | live, `integrity 700`, plinth 25 mm |
| `rel-d5` | door release | riser | `steel` | `Breaks { Portal(door-d5), Wrench, 2 }` | unused |
| `terminal-svc` | terminal | lobby | `pvc` | — | — |

The pump's `integrity 620` is authored damage (a prior incident). It is the
reason the pump is not simply "safe when wet": **damaged** plus **wet** plus
**energised** is the arc condition (`WE-2`). A pristine pump would merely be wet.

Two authored choices in that table are load-bearing and easy to get wrong:

- **`iso-m` locks out `pump-p1`, not circuit `M`.** A circuit-targeted lockout
  would de-energise the door, the lift and the socket as well, and A4 requires
  exactly the opposite — protection *without* losing the rest of the floor. The
  isolator is a local disconnect on the pump's own branch. `WE-4` supports both
  scopes; this level uses the entity one, and A5 is what a *circuit* lockout
  looks like (the hacker's cut), which is why the two routes teach different
  things.
- **`socket-l2`'s material is `copper`, not `pvc`.** An entity's `material` is
  the thing that arcs, and `pvc` has `water_film_conductive: false` in the
  catalogue — a wet plastic housing cannot satisfy `WE-2`. What arcs is the
  fitting's contacts behind the damaged faceplate (`integrity 700`). Authoring
  the housing material there would have produced a socket that never arcs,
  silently deleting A6's hazard.

### 2.2 Portals, circuits, objectives

| Portal | Kind | Between | Travel | Circuit | Notes |
|---|---|---|---|---|---|
| `stair-s1` | Stair | `svc/riser` ↔ floor 0 landing | 3.0 s | — | always open |
| `ladder-l1` | Ladder | riser ↔ plant | 1.6 s | — | always open |
| `door-d5` | Door | riser ↔ lobby | 1.5 s | `M` | powered; `lock: d5-lock`; manual release `rel-d5` |
| `crawl-c1` | Vent | plant ↔ lobby | 2.2 s | — | flooded above 180 mm |
| `lift-l1` | Lift | lobby ↔ floor 0 | 2.4 s | `M` | **strands you if M dies** |

| Circuit | Source node | Serves |
|---|---|---|
| `M` | `feed-m` (`PowerStation`, zone `svc`) | pump-p1, socket-l2, door-d5, lift-l1, rel-d5's controller |

Objectives: `Reach(svc/lobby)` → `Interact(terminal-svc)` → `Extract(stair-s1 or
lift-l1)`.

### 2.3 The signal network (level 1, minimal)

| Device | Kind | Bound entity | Node | Access |
|---|---|---|---|---|
| `sig-pump` | `PlantActuator` | `pump-p1` | `pump-p1` | hacker-only |
| `sig-iso` | `PlantSensor` | `iso-m` | `iso-m` | both |

No wiring is shipped. The hacker can lay one wire to read the isolator (useful:
"is it still thrown?") and one to start the pump. That is enough for level 1 to
teach the bridge without a wiring puzzle.

---

## 3. The clock

Depth is `water_ml / 1 600` mm. The leak is 800 ml/s from t = 0 (`valve-v1` is
open and `iso-m` is closed at load). The grate, once cleared, drains
`min(10 000, 1 200 × depth × 0.01)` ml/s — 12 ml/s per mm of depth, capped — and
the crawl weirs `min(4 000, 2 000 × (depth − 180) × 0.01)` ml/s once the plant is
above 180 mm.

| t (s) | Event | Depth | Consequence |
|---|---|---|---|
| 0 | leak running | 110 mm (176 L) | nothing is hazardous yet |
| 80 | water reaches the pump plinth | 150 mm (240 L) | `WE-1` begins wetting the pump |
| 84 | pump at full wetness | 150 mm | a start from here is a `WE-7` arc risk |
| 140 | water reaches the crawl sill | 180 mm (288 L) | `WA-2`: `crawl-c1` denied |
| 140+ | crawl becomes a weir: flow rises with head above 180 mm | 180 → 220 mm | the plant settles near 220 mm, spilling up to ≈ 0.8 L/s |
| ≈ 270 | lobby socket submerged | 25 mm in lobby | `WE-2` → shock field in the lobby |
| any time (grate open) | equilibrium where inflow = outflow | 67 mm | depth **falls** to 67 mm and stays; the clock stops |

The first 80 seconds are the free window. Everything after that is a choice.

---

## 4. Approaches

Six routes. Two destroy nothing and use no electricity. Each is playable from
the entry with no seed dependence — the level has no RNG.

### A1 · Close the valve (non-destructive, no power)

1. `stair-s1` → riser → `ladder-l1` → plant (wade; depth 110 mm is below the
   socket's plinth and below the pump's, so nothing is live in the water).
2. `valve-v1`: `Local` + `Wrench`, 90 ticks (1.5 s). Leak → 0.
3. Depth holds at ~110 mm. `crawl-c1` stays passable (110 < 180).
4. Crawl to the lobby, take the manifest, leave by `lift-l1` (M is live) or back
   through the crawl and the stair.

Cost: one wade, one tool action. Evidence: `WetFloor` in the plant, wet
footprints, and a closed valve that a maintenance check would find later.
Destroyed: nothing. Irreversible: nothing. **This is the intended clean run.**

### A2 · Clear the grate (non-destructive, no power, loud)

1. Wade to `grate-d1` (plinth 0). Clear debris: `Local` + `Hands`, 120 ticks.
2. Gate opens. Outflow at 110 mm is `1 200 × 110 × 0.01 = 1.32 L/s` against
   0.8 L/s inflow, so the depth falls towards the equilibrium 67 mm — but it is
   an exponential approach with `τ = 1 600 ml/mm ÷ 12 ml/s per mm ≈ 133 s`, not a
   step: 89 mm after 90 s, 72 mm after 240 s, inside 67 ± 2 mm only by ≈ 400 s.
   The crawl (sill 180) is passable the entire time.
3. `crawl-c1` stays passable throughout. Exit as A1.

Cost: 2 s of loud scraping (`Scraping`, `WaterSlosh`), plus the wade. The
convergence is slow, so this is the route you take when you are not in a hurry.
Evidence:
`PumpNoise`-class sound while working, `DebrisMoved`, `WetFloor`. Destroyed:
nothing. The drain's 900 L capacity is never approached.

The trade against A1 is legibility: the valve is one small action and silent;
the grate is two seconds of noise in a room whose only exit is a ladder.

### A3 · Pump it (uses the feed; fast; loud; dangerous if late)

1. Hacker energises `M` (or the infiltrator starts the pump locally at
   `sig-pump`; local `Hands`, 30 ticks).
2. Start `pump-p1`: `EG-1` contactor pull-in 6 ticks, then 6 L/s to the sewer.
   176 L clears in ~29 s; the crawl is passable for the whole run.
3. Walk the crawl while the pump works; it is a race against the guard
   responding to the noise.

- **Before t = 80 s** the pump is dry. This is a clean, fast, *loud* win.
- **After t = 84 s** the pump is wet. `WE-7` starts it, `WE-2` arcs it 15 ticks
  later, and the wading infiltrator is in the `WE-3` shock field. From
  `integrity 620`, 155 arcs (77.5 s) destroy the pump.
- **If the player then cuts M** (a common panic move): the pump stops, the
  un-check-valved discharge returns 20 L to the floor, and M being dead also
  kills `door-d5` and the lift (see A5).

Evidence: `PumpNoise` for the whole run (audible in the lobby, attenuated by
`crawl-c1`), then `BurntInsulation` if it arcs. Destroyed: possibly the pump.

### A4 · Isolate, then use the door (protective, uses the feed)

1. `iso-m`: `Local` + `Hands`, 30 ticks. Pump is now R0-locked out; a remote
   start will be refused with `Denied{LockedOut}`.
2. Hacker energises `M` and opens `door-d5` (controller, circuit M).
3. Riser → D5 → lobby → manifest → lift.

This is the "protective isolation versus continuing operation" comparison. The
pump is never at risk, M is live, the lift works. The cost is that water keeps
rising: `crawl-c1` closes at 140 s, so the return route through the crawl is
gone. You must leave by D5/lift, or lift the isolator and pump later.

Evidence: `OpenIsolator`, `LockoutTag`, and a flooded plant. Destroyed: nothing.

### A5 · Cut the feed (destructive to *control*)

1. Hacker cuts `feed-m` over the network (`CutPower` on the feed node).
2. `M` dies. Pump dead, D5 dead, lift dead, `socket-l2` dead. The arc hazard is
   *gone* — this is the seductive part.
3. The only way into the lobby is `crawl-c1` (closes at 140 s) or the manual
   release: `rel-d5`, `Local` + `Wrench`, 120 ticks, **loud**.
4. `Irreversible{ DoorMechanismBroken }`. The guard finds `ForcedDoor`.

A cut feed cannot be restored in this level: the network's power cut is
irreversible for the run, exactly as `netsim::apply_actuation` already treats a
cut substation. Cutting power is therefore a *valid* answer to the shock hazard
and a *bad* answer to everything else, and the level never says which it is
until it happens.

### A6 · Let it flood (hostile; makes the level worse)

Do nothing for 140 s:

1. `crawl-c1` closes — your own shortcut is gone.
2. The plant becomes a weir: it settles near 220 mm and spills up to ≈ 0.8 L/s
   into the lobby (the approach is exponential, τ ≈ 80 s).
3. At ≈ 270 s the lobby socket is submerged and arcing: a `WE-3` shock field in
   the room with the terminal and the lift, plus `GasAlarm`-class noise from the
   breaker trip and `BurntInsulation` evidence.

Useful only if you are already past the crawl and want the guard's route made
dangerous. It also makes your own exit dangerous. This is the designed
punishment for treating "water" as scenery.

---

## 5. What the player perceives, what the guard can know

| State | Player cue | Guard evidence |
|---|---|---|
| leak running | `WaterRunning` (plant) | `WetFloor` after 30 ticks |
| wading | `WaterSlosh` ≥ 25 mm | `WetFootprints` at the door/crawl |
| valve closed | the sound stops; a small clack | closed valve found on inspection |
| grate cleared | `Scraping` then `WaterRunning` louder | `DebrisMoved`, `WetFloor` |
| pump running | `PumpHum`, audible in the lobby through the crawl | `PumpNoise` → investigate |
| pump wet and running | `PumpCavitation`, then `ArcCrackle` | `BurntInsulation` |
| isolator thrown | `IsolatorTag`, clack | `OpenIsolator` (permanent) |
| feed cut | lights in the service level go out; `RelayClack` | `AlarmLog` (network), dark floor |
| door forced | grating, then the door gives | `ForcedDoor` (permanent) |
| flood over sill | `WaterRise` under the crawl, then silence | `WaterUnderDoor` |

No guard evidence is emitted from a place the guard cannot perceive. A pump
heard through `crawl-c1` reaches the lobby at
`700 × (1000 − 550)/1000 = 315` level; the guard's threshold is 250, so he
investigates *the plant*, not the lobby. In the riser it arrives at
`700 × 700/1000 = 490` across `ladder-l1` — a ladder is not a duct, but it is a
fine corridor for sound — and 490 is well over his 250, so he comes down the
stair.

---

## 6. Causal chains (developer view)

### 6.1 α — pumping wet

```
t=5400  [env ] WE-1  plant depth 150 mm → pump-p1 wet 0→1000 ‰        cause 12
t=5700  [R4  ] EG-1  sig-pump Enable → request on                     cause 13
t=5706  [phy ] EG-1  contactor pulled in (6 ticks)                    cause 14  parent 13
t=5721  [phy ] WE-7  wet start → arc conditions hold                  cause 15  parent 14
t=5721  [phy ] WE-2  arc #1: integrity 620→616, hazard Arc(1)         cause 16  parent 15
t=5721  [phy ] WE-3  shock field in plant (depth 151 ≥ 12)            cause 17  parent 16
t=5721  [act ] WE-3  infiltrator in contact: dose +2                  cause 18  parent 17
t=5730  [act ] WE-3  dose 18 → flinch cue, no injury                  cause 18
…
t=10341 [phy ] WE-2  arc #155: integrity 0 → EquipmentDestroyed       cause 401 parent 16
```

The player perceives a start, a wrong-sounding hum, then crackling; the guard
hears the hum, then smells burnt insulation. The developer sees a single chain
from "the pump got wet" to "the pump is gone", with every intermediate step
named.

### 6.2 β — the cut-feed trap

```
t=1200  [net ] Actuation::CutPower(feed-m) → PowerCut               cause 30
t=1201  [phy ] circuit M energised: true → false                    cause 31 parent 30
t=1202  [phy ] door-d5 powered: false; lift-l1 powered: false       cause 32 parent 31
t=1202  [act ] Traverse(door-d5) → TraversalDenied{Unpowered}       cause 33
t=8400  [act ] Traverse(crawl-c1) → TraversalDenied{Flooded}        cause 34
t=8460  [act ] rel-d5 cut (120 ticks, Wrench)                       cause 35
t=8580  [act ] Irreversible{DoorMechanismBroken}                    cause 36 parent 35
t=8580  [obs ] Evidence ForcedDoor at svc/riser                     cause 37
```

Note the shape: the *first* consequence of the player's action (control lost) is
tempting, and the *last* (the only exit is a forced door) is the one that
matters. That is what the staging and causal record are for — the level can show
it after the fact in the debrief.

### 6.3 γ — the non-destructive run

```
t=600   [act ] Traverse(stair-s1) → Entered(riser)                  cause 50
t=780   [act ] Traverse(ladder-l1) → Entered(plant)                 cause 51
t=876   [act ] Local + Wrench at valve-v1 (90 ticks)                cause 52
t=966   [phy ] Gates(pipe-spr) open → closed                        cause 53 parent 52
t=966   [phy ] leak 800 → 0 ml/s                                    cause 54 parent 53
t=967   [obs ] Evidence WetFloor at svc/plant                       cause 55
t=1320  [act ] Traverse(crawl-c1) → Entered(lobby)                  cause 56
```

Three load-bearing events, one piece of evidence, zero irreversibles. The
debrief can say so.

---

## 7. Failure, recovery, irreversibles

| Failure | Recovery | Irreversible? |
|---|---|---|
| pump wet and arcing | throw `iso-m` (R0) then wait 20 s to dry | possible `EquipmentDestroyed` |
| pump destroyed | none; the crawl/valve routes still work | **yes** |
| infiltrator shocked | step out; dose decays; incap 180 ticks | no |
| electrocuted | run ends | — |
| crawl flooded | clear the grate or close the valve; the depth falls | no |
| feed cut | none; network power cuts are permanent in-run | **yes** (control) |
| door forced | none | **yes** (`DoorMechanismBroken`) |
| lobby socket arcing | isolate M; the socket stays burnt | `BurntInsulation` permanent |
| flooded lobby | drain via the grate once cleared | no |

The level is winnable after every one of these except electrocution and a
destroyed pump *combined with* a missed crawl. That is deliberate: the
irreversible costs are about *quality and recovery*, not about a single
mistake ending the run.

---

## 8. Regression tests for this level

Level-specific, on top of the rule tests in the catalogue:

| Test | Asserts |
|---|---|
| `l1_valve_route_is_clean` | A1 completes with `equipment_destroyed == 0` and no irreversible tags |
| `l1_grate_equilibrium_is_67mm` | A2 approaches 67 mm with τ ≈ 133 s, is within 2 mm by 400 s, and the crawl stays passable throughout |
| `l1_pump_route_before_80s_is_safe` | A3 at t < 80 s: no arc, no shock, crawl passable |
| `l1_pump_route_after_80s_arcs` | A3 at t > 84 s: arc within 21 ticks of the start |
| `l1_isolation_blocks_remote_start` | A4: remote start refused, pump dry, D5 opens |
| `l1_cut_feed_denies_door_and_lift` | A5: `TraversalDenied{Unpowered}` for both |
| `l1_crawl_closes_at_140s` | A6: passable at 139 s, `Flooded` at 141 s |
| `l1_lobby_socket_arcs_when_submerged` | A6: shock field appears at ≈ 265 s (the weir ramps, so the last 50 L takes longer than a constant 0.8 L/s) |
| `l1_overtop_is_a_weir_not_a_drain` | plant settles at 220 ± 5 mm, not 180 and not the ceiling |
| `l1_no_rng` | identical event log across two runs with the same script |
| `l1_backflow_returns_20l` | stopping the pump adds 20 000 ml to `plant` |

---

## 9. Authoring excerpt

Abridged; the full definition is the level's own payload.

```json
{
  "format": "idaptik-scenario/1",
  "scenario_id": "sump",
  "name": "Sump",
  "floor_id": 1,
  "infra": {
    "format": "idaptik-infrastructure/1",
    "ruleset_version": 1,
    "compartments": [
      { "id": "plant", "room": { "floor": "service", "room": "plant" },
        "air_l": 3800, "floor_cm2": 16000, "elevation": -1 },
      { "id": "riser", "room": { "floor": "service", "room": "riser" },
        "air_l": 2400, "floor_cm2": 8000, "elevation": -1 },
      { "id": "lobby", "room": { "floor": "service", "room": "lobby" },
        "air_l": 5000, "floor_cm2": 20000, "elevation": -1 },
      { "id": "sewer", "room": null, "air_l": 0, "floor_cm2": 0,
        "elevation": -3, "exterior": true },
      { "id": "outside", "room": null, "air_l": 0, "floor_cm2": 0,
        "elevation": 0, "exterior": true }
    ],
    "links": [
      { "id": "crawl-c1", "a": "plant", "b": "lobby", "reach": "duct",
        "aperture_cm2": 2000, "capacity_ml_per_s": 4000,
        "sill_mm": 180, "clearance_mm": 180,
        "actor": "passable", "fluid_block": "above_sill", "noise_permille": 550 },
      { "id": "grate-d1", "a": "plant", "b": "sewer", "reach": "grate",
        "aperture_cm2": 1200, "capacity_ml_per_s": 10000,
        "sill_mm": 0, "clearance_mm": 0,
        "actor": "blocked", "gate": { "entity": "grate-d1" },
        "fluid_block": "never" },
      { "id": "door-d5", "a": "riser", "b": "lobby", "reach": "opening",
        "aperture_cm2": 400, "capacity_ml_per_s": 2000,
        "sill_mm": 30, "clearance_mm": 2000,
        "actor": "needs_gate", "gate": { "portal": "door-d5" },
        "fluid_block": "never" },
      { "id": "ladder-l1", "a": "riser", "b": "plant", "reach": "opening",
        "aperture_cm2": 0, "capacity_ml_per_s": 0,
        "sill_mm": 0, "clearance_mm": 900, "noise_permille": 700,
        "actor": "passable", "fluid_block": "never" }
    ],
    "entities": [
      { "id": "iso-m", "kind": "isolator", "compartment": "plant",
        "material": "steel",
        "caps": [
          { "lockout": { "series": { "entity": "pump-p1" } } },
          { "sounds": { "cue": "breaker-clunk", "level": 600, "radius_links": 1 } }
        ],
        "init": { "position": "closed" } },
      { "id": "pump-p1", "kind": "pump", "compartment": "plant",
        "material": "steel", "node": "pump-p1",
        "caps": [
          { "powered": { "circuit": "M", "load": 6 } },
          { "moves": { "medium": "water", "rate_ml_per_s": 6000,
                       "intake": "plant", "discharge": "sewer", "path": [],
                       "check_valve": false, "return_ml": 20000 } },
          { "ignites": { "class": "inductive" } },
          { "sounds": { "cue": "pump-hum", "level": 700, "radius_links": 2 } }
        ],
        "init": { "position": "off", "integrity_permille": 620,
                  "wetness_permille": 0, "plinth_mm": 150 } },
      { "id": "valve-v1", "kind": "valve", "compartment": "plant",
        "material": "steel",
        "caps": [{ "gates": { "flow": { "entity": "pipe-spr" },
                              "initial": "open" } }],
        "init": { "position": "open" } },
      { "id": "pipe-spr", "kind": "pipe", "compartment": "plant",
        "material": "copper",
        "caps": [{ "carries": { "medium": "water", "pressurised": true,
                                "leak_ml_per_s": 800 } }],
        "init": { "integrity_permille": 400 } },
      { "id": "grate-d1", "kind": "drain", "compartment": "plant",
        "material": "steel",
        "caps": [{ "stores": { "medium": "water", "capacity_ml": 900000,
                               "accepts_ml_per_s": 10000,
                               "outlet": "sewer" } }],
        "init": { "position": "blocked" } },
      { "id": "socket-l2", "kind": "socket", "compartment": "lobby",
        "material": "copper",
        "caps": [
          { "powered": { "circuit": "M", "load": 1 } },
          { "ignites": { "class": "arc" } }
        ],
        "init": { "position": "on", "integrity_permille": 700,
                  "plinth_mm": 25 } },
      { "id": "rel-d5", "kind": "release", "compartment": "riser",
        "material": "steel",
        "caps": [{ "breaks": { "target": { "portal": "door-d5" },
                               "tool": "wrench", "hardness": 2 } }],
        "init": { "position": "unused" } },
      { "id": "terminal-svc", "kind": "terminal", "compartment": "lobby",
        "material": "pvc", "node": "terminal-svc",
        "caps": [], "init": {} }
    ],
    "rules": [
      "water-wets-surfaces", "wet-arc", "shock-field", "protective-isolation",
      "drying", "wet-run-damage", "re-energise-wet", "powered-actuator",
      "flood-obstructs-link", "flood-blocks-traversal", "pressure-equalisation",
      "manual-override", "remote-operation", "noise-and-vibration"
    ]
  }
}
```

This is an excerpt: the compartment, link and entity lists are abridged where
the tables above are the authority, the floors/rooms/world block that every
scenario carries is omitted, and the signal block of §2.3 is omitted.

`circuits` is a **building** field, not a scenario one: circuit `M` is declared
by Exchange House and referenced here by id (`"circuit": "M"`). The plant never
redeclares a circuit, a portal or a zone.

The `rules` array is the level's ruleset selection, in `rule_rank` order and
complete (catalogue §10.1). Validation refuses a selection naming a rule the
build does not carry, refuses a rule with no test id (`rules.tests_present`),
and refuses a rule whose preconditions can arise in the level but which the
level did not cite.
