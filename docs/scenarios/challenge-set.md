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

The bell's room centre (652.5) is where Billy's Assess mode is pulled first —
the strongest heard observation wins, ties break first-declared (flood bell
before tamper), and observations inside 15 px never re-pull. The flood drowns
the pump hall's electrics including the pump, so the tamper bell (sounder in
the hall, centre 390) trips too — and once Billy stands at one bell the 15 px
gate mutes it under his feet while the other, still heard and still strongest,
pulls him across the floor: he pendulums between the two. Darkness, water at
his feet and sparking electrics are *seen* observations: news, but not a
destination.

## The four solutions

All four: rope off the roof, pick the front door (4 s hold), take the note
while the guard is still offsite, and ride the laundry chute (search 1.55 s,
enter on the press). What separates them is how each line answers the
floor's two standing problems: the **closed interior doors** (they block
everyone — the infiltrator cannot open them by hand; they open only to a
routed network hold, a hack on the door's own node, or the guard's badge as
he walks) and the **guard committed to the building**. The roof line — back
out the front door, up the rope, over the hatch at 990 — is the physical
bypass every seat can take; the fall is loud (`NOISE_LAND`), so it lands
only with the guard beyond 260 px of cover.

### 1. Physical access — the roof line and the clock

No network, no companion, no water. Note in hand, the body leaves the way
the rope came in: out the picked front door, back up to the roof, and east
along it to the hatch over the sump gallery. The drop lands inside the
chute's own search band; search and ride. The line beats (or sidesteps) the
guard's arrival clock — and even when the shift starts mid-run, the guard
patrols the west end two floors of sight-lines away, never east of the pump
hall. Evidence: the full physical verb chain (`RopeTaken` down **and** up,
`FrontDoorPicked`, `NoteSecured { seen: false }`, `HatchDropped`,
`Extracted`), a dry zone (level exactly 0.0, no `WaterReleased`), zero
network or companion events, and the guard held west of 520.

The manual wheel at 470 is a network-assisted option on this floor: the
pump hall's doors make it unreachable to a purely physical line (the guard
badges you in but never badges you out). Its mechanics — the silent 1.5 s
hold, the release through the same canonical path — are covered by the
`pump_house` unit tests.

### 2. Hacker / control — the valve, the doors, and the lights

Pivot to the maintenance bridge and `NetHack` `valve-hall-sump` from the
roof, first thing: the zone releases with the wheel **never touched**
(`valve_turn` stays 0). The body then walks east through doors the network
opens one by one — `Uplink { Door }` routed-holds the door nearest it (D1
into the wading pump hall, D2 out), and `NetHack { door-d3 }` opens the
gallery door *by name* (the nearest-door uplink from mid-office would
answer D2 again); the office lights die to a `NetHack { light-office }` on
the way through, buying a flicker-covered crossing. The body stages in the
locker bay **before the shift starts**. When the guard arrives, the flood
bell — ringing since six seconds after the release — pulls him to the
control office; the drowned hall's tamper bell pulls him back again, and he
pendulums between the two, never within hearing of the gallery while it is
searched and the chute is ridden. Evidence: `NetHackAction { valve-hall-sump
}`, `WaterReleased { cause: Valve }` with `valve_turn == 0`,
`UplinkAction { Lights }` (`lights_uses >= 1`), three
`UplinkAction { Door }` holds, `AlarmChanged`, `InfraShorted`,
`GuardObserved { Alarm }`, the guard's `last_known_x` committed to one bell
or the other (652.5 or the hall's 390) and his body west of 760 at the
finish — never inside the chute's 260 px of hearing.

### 3. Cooperative — the decoy sweeps

No water, no network: the companion stages the guard across the floor and
back again. From the counter hide, as the guard moves on his patrol, one
staged sequence of underground runs (200 px/s) carries the mole — and its
refreshed synth decoy, every 30 ticks — from the pump hall to the gallery:
legs at 560, 760 and 960, each waiting at its endpoint until the guard
closes to within 45 px (the turn-back fires once he is within 145 px of
the gallery's east end — committed far enough that the canteen and the
roof line are safe), because one continuous run outruns his 260 px
hearing while he badges a door. The east turn deliberately stops short of
the service exit: the chase asks the guard to badge only the doors his
FSM demonstrably badges. He follows the sound the whole way, badging
every door behind him. Once he is pinned east, staged legs back west
(800, 640, 500, 390) park him at the pump hall's west door. Meanwhile the body leaves the
counter, climbs back to the roof, and **walks** it east — walked, never
sprinted: `noise_invest` has no room check, and a sprint on the roof would
pull the guard off the decoy. The hatch drop lands only once he is beyond
hearing of the gallery (`billy.x <= 700`, 290 px of cover); the silent
search and the ride happen with the guard walking the wrong way, chasing a
sound. Evidence: `Mole { SynthSoundPlayed }` throughout, no `MoleDenied`,
`GuardObserved { UnusualNoise }` announced room by room, the guard held
west of 780 with `last_known_x` tracking the decoy — and **zero**
`WaterReleased`: the zone's level is exactly 0.

### 4. Emergent — the cascade nobody scripted

Two `NetSsh` hops up the ISP line (`isp-ops`, `grid-jump`) and one
`NetHack` on the **substation**, fired from behind the canteen counter — a
grid cut aimed at the upstream power line. The cut cascades substation →
feed → every fixture: all lights die (every room goes dark), the cameras
die, and the **sump pump dies — releasing the fail-open zone with
`cause: PumpFailure`**. The tamper bell trips the same tick (power loss *is*
its trip), the flood bell follows at +6 s, and the guard — arriving into a
dark, ringing building — announces what he perceives (`GuardObserved {
Darkness }`, `{ Alarm }`), badges east past the hidden body, and is held
between the two bells — the office flood bell and the hall tamper — neither
of which is the player. The body leaves over the roof: out the front door,
up the rope, over the hatch — 300+ px of cover from a guard the bells own —
and rides the chute. Evidence: the `PumpFailure` provenance with **no**
`Valve` release anywhere and the wheel never turned, `PowerLost { nodes >=
2 }`, both bells, the dead `pump-*` and `light-office` nodes in canonical
state, and the guard committed to a bell (`last_known_x` at 652.5 or 390),
never east of 760.

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
