# Challenge Set 001 — Pump House

The floor the infrastructure consequences (ADR-0013) were built for: the
canonical Ghost Lobby rooftop scenario, relabelled as a municipal pump house
and carrying one floodable water zone, two alarms, a derived sump pump on the
power graph, and a hosted Moletaire companion. Builder:
`idaptik_core::scenario::pump_house()` — `ghost_lobby_rooftop()` plus the
infrastructure block, the mole host, and new labels over the **same five
canonical room ids** (`kitchen`, `hall`, `office`, `laundry`, `exit`), so
every interaction, hide spot and route the canonical floor carries still
lines up. Scenario id: `challenge-set-pump-house`.

Every number cited here is a `pub const` in `constants.rs` or a field of the
builder; the tests in `crates/idaptik-core/tests/challenge_set.rs` run all
four solutions tick by tick and assert the evidence listed under each.

## The floor

| Canonical room | Pump-house label | x | Notes |
|---|---|---|---|
| kitchen | WET CANTEEN | 20–270 | the note (92–170), counter hide @205 |
| hall | PUMP HALL | 270–510 | **the sump zone**, wheel @470, tool recess hide @404 |
| office | CONTROL OFFICE | 510–795 | flood bell, desk hide @655, USB @622 |
| laundry | SUMP GALLERY | 795–1040 | locker hide @885, **the chute @946** |
| exit | SLUICE EXIT | 1040–1260 | service exit @1242, crate hide @1125 |

Infrastructure (`InfraDef`):

- **Zone `hall-sump`** — room `hall`, manual wheel at `valve_x = 470`
  (reachable band ±`VALVE_DIST` 40, hold `VALVE_TURN_HOLD` 1.5 s),
  `fill_time = 24 s`, `fail_open = true`. Derived graph nodes
  `valve-hall-sump` (actuatable: `OpenValve`) and `pump-hall-sump` (passive:
  it only ever dies — through the feed's cascade).
- **Alarm `flood-alarm`** — sounder in the **office** (centre 652.5, east of
  Billy's patrol band), trips `Water { zone: hall-sump }` at
  `ALARM_WATER_TRIP_LEVEL = 0.25`.
- **Alarm `sump-tamper`** — sounder in the **hall**, trips
  `PowerLost { watch: pump-hall-sump }`: a normally-energised tamper circuit,
  so cutting the pump's power *is* the trip.

Hosted companion (`MoleHostDef`): spawn x 390 (pump hall), underground start,
one fitted upgrade — the AudioSynthesiser bay, three diversion sounds
(`MOLE_SYNTH_NOISE = 0.8`, window `MOLE_SYNTH_DURATION = 2.2 s`). Dig and
sabotage orders need depth past `dig_min_depth = 0.1`; a synth order the
fitted bay cannot carry is a typed `MoleDenied { Unavailable }`.

Objectives are the canonical three, relabelled only: note (canteen ledger),
misdirect, exit ("reach the sluice exit or the sump chute"). **No objective
mentions water, pumps, valves or power** — `challenge_set.rs` asserts that
structurally. The flood is a consequence, never a scripted trigger.

## The consequence timeline

From any release (wheel, hacked valve, or fail-open pump), with the zone's
24 s fill:

| t + | level | what happens |
|---|---|---|
| 3.6 s | 0.15 | `WATER_WADE_AT`: bodies in the pump hall wade at 0.62× and their noise meter floors at `WATER_WADE_NOISE = 0.62` — above Billy's `invest_noise = 0.45` inside 260 px |
| 6.0 s | 0.25 | the flood bell trips (`AlarmChanged`); a ringing bell climbs alert at `ALARM_ALERT_RATE = 0.9`/s (lockdown is ~111 s away — a clock, not a wall) |
| 13.2 s | 0.55 | `WATER_SHORT_AT`: the hall's electrics short **through the ordinary power-loss path** — lights, cameras, doors, valve and pump join `dead_nodes` (`InfraShorted`), the floor flickers 1.45 s, Billy is stunned 1.05 s, and the pump hall stays dark for the rest of the run |

The bell's room centre (652.5) is where Billy's Assess mode is pulled — the
strongest heard observation wins, ties break first-declared (flood bell
before tamper), and observations inside 15 px never re-pull. Darkness, water
at his feet and sparking electrics are *seen* observations: news, but not a
destination.

## The four solutions

All four: rope off the roof, pick the front door (4 s hold), take the note
while Billy is still offsite, and ride the laundry chute (search 1.55 s,
enter on the press). What separates them is how the crossing — canteen to
gallery, past a guard committed to the building — is bought.

### 1. Physical access — the wheel and the window

Turn the wheel **by hand** at 470 (`valve_turn` accumulates; the release
resets it), then wait at the pump hall's east end. The bell has been ringing
for seconds when Billy's shift starts: he is pulled to the control office and
is still wading the pump hall when the body sprints the crossing and rides
the chute. Evidence: `WaterReleased { cause: Valve }` with a sustained
`valve_turn` hold, `AlarmChanged`, `InfraShorted`, `GuardObserved { Alarm }`,
Billy's `last_known_x` at the bell's centre, and him still west of 520 at
the crossing. No network event of any kind fires.

### 2. Hacker / control — the valve and the lights

Pivot to the maintenance bridge and `NetHack` `valve-hall-sump`: the zone
releases with the wheel **never touched** (`valve_turn` stays 0). Hide in the
tool recess; after the short darkens the hall and Billy commits east past
560, crouch to the hall's east end (0.31 noise — under his 0.45 threshold at
point-blank range), `NetHack` `light-office` (the Lights action economy:
flicker 1.45 s, stun 1.05 s), `Unpivot` so support recovers, and sprint the
crossing inside the flicker. Evidence: `NetHackAction { valve-hall-sump }`,
`WaterReleased { cause: Valve }`, `UplinkAction { Lights }`,
`PivotClosed { hops: 0 }`, the pulled `last_known_x`, a clean finish.

### 3. Cooperative — the decoy chain

No water, no network: the companion stages Billy across the floor. From the
counter hide, as Billy crosses 200 on patrol, the mole runs legs
390 → 560 → 700 → 850 → 1000 → 1150, tunnelling at 200 px/s and refreshing
`Synth` every ~0.5 s so the diversion never lapses. Every leg sits inside
260 px of the last — audible from wherever the previous decoy left him — and
each arrival re-pulls him east (`GuardObserved { UnusualNoise }` per room,
episode-latched). Pinned near the sluice exit, the infiltrator sprints while
he is too far to hear it, **crouch-walks** the last stretch (below his
footfall threshold even inside his hearing ring), and rides the chute.
Evidence: `Mole { SynthSoundPlayed }`, the diversion live in canonical state
at the finish, Billy held past 1000 with `last_known_x` tracking the decoy,
**zero** `WaterReleased` — the zone's level is exactly 0.

### 4. Emergent — the cascade nobody scripted

Two `NetSsh` hops up the ISP line (`isp-ops`, `grid-jump`) and one
`NetHack` on the **substation** — a grid cut aimed at the upstream power
line, from a locker-bay hide. The cut cascades substation → feed → every
fixture: all lights die (every room dark, floor flickers), cameras die, and
the **sump pump dies — releasing the fail-open zone with
`cause: PumpFailure`**. The tamper bell trips the same tick (power loss *is*
its trip), the flood bell follows at +6 s, and Billy arrives into a dark,
ringing building, announces what he perceives (`GuardObserved { Darkness }`,
`{ Alarm }`), and is pulled to the control office while the infiltrator
crouch-walks two rooms east and rides the chute. Evidence: the
`PumpFailure` provenance with **no** `Valve` release anywhere, the wheel
never turned, `PowerLost { nodes ≥ 2 }`, both bells, the dead `pump-*` and
`light-office` nodes in canonical state.

The emergence is the point: the player aimed at the power grid; the flood,
the bells, the darkness and the pulled guard are consequences the systems
produced together. Nothing in the objectives, the tick scripts or the sim
names that outcome.

## Multiplayer and replay notes

- `Command::Mole` folds through the same envelope as every command
  (`seat_for` routes it to the infiltrator), and
  `pump_house_mole_orders_lockstep_with_the_reference` pins the interleaving
  of mole orders against the headless reference.
- A mid-flood snapshot restores and replays identically — companion RNG
  stream included (`a_mid_flood_snapshot_restores_and_replays_identically`).
- The wire alphabet (`fixtures/session_relay/commands.json`,
  `session_relay_fixture.rs`) carries all four `MoleOp` forms.
- **External follow-up:** the pinned burble relay's `command_roles()` must
  learn `"Mole" → infiltrator` before a *live* burble session routes mole
  orders; this repository's fixtures and lockstep gate do not depend on it
  (the loopback scripts carry no mole commands).

## Where it is enforced

| Concern | Gate |
|---|---|
| Observation-only guard reaction; no leak of player position | `tests/infra_observations.rs` |
| Infra-free floors untouched (snapshots byte-clean, verb denies) | `tests/infra_observations.rs`, existing goldens |
| The four solutions and their evidence | `tests/challenge_set.rs` |
| Authoring: audit shape, dangling infra refused | `tests/building_audit.rs`, `scenario/building/audit.rs` units |
| Builder invariants, cascade provenance | `scenario/pump_house.rs` units |
| Lockstep with mole orders | `idaptik-net` lockstep fixture |
