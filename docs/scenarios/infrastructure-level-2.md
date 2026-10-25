<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Infrastructure test level 2 — "Sable"

> **Status: not buildable today.** This level references 6 cues —
> `CutterGrating`, `FireWhoomph`, `GasAlarm`, `GasReek`, `VentHum`,
> `VentStops` — of which **none** exist in `CueKind`, and it depends on the
> `sable-2` gas species and the `GasWaterRule` category the shipped `GasClass`
> (`Haze`/`Hush`/`Ember`) does not carry. It is kept as the specification of
> what Level 2 requires: causal chains legible to the developer and perceptible
> to the player.


**Status: design of record. Not implemented, not authored, not loadable.**

Level 1 with ventilation and one fictional gas category. It exists to answer a
second question: *can you tell what a single breaker does before you throw it?*

- Floor: the same Exchange House `service` level, with ventilation added.
- Rules cited: level 1's list, plus `EG-2`–`EG-6`, `WA-3`–`WA-5`, `TU-1`–`TU-4`,
  `TU-6`, `CP-1`–`CP-3`, `CP-5`, `CP-6` — level 1's list including `WE-1`–`WE-7`,
  because this level inherits its flooded plant. `X-1`–`X-4` are always active
  and are never cited.
- Gas category: **`sable-2`**. It is authored data, not an engine feature.

---

## 1. What is added

```
                             outside
                                ▲
                                │ vent-stack   shutter dmp-x1, fan-f1   [N]
                                │
        ┌──────────────┐        │        D5         ┌──────────────┐
 stair  │    riser     │        │      ┌─────────┐  │    lobby     │
  S1 ───┤  cyl-s2 src  ├────────┴──────┤ door D5 ├──┤  terminal    │
        │  gd-1, alarm │  pen-1 +fan-f2└─────────┘  │  socket-l2   │
        │  fan-f1      │   [N]          duct-high   │  5 000 L air │
        └──────┬───────┘                [shut]      └──────┬───────┘
          ladder │                            duct-low [N] │
        ┌───────┴─────────────────────────────────────────┴──────┐
        │  plant: pump, valve, grate, iso-m      crawl-c1         │
        └─────────────────────────────────────────────────────────┘
```

| Link | Between | Aperture (cm²) | Sill (mm) | Clearance (mm) | Gate | Fluid block |
|---|---|---|---|---|---|---|
| `vent-stack` | riser → outside | 800 (cap 5 000) | 1 500 | 0 | `dmp-x1` (motorised) | `Never` (gate still blocks) |
| `pen-1` | riser ↔ plant | 400 (cap 2 000) | 220 | 0 | `dmp-p1` (manual), inline `fan-f2` | `AboveSill` |
| `duct-low` | plant ↔ lobby | 600 (cap **50**) | 60 | 0 | `dmp-l1` (motorised) | `AboveSill` |
| `duct-high` | riser ↔ lobby | 250 (cap 1 200) | 1 200 | 0 | `dmp-h1` (manual) | `Never` (gate still blocks) |
| `wall-pl` | plant ↔ lobby | 0 → 900 after breach (cap 4 000) | 900 | 0 → 900 | — | never (until breached) |
| `crawl-c1`, `grate-d1`, `door-d5`, `ladder-l1` | as level 1 | | | | | |

The Gate and Fluid block columns apply independently: both `pen-1` and
`duct-low` stop gas when submerged **or** when their damper is shut. Their
JSON encodes a `gate` reference alongside `fluid_block: above_sill`.

Four notes on the model, all from the design document:

- **sills are a water quantity.** Gas passes any open link, whatever its height;
  a 4 L/s extractor clears gas from a room whose vent is at ceiling height. What
  water can do is *cover an opening* and close it to air (`WA-1`) — to air, not
  to water, which is why the next note matters.
- **the plant starts at 110 mm**, above `duct-low`'s 60 mm sill, so the duct
  mouth is under water and `WA-1` closes it **to gas** before the level begins.
  The lobby is isolated from the riser by the flood, not only by a door.
- that does not make `duct-low` a drain. Its rating is 50 ml/s, so with 50 mm
  of head the weir formula yields `min(50, 600 × 50 × 0.01) = 50` ml/s: 10 L
  over the level's 200 s, which is **5 mm** in the lobby — below
  `socket-l2`'s 25 mm plinth, and not enough to lower the plant to the 60 mm
  sill. A submerged duct is nearly sealed to water and completely sealed to
  air, and both halves of that sentence matter.
- **this level does not run level 1's leak.** `valve-v1` is closed at load
  (the previous shift's fix), so the branch is dead-legged and nothing is being
  added to the 176 L standing in the plant. `iso-m` is **closed**, so the pump
  still has supply and anybody may start it — B3 does exactly that, and that is
  the trap. `M` stays live for the lift, `door-d5` and the socket throughout.
  The standing water is what is left of the earlier incident, and `WE-5` never
  finished drying it. B4 reopens the valve, deliberately.

### 1.1 Entities added

| Id | Compartment | Capabilities | Initial | Why it exists |
|---|---|---|---|---|
| `fan-f1` | riser | `Powered { N, 4 }`, `Moves { Gas, intake: riser, discharge: outside, path: [vent-stack], 4 000 ml/s }`, `Ignites { Inductive }`, `Sounds { VentHum, 500 }` | off | extraction to outside |
| `dmp-x1` | riser | `Powered { N, 1 }`, `Gates { vent-stack }`, opened by `fan-f1`'s run state (`ctl-stack`) | shut | stack shutter |
| `fan-f2` | riser | `Powered { N, 2 }`, `Moves { Gas, intake: riser, discharge: plant, path: [pen-1], 1 500 ml/s, reversible }`, `Sounds { VentHum, 400 }` | off | transfer between riser and plant — the only fast way to move gas between compartments |
| `dmp-p1` | riser | `Gates { pen-1 }` | open | manual damper; closing it traps whatever is in the plant |
| `dmp-l1` | plant | `Powered { N, 1 }`, `Gates { duct-low }` | open | motorised damper |
| `dmp-h1` | riser | `Gates { duct-high }` | shut | manual damper |
| `cyl-s2` | riser | `Carries { Gas sable-2, pressurised, leak 400 ml/s }`, `Gates { regulator }` | open | the source |
| `gd-1` | riser | `Powered { N, 1 }`, `Senses { GasConcentration(sable-2), 20 ‰, hysteresis 5, dwell_ticks 120 }`, `Alarms { GasAlarm }` | live | detector |
| `alarm-a1` | riser | `Powered { N, 1 }`, `Alarms { GasAlarm, GasAlarmPanel }` | live | audible/visual alarm |
| `cut-m` | lobby | `Breaks { Link(wall-pl), Cutter, 4 }` | unused | the breach (`TU-2`); it targets the wall, not the duct |

`door-d5`'s operator is in the riser and is authored `Ignites { Inductive }`.
That one property is what makes the escape door a potential ignition source.
`socket-l2` is `Ignites { Arc }`. The lobby light switch is `Ignites { None }` and
its housing is `pvc`.

### 1.2 Circuits

Level 1's `M` is unchanged. The ventilation is on its own circuit, which is the
whole point of the level: one breaker, six consequences.

| Circuit | Source node | Serves |
|---|---|---|
| `M` | `feed-m` (`PowerStation`, zone `svc`) | pump-p1, socket-l2, door-d5, lift-l1 |
| `N` | `feed-n` (`PowerStation`, zone `svc`) | fan-f1, fan-f2, dmp-x1, dmp-l1, gd-1, alarm-a1 |

`N` is *not* the same feed as `M`. Cutting `feed-n` over the network kills the
ventilation and nothing else; cutting `feed-m` kills the pump, the door, the
lift and the socket, and leaves the ventilation running. That split is what
makes the two levels teach different lessons with the same building.

### 1.3 The gas

| Property | `sable-2` | Consequence in this level |
|---|---|---|
| density | 1 600 ‰ (sinks) | pools where it leaks; the transfer fan can push it **down** into the plant |
| LFL / UFL | 40 ‰ / 130 ‰ | 96 L / 312 L in a 2 400 L riser |
| ignition classes | `Inductive`, `Arc`, `HotSurface` | a door motor lights it; a light switch cannot |
| oxygen min | 170 ‰ | a big fire starves itself |
| irritant | 8 ‰ | the player has a warning from t = 0 |
| incapacitate | 120 ‰ | 288 L; above UFL a person is not safe, just not explosive |
| water | `DisplacedOnly` | flooding moves it; it does not dissolve it |

## 2. The clock

Source 400 ml/s of *species*. Riser 2 400 L. Start 24 L = **10 ‰** — above
irritant (8 ‰, the player is warned immediately), below the detector (20 ‰) and
far below LFL.

| t (s) | Gas (L) | ‰ | Event |
|---|---|---|---|
| 0 | 24 | 10 | `GasReek`; nobody else knows |
| 60 | 48 | 20 | `gd-1` trips → alarm + `ctl-vent` starts `fan-f1` |
| 73 | 0 | 0 | 4 L/s extraction against a 0.4 L/s source is 3.6 L/s net, so 48 L takes 13.3 s of running, not 12; the alarm clears after `dwell_ticks` |
| 180 | 96 | 40 | **LFL** (if extraction never ran) |
| 195 | 102 | 42 | an `Inductive` start in the riser now ignites |

Doing nothing is *safe but not quiet*: the alarm at 60 s tells the guard
something is wrong in the riser. Interfering is what kills people.

## 3. What cutting `N` does

This table is the level's teaching device. One action, six consequences:

| Consequence | Rule | Perceptible |
|---|---|---|
| `fan-f1` stops | `EG-1` | `VentStops` |
| shutter `dmp-x1` freezes **shut** | `EG-1` | no draught at the grille |
| `fan-f2` stops | `EG-1` | the transfer route closes; no effect in B2 |
| detector `gd-1` reports nothing | `EG-6` | the panel is dark; **the gas is still rising** |
| alarm `alarm-a1` cannot sound | `EG-5` | silence |
| `dmp-l1` freezes **open** | `EG-1` | the plant's duct damper stays where it was — which only matters once the plant is pumped below 60 mm, and that is exactly when plant gas could reach the lobby |

A player who cuts `N` to silence an alarm has also disabled both fans, the
detector and the alarm. The level never says so; this table does, to the
reviewer.

## 4. Approaches

### B1 · Extract, then seal (non-destructive)

1. Keep `N` live. Start `fan-f1` from the panel (`sig-vent`; local `Hands` or
   hacker remote at R4) at t ≈ 10 s, *before* `gd-1` trips at 20 ‰.
2. The riser clears in ~8 s (28 L at a net 3.6 L/s); the detector never trips
   and no alarm sounds. The fan hum can still prompt a guard response.
3. Enter the riser and close the cylinder regulator: `Local` + `Wrench`,
   60 ticks. Source → 0.
4. Leave. Stop the fan or leave it; with the source sealed, it has nothing to do.

Cost: a fan hum (`level 500`, one link → 310 at the stair, exceeding the guard's
250 threshold; the guard hears it and may investigate). Destroyed: nothing.
Irreversible: nothing. **The clean run, with audible evidence.** It requires
knowing the fan can be started *before*
the detector demands it — which the reek is there to teach.

### B2 · Move the problem into the plant (non-destructive, costs you the plant)

1. Check `dmp-p1` is open and `duct-low` is shut (it is: the plant's 110 mm is
   over its 60 mm sill, so `WA-1` has already closed it to gas — the lobby is
   safe by construction, not by luck).
2. Start `fan-f2` **towards the plant at tick 600** (10 s). Keep `fan-f1` off;
   `dmp-x1`, `dmp-h1` and `door-d5` stay shut. By then the 0.4 L/s source has
   added 4 L to the initial 24 L. Passive flow through `pen-1` is
   `400 × 0.05 = 20 ml/s`, so about 27.8 L is in the riser and 0.2 L in the
   plant. The fan moves 1 500 ml/s of *species*, not mixture (design §3.1a),
   after its 6-tick pull-in.
3. While it runs, close `cyl-s2`'s regulator: `Local` + `Wrench` from tick 1140
   to 1200 (19–20 s). The source continues until tick 1200, adding another
   4 L after the fan request. With no extraction, **32 L** must end in the
   plant. Passive flow may aid or oppose the fan by up to 0.02 L/s. The
   transfer therefore takes `(32 − 0.202) / (1.5 ± 0.02)`, about
   **20.92–21.49 s** at rate, or **21.02–21.59 s** after the request including
   pull-in: the riser clears by about tick **1896**. Allow tick rounding.
4. Keep the fan running until `dmp-p1` is closed at tick 1920, then stop it.
   Only after the source is isolated and the riser has cleared is the riser
   breathable. The lobby stays breathable; the plant holds
   `32 / 3 800 × 1000 ≈ 8.4 ‰` — above irritant, below LFL.

Cost: the plant is unusable — no valve, no grate, no pump, no crawl — and the
gas is still there. Destroyed: nothing. Irreversible: nothing, but the debrief
records a gas-filled compartment. At 8.4 ‰ it is an irritant room; an arc there
does not satisfy sable-2's 40 ‰ LFL. This is the honest
"not my problem" solution and the level's second-best outcome.

The transfer fan is also the answer to a question the level wants asked: **the
gas is moved, never destroyed.** `fan-f2` does not filter, scrub or burn
anything; it relocates the species at a rate. If `dmp-p1` is left open after
the fan stops, `WA-6` slowly puts it back through the same 400 cm² of duct.

### B3 · Kill the alarm and pump it (the trap)

1. Cut `N` (or spoof `gd-1`; §6) to keep the guard quiet.
2. Start `pump-p1` to clear the flood (level 1's A3).
3. Gas reaches LFL at ~180 s. The pump is **in the plant**, where concentration
   is ~2 ‰, so nothing happens. The player concludes the pump is safe.
4. Open `door-d5` to leave. The operator is in the **riser** at ≥ LFL and is
   `Inductive`: 6-tick pull-in, then `EG-3` after the 6-tick dwell.
5. Ignition. Fire in the riser; the stair is in the riser and is now `OnFire`;
   `StructuralChar` after 30 ticks; spread to the lobby through the open door
   (400 cm² ≥ 200, carpet combustible).

This chain is the level's thesis: **spatial scope matters**. The pump was safe,
the door was not, and both are "electrical".

### B4 · Water moves the gas; it does not remove it (demonstration, caution)

Run B2, then at tick 6000 (100 s) reopen `dmp-p1`, **restore the supply**
(`valve-v1` open) and let the leak come. Keep `cyl-s2` sealed and both fans off.
The 50 ml/s seep through `duct-low` has left about 171 L (107 mm) in the plant.
Net inflow is now `800 − 50 = 750 ml/s`, so reaching the 288 L / 180 mm crawl
sill takes about **156 s**. The added 117 L displaces mixture carrying at most
`117 × (32 / 3 800) ≈ 0.99 L` of sable-2 via `WA-3`. Passive return through `pen-1`
(`WA-6`) is separate, at most another 3.12 L over that interval.

Together that is at most about 1.7 ‰ in the riser: measurable on the panel,
below the 8 ‰ reek threshold. The **perceptible** part is the haze layer
climbing the plant wall; this route does not promise a returning `GasReek`.
Leave the supply open past the weir and level 1's clock resumes: `crawl-c1`
spills into the lobby and the socket's 25 mm fuse restarts. The demonstration is
bounded by the same hydraulics, not by a scripted stop.

The point is the arrow: **the plant was never a
disposal site.** Conservation is asserted by the test, not drama by the fiction —
a 3 % flood can only move 3 % of the gas, and pretending otherwise would be a
special effect, not a rule.

### B5 · Breach the wall (dry wall breach)

1. `iso-m` thrown (`WE-4`), `valve-v1` closed. `M` stays live for the lift, the
   pump is protected and the supply leak is stopped.
2. Cut `cut-m` (lobby side of the plant wall): `Local` + `Cutter`, hardness 4
   with a rating-6 cutter → 20 ticks. No utility is authored behind `wall-pl`,
   so the cut produces no arc even if `iso-m` was not thrown. `iso-m` still
   protects only the pump branch; leaving circuit `M` live does not put a
   cable in this wall. This route exercises `TU-2`; `TU-6` needs an authored
   utility to demonstrate isolation preventing a cutting arc.
3. `TU-2` opens `wall-pl`: the hole's sill is authored at 900 mm, so it is above
   the 110 mm flood and below the ceiling. Both `clearance_mm` and `aperture_cm2`
   rise, which is the point — a breach is an actor path *and* an air path, and
   the second is how gas would follow you through it. Walk lobby ↔ plant without `door-d5` and without power.

Cost: `StructuralBreach` (permanent) and sustained `CutterGrating`
(`level 800`, two links → 350 at the stair: the guard hears it). Destroyed: the
wall. Irreversible: yes. The high sill keeps the opening above the flood;
the cut's lack of an arc follows from the absence of an authored utility.

---

## 5. Causal chains (developer view)

### 5.1 α — the escape door ignites the room

```
t=1200  [net ] Actuation::CutPower(feed-n) → PowerCut                 cause 60
t=1201  [phy ] circuit N: true → false                                cause 61 parent 60
t=1201  [phy ] fan-f1 running → stopped; cue VentStops                cause 62 parent 61
t=1201  [phy ] dmp-x1 position open → shut (motor frozen)             cause 63 parent 61
t=1201  [phy ] gd-1 reported: 15 ‰ → dead (EG-6)                      cause 64 parent 61
t=1201  [ctl ] ctl-vent sees no trip; fan stays off                   cause 65 parent 64
t=10800 [phy ] riser sable-2 96 L → 40 ‰ = LFL                        cause 66  (env)
t=12000 [R4  ] door-d5 open request → motor pulls in (6 ticks)        cause 67
t=12006 [phy ] door operator ignition event (Inductive)               cause 68 parent 67
t=12006 [phy ] EG-3 ignition: dwell satisfied, oxygen 210 ≥ 170       cause 69 parent 68
t=12006 [phy ] Irreversible{GasIgnited}; fire intensity 400 ‰          cause 70 parent 69
t=12036 [phy ] fire spreads to lobby via door-d5 (400 cm², carpet)    cause 71 parent 70
t=12036 [obs ] Evidence GasAlarmPanel (dark), AlarmLog gap            cause 72
t=12200 [act ] Traverse(stair-s1) → TraversalDenied{OnFire}           cause 73
```

### 5.2 β — B2, and the plant giving the gas back

```
t=600   [act ] fan-f2 start, direction riser → plant                   cause 80
t=606   [phy ] fan-f2 at rate; source still adds 400 ml/s               cause 81 parent 80
t=1140  [act ] Local + Wrench at cyl-s2 (60 ticks)                     cause 82
t=1200  [phy ] regulator shut; source 400 → 0 ml/s                     cause 83 parent 82
t=1896  [phy ] transfer cleared by now: riser ≈0, plant 32 L ≈8.4 ‰    cause 84 parent 80
t=1920  [act ] dmp-p1 closed, then fan-f2 stopped                      cause 85
t=6000  [act ] B4: dmp-p1 reopened, valve-v1 opened, leak 800 ml/s      cause 86
t=15360 [phy ] plant ≈180 mm; WA-3 has displaced ≤0.99 L to riser      cause 87 parent 86
t=15360 [obs ] haze layer rises; riser below GasReek threshold         cause 88 parent 87
```

The source adds 8 L before it is sealed at tick 1200. Thereafter total species
across compartments is invariant: `Σ gas_ml = 32 000` (tick rounding aside).
The B4 displacement bound excludes passive transfer, which is counted
separately; neither process destroys gas.

### 5.3 γ — B1 in full

```
t=600   [act ] start fan-f1 (local panel)                             cause 90
t=606   [phy ] dmp-x1 opens, fan runs, 4 L/s                          cause 91 parent 90
t=960   [act ] regulator closed (60 ticks, Wrench)                    cause 93
t=1020  [phy ] source 400 → 0 ml/s                                    cause 94 parent 93
t=1080  [phy ] riser cleared; concentration ≈0 ‰                     cause 95
t=1140  [phy ] detector never tripped; alarm never sounded            cause 96
```

No alarm, no evidence except the fan hum, no irreversible tags.

---

## 6. Spoofing: the deception is real, the physics is not

The hacker forges `gd-1`'s reported value to 0 ‰ at **t = 30 s** — before the
detector's 20 ‰ trip at t = 60 s, and that timing is the whole point. Spoof after
the trip and the controller has already started `fan-f1` (§7); spoof before it
and the automation never begins, because `ctl-vent` reads the forged value and
sees a clean room while the gas keeps arriving at 400 ml/s.

| t (s) | Truth in the riser | Reported | What happens |
|---|---|---|---|
| 0 | 10 ‰ | 10 ‰ | `GasReek`; the panel agrees with the player's nose |
| 30 | 15 ‰ (36 L) | **0 ‰** | spoof applied; `ctl-vent` sees a clean room |
| 60 | 20 ‰ (48 L) | 0 ‰ | the detector *would* trip; `alarm-a1` stays silent |
| 120 | 30 ‰ (72 L) | 0 ‰ | the hacker's panel says empty; the infiltrator is choking |
| 180 | 40 ‰ (96 L) = **LFL** | 0 ‰ | `EG-3` is one door-motor arc away |
| 195 | 42 ‰ | 0 ‰ | `Inductive` start in the riser now ignites |

| Reader | Reads | Value at t = 120 s | Consequence |
|---|---|---|---|
| `ctl-vent` controller | reported | 0 ‰ | never starts the fan |
| hacker's plant panel | reported | 0 ‰ | "room is clean" |
| `alarm-a1` | reported | 0 ‰ | silent |
| infiltrator's HUD + nose | truth | 30 ‰ | `GasReek`, HUD tint, haze at floor level |
| `EG-3` ignition rule | truth | 30 ‰ and rising | fires when a source arcs in band |

The two seats now hold **different, both-honest** pictures, and the only way to
resolve it is for one to tell the other. When the door motor arcs at 40 ‰, the
ignition rule has never heard of the spoof.

`CP-5`'s interlock (an authored "no inductive start above 20 ‰" law) reads the
same reported value and is defeated by the same spoof. That is deliberate: an
interlock is only as good as its sensor.

Two constraints keep this reproducible rather than hand-waved:

- the spoof is a `spoofs: BTreeMap<SignalId, u32>` entry in `InfraInput`
  (design §10.2), so it is part of the command stream and replays exactly;
- `l2_spoof_misleads_controller_not_physics` asserts the t = 30 s starting
  state explicitly — reported 0, truth 15 ‰ — so a later change to the source
  rate cannot quietly move the scenario.

Spoofing *after* extraction has started is a different, weaker move: it hides a
clean room from everyone and changes nothing physical. The level teaches the
version that matters.

## 7. The feedback loop

`ctl-vent`: trip at 20 ‰, clear at 15 ‰ (`hysteresis_permille 5`),
`dwell_ticks 120`, `cooldown_ticks 30`, target `fan-f1`.

| Phase | Arithmetic | Ticks |
|---|---|---|
| fan on, 20 → 15 ‰ | 12 L at net 3.6 L/s = 3.33 s, but `dwell_ticks` 120 | 200 |
| off, cooldown | — | 30 |
| off, 15 → 20 ‰ | 12 L at 0.4 L/s = 30 s | 1 800 |
| **period** | 200 + 30 + 1 800 | **2 030** |

The builtin wire (`sig-gd1` → `sig-vent`) and `ctl-vent` deliberately share a
target, because that is how real plant is wired: the wire's `Enable` is
**edge-triggered**, so it starts the fan once when the detector trips, and the
controller's `Disable` can then stop it. The wire must fall and rise again to
restart it. That the two coexist without fighting is the existing edge/level
rule doing its job, and it is asserted by `l2_wire_and_controller_do_not_fight`.

The guard hears the fan cycle every ~34 s and, unless he is told otherwise,
learns to ignore it. The player can use the period: extraction is off for 30 s
at a time, which is long enough to cross the riser without being heard over it.
`a_feedback_loop_is_bounded_and_repeats_exactly` asserts the same period across
10 000 ticks and two independent runs.

## 8. What the player perceives, what the guard can know

| State | Player | Guard |
|---|---|---|
| gas at 8–20 ‰ | `GasReek`, HUD tint, haze at floor level | smell, **inside the riser only** |
| detector tripped | `GasAlarm`, panel red | `GasAlarmPanel`, `AlarmLog` |
| fan running | `VentHum`, draught at the grille | `GasAlarmPanel` → riser |
| N cut | `VentStops`, then *silence* | `AlarmLog` (network event) |
| spoof active | hacker sees clean; infiltrator does not | nothing |
| LFL reached | a low roar as the air itself sounds wrong | nothing, unless he is inside |
| ignition | `FireWhoomph`, flash, shake | smoke from the stair, `AlarmLog` |
| duct flooded shut | no draught; a gurgle | `WaterUnderDoor` at the duct |
| water displacing gas | the haze climbs the wall | nothing |
| wall cut | `CutterGrating` | sound at two links, `FreshOpening` |

## 9. Regression tests for this level

| Test | Asserts |
|---|---|
| `l2_clean_run_never_trips_the_alarm` | B1: no `GasAlarm`, no irreversible, source 0 |
| `l2_cutting_n_stops_both_fans_and_detector` | the six consequences of §3 |
| `l2_lfl_reached_at_180s_without_extraction` | 40 ‰ at 180 ± 2 s |
| `l2_door_motor_ignites_in_band` | B3: ignition within 6 ticks of the door request |
| `l2_pump_in_plant_does_not_ignite_riser_gas` | spatial scope: no ignition from a plant arc at 2 ‰ |
| `l2_light_switch_is_not_an_ignition_source` | same atmosphere, `Ignites: None` → nothing |
| `l2_transfer_moves_the_species_downhill` | B2: source sealed at tick 1200; riser ≈0, plant 32 L ≈8.4 ‰ by tick 1896; damper closed at 1920 |
| `l2_water_displacement_conserves_species` | B4: `Σ gas_ml` invariant; WA-3 contribution `Δriser = Σ(V × c)` with passive transfer accounted separately |
| `l2_flood_blocks_duct_low` | the lobby duct is closed **to gas** at 110 mm, and reopens only when the plant is drained below its 60 mm sill |
| `l2_breach_is_safe_after_isolation` | B5: no arc, no new water path through the high breach |
| `l2_breach_without_isolation_has_no_arc` | the same cut with `M` live and no lockout emits no `Arc`: no utility is authored behind `wall-pl` |
| `l2_spoof_misleads_controller_not_physics` | §6 table, both seats |
| `l2_controller_loop_period_is_2030_ticks` | exact period, twice |
| `l2_signal_hops_are_one_per_tick` | physical change → fan actuation is 3 ticks (2 hops + 1 readout lag) |
| `l2_wire_and_controller_do_not_fight` | edge Enable + controller Disable settle, no oscillation |
| `l2_no_rng` | identical logs on replay |

## 10. Authoring excerpt — the part that matters

Illustrative, not a loaded fixture; nothing here is validated by a gate.

`signal` and `infra` are **sibling** optional blocks on `ScenarioDefinition` —
the same seam ADR-0012 created for `signal` alone. `circuits` stay on the
building.

```json
{
  "signal": {
    "format": "idaptik-signal/2",
    "devices": [
      { "id": "sig-gd1", "kind": "plant_sensor", "node": "gd-1",
        "room": "svc/riser" },
      { "id": "sig-vent", "kind": "plant_actuator", "node": "fan-f1",
        "room": "svc/riser", "hacker_only": true },
      { "id": "sig-transfer", "kind": "plant_actuator", "node": "fan-f2",
        "room": "svc/riser", "hacker_only": true },
      { "id": "sig-door", "kind": "plant_actuator", "node": "door-d5",
        "room": "svc/riser" }
    ],
    "links": [
      { "from": "sig-gd1", "from_port": "tripped",
        "to": "sig-vent", "to_port": "enable", "builtin": true }
    ]
  },
  "infra": {
    "format": "idaptik-infrastructure/1",
    "ruleset_version": 1,
    "gases": [
      { "id": "sable-2", "name": "sable-2", "density_vs_air_permille": 1600,
        "lfl_permille": 40, "ufl_permille": 130,
        "ignition_classes": ["inductive", "arc", "hot_surface"],
        "oxygen_min_permille": 170, "oxygen_per_litre_permille": 2100,
        "irritant_permille": 8, "incapacitate_permille": 120,
        "water": "displaced_only" }
    ],
    "links": [
      { "id": "vent-stack", "a": "riser", "b": "outside", "reach": "vent_stack",
        "aperture_cm2": 800, "capacity_ml_per_s": 5000,
        "sill_mm": 1500, "clearance_mm": 0,
        "actor": "blocked", "gate": { "entity": "dmp-x1" },
        "fluid_block": "never" },
      { "id": "pen-1", "a": "riser", "b": "plant", "reach": "duct",
        "aperture_cm2": 400, "capacity_ml_per_s": 2000,
        "sill_mm": 220, "clearance_mm": 0,
        "actor": "blocked", "gate": { "entity": "dmp-p1" },
        "fluid_block": "above_sill" },
      { "id": "duct-low", "a": "plant", "b": "lobby", "reach": "duct",
        "aperture_cm2": 600, "capacity_ml_per_s": 50,
        "sill_mm": 60, "clearance_mm": 0,
        "actor": "blocked", "gate": { "entity": "dmp-l1" },
        "fluid_block": "above_sill" },
      { "id": "duct-high", "a": "riser", "b": "lobby", "reach": "duct",
        "aperture_cm2": 250, "capacity_ml_per_s": 1200,
        "sill_mm": 1200, "clearance_mm": 0,
        "actor": "blocked", "gate": { "entity": "dmp-h1" },
        "fluid_block": "never" },
      { "id": "wall-pl", "a": "lobby", "b": "plant", "reach": "opening",
        "aperture_cm2": 0, "capacity_ml_per_s": 4000,
        "sill_mm": 900, "clearance_mm": 0,
        "actor": "blocked", "fluid_block": "never" }
    ],
    "entities": [
      { "id": "fan-f1", "kind": "fan", "compartment": "riser",
        "material": "steel", "node": "fan-f1",
        "caps": [
          { "powered": { "circuit": "N", "load": 4 } },
          { "moves": { "medium": "gas", "rate_ml_per_s": 4000,
                       "intake": "riser", "discharge": "outside",
                       "path": ["vent-stack"] } },
          { "ignites": { "class": "inductive" } },
          { "sounds": { "cue": "vent-hum", "level": 500, "radius_links": 2 } }
        ],
        "init": { "position": "off" } },
      { "id": "dmp-x1", "kind": "damper", "compartment": "riser",
        "material": "steel",
        "caps": [
          { "powered": { "circuit": "N", "load": 1 } },
          { "gates": { "flow": { "link": "vent-stack" },
                       "initial": "shut", "follows": "fan-f1" } }
        ],
        "init": { "position": "shut" } },
      { "id": "fan-f2", "kind": "fan", "compartment": "riser",
        "material": "steel", "node": "fan-f2",
        "caps": [
          { "powered": { "circuit": "N", "load": 2 } },
          { "moves": { "medium": "gas", "rate_ml_per_s": 1500,
                       "intake": "riser", "discharge": "plant", "path": ["pen-1"],
                       "reversible": true } },
          { "sounds": { "cue": "vent-hum", "level": 400, "radius_links": 2 } }
        ],
        "init": { "position": "off", "direction": "riser_to_plant" } },
      { "id": "dmp-p1", "kind": "damper", "compartment": "riser",
        "material": "steel",
        "caps": [{ "gates": { "flow": { "link": "pen-1" },
                              "initial": "open" } }],
        "init": { "position": "open" } },
      { "id": "dmp-l1", "kind": "damper", "compartment": "plant",
        "material": "steel",
        "caps": [
          { "powered": { "circuit": "N", "load": 1 } },
          { "gates": { "flow": { "link": "duct-low" }, "initial": "open" } }
        ],
        "init": { "position": "open" } },
      { "id": "dmp-h1", "kind": "damper", "compartment": "riser",
        "material": "steel",
        "caps": [{ "gates": { "flow": { "link": "duct-high" },
                              "initial": "shut" } }],
        "init": { "position": "shut" } },
      { "id": "gd-1", "kind": "detector", "compartment": "riser",
        "material": "pvc", "node": "gd-1",
        "caps": [
          { "powered": { "circuit": "N", "load": 1 } },
          { "senses": { "quantity": { "gas": "sable-2" },
                        "threshold_permille": 20, "hysteresis_permille": 5,
                        "dwell_ticks": 120 } },
          { "alarms": { "cue": "gas-alarm", "evidence": "gas-alarm-panel" } }
        ],
        "init": { "position": "live" } },
      { "id": "alarm-a1", "kind": "alarm", "compartment": "riser",
        "material": "pvc",
        "caps": [
          { "powered": { "circuit": "N", "load": 1 } },
          { "alarms": { "cue": "gas-alarm", "evidence": "gas-alarm-panel" } }
        ],
        "init": { "position": "live" } },
      { "id": "cyl-s2", "kind": "cylinder", "compartment": "riser",
        "material": "steel",
        "caps": [
          { "carries": { "medium": { "gas": "sable-2" }, "pressurised": true,
                         "leak_ml_per_s": 400 } },
          { "gates": { "flow": { "entity": "cyl-s2" }, "initial": "open" } }
        ],
        "init": { "integrity_permille": 1000 } },
      { "id": "cut-m", "kind": "cutter", "compartment": "lobby",
        "material": "steel",
        "caps": [{ "breaks": { "target": { "link": "wall-pl" },
                               "tool": "cutter", "hardness": 4 } }],
        "init": { "position": "unused" } }
    ],
    "controllers": [
      { "id": "ctl-vent", "reads": "sig-gd1", "targets": ["fan-f1"],
        "law": { "on_at_permille": 20, "off_at_permille": 15 },
        "hysteresis_permille": 5, "dwell_ticks": 120,
        "cooldown_ticks": 30, "rank": "control" }
    ],
    "rules": [
      "water-wets-surfaces", "wet-arc", "shock-field", "protective-isolation",
      "drying", "wet-run-damage", "re-energise-wet", "powered-actuator",
      "loss-of-extraction", "ignition", "combustion", "detector-alarm",
      "loss-of-detection", "flood-obstructs-link", "flood-blocks-traversal",
      "water-displaces-gas", "airflow-redistributes", "extraction-clears",
      "pressure-equalisation", "sensor-to-actuator", "controller-regulation",
      "spoofed-readout", "manual-override", "interlock", "bounded-feedback",
      "remote-operation", "excavate-exposes", "excavate-breach",
      "sever-utility", "water-enters-tunnel", "noise-and-vibration",
      "isolate-then-cut"
    ]
  }
}
```
